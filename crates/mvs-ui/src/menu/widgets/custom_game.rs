//! The custom game lobby: WBP_CustomGame over the offline `LocalLobby`
//! (menu/lobby), its team columns (WBP_CustomGameTeam), their player rows and empty slots (WBP_CustomGamePlayer) and
//! the maps' thumbnails (WBP_CustomGameMapThumbnail).
//!
//! The lobby is the CustomGame's own (`LocalLobby`). Its requests answer with echoes, which the
//! widget drains in its Tick (`LocalLobby::take_events`: a Delay(0)). The rows of a column know what
//! they read of the lobby through `Facts`.
//!
//! Not ported (R4 or other tracks'): the online lobby (NetLobby, the room code and settings buttons, the chat), the
//! social panel and invite modal, the spectator block.
//!
//! Start: after the 1.0 s of the build's rematch start the modals are dismissed and the PreMatch level is opened
//! (`MenuExit::PreMatch`) with the set-up of the lobby, the lobby going with it to take the picks and show again at
//! the match's end.

use std::any::Any;

use mvs_protocol::lobby_rules::{BOT_DIFFICULTY_EXPERT, LobbyPlayer, STYLE_FFA, TEAM_STYLE_IDS};
use serde_json::json;

use super::super::screens::lab_selectors::{LabCharacterSelector, LabMapSelector, LabSkinSelector};
use super::common::{ModalBase, fill_page, palette_entry};
use super::controls::buttons::{ButtonMain, ButtonSecondary, ButtonSecondaryVertical, IconButton, ToggleSwitch};
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{self, batch_set_context, batch_set_scroll_box};
use super::controls::tooltip::{TooltipTarget, tooltip_params};
use super::controls::{bind, find, focus_frame, slot_z_order, wd};
use super::custom_settings::{CustomGameSettings, LobbyView, SettingsPage, experimental_page, room_page};
use super::home::Homescreen;
use super::lobby_chat::{self, ChatAction, LobbyChat};
use super::nav::{GlobalNav, live};
use super::social::{SocialInviteModal, SocialPanel};
use crate::bundle::Rgba;
use crate::bundle::{MapInfo, RosterEntry, SkinInfo};
use crate::menu::connection_line::{self, LineState};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::custom_text;
use crate::menu::hydra::picture_name;
use crate::menu::lobby::loadout::{BotPool, picked_skin, skin_by_name};
use crate::menu::lobby::net::{NetLobby, Say, note_text};
use crate::menu::lobby::{self, Lobby, LobbyEvent, LocalLobby, SetupSource};
use crate::menu::manager::{DialogParams, Manager, Modal, NotificationParams, TransitionParams};
use crate::menu::online::{OfflineWhy, OnlineCmd, OnlineLine, OnlineView, PartyCmd, SessionLink};
use crate::menu::own_profile::{my_profile_icon_asset, profile_icon_by_asset, watch_profile_icon};
use crate::menu::screens::pre_match::setup::PreMatchStart;
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// (the port's own) the space between the two columns of the free-for-all grid (CustomGameTeam.asGrid).
const GRID_GAP: f64 = 40.0;

/// (the port's own) The lobby's difficulty of the Expert bot, after EBotDifficulty's Hard (5), offline and online:
/// its match level is the Expert level (lobby_rules' difficulty - 2).
const EXPERT_DIFFICULTY: i32 = BOT_DIFFICULTY_EXPERT;
const _: () = assert!(EXPERT_DIFFICULTY - 2 == mvs_sim::ai::expert::EXPERT_LEVEL);
/// The difficulty drop's Expert id.
const EXPERT_ID: &str = "expert";

/// EBotDifficulty of the difficulty drop's rows (EXPERT_DIFFICULTY the port's own).
fn bot_difficulty_id(d: i32) -> &'static str {
    match d {
        2 => "very_easy",
        3 => "easy",
        4 => "medium",
        5 => "hard",
        EXPERT_DIFFICULTY => EXPERT_ID,
        _ => "",
    }
}

const EDIT_FOCUSED: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const EDIT_IDLE: Rgba = [0.194618, 0.571125, 0.552012, 1.0];
/// (the port's own) A slot's ready tag: the rematch vote's green (UI_EOG_RematchStatus) and the icon buttons'
/// DisabledColor (#8CA3A9).
const READY_GREEN: Rgba = [0.008023, 0.693872, 0.258183, 1.0];
const READY_GREY: Rgba = [0.262251, 0.366253, 0.396755, 1.0];

/// The row of the page's top-right buttons (the browser's REFRESH and ENTER ROOM CODE): in Overlay_128 over the header,
/// centred on the title's line and ending where the bottom row's buttons do (VerticalBox_101's right offset, 200),
/// TOP_GAP apart.
pub const TOP_BUTTONS_PAD: Margin = Margin { l: 0.0, t: 58.0, r: 200.0, b: 0.0 };
/// (the port's own) The connection line, a subtitle under the page's title in the header band: its left edge on the
/// title text's (WBP_MenuHeader: CollectionHomeHeader's Left 100, the 64 icon and its Right 4), its top under the
/// title's row (Top 40). The face and weight of the build's subtitles under a page title (UI_LocalPlay settingsLabel,
/// NormsCond-ExtraBoldItalic), a size that keeps the longest line above the band's slanted edge. The state word in
/// the state's colour: a good round trip's green (hud::ping) online and on a local network,
/// the build's orange text (UI_Homescreen_v2 TextBlock_66) offline. The rest in the title's cream, LINK_GAP after
/// the state word.
const LINK_FACE: &str = "NormsCond-ExtraBoldItalic";
const LINK_SIZE: f64 = 20.0;
const LINK_LEFT: f64 = 168.0;
const LINK_TOP: f64 = 132.0;
const LINK_GAP: f64 = 6.0;
const LINK_ONLINE: Rgba = crate::hud::ping::GREEN;
const LINK_LOCAL: Rgba = [0.982251, 0.558341, 0.056128, 1.0];
const LINK_TEXT: Rgba = EDIT_FOCUSED;

/// (the port's own) What the lobby's connection line says: its session's link once the session opened; a lobby with no
/// net service in this run (a UI-only local lobby) is local for the menus' reason.
pub fn lobby_link(lobby: &Lobby, online: &OnlineView) -> Option<SessionLink> {
    match lobby {
        Lobby::Local(_) => Some(SessionLink::Local(crate::menu::online::LocalReason::Offline(match &online.line {
            OnlineLine::Offline(why) => why.clone(),
            OnlineLine::Online | OnlineLine::Slower => OfflineWhy::Other,
        }))),
        Lobby::Net(n) if n.is_open() => online.session.as_ref().and_then(|s| s.link.clone()),
        Lobby::Net(_) => None,
    }
}

/// The connection line's widgets and what they show.
#[derive(Debug)]
struct LinkLine {
    root: WidgetId,
    head: WidgetId,
    detail: WidgetId,
    tip: WidgetId,
    shown: Option<SessionLink>,
}
pub const TOP_GAP: f64 = 24.0;

/// What a row reads of the lobby.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub my_id: String,
    pub am_leader: bool,
    /// ReadyUsers (lobby_rules ready).
    pub ready: Vec<String>,
    /// The lobby has a READY button (an online one): the rows show their ready tags.
    pub ready_button: bool,
}

impl Facts {
    fn as_row(&self) -> Facts {
        self.clone()
    }
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

// ---------- WBP_CustomGamePlayer ----------

const P_BOT_FOCUS: u32 = 1;
const P_BOT_UNFOCUS: u32 = 2;
const P_INVITE_FOCUS: u32 = 3;
const P_INVITE_UNFOCUS: u32 = 4;
const P_INVITE_CLICK: u32 = 5;
const P_BOT_CLICK: u32 = 6;
const P_ICON_CLICK: u32 = 7;
const P_SWITCH_CLICK: u32 = 8;
const P_PROMOTE_CLICK: u32 = 9;
const P_KICK_CLICK: u32 = 10;
const P_DIFFICULTY: u32 = 11;
const P_ICON_FOCUS: u32 = 12;
const P_ICON_UNFOCUS: u32 = 13;
const P_KICK_CONFIRMED: u32 = 14;
const P_CHARACTER_SELECTED: u32 = 15;
const P_CHARACTER_CANCELED: u32 = 16;
const P_SKIN_SELECTED: u32 = 17;
const P_SKIN_CANCELED: u32 = 18;
const R_SHOW_CHARACTER: u32 = 1;
const R_SHOW_SKIN: u32 = 2;

/// AreSelectorsActive?'s Modal IDs.
const BOT_SELECTOR_MODALS: [&str; 2] = ["labCharacterSelector", "labSkinSelector"];

/// A player row of a team, or one of its empty slots. A bot's character and skin are the port's fighter name and the
/// skin's SkinData name, '' the default (the build: the CharacterData and SkinData asset paths).
/// Not ported: the social actions menu of a human's portrait button (it does nothing here). Humans pick their
/// fighter on the pre-match screen, as in the build.
#[derive(Debug, Default)]
pub struct CustomGamePlayer {
    pub add_bot_dispatch: Event<(String, String)>,
    pub invite_player_clicked: Event<()>,
    pub switch_team_clicked: Event<(String, WidgetId)>,
    pub promote_player_clicked: Event<String>,
    pub kick_player_clicked: Event<String>,
    /// (account id, character, difficulty, skin).
    pub on_bot_settings_changed: Event<(String, String, i32, String)>,
    pub slot_is_empty: bool,
    pub player: Option<LobbyPlayer>,
    pub leader_id: String,
    pub is_ready: bool,
    pub can_swap_teams: bool,
    pub facts: Facts,
    is_leader: bool,
    can_kick: bool,
    /// The row's id a kick dialog was asked for.
    pending_kick: String,
    /// BotCharacter of the selector flow.
    bot_character: String,
    /// The fighter the skin selector opens on (OnCharacterSelected's Delay(0)).
    pending_skin_of: String,
    /// (the port's own) The ready tag under the player's name, made at its first show.
    ready_tag: Option<WidgetId>,
}

impl CustomGamePlayer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGamePlayer::default())
    }

    fn account_id(&self) -> &str {
        self.player.as_ref().map_or("", |p| p.account_id.as_str())
    }

    fn is_me(&self) -> bool {
        self.account_id() == self.facts.my_id
    }

    fn btn_interactor(cx: &Cx<'_>, name: &str) -> WidgetId {
        let b = find(cx, name);
        cx.view.tree().find_in(b, "UI_Interactor")
    }

    /// The button the selectors hand the focus back to.
    fn picker(&self, cx: &Cx<'_>) -> WidgetId {
        if self.slot_is_empty || self.player.as_ref().is_some_and(|p| p.is_bot) {
            Self::btn_interactor(cx, "btn_InviteBot")
        } else {
            Self::btn_interactor(cx, "IconButton")
        }
    }

    /// AreSelectorsActive? @0: the latest modal is one of the two selectors.
    fn selectors_active(cx: &Cx<'_>) -> bool {
        Manager::is_modal_active(cx.view) && BOT_SELECTOR_MODALS.contains(&Manager::latest_modal_id(cx.view).as_str())
    }

    /// @3126: BotCharacter = the first of Get Characters(false, false), whatever the row's bot is; Delay(0) -> @15.
    /// Unverified: Get Characters' order is read as the selector's own (the roster), whose first tile it then
    /// focuses either way.
    fn pick_bot(&mut self, cx: &mut Cx<'_>) {
        self.bot_character = cx.view.roster().first().and_then(|r| r.fighter.clone()).unwrap_or_default();
        cx.view.delay(0.0, Handler { owner: cx.w, id: R_SHOW_CHARACTER });
    }

    /// @112: instructions "bot"; SelectedEvent -> OnCharacterSelected, CanceledEvent -> OnCharacterSelectCanceled
    /// (@7997: btn_InviteBot.SetFocused(true), on a bot's row too).
    fn show_character_selector(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let Some(m) =
            Manager::show_modal(cx, "UI_LAB_CharacterSelector", "labCharacterSelector", "bot", &self.bot_character)
        else {
            return;
        };
        cx.visit::<LabCharacterSelector, _>(m, |s, _| {
            s.selected_event.add(Handler { owner: me, id: P_CHARACTER_SELECTED });
            s.canceled_event.add(Handler { owner: me, id: P_CHARACTER_CANCELED });
        });
    }

    /// OnCharacterSelected @3071: Delay(0) -> @565, the skin selector of the event's character (instructions "lab").
    /// ChoseRandom is not read: the fighter the Random tile rolled goes on to its skins like a picked one.
    /// Deviation: a roll on a fighter the port does not have takes one of the ported ones (as the Lab's Random does).
    fn on_character_selected(&mut self, cx: &mut Cx<'_>, c: &RosterEntry, random: bool) {
        let fighter = match c.fighter.clone() {
            Some(f) => f,
            None if random => {
                let n = cx.view.fighters().len();
                let at = ((cx.view.manager.random() * n as f64).floor() as usize).min(n.saturating_sub(1));
                match cx.view.fighters().keys().nth(at) {
                    Some(k) => k.clone(),
                    None => return,
                }
            }
            None => return,
        };
        self.pending_skin_of = fighter;
        cx.view.delay(0.0, Handler { owner: cx.w, id: R_SHOW_SKIN });
    }

    fn show_skin_selector(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let f = self.pending_skin_of.clone();
        let Some(m) = Manager::show_modal(cx, "UI_LAB_SkinSelector", "labSkinSelector", "lab", &f) else { return };
        cx.visit::<LabSkinSelector, _>(m, |s, _| {
            s.selected_event.add(Handler { owner: me, id: P_SKIN_SELECTED });
            // OnSkinCanceled @4733 -> @112: the character selector again, on BotCharacter
            s.canceled_event.add(Handler { owner: me, id: P_SKIN_CANCELED });
        });
    }

    /// OnSkinSelected @4631: an empty slot asks for the bot (AddBotDispatch @4593); a bot's row asks for the change
    /// (@3607: UpdateBotPortrait, which reloads the row's present character, then OnBotSettingsChanged with the row's
    /// difficulty). The row shows the change at the lobby's echo (UpdateData). `skin`: the pick's MatchPlayer.skin
    /// ('' for the port's default).
    fn bot_picked(&mut self, cx: &mut Cx<'_>, character: &str, skin: &str) {
        self.bot_character = character.to_string();
        let p = self.picker(cx);
        interactor::set_focused(cx, p, true);
        if self.slot_is_empty {
            cx.emit(self, |c| &c.add_bot_dispatch, &(character.to_string(), skin.to_string()));
        } else if let Some(pl) = self.player.clone().filter(|p| p.is_bot) {
            let a = (pl.account_id, character.to_string(), pl.difficulty, skin.to_string());
            cx.emit(self, |c| &c.on_bot_settings_changed, &a);
        }
    }

    fn update_add_button_visibility(cx: &mut Cx<'_>, focused: bool) {
        let b = find(cx, "AddPlayerButtonBox");
        wd(cx, b).opacity = if focused { 1.0 } else { 0.0 };
        set_vis(cx, "Icon_plus", if focused { Vis::Hidden } else { Vis::SelfHitTestInvisible });
    }

    fn update_leader_info(&mut self, leader_id: &str) {
        self.leader_id = leader_id.to_string();
        self.is_leader = self.account_id() == leader_id;
        self.can_kick = self.facts.am_leader && !self.is_leader;
    }

    pub fn update_data(
        &mut self,
        cx: &mut Cx<'_>,
        player: LobbyPlayer,
        leader_id: &str,
        is_ready: bool,
        can_swap_teams: bool,
        facts: Facts,
    ) {
        self.player = Some(player);
        self.facts = facts;
        self.is_ready = is_ready;
        self.can_swap_teams = can_swap_teams;
        self.update_leader_info(leader_id);
        let s = Self::btn_interactor(cx, "buttonSwitchTeam");
        interactor::set_interactable(cx, s, true);
        self.update_appearance(cx);
    }

    pub fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        if self.slot_is_empty {
            set_vis(cx, "Icon_plus", Vis::SelfHitTestInvisible);
            set_vis(cx, "QuadBorder_1", Vis::SelfHitTestInvisible);
            set_vis(cx, "PlayerInfoBox", Vis::Collapsed);
            set_vis(cx, "MeIndicator", Vis::Collapsed);
            set_vis(cx, "emptyBG", Vis::Hidden);
            set_vis(cx, "AddPlayerButtonBox", Vis::SelfHitTestInvisible);
            return;
        }
        let Some(p) = self.player.clone() else { return };
        let me = self.is_me();
        let leader = self.facts.am_leader;
        let icon_btn = find(cx, "IconButton");
        let icon_interactor = Self::btn_interactor(cx, "IconButton");
        set_vis(cx, "Icon_plus", Vis::Collapsed);
        set_vis(cx, "QuadBorder_1", Vis::Collapsed);
        let container = cx.view.tree().find_in(icon_btn, "IconContainer");
        wd(cx, container).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 0.0 };
        set_vis(cx, "PlayerInfoBox", Vis::SelfHitTestInvisible);
        set_vis(cx, "buttonKickPlayer", if self.can_kick { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, "buttonPromotePlayer", if leader && !me && !p.is_bot { Vis::Visible } else { Vis::Collapsed });
        let diff = find(cx, "dropdownBotDifficulty");
        let diff_main = cx.view.tree().find_in(diff, "mainInteractor");
        interactor::set_interactable(cx, diff_main, self.can_kick);
        set_vis(cx, "EditIcon", if self.can_kick { Vis::HitTestInvisible } else { Vis::Collapsed });
        set_vis(cx, "buttonSwitchTeam", if me && self.can_swap_teams { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, "MeIndicator", if me { Vis::HitTestInvisible } else { Vis::Collapsed });
        let me_ind = find(cx, "MeIndicator");
        let c = palette_entry(cx, 2);
        wd(cx, me_ind).color = c;
        set_vis(cx, "emptyBG", Vis::SelfHitTestInvisible);
        // SetReady (@4748, UpdateData @7411): a ready row's ReadyCheck collapsed and its portrait white; a row not
        // ready plays SpinAnimation over a dimmed portrait.
        // Deviation: (the port's own, user decision) every joined row is drawn as a ready one: the spin and the grey
        // read as a player still joining. The ready state is the tag under the name (`update_ready_tag`).
        set_vis(cx, "ReadyCheck", Vis::Collapsed);
        let owner = cx.w;
        if cx.view.is_animation_playing(owner, "SpinAnimation") {
            cx.view.stop_animation(owner, "SpinAnimation");
        }
        wd(cx, icon_btn).color = [1.0, 1.0, 1.0, 1.0]; // @10587
        self.update_ready_tag(cx, &p);
        let title = find(cx, "PlayerTitle");
        let kick = find(cx, "buttonKickPlayer");
        let kick_image = cx.view.tree().find_in(kick, "iconImage");
        let icon_image = cx.view.tree().find_in(icon_btn, "iconImage");
        if p.is_bot {
            interactor::set_interactable(cx, icon_interactor, self.can_kick);
            set_vis(cx, "PlayerButtonStrip", Vis::Collapsed);
            set_vis(cx, "statusIndicator_1", Vis::Collapsed);
            set_vis(cx, "IsLeaderContainer", Vis::Collapsed);
            set_vis(cx, "dropdownBotDifficulty", Vis::SelfHitTestInvisible);
            wd(cx, kick_image).tex = Some("ui_icons_x".into());
            // (the port's own) Expert after Hard
            DropMenu::offer_own_item(cx, diff, custom_text::EXPERT_DIFFICULTY, EXPERT_ID);
            let id = bot_difficulty_id(p.difficulty);
            cx.visit::<DropMenu, _>(diff, |d, cx| d.set_active_by_id(cx, id));
            // UpdateBotPortrait @2929 -> OnCharacterLoaded @3708: "{DisplayName} (Bot)" of the CharacterData;
            // OnSkinAssetLoaded @13140: the SkinData's VariantIconMaterial (a skin the bundle does not have: the
            // fighter's default one; a fighter it does not have: the tree's icon and the lobby's name)
            let f = cx.view.fighters().get(&p.character).cloned();
            wd(cx, title).text =
                format!("{} (Bot)", f.as_ref().map_or(p.username.as_str(), |f| f.display_name.as_str()));
            if let Some(f) = &f {
                let tex = skin_by_name(Some(f), &p.skin).map_or(&f.skin.icon, |s| &s.icon).clone();
                wd(cx, icon_image).tex = Some(tex);
            }
        } else {
            interactor::set_interactable(cx, icon_interactor, true);
            set_vis(cx, "PlayerButtonStrip", Vis::SelfHitTestInvisible);
            set_vis(cx, "statusIndicator_1", Vis::SelfHitTestInvisible);
            set_vis(cx, "dropdownBotDifficulty", Vis::Collapsed);
            // (a human's EditIcon is Collapsed)
            set_vis(cx, "EditIcon", Vis::Collapsed);
            // the row's ProfileIconPath (@7292 / @12978): not empty, LoadAsset -> IconButton.iconImage
            // SetBrushFromSoftTexture(the ProfileIconData's TextureRef) (@1164 / @1514); empty, the brush stays. The
            // port's lobby row carries the asset name (the server's, from each player's profile.icon); the player's
            // own row takes the stored one, which a local lobby's row lacks.
            let asset = if me { my_profile_icon_asset(&cx.prefs.hydra) } else { p.icon.clone() };
            if let Some(tex) = profile_icon_by_asset(cx.view, &asset).and_then(|e| picture_name(e.icon.as_deref()?)) {
                wd(cx, icon_image).tex = Some(tex);
            }
            wd(cx, kick_image).tex = Some("ui_icons_kick".into()); // @12613
            let il = find(cx, "IsLeader");
            let c = palette_entry(cx, 2);
            wd(cx, il).color = c;
            set_vis(cx, "IsLeaderContainer", if self.is_leader { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
            wd(cx, title).text = p.username.clone();
        }
        // @10120-10448: the tip of the leader mark, shown while the mouse is on it
        let tt = find(cx, "leaderTooltipTarget");
        cx.visit::<TooltipTarget, _>(tt, |t, _| t.params = tooltip_params("Lobby Leader", true, 0.0, 45.0, 2));
    }

    /// (the port's own) READY in green or NOT READY in grey under a human's name, in an online lobby (one with a
    /// READY button); none for the leader (who starts the match) and for a bot.
    fn update_ready_tag(&mut self, cx: &mut Cx<'_>, p: &LobbyPlayer) {
        let show = self.facts.ready_button && !p.is_bot && !self.is_leader;
        if !show && self.ready_tag.is_none() {
            return;
        }
        let tag = match self.ready_tag {
            Some(t) => t,
            None => {
                // the name's face (PlayerTitle, NormsCond-ExtraBoldItalic at its default 24) a size down
                let t = super::lobby_chat::plain(
                    cx,
                    "TextBlock",
                    "ReadyTag",
                    json!({
                        "Text": { "CultureInvariantString": "" },
                        "Font": { "FontObject": "PFG", "TypefaceFontName": "NormsCond-ExtraBoldItalic", "Size": 18 },
                        "TextTransformPolicy": "ETextTransformPolicy::ToUpper",
                    }),
                    json!({ "Padding": { "Top": 2 }, "HorizontalAlignment": "HAlign_Left" }),
                    "VerticalBoxSlot",
                );
                let bx = find(cx, "VerticalBox_0");
                MenuView::add_child(cx, bx, t, Some(1));
                self.ready_tag = Some(t);
                t
            }
        };
        let (text, color) = if self.is_ready {
            (custom_text::SLOT_READY, READY_GREEN)
        } else {
            (custom_text::SLOT_NOT_READY, READY_GREY)
        };
        let w = wd(cx, tag);
        w.vis = if show { Vis::HitTestInvisible } else { Vis::Collapsed };
        w.text = text.to_string();
        w.color = color;
    }

    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str) {
        let mut list = vec![Self::btn_interactor(cx, "btn_InviteBot"), Self::btn_interactor(cx, "btn_InvitePlayer")];
        for n in ["IconButton", "buttonKickPlayer", "buttonSwitchTeam", "buttonPromotePlayer"] {
            list.push(Self::btn_interactor(cx, n));
        }
        batch_set_context(cx, &list, ctx);
        let d = find(cx, "dropdownBotDifficulty");
        cx.visit_dyn(d, |_, cx| DropMenu::update_context(cx, ctx, None));
    }
}

impl Controller for CustomGamePlayer {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.slot_is_empty = cx.view.tree().props(cx.w).get("SlotIsEmpty?").and_then(|v| v.as_bool()).unwrap_or(false);
        // (the catalogue's arrival seeds an empty icon: a row reads the icon after it)
        if !self.slot_is_empty {
            watch_profile_icon(cx);
        }
        let me = cx.w;
        let (bot, invite) = (Self::btn_interactor(cx, "btn_InviteBot"), Self::btn_interactor(cx, "btn_InvitePlayer"));
        if let Some(s) = cx.view.manager.interactor_mut(invite) {
            s.manual_left = Some(bot);
        }
        if let Some(s) = cx.view.manager.interactor_mut(bot) {
            s.manual_right = Some(invite);
        }
        // @5109: the slot's wobbler is an outline quad; the second SetInteractor replaces the first
        let fc = find(cx, "UI_FocusedCommon");
        let wob = cx.view.tree().find_in(fc, "Wobbler");
        cx.visit::<super::common::Quadrilateral, _>(wob, |q, cx| q.set_outline(cx, 0.1));
        focus_frame(cx, fc, bot);
        for (n, f, u) in
            [("btn_InviteBot", P_BOT_FOCUS, P_BOT_UNFOCUS), ("btn_InvitePlayer", P_INVITE_FOCUS, P_INVITE_UNFOCUS)]
        {
            let b = find(cx, n);
            cx.visit::<ButtonSecondaryVertical, _>(b, |b, _| {
                b.on_focus.add(Handler { owner: me, id: f });
                b.on_unfocus.add(Handler { owner: me, id: u });
            });
        }
        let ib = find(cx, "btn_InvitePlayer");
        cx.visit::<ButtonSecondaryVertical, _>(ib, |b, _| b.on_click.add(Handler { owner: me, id: P_INVITE_CLICK }));
        // btn_InviteBot OnClick @4738 -> @3126
        let bb = find(cx, "btn_InviteBot");
        cx.visit::<ButtonSecondaryVertical, _>(bb, |b, _| b.on_click.add(Handler { owner: me, id: P_BOT_CLICK }));
        // IconButton OnClick @3303: a bot's goes the same way; a human's opens the social actions menu (not ported:
        // nothing happens)
        for (n, id) in [
            ("IconButton", P_ICON_CLICK),
            ("buttonSwitchTeam", P_SWITCH_CLICK),
            ("buttonPromotePlayer", P_PROMOTE_CLICK),
            ("buttonKickPlayer", P_KICK_CLICK),
        ] {
            let b = find(cx, n);
            cx.visit::<IconButton, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
        let d = find(cx, "dropdownBotDifficulty");
        cx.visit::<DropMenu, _>(d, |d, _| d.on_click.add(Handler { owner: me, id: P_DIFFICULTY }));
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        Self::update_add_button_visibility(cx, false);
        let l = self.leader_id.clone();
        self.update_leader_info(&l);
        self.update_appearance(cx);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = Self::btn_interactor(cx, "IconButton");
        bind(cx, i, |s| &mut s.on_focus, P_ICON_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, P_ICON_UNFOCUS);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            R_SHOW_CHARACTER if !Self::selectors_active(cx) => self.show_character_selector(cx),
            R_SHOW_SKIN if !Self::selectors_active(cx) => self.show_skin_selector(cx),
            _ => {}
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let _ = arg;
        match id {
            P_BOT_FOCUS | P_INVITE_FOCUS => Self::update_add_button_visibility(cx, true),
            P_BOT_UNFOCUS | P_INVITE_UNFOCUS => Self::update_add_button_visibility(cx, false),
            P_INVITE_CLICK => {
                cx.emit(self, |c| &c.invite_player_clicked, &());
                Self::update_add_button_visibility(cx, false);
            }
            P_BOT_CLICK => self.pick_bot(cx),
            P_CHARACTER_SELECTED => {
                if let Some((c, random)) = arg.downcast_ref::<(RosterEntry, bool)>() {
                    self.on_character_selected(cx, c, *random);
                }
            }
            P_CHARACTER_CANCELED => {
                let p = self.picker(cx);
                interactor::set_focused(cx, p, true);
            }
            P_SKIN_SELECTED => {
                if let Some((character, skin)) = arg.downcast_ref::<(String, SkinInfo)>() {
                    // (the pick's MatchPlayer.skin: '' for the port's default)
                    let name = picked_skin(cx.view.fighters().get(character), &skin.name);
                    self.bot_picked(cx, character, &name);
                }
            }
            P_SKIN_CANCELED => self.show_character_selector(cx),
            P_ICON_CLICK => {
                if self.player.as_ref().is_some_and(|p| p.is_bot) {
                    self.pick_bot(cx);
                }
            }
            P_SWITCH_CLICK => {
                let a = (self.account_id().to_string(), cx.w);
                cx.emit(self, |c| &c.switch_team_clicked, &a);
                let s = Self::btn_interactor(cx, "buttonSwitchTeam");
                interactor::set_interactable(cx, s, false);
            }
            P_PROMOTE_CLICK => {
                let a = self.account_id().to_string();
                cx.emit(self, |c| &c.promote_player_clicked, &a);
            }
            // a bot goes at once; a human after a confirmation dialog (ConfirmKick). The build also destroys the
            // row here; the lobby's echo removes it.
            P_KICK_CLICK => {
                let a = self.account_id().to_string();
                if self.player.as_ref().is_some_and(|p| p.is_bot) {
                    cx.emit(self, |c| &c.kick_player_clicked, &a);
                    return;
                }
                self.pending_kick = a;
                let d = Manager::show_dialog(
                    cx,
                    DialogParams {
                        title: "REALLY KICK PLAYER?".into(),
                        button_left_label: "OK".into(),
                        button_right_label: "CANCEL".into(),
                        ..DialogParams::default()
                    },
                );
                let me = cx.w;
                if let Some(dl) = cx.view.dialog(d) {
                    dl.button_left_clicked().add(Handler { owner: me, id: P_KICK_CONFIRMED });
                }
            }
            P_KICK_CONFIRMED => {
                let a = std::mem::take(&mut self.pending_kick);
                cx.emit(self, |c| &c.kick_player_clicked, &a);
            }
            P_DIFFICULTY => {
                let Some((index, _)) = arg.downcast_ref::<(i32, String)>() else { return };
                let Some(p) = self.player.clone() else { return };
                let levels = [2, 3, 4, 5, EXPERT_DIFFICULTY];
                let d = usize::try_from(*index).ok().and_then(|i| levels.get(i).copied()).unwrap_or(p.difficulty);
                let a = (p.account_id, p.character, d, p.skin);
                cx.emit(self, |c| &c.on_bot_settings_changed, &a);
                let m = cx.view.tree().find_in(find(cx, "dropdownBotDifficulty"), "mainInteractor");
                interactor::set_interactable(cx, m, false);
            }
            P_ICON_FOCUS | P_ICON_UNFOCUS => {
                let w = find(cx, "WBP_Quadrilateral_126");
                wd(cx, w).color = if id == P_ICON_FOCUS { EDIT_FOCUSED } else { EDIT_IDLE };
            }
            _ => {}
        }
    }
}

// ---------- WBP_CustomGameTeam ----------

const T_KICK: u32 = 1;
const T_SWITCH: u32 = 2;
const T_BOT_SETTINGS: u32 = 3;
const T_PROMOTE: u32 = 4;

/// A team's column. The colour of TeamIndex -1 (the free-for-all column) comes from a Select without that case: the
/// tree's colour stays. Unverified.
#[derive(Debug, Default)]
pub struct CustomGameTeam {
    pub on_kick_player_clicked: Event<String>,
    pub on_join_team_clicked: Event<(String, WidgetId)>,
    pub on_bot_settings_change_requested: Event<(String, String, i32, String)>,
    pub on_promote_new_leader_clicked: Event<String>,
    pub cached_num_players: usize,
    /// (account id, row).
    players: Vec<(String, WidgetId)>,
    context: String,
    grid: Option<WidgetId>,
}

impl CustomGameTeam {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGameTeam::default())
    }

    fn team_index(cx: &Cx<'_>) -> i32 {
        super::controls::prop_f64(cx, "TeamIndex", 0.0) as i32
    }

    /// EmptySlot1..4.
    pub fn empty_slots(cx: &Cx<'_>) -> Vec<WidgetId> {
        (1..=4).map(|n| find(cx, &format!("EmptySlot{n}"))).collect()
    }

    #[allow(clippy::too_many_arguments)] // populate: one parameter per field of the Lobby call.
    pub fn populate(
        &mut self,
        cx: &mut Cx<'_>,
        can_add_bots: bool,
        leader_id: &str,
        players: &[LobbyPlayer],
        team_index: i32,
        available_slots: usize,
        facts: &Facts,
    ) {
        if team_index != -1 && team_index != Self::team_index(cx) {
            return;
        }
        let gone: Vec<WidgetId> =
            self.players.iter().filter(|(id, _)| !players.iter().any(|p| p.account_id == *id)).map(|x| x.1).collect();
        for w in gone {
            MenuView::discard(cx, w);
        }
        self.players.retain(|(id, _)| players.iter().any(|p| p.account_id == *id));
        for p in players {
            self.update_or_create_player_widget(cx, p, leader_id, facts);
        }
        self.cached_num_players = players.len();
        self.update_empty_slot_visibility(cx, can_add_bots, Some(available_slots));
    }

    // bots are ready; a human is by the lobby's list (ReadyUsers; lobby_rules ready)
    fn update_or_create_player_widget(&mut self, cx: &mut Cx<'_>, p: &LobbyPlayer, leader_id: &str, facts: &Facts) {
        let swap = super::controls::prop_bool(cx, "SwapTeamsEnabled");
        let ready = p.is_bot || facts.ready.contains(&p.account_id);
        let bx = find(cx, "CustomGamePlayers");
        if let Some(&(_, w)) = self.players.iter().find(|(id, _)| *id == p.account_id) {
            // A row SwitchTo moved to the other column's box whose JoinTeam the lobby refused (a full team): the
            // answer still has it here, so it comes back (the port's own: the build's answer to a refused JoinTeam
            // is not decoded).
            if cx.view.tree().widget(w).parent != Some(bx) {
                cx.view.tree_mut().remove(w);
                wd(cx, w).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 40.0 };
                cx.view.tree_mut().add(bx, w);
            }
            let (p2, l2, f2) = (p.clone(), leader_id.to_string(), facts.as_row());
            cx.visit::<CustomGamePlayer, _>(w, |c, cx| c.update_data(cx, p2, &l2, ready, swap, f2));
            return;
        }
        let w = MenuView::create(cx, "WBP_CustomGamePlayer");
        let (p2, l2, f2) = (p.clone(), leader_id.to_string(), facts.as_row());
        cx.visit::<CustomGamePlayer, _>(w, |c, _| {
            c.slot_is_empty = false;
            c.player = Some(p2);
            c.leader_id = l2;
            c.is_ready = ready;
            c.can_swap_teams = swap;
            c.facts = f2;
        });
        self.players.push((p.account_id.clone(), w));
        let ctx = self.context.clone();
        cx.visit::<CustomGamePlayer, _>(w, |c, cx| c.update_context(cx, &ctx));
        wd(cx, w).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 40.0 };
        MenuView::add_child(cx, bx, w, None);
        let me = cx.w;
        cx.visit::<CustomGamePlayer, _>(w, |c, _| {
            c.kick_player_clicked.add(Handler { owner: me, id: T_KICK });
            c.switch_team_clicked.add(Handler { owner: me, id: T_SWITCH });
            c.on_bot_settings_changed.add(Handler { owner: me, id: T_BOT_SETTINGS });
            c.promote_player_clicked.add(Handler { owner: me, id: T_PROMOTE });
        });
    }

    pub fn update_empty_slot_visibility(&mut self, cx: &mut Cx<'_>, can_add_bots: bool, available: Option<usize>) {
        let available = available.unwrap_or(0);
        let max = super::controls::prop_f64(cx, "MaxPlayers", 2.0) as i64;
        let open = (max - self.cached_num_players as i64).min(available as i64);
        for (i, s) in Self::empty_slots(cx).into_iter().enumerate() {
            wd(cx, s).vis = if open > i as i64 { Vis::Visible } else { Vis::Collapsed };
            let b = cx.view.tree().find_in(cx.view.tree().find_in(s, "btn_InviteBot"), "UI_Interactor");
            interactor::set_interactable(cx, b, can_add_bots);
        }
        self.place_cells(cx);
    }

    /// Deviation: the free-for-all column's rows and empty slots in two columns of two, as the two team columns hold
    /// theirs, not the build's one column of four as wide as both (the port's own layout: room for the lobby's chat
    /// under them). The box keeps the tree's name and place; the slots move into it.
    pub fn as_grid(&mut self, cx: &mut Cx<'_>) {
        let old = find(cx, "CustomGamePlayers");
        let Some(parent) = cx.view.tree().widget(old).parent else { return };
        let bundle = cx.view.bundle.clone();
        let grid = cx.view.tree_mut().create_plain(&bundle, "UniformGridPanel", "CustomGamePlayers");
        let (pad, ha, va, fill) = {
            let o = cx.view.tree().widget(old);
            (o.pad, o.h_align, o.v_align, o.fill)
        };
        let at = cx.view.tree().widget(parent).children.iter().position(|c| *c == old);
        {
            let g = wd(cx, grid);
            g.pad = Margin { l: pad.l - GRID_GAP / 2.0, t: pad.t, r: pad.r - GRID_GAP / 2.0, b: pad.b };
            g.h_align = ha;
            g.v_align = va;
            g.fill = fill;
            g.slot_pad = Margin { l: GRID_GAP / 2.0, t: 0.0, r: GRID_GAP / 2.0, b: 40.0 };
        }
        MenuView::add_child(cx, parent, grid, at);
        cx.view.tree_mut().remove(old);
        for s in Self::empty_slots(cx) {
            cx.view.tree_mut().add(grid, s);
        }
        self.grid = Some(grid);
    }

    /// The shown rows, then the shown empty slots, row by row.
    fn place_cells(&self, cx: &mut Cx<'_>) {
        if self.grid.is_none() {
            return;
        }
        let mut cells: Vec<WidgetId> = self.players.iter().map(|p| p.1).collect();
        cells.extend(Self::empty_slots(cx));
        cells.retain(|w| cx.view.tree().widget(*w).vis != Vis::Collapsed);
        for (i, w) in cells.into_iter().enumerate() {
            let x = wd(cx, w);
            x.row = (i / 2) as i32;
            x.col = (i % 2) as i32;
            x.h_align = HAlign::Fill;
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let mut all = Self::empty_slots(cx);
        all.extend(self.players.iter().map(|p| p.1));
        for w in all {
            cx.visit::<CustomGamePlayer, _>(w, |c, cx| c.update_context(cx, ctx));
        }
    }
}

impl Controller for CustomGameTeam {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let t = Self::team_index(cx);
        let title = find(cx, "TeamTitle");
        let team = t == 0 || t == 1;
        {
            let w = wd(cx, title);
            w.text = if team { format!("Team {}", t + 1) } else { "Players".into() };
            // Deviation: the free-for-all title keeps the teams' left space of 20; the build's (Left 0) has it flush
            // against the bar's edge.
            w.pad = Margin { l: 20.0, t: 10.0, r: 0.0, b: 10.0 };
        }
        if let Some(kind) = usize::try_from(t).ok().and_then(|t| [0u8, 1, 3, 4].get(t).copied()) {
            let img = find(cx, "TeamColorImage");
            let c = palette_entry(cx, kind);
            wd(cx, img).color = c;
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            T_KICK => {
                if let Some(a) = arg.downcast_ref::<String>() {
                    cx.emit(self, |c| &c.on_kick_player_clicked, a);
                }
            }
            T_SWITCH => {
                if let Some(a) = arg.downcast_ref::<(String, WidgetId)>() {
                    cx.emit(self, |c| &c.on_join_team_clicked, a);
                }
            }
            T_BOT_SETTINGS => {
                if let Some(a) = arg.downcast_ref::<(String, String, i32, String)>() {
                    cx.emit(self, |c| &c.on_bot_settings_change_requested, a);
                }
            }
            T_PROMOTE => {
                if let Some(a) = arg.downcast_ref::<String>() {
                    cx.emit(self, |c| &c.on_promote_new_leader_clicked, a);
                }
            }
            _ => {}
        }
    }
}

// ---------- WBP_CustomGameMapThumbnail ----------

/// A map of the lobby's rotation: display only (no click handler). Construct loads the soft MapData: the thumbnail
/// (the label stays Collapsed as in the tree) and the name as the tip of its own UI_TooltipTarget, shown while the
/// mouse is on the thumbnail.
#[derive(Debug, Default)]
pub struct CustomGameMapThumbnail {
    pub map_data: Option<MapInfo>,
}

impl CustomGameMapThumbnail {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGameMapThumbnail::default())
    }
}

impl Controller for CustomGameMapThumbnail {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let i = find(cx, "UI_Interactor");
        focus_frame(cx, fc, i);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let Some(m) = self.map_data.clone() else { return };
        let label = find(cx, "mapLabel");
        wd(cx, label).text = m.data.display_name.clone();
        // @590-897: Offset (45, 45), Center
        let tt = find(cx, "UI_TooltipTarget");
        let name = m.data.display_name.clone();
        cx.visit::<TooltipTarget, _>(tt, |t, _| t.params = tooltip_params(&name, true, 45.0, 45.0, 10));
        cx.view.load_texture(&m.data.texture); // SetBrushFromSoftTexture
        let icon = find(cx, "mapIcon");
        let w = wd(cx, icon);
        w.tex = Some(m.data.texture.clone());
        w.vis = Vis::Visible;
    }
}

// ---------- WBP_CustomGame ----------

/// The owner of the pictures the lobby holds ahead.
const PICTURES_LOBBY: &str = "lobby";
/// EMapRotation of the lobby's team style (0 1v1, 1 2v2, 2 ffa): CustomSolos, CustomDuos, CustomFreeForAll.
const LOBBY_ROTATIONS: [i32; 3] = [6, 5, 7];
const LOBBY_SELECTOR_COUNTS: [i32; 3] = [2, 4, 4];
/// EMapRotation names of the three custom rotations: the maps of the lobby's map selector.
const LOBBY_MAP_ROTATIONS: [&str; 3] = ["CustomSolos", "CustomDuos", "CustomFreeForAll"];
/// How far a lobby scroll box's clip reaches past its sides: room for a hover outline.
const CLIP_BLEED: f64 = 16.0;
const LOBBY_DROPS: [&str; 4] = ["UI_DropGameMode", "UI_DropRingouts", "UI_DropTime", "UI_DropMatches"];
const LOBBY_TEAMS: [&str; 3] = ["WBP_CustomGameTeam", "WBP_CustomGameTeam_1", "WBP_CustomGameTeam_ffa"];
const EXPERIMENTAL_MODAL: &str = "customGameExperimental";
const SETTINGS_MODAL: &str = "customGameSettings";

const G_SUBMIT3: u32 = 1;
const G_DROP_MODE: u32 = 2;
const G_DROP_RINGOUTS: u32 = 3;
const G_DROP_TIME: u32 = 4;
const G_HAZARDS: u32 = 5;
const G_EXPERIMENTAL: u32 = 6;
const G_LEAVE: u32 = 7;
const G_SELECT_MAPS: u32 = 8;
const G_START: u32 = 9;
const G_LEAVE_CONFIRMED: u32 = 10;
const G_OPTION: u32 = 11;
const G_COPY_CODE: u32 = 12;
const G_ROOM_SETTINGS: u32 = 13;
const G_INVITED: u32 = 14;
const G_KEY: u32 = 15;
const G_MAPS_SELECTED: u32 = 16;
/// (the port's own) The eye beside the room code: shows or masks it.
const G_EYE: u32 = 17;
/// The chat input's events (lobby_chat::H_*) arrive under these ids.
const G_CHAT: u32 = 0x200;
/// A team's events: `G_TEAM + team * 16 + kind`.
const G_TEAM: u32 = 0x100;
const K_KICK: u32 = 0;
const K_PROMOTE: u32 = 1;
const K_BOT_SETTINGS: u32 = 2;
const K_SWITCH: u32 = 3;
const K_ADD_BOT: u32 = 4;
const K_INVITE: u32 = 5;
const R_UNFOCUS_MODE: u32 = 1;
const R_LOADING_WAIT: u32 = 2;
const R_START: u32 = 3;
/// The code button's label and icon after its click (the button's controller is busy in its own click).
const R_SHOW_CODE: u32 = 4;

// the code button: CODE_TEXT is the port's own (asked for by the user)
const CODE_ICON: &str = "ui_icons_hyperlink";
const SETTINGS_ICON: &str = "ui_icons_settings";
const CODE_BUTTON_H: f64 = 72.0;
/// (the port's own) The room code button's icon (the port's own pictures, the build having no eye): the eye struck
/// through while the code is masked, open while shown.
const CODE_HIDDEN_ICON: &str = "port_icon_eye_off";
const CODE_SHOWN_ICON: &str = "port_icon_eye";
/// (the port's own) The gap between the code and the eye (Slate units).
const CODE_EYE_GAP: f64 = 8.0;

fn team_h(team: usize, kind: u32) -> u32 {
    G_TEAM + team as u32 * 16 + kind
}

/// WBP_CustomGame (modal customGame) over the lobby.
#[derive(Debug)]
pub struct CustomGame {
    base: ModalBase,
    lobby: Option<Lobby>,
    /// The page's top-right row (the room code and settings buttons), made once.
    top_row: Option<WidgetId>,
    code_button: Option<WidgetId>,
    /// (the port's own) The room code's button beside its copy button: the code masked until clicked, an eye icon.
    code_view: Option<WidgetId>,
    /// (the port's own) The code is shown, not masked: off each time the lobby opens.
    code_shown: bool,
    settings_button: Option<WidgetId>,
    /// (the port's own) The connection line left of the room code's buttons.
    link_line: Option<LinkLine>,
    /// (the port's own) The lobby is local as the controls last showed it (None: not shown yet).
    local_shown: Option<bool>,
    /// An online lobby's chat.
    chat: Option<LobbyChat>,
    /// online.connected as the last frame saw it (acted on at a change).
    was_connected: bool,
    loaded: bool,
    is_loading: bool,
    hiding: bool,
    map_thumbs: Vec<WidgetId>,
    /// CachedSelectedMaps.
    cached_selected_maps: Vec<String>,
    /// The ring-out drop's heading (CustomGames_2) as the tree has it (the stock rule shows its own).
    ringouts_label: String,
}

impl CustomGame {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGame {
            base: ModalBase::default(),
            lobby: None,
            top_row: None,
            code_button: None,
            code_view: None,
            code_shown: false,
            settings_button: None,
            link_line: None,
            local_shown: None,
            chat: None,
            was_connected: false,
            loaded: false,
            is_loading: false,
            hiding: false,
            map_thumbs: Vec::new(),
            cached_selected_maps: Vec::new(),
            ringouts_label: String::new(),
        })
    }

    /// The lobby's state of this moment, which the rows read.
    fn facts(&self) -> Facts {
        let l = self.lobby();
        Facts {
            my_id: l.my_id().to_string(),
            am_leader: l.am_i_leader(),
            ready: l.players().iter().filter(|p| l.is_ready(&p.account_id)).map(|p| p.account_id.clone()).collect(),
            ready_button: l.has_ready_button(),
        }
    }

    fn lobby(&self) -> &Lobby {
        self.lobby.as_ref().expect("the lobby is made at OnInitialized and kept until the match starts")
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        self.lobby.as_mut().expect("the lobby is made at OnInitialized and kept until the match starts")
    }

    /// (the port's own) An invite from outside the page (the social panel's "+"): the lobby's own invite, as the
    /// invite modal's choice makes it (its keeper message and the online invite).
    pub fn invite(&mut self, id: &str) {
        if let Some(l) = self.lobby.as_mut() {
            l.invite(id);
        }
    }

    /// The page's top-right row (TOP_BUTTONS_PAD), made once.
    fn top_row(&mut self, cx: &mut Cx<'_>) -> WidgetId {
        if let Some(r) = self.top_row {
            return r;
        }
        let p = TOP_BUTTONS_PAD;
        let row = lobby_chat::plain(
            cx,
            "HorizontalBox",
            "LobbyTopButtons",
            json!({}),
            json!({
                "HorizontalAlignment": "HAlign_Right",
                "VerticalAlignment": "VAlign_Top",
                "Padding": { "Left": p.l, "Top": p.t, "Right": p.r, "Bottom": p.b },
            }),
            "OverlaySlot",
        );
        let overlay = find(cx, "Overlay_128");
        MenuView::add_child(cx, overlay, row, None);
        self.top_row = Some(row);
        row
    }

    /// A ButtonSecondary of the lobby's top row, made like LEAVE (the code's and the settings'), collapsed until the
    /// lobby has a code.
    #[allow(clippy::too_many_arguments)] // the code and settings buttons: one body, the fields differ.
    fn top_button(
        &mut self,
        cx: &mut Cx<'_>,
        name: &str,
        label: &str,
        icon: &str,
        at: usize,
        gap: f64,
        click: u32,
    ) -> WidgetId {
        let w = MenuView::create(cx, "UI_ButtonSecondary");
        let leave = find(cx, "CancelCustomGameButton");
        // (the button's own class defaults, LEAVE's font size and sounds: only these are assigned)
        let leave_props = cx.view.tree().props(leave).clone();
        let mut set = vec![
            ("icon", json!(icon)),
            ("IsMirrored", json!(true)),
            ("buttonLabel", json!({ "CultureInvariantString": label })),
            ("LabelPadding", json!({ "Left": 20, "Top": 0, "Right": 20, "Bottom": 0 })),
        ];
        for k in ["FontSize", "Hover Sound Override", "Confirm Sound Override"] {
            if let Some(v) = leave_props.get(k) {
                set.push((k, v.clone()));
            }
        }
        for (k, v) in set {
            cx.view.tree_mut().set_prop(w, k, v);
        }
        {
            let x = wd(cx, w);
            x.name = name.to_string();
            x.v_align = VAlign::Top;
            x.pad = Margin { l: gap, t: 0.0, r: 0.0, b: 0.0 };
            x.vis = Vis::Collapsed;
        }
        let row = self.top_row(cx);
        MenuView::add_child(cx, row, w, Some(at));
        let sb = cx.view.tree().find_in(w, "SizeBox_0");
        wd(cx, sb).h_override = CODE_BUTTON_H;
        let me = cx.w;
        cx.visit::<ButtonSecondary, _>(w, |b, cx| {
            b.set_label(cx, label);
            b.on_click.add(Handler { owner: me, id: click });
        });
        w
    }

    fn add_code_button(&mut self, cx: &mut Cx<'_>) {
        self.code_button =
            Some(self.top_button(cx, "LobbyCodeButton", custom_text::COPY_ROOM_CODE, CODE_ICON, 0, 0.0, G_COPY_CODE));
    }

    /// (the port's own) The room code's button beside its copy button, made like it: the code masked as one dot a
    /// character and a struck-through eye, a click shows it in capitals with the eye open (the code is a key to the
    /// room: it stays off a streamed or shared screen until asked for).
    fn add_code_view(&mut self, cx: &mut Cx<'_>) {
        let b = self.top_button(cx, "LobbyCodeView", "", CODE_HIDDEN_ICON, 1, TOP_GAP, G_EYE);
        // (a mirrored ButtonSecondary collapses both icon spacers: the eye gets its gap from the code here)
        let icon = cx.view.tree().find_in(b, "IconSize_R");
        wd(cx, icon).pad.l = CODE_EYE_GAP;
        self.code_view = Some(b);
    }

    /// (the port's own) The code button's label, masked or shown, and its eye.
    fn show_code_text(&mut self, cx: &mut Cx<'_>) {
        let Some(b) = self.code_view else { return };
        let code = self.lobby().net().map_or("", NetLobby::code).to_uppercase();
        let shown = self.code_shown;
        let label = if shown { code } else { "\u{2022}".repeat(code.chars().count()) };
        let icon = if shown { CODE_SHOWN_ICON } else { CODE_HIDDEN_ICON };
        cx.visit::<ButtonSecondary, _>(b, |b, cx| {
            b.set_label(cx, &label);
            b.set_icon(cx, Some(icon.to_string()));
        });
    }

    /// An online lobby's settings open from this button beside the code's; a local lobby has no settings.
    fn add_settings_button(&mut self, cx: &mut Cx<'_>) {
        self.settings_button = Some(self.top_button(
            cx,
            "LobbySettingsButton",
            custom_text::ROOM_SETTINGS,
            SETTINGS_ICON,
            2,
            TOP_GAP,
            G_ROOM_SETTINGS,
        ));
    }

    /// (the port's own) The connection line under the page's title: one row of runs (the state, what the lobby is
    /// connected through or why a local lobby is local), and over it a tooltip
    /// target whose tip says what to do for a local lobby to be online.
    /// Deviation: the build's lobby shows no connection (WBP_CustomGame has the room code alone); the port says it,
    /// as a lobby may be local, on a player's game or on the relay.
    fn add_link_line(&mut self, cx: &mut Cx<'_>) {
        let header = find(cx, "WBP_MenuHeader");
        let banner = cx.view.tree().find_in(header, "HeaderBanner");
        let root = lobby_chat::plain(
            cx,
            "Overlay",
            "LobbyConnection",
            json!({}),
            json!({
                "HorizontalAlignment": "HAlign_Left",
                "VerticalAlignment": "VAlign_Top",
                "Padding": { "Left": LINK_LEFT, "Top": LINK_TOP, "Right": 0, "Bottom": 0 },
            }),
            "OverlaySlot",
        );
        MenuView::add_child(cx, banner, root, None);
        let bundle = cx.view.bundle.clone();
        let t = cx.view.tree_mut();
        let row = t.create_plain(&bundle, "HorizontalBox", "LobbyConnectionRow");
        let runs = ["LobbyConnectionHead", "LobbyConnectionDetail"].map(|n| t.create_plain(&bundle, "TextBlock", n));
        for (i, &w) in runs.iter().enumerate() {
            let x = t.widget_mut(w);
            x.typeface = Some(LINK_FACE.to_owned());
            x.font_size = LINK_SIZE;
            x.color = LINK_TEXT;
            x.v_align = VAlign::Bottom;
            x.pad = Margin { l: if i == 0 { 0.0 } else { LINK_GAP }, t: 0.0, r: 0.0, b: 0.0 };
            x.vis = Vis::HitTestInvisible;
            t.add(row, w);
        }
        t.widget_mut(row).vis = Vis::HitTestInvisible;
        t.add(root, row);
        t.widget_mut(root).vis = Vis::Collapsed;
        let tip = MenuView::create(cx, "UI_TooltipTarget");
        {
            let x = wd(cx, tip);
            x.name = "LobbyConnectionTip".to_owned();
            x.h_align = HAlign::Fill;
            x.v_align = VAlign::Fill;
        }
        MenuView::add_child(cx, root, tip, None);
        let [head, detail] = runs;
        self.link_line = Some(LinkLine { root, head, detail, tip, shown: None });
    }

    /// (the port's own) The connection line as the session has it now, and a local lobby's controls greyed (the room
    /// code's buttons, the room settings with the public listing, the invites) when the lobby turns local or back.
    fn show_link(&mut self, cx: &mut Cx<'_>) {
        if self.lobby.is_none() {
            return;
        }
        let o = &cx.view.session.online;
        let link = lobby_link(self.lobby(), o);
        let local = matches!(link, Some(SessionLink::Local(_)));
        if self.local_shown != Some(local) {
            self.local_shown = Some(local);
            self.show_code(cx);
        }
        let Some(l) = self.link_line.as_mut() else { return };
        if l.shown == link {
            return;
        }
        l.shown.clone_from(&link);
        let (root, head, detail, tip) = (l.root, l.head, l.detail, l.tip);
        let Some(link) = link else {
            wd(cx, root).vis = Vis::Collapsed;
            return;
        };
        wd(cx, root).vis = Vis::SelfHitTestInvisible;
        let line = connection_line::line(&link);
        let h = wd(cx, head);
        h.text = line.head.to_owned();
        h.color = match line.state {
            LineState::Online | LineState::LocalNetwork => LINK_ONLINE,
            LineState::Offline => LINK_LOCAL,
        };
        wd(cx, detail).text = line.detail;
        // the tip: what to do, for a local lobby alone (an online lobby's target takes no hover)
        let how = line.how.unwrap_or_default();
        wd(cx, tip).vis = if how.is_empty() { Vis::Collapsed } else { Vis::Visible };
        cx.visit::<TooltipTarget, _>(tip, |t, _| t.params = tooltip_params(&how, !how.is_empty(), 0.0, 8.0, 0));
    }

    /// (the port's own) The lobby is local: its code, settings and invites are off.
    fn local(&self) -> bool {
        self.local_shown == Some(true)
    }

    /// The lobby-code hook for the menu tapes: the code button shown with `text` as its label.
    #[doc(hidden)]
    pub fn force_code(&mut self, cx: &mut Cx<'_>, text: &str) {
        let Some(b) = self.code_button else { return };
        cx.visit::<ButtonSecondary, _>(b, |b, cx| b.set_label(cx, text));
        wd(cx, b).vis = Vis::Visible;
    }

    fn show_code(&mut self, cx: &mut Cx<'_>) {
        let Some(code) = self.code_button else { return };
        // (the port's own) a local lobby's code and settings buttons stay in view, greyed, with the connection line's
        // reason beside them; it has no code to show
        let local = self.local();
        let has = local || self.lobby().net().is_some_and(|n| !n.code().is_empty());
        let vis = if has { Vis::Visible } else { Vis::Collapsed };
        for b in [Some(code), self.settings_button].into_iter().flatten() {
            wd(cx, b).vis = vis;
            let i = cx.view.tree().find_in(b, "UI_Interactor");
            interactor::set_interactable(cx, i, !local);
        }
        if let Some(b) = self.code_view {
            wd(cx, b).vis = if has && !local { Vis::Visible } else { Vis::Collapsed };
        }
        for i in 0..LOBBY_TEAMS.len() {
            let team = Self::team(cx, i);
            for n in 1..=4 {
                let slot = cx.view.tree().find_in(team, &format!("EmptySlot{n}"));
                let b = cx.view.tree().find_in(slot, "btn_InvitePlayer");
                let b = cx.view.tree().find_in(b, "UI_Interactor");
                interactor::set_interactable(cx, b, !local);
            }
        }
        self.show_code_text(cx);
    }

    /// The chat under the player cards, which give up the room they do not fill.
    fn add_chat(&mut self, cx: &mut Cx<'_>) {
        let log: Vec<_> = self.lobby().net().map(|n| n.chat.clone()).unwrap_or_default();
        let bx = find(cx, "VerticalBox_0");
        let players = find(cx, "AddPlayerScrollContainer");
        wd(cx, players).fill = 0.0;
        let chat = LobbyChat::new(cx, &log);
        let at = cx.view.tree().widget(bx).children.iter().position(|&c| c == players).map(|i| i + 1);
        MenuView::add_child(cx, bx, chat.root, at);
        self.chat = Some(chat);
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
    }

    /// Enter opens the chat's input (the menu's Enter, a click of the focused button, is not the lobby's while it
    /// has a chat: Space still clicks). Not while a modal is over the lobby or a box has the keys.
    fn open_chat(&mut self, cx: &mut Cx<'_>) {
        let Some(c) = self.chat.take() else { return };
        let latest = Manager::latest_modal_id(cx.view);
        let blocked = (!latest.is_empty() && latest != self.base.modal_id) || c.typing(cx);
        if !blocked {
            c.open(cx);
        }
        self.chat = Some(c);
    }

    /// The input committed `text` with Enter: the lobby's gate says if it goes. The gate's time is online play's
    /// clock: the lobby and its budget outlive this page (a match, then a new menu whose clock starts again).
    fn send_chat(&mut self, cx: &mut Cx<'_>, chat: &mut LobbyChat, text: &str) {
        let now = cx.view.session.online.now;
        let Some(n) = self.lobby_mut().net_mut() else { return };
        match n.say(text, now) {
            Say::Sent(_) | Say::Nothing => chat.sent(cx),
            Say::TooFast => chat.shake(),
        }
    }

    /// ShowInviteDialog: the invite modal over the lobby; each selected player is invited (BPI_InviteToCustomGame:
    /// InvitePlayerToCurrentCustomGameLobby, with nothing shown to the sender on success).
    fn show_invite_dialog(&mut self, cx: &mut Cx<'_>) {
        if cx.view.manager.get_modal("socialInvite").is_some() {
            return;
        }
        let Some(w) = Manager::show_modal(cx, "UI_SocialInviteModal", "socialInvite", "customGame", "") else {
            return;
        };
        // (the port's) the lobby's members are not listed
        let ids: Vec<String> =
            self.lobby().teams().iter().flatten().filter(|p| !p.is_bot).map(|p| p.account_id.clone()).collect();
        let me = cx.w;
        cx.visit::<SocialInviteModal, _>(w, |m, cx| {
            m.set_excluded(cx, ids);
            m.selections_made_event.add(Handler { owner: me, id: G_INVITED });
        });
    }

    /// An invitation accepted while this lobby's screen is up: the online lobby moves to the other one.
    pub fn join_invited(&mut self, cx: &mut Cx<'_>, lobby_id: &str) -> bool {
        if !self.lobby.as_ref().and_then(Lobby::net).is_some_and(NetLobby::alive) {
            return false;
        }
        Self::leave_full_party(cx);
        self.toggle_loading(cx, true, true);
        Self::set_start_interactable(cx, false);
        if let Some(n) = self.lobby_mut().net_mut() {
            n.join(lobby_id);
        }
        self.flush(cx);
        true
    }

    /// PGI OnJoinCustomLobbyResult @109848-110106: a full party (GetPartyData.IsPartyFull) is left
    /// (PartyManager.RemakeParty) before the lobby's setup, with no dialog; the party ends for the other member,
    /// who is told "Party Disbanded" (hub.rs).
    fn leave_full_party(cx: &mut Cx<'_>) {
        if cx.view.session.online.party_full() {
            cx.online(OnlineCmd::Party(PartyCmd::Leave));
        }
    }

    fn drop_menu(cx: &Cx<'_>, name: &str) -> WidgetId {
        find(cx, name)
    }

    fn main_interactor(cx: &Cx<'_>, drop: &str) -> WidgetId {
        cx.view.tree().find_in(Self::drop_menu(cx, drop), "mainInteractor")
    }

    fn start_interactor(cx: &Cx<'_>) -> WidgetId {
        cx.view.tree().find_in(find(cx, "Btn_StartMatch"), "UI_Interactor")
    }

    fn hazards_interactor(cx: &Cx<'_>) -> WidgetId {
        cx.view.tree().find_in(find(cx, "Toggle_Hazards"), "UI_Interactor")
    }

    fn set_start_interactable(cx: &mut Cx<'_>, on: bool) {
        let i = Self::start_interactor(cx);
        interactor::set_interactable(cx, i, on);
    }

    /// (the port's own, a script's `do lobby`) the lobby as the Start button sees it, one line.
    pub fn probe(&self, cx: &Cx<'_>) -> String {
        let Some(l) = self.lobby.as_ref() else { return "no lobby".into() };
        let i = Self::start_interactor(cx);
        let start = cx.view.manager.interactor(i).is_some_and(|s| s.get_interactable());
        let team = |t: &[LobbyPlayer]| t.iter().map(|p| p.account_id.as_str()).collect::<Vec<_>>().join(",");
        format!(
            "me={} leader={} style={} teams=[{}] [{}] ready=[{}] maps={} blockers={:?} can={} start_on={start}",
            l.my_id(),
            l.leader_id(),
            l.team_style(),
            team(&l.teams()[0]),
            team(&l.teams()[1]),
            l.rules().ready.join(","),
            l.enabled_maps().len(),
            l.get_start_match_blockers(),
            l.can_start_match(),
        )
    }

    fn team(cx: &Cx<'_>, i: usize) -> WidgetId {
        find(cx, LOBBY_TEAMS[i])
    }

    /// A leader request: the Start button off until the lobby's echo (update_start_match_enabled).
    /// Deviation: the build puts the loading cover up until the echo (ToggleLoading(true, true)), whose LoadingDone
    /// fades the whole lobby in again on every change; here the lobby stays as it is and only the rows the echo
    /// changes are redrawn (user request).
    fn action(cx: &mut Cx<'_>) {
        Self::set_start_interactable(cx, false);
    }

    // ---- the echo ----

    fn on_event(&mut self, cx: &mut Cx<'_>, e: LobbyEvent) {
        match e {
            LobbyEvent::LobbyStarted => {
                self.update_teams(cx);
                self.update_start_match_enabled(cx);
                self.update_is_leader(cx);
            }
            LobbyEvent::TeamsUpdated => {
                self.update_teams(cx);
                self.update_start_match_enabled(cx);
            }
            LobbyEvent::TeamStyleUpdated(style) => {
                self.update_ringouts(cx, Some(style == STYLE_FFA));
                let id = TEAM_STYLE_IDS.get(usize::from(style)).copied().unwrap_or("");
                let d = Self::drop_menu(cx, "UI_DropGameMode");
                cx.visit::<DropMenu, _>(d, |d, cx| d.set_active_by_id(cx, id));
                self.update_teams(cx);
                self.update_start_match_enabled(cx);
                let ctx = self.base.context.clone();
                self.update_context(cx, &ctx);
                self.update_is_leader(cx);
            }
            LobbyEvent::MatchDurationUpdated(sec) => {
                let d = Self::drop_menu(cx, "UI_DropTime");
                cx.visit::<DropMenu, _>(d, |d, cx| d.set_active_by_id(cx, &sec.to_string()));
            }
            LobbyEvent::NumRingoutsForWinUpdated(_) => self.update_ringouts(cx, None),
            LobbyEvent::AllowHazardsUpdated(b) => {
                let t = find(cx, "Toggle_Hazards");
                cx.visit::<ToggleSwitch, _>(t, |t, cx| t.set_on(cx, b));
            }
            LobbyEvent::EnabledMapsUpdated(maps) => self.update_maps(cx, &maps),
            LobbyEvent::OptionsUpdated => {
                self.refresh_settings(cx);
                self.update_ringouts(cx, None);
            }
            // OnStartMatchResult false (CG@23044-23174) and the failed party call (CG@22531-22695): the fade back
            // in, the Game Mode drop on again
            LobbyEvent::StartMatchFailed => {
                Manager::transition(
                    cx,
                    TransitionParams { kind: 1, level_options: "Lobby".into(), ..TransitionParams::default() },
                );
                let leader = self.lobby().am_i_leader();
                let m = Self::main_interactor(cx, "UI_DropGameMode");
                interactor::set_interactable(cx, m, leader);
                self.update_start_match_enabled(cx);
            }
            // The match start. Every client's Matchmaking_HandleOnRematchStarted (PGI_MatchmakingHandler
            // MH@40271): CleanupForRematch, Delay(1.0) MH@29602, then MH@562 UnfocusAll(0) and Disable All
            // Interactors, BPI_OpenLevel(PreMatch) MH@2015 and BPI_RemoveCustomGameUI MH@3220 (DismissModal
            // customGame). Not ported: Disable All Interactors (the Start button is off since its click).
            // The Delay(1.0) runs on the menu's clock.
            LobbyEvent::MatchStarting => {
                let me = cx.w;
                cx.view.delay(1.0, Handler { owner: me, id: R_START });
            }
            LobbyEvent::ReadyPlayersUpdated => {
                self.update_teams(cx);
                self.update_start_match_enabled(cx);
            }
            LobbyEvent::NewLeader(id) => {
                if id == self.lobby().my_id() {
                    Self::note(cx, custom_text::NOTE_NEW_LEADER, "");
                }
                self.update_is_leader(cx);
                self.update_teams(cx);
                self.update_start_match_enabled(cx);
                self.refresh_settings(cx);
            }
            LobbyEvent::PlayerKicked(p) => {
                if !p.is_bot {
                    Self::note(cx, custom_text::NOTE_PLAYER_KICKED, "");
                }
                self.update_teams(cx);
                self.update_start_match_enabled(cx);
            }
            LobbyEvent::Kicked => {
                Self::note(cx, custom_text::NOTE_KICKED, "");
                let id = self.base.modal_id.clone();
                Manager::dismiss_modal(cx, &id);
            }
            // OnCustomGameError: DisplayError(Text, true), a notification titled "Error!". An error before the
            // lobby's first answer (an invitation that no longer stands) leaves no lobby to show.
            LobbyEvent::Error(n) => {
                let title = cx.view.game_text(super::main_menu::NOTICE_TITLE).to_string();
                Self::note(cx, &title, &note_text(&n));
                if self.loaded {
                    self.update_teams(cx);
                    self.update_start_match_enabled(cx);
                } else {
                    let id = self.base.modal_id.clone();
                    Manager::dismiss_modal(cx, &id);
                }
            }
            // Deviation: the build's handling of a lost connection is native; the text is the port's.
            LobbyEvent::Closed => {
                let title = cx.view.game_text(super::main_menu::NOTICE_TITLE).to_string();
                Self::note(cx, &title, custom_text::LOST_CONNECTION);
                let id = self.base.modal_id.clone();
                Manager::dismiss_modal(cx, &id);
            }
            LobbyEvent::Chat(line) => {
                let kept = self.lobby().net().map_or(0, |n| n.chat.len());
                if let Some(c) = self.chat.as_mut() {
                    c.add(cx, &line, kept);
                }
            }
        }
    }

    fn note(cx: &mut Cx<'_>, title: &str, body: &str) {
        Manager::show_notification(
            cx,
            NotificationParams { title: title.to_string(), body: body.to_string(), ..NotificationParams::default() },
        );
    }

    /// The commands the lobby queued, to the program.
    fn flush(&mut self, cx: &mut Cx<'_>) {
        if let Some(l) = self.lobby.as_mut() {
            for c in l.take_cmds() {
                cx.online(c);
            }
        }
    }

    /// The pre-match screen handed to the program, the lobby with it (m.openLevelEvent.fire('PreMatch', 'Lobby')).
    fn begin_match(&mut self, cx: &mut Cx<'_>) {
        Manager::unfocus_all(cx, 0);
        // An online lobby's start is the server's pre-match begin: the lobby is kept for the screen and its return,
        // the picks and the match are the net session's (the program's).
        if let Some(Lobby::Net(mut n)) = self.lobby.take_if(|l| l.online()) {
            n.detach();
            lobby::keep_net(n);
            Manager::dismiss_all_modals(cx);
            Manager::open_level_now(cx, "PreMatch", "Lobby");
            return;
        }
        if let Some(Lobby::Local(mut lobby)) = self.lobby.take() {
            let mut state = (cx.view.manager.random() * 4_294_967_296.0) as u64 | 1;
            let mut random = move || {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
            };
            let pool = cx.view.bundle.menu.as_ref().map(|m| BotPool::of(&m.cosmetics)).unwrap_or_default();
            let drawable = |c: &str| cx.view.fighters().contains_key(c);
            let src = SetupSource {
                drawable: &drawable,
                hydra: &cx.prefs.hydra,
                input: &cx.prefs.input,
                roster: cx.view.roster(),
                bot_pool: &pool,
            };
            let setup = lobby.pre_match_setup(&src, &mut random);
            lobby.detach();
            lobby::with_carry(|c| c.pre_match = Some(Box::new(PreMatchStart { setup, lobby: Some(lobby) })));
        }
        Manager::open_level_now(cx, "PreMatch", "Lobby");
    }

    // ---- the page ----

    fn fill(cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_242");
        fill_page(cx, b);
    }

    /// UpdateContext.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let mut list = Vec::new();
        for n in [
            "CancelCustomGameButton",
            "ResetToDefaultsButton",
            "SelectMapsButton",
            "Button_JoinSpectators",
            "Button_JoinPlayers",
        ] {
            let b = find(cx, n);
            list.push(cx.view.tree().find_in(b, "UI_Interactor"));
        }
        for n in LOBBY_DROPS {
            let d = Self::drop_menu(cx, n);
            if let Some(v) = cx.visit_dyn(d, |_, cx| DropMenu::interactors(cx)) {
                list.extend(v);
            }
        }
        for b in [self.code_button, self.code_view, self.settings_button].into_iter().flatten() {
            list.push(cx.view.tree().find_in(b, "UI_Interactor"));
        }
        if let Some(c) = &self.chat {
            list.push(c.interactor(cx));
        }
        list.push(Self::start_interactor(cx));
        list.push(Self::hazards_interactor(cx));
        batch_set_context(cx, &list, ctx);
        let nav = find(cx, "WBP_GlobalNav_CustomGame");
        cx.visit::<GlobalNav, _>(nav, |n, cx| n.update_context_global_nav(cx, ctx, true, false));
        let social = find(cx, "UI_SocialPanel_CustomGame");
        cx.visit::<SocialPanel, _>(social, |p, cx| p.update_context(cx, ctx));
        for other in live(cx, "WBP_GlobalNav").into_iter().filter(|&o| o != nav) {
            cx.visit::<GlobalNav, _>(other, |n, _| n.refresh_context_paused = true);
        }
        for i in 0..3 {
            let t = Self::team(cx, i);
            cx.visit::<CustomGameTeam, _>(t, |t, cx| t.update_context(cx, ctx));
        }
    }

    fn set_ringouts_label(cx: &mut Cx<'_>, t: &str) {
        let d = Self::drop_menu(cx, "UI_DropRingouts");
        for n in ["WrapSelectionLabel", "AutoWrapSelectionLabel"] {
            let l = cx.view.tree().find_in(d, n);
            wd(cx, l).text = t.to_string();
        }
    }

    /// The ring-out drop for the lobby's style and rules: Free For All's "Highest Score Wins", locked, or the count.
    /// (the port's own) Under the experimental stock rule the count is each player's lives in every style, Free For
    /// All too, and the drop's label is the rule's word.
    fn update_ringouts(&mut self, cx: &mut Cx<'_>, ffa: Option<bool>) {
        let (style, stocks, count, leader) = {
            let l = self.lobby();
            (l.team_style(), l.options().stocks, l.num_ringouts_for_win(), l.am_i_leader())
        };
        let ffa = ffa.unwrap_or(style == STYLE_FFA);
        let heading = find(cx, "CustomGames_2");
        if self.ringouts_label.is_empty() {
            self.ringouts_label = cx.view.tree().widget(heading).text.clone();
        }
        wd(cx, heading).text = if stocks { custom_text::STOCKS.to_string() } else { self.ringouts_label.clone() };
        if ffa && !stocks {
            Self::set_ringouts_label(cx, "Highest Score Wins");
        } else {
            let d = Self::drop_menu(cx, "UI_DropRingouts");
            cx.visit::<DropMenu, _>(d, |d, cx| d.set_active_by_id(cx, &count.to_string()));
        }
        let m = Self::main_interactor(cx, "UI_DropRingouts");
        interactor::set_interactable(cx, m, leader && (!ffa || stocks));
    }

    fn set_game_mode_from_dropdown(&mut self, cx: &mut Cx<'_>, id: &str) {
        let style = TEAM_STYLE_IDS.iter().position(|s| *s == id);
        if let Some(style) = style
            && style as u8 != self.lobby().team_style()
        {
            self.lobby_mut().set_team_style(style as u8);
        }
        self.update_ringouts(cx, Some(id == "ffa"));
        for i in 0..2 {
            let t = Self::team(cx, i);
            cx.view.tree_mut().set_prop(t, "MaxPlayers", json!(if id == "1v1" { 1 } else { 2 }));
        }
        for i in 0..3 {
            let t = Self::team(cx, i);
            cx.visit::<CustomGameTeam, _>(t, |t, cx| t.update_empty_slot_visibility(cx, false, None));
        }
    }

    fn update_is_leader(&mut self, cx: &mut Cx<'_>) {
        let leader = self.lobby().am_i_leader();
        for n in ["UI_DropGameMode", "UI_DropTime"] {
            let m = Self::main_interactor(cx, n);
            interactor::set_interactable(cx, m, leader);
        }
        let h = Self::hazards_interactor(cx);
        interactor::set_interactable(cx, h, leader);
        self.update_ringouts(cx, None);
        set_vis(cx, "SelectMapsButton", if leader { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        // (the experimental settings button is every member's: custom_settings greys the rows of one who does not
        // lead)
        set_vis(cx, "ResetToDefaultsButton", Vis::SelfHitTestInvisible);
        let label = if leader { "Start Match".to_string() } else { self.ready_label() };
        let b = find(cx, "Btn_StartMatch");
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
        let on = leader || self.lobby().has_ready_button();
        Self::set_start_interactable(cx, on);
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
    }

    fn update_teams(&mut self, cx: &mut Cx<'_>) {
        self.hold_pictures(cx); // (the rows' cards follow the lobby)
        // UpdateTeams @5952: ToggleLoading(false, false), which plays LoadingDone from its start (the column fades
        // in again) on every native event.
        // Deviation: only while the cover is up (the lobby's first answer); any other change (a team switch, an
        // icon, a ready, a bot) redraws the rows it touches and nothing else (user request).
        if self.is_loading {
            self.toggle_loading(cx, false, false);
        }
        self.show_code(cx);
        let facts = self.facts();
        let (ffa, leader, slots, leader_id, players, teams) = {
            let l = self.lobby();
            (
                l.team_style() == STYLE_FFA,
                l.am_i_leader(),
                l.get_available_player_slot_count(),
                l.leader_id().to_string(),
                l.players().into_iter().cloned().collect::<Vec<_>>(),
                l.teams().clone(),
            )
        };
        let (t0, t1, tf) = (Self::team(cx, 0), Self::team(cx, 1), Self::team(cx, 2));
        wd(cx, t0).vis = if ffa { Vis::Collapsed } else { Vis::Visible };
        wd(cx, t1).vis = if ffa { Vis::Collapsed } else { Vis::Visible };
        wd(cx, tf).vis = if ffa { Vis::Visible } else { Vis::Collapsed };
        if ffa {
            cx.visit::<CustomGameTeam, _>(tf, |t, cx| t.populate(cx, leader, &leader_id, &players, -1, slots, &facts));
        } else {
            cx.visit::<CustomGameTeam, _>(t0, |t, cx| t.populate(cx, leader, &leader_id, &teams[0], 0, slots, &facts));
            cx.visit::<CustomGameTeam, _>(t1, |t, cx| t.populate(cx, leader, &leader_id, &teams[1], 1, slots, &facts));
        }
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
    }

    /// Btn_StartMatch of a player who is not the leader. The build: "Waiting for Lobby Leader", never interactable
    /// (UpdateIsLeader), and nothing in WBP_CustomGame makes a player ready.
    /// (the port's own) On an online lobby that button is the player's READY button; a local lobby has no such
    /// player.
    fn ready_label(&self) -> String {
        let l = self.lobby();
        if !l.has_ready_button() {
            "Waiting for Lobby Leader".into()
        } else if l.is_ready(l.my_id()) {
            "Cancel Ready".into()
        } else {
            "Ready".into()
        }
    }

    /// The last blocker of the list is the one shown.
    fn update_start_match_enabled(&mut self, cx: &mut Cx<'_>) {
        let (leader, blockers, can, ready_button) = {
            let l = self.lobby();
            (l.am_i_leader(), l.get_start_match_blockers(), l.can_start_match(), l.has_ready_button())
        };
        if leader {
            let error = find(cx, "ErrorInfoText");
            let w = wd(cx, error);
            match blockers.last() {
                Some(b) => {
                    w.vis = Vis::SelfHitTestInvisible;
                    w.text = (*b).to_string();
                }
                None => w.vis = Vis::Hidden,
            }
            Self::set_start_interactable(cx, can);
        } else {
            let label = self.ready_label();
            let b = find(cx, "Btn_StartMatch");
            cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
            Self::set_start_interactable(cx, ready_button);
        }
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
    }

    fn toggle_loading(&mut self, cx: &mut Cx<'_>, is_loading: bool, force: bool) {
        if !force && is_loading && self.loaded {
            return;
        }
        self.is_loading = is_loading;
        let indicator = find(cx, "LoadingIndicator");
        if is_loading {
            let w = wd(cx, indicator);
            w.opacity = 1.0;
            w.vis = Vis::Visible;
            return;
        }
        let me = cx.w;
        cx.view.play(me, "LoadingDone", false, 1.0);
        if self.hiding {
            return;
        }
        self.hiding = true;
        cx.view.delay(0.0, Handler { owner: me, id: R_LOADING_WAIT });
    }

    /// The `wait` loop of ToggleLoading's end.
    fn loading_wait(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        if cx.view.is_animation_playing(me, "LoadingDone") {
            cx.view.delay(0.0, Handler { owner: me, id: R_LOADING_WAIT });
            return;
        }
        self.hiding = false;
        let indicator = find(cx, "LoadingIndicator");
        wd(cx, indicator).vis = Vis::Collapsed;
        let v = find(cx, "VerticalBox_101");
        wd(cx, v).opacity = 1.0;
        if !self.is_loading {
            self.loaded = true;
        }
    }

    /// (the port's own; pictures.rs) While the lobby is up: the tiles of what it leads to (the bot's character
    /// selector and the pre-match screen: the roster's portraits; the map selector's maps, the thumbnails among
    /// them). Let go when the lobby goes (destruct).
    fn hold_pictures(&mut self, cx: &mut Cx<'_>) {
        let mut names: Vec<String> = Vec::new();
        let l = self.lobby();
        for p in l.players() {
            if p.is_bot
                && let Some(f) = cx.view.fighters().get(&p.character)
            {
                names.push(skin_by_name(Some(f), &p.skin).map_or(&f.skin.icon, |s| &s.icon).clone());
            }
        }
        names.extend(cx.view.roster().iter().map(|r| r.portrait.clone()));
        let maps = cx.view.soft_maps(&LOBBY_MAP_ROTATIONS);
        for m in &maps {
            if let Some(i) = cx.view.map(m) {
                names.push(i.data.texture);
            }
        }
        names.retain(|n| !n.is_empty());
        cx.view.hold_pictures(PICTURES_LOBBY, names.iter().map(String::as_str));
    }

    /// UpdateMaps @15118 -> UpdateMapGrid(EnabledMaps, GetAllMapsForContext, AmILeader): the grid is made anew, a
    /// thumbnail per enabled map.
    fn update_maps(&mut self, cx: &mut Cx<'_>, enabled: &[String]) {
        for w in std::mem::take(&mut self.map_thumbs) {
            MenuView::discard(cx, w);
        }
        self.cached_selected_maps.clear();
        self.hold_pictures(cx);
        let bx = find(cx, "MapWrapBox");
        for name in enabled {
            let m = cx.view.map(name);
            let w = MenuView::create(cx, "WBP_CustomGameMapThumbnail");
            let known = m.is_some();
            cx.visit::<CustomGameMapThumbnail, _>(w, |t, _| t.map_data = m);
            MenuView::add_child(cx, bx, w, None);
            self.map_thumbs.push(w);
            if known {
                self.cached_selected_maps.push(name.clone());
            }
        }
    }

    /// SelectMapsButton OnClick @15203: the Lab's map selector in its multi select mode, over the maps of the three
    /// custom rotations; the rotation of the lobby's style says which are available while connected (the port's
    /// rule is the Lab's: the maps it has a stage for). Accept -> SetMaps; the echo redraws the grid.
    fn select_maps(&mut self, cx: &mut Cx<'_>) {
        let style = usize::from(self.lobby().team_style());
        let Some(w) = Manager::show_modal(cx, "UI_LAB_MapSelector", "mapSelector", "", "") else { return };
        let all = cx.view.soft_maps(&LOBBY_MAP_ROTATIONS);
        let selected = self.cached_selected_maps.clone();
        let me = cx.w;
        cx.visit::<LabMapSelector, _>(w, |m, cx| {
            m.selected_rotation = LOBBY_ROTATIONS.get(style).copied().unwrap_or(12);
            let count = LOBBY_SELECTOR_COUNTS.get(style).copied().unwrap_or(0);
            m.init(cx, &all, &selected, count, true, false);
            m.multi_selected_event.add(Handler { owner: me, id: G_MAPS_SELECTED });
        });
    }

    /// The settings modals' state.
    fn lobby_view(&self) -> LobbyView {
        let l = self.lobby();
        LobbyView { leader: l.am_i_leader(), team_style: l.team_style(), options: *l.options() }
    }

    fn show_settings(&mut self, cx: &mut Cx<'_>, id: &str, page: SettingsPage) {
        let Some(w) = Manager::show_modal(cx, "CustomGameSettings", id, "", "") else { return };
        let view = self.lobby_view();
        let me = cx.w;
        cx.visit::<CustomGameSettings, _>(w, |s, cx| {
            s.on_option.add(Handler { owner: me, id: G_OPTION });
            s.bind(cx, view, page);
        });
    }

    /// The settings modals' rows again (an options change, a new leader).
    fn refresh_settings(&mut self, cx: &mut Cx<'_>) {
        let view = self.lobby_view();
        for id in [SETTINGS_MODAL, EXPERIMENTAL_MODAL] {
            if let Some(w) = cx.view.manager.get_modal(id) {
                cx.visit::<CustomGameSettings, _>(w, |s, cx| s.refresh(cx, Some(view)));
            }
        }
    }

    /// The Leave button: a confirmation while others are in the lobby, then LeaveConfirmed (the lobby is left as the
    /// widget goes). Unverified: the player count the dialog starts at is not decoded; here it is two.
    fn leave(&mut self, cx: &mut Cx<'_>) {
        if self.lobby().players().len() < 2 {
            let id = self.base.modal_id.clone();
            Manager::dismiss_modal(cx, &id);
            return;
        }
        if Manager::is_dialog_active(cx.view) {
            return;
        }
        let d = Manager::show_dialog(
            cx,
            DialogParams {
                title: "Are you sure you want to leave the lobby?".into(),
                button_left_label: "Yes".into(),
                button_right_label: "No".into(),
                ..DialogParams::default()
            },
        );
        let me = cx.w;
        if let Some(dl) = cx.view.dialog(d) {
            dl.button_left_clicked().add(Handler { owner: me, id: G_LEAVE_CONFIRMED });
        }
    }

    /// The Start (or READY) button's click.
    pub fn start_clicked(&mut self, cx: &mut Cx<'_>) {
        let i = Self::start_interactor(cx);
        if !cx.view.manager.interactor(i).is_some_and(|s| s.get_interactable()) {
            return;
        }
        for n in LOBBY_DROPS {
            let d = Self::drop_menu(cx, n);
            cx.visit::<DropMenu, _>(d, |d, cx| d.close(cx));
        }
        Self::set_start_interactable(cx, false);
        // not the leader: the READY button, off until the lobby's answer (update_start_match_enabled)
        if self.lobby().has_ready_button() {
            let on = !self.lobby().is_ready(self.lobby().my_id());
            self.lobby_mut().set_ready(on);
            return;
        }
        if !self.lobby().can_start_match() {
            return;
        }
        // CG@17287: the Game Mode drop off; CG@17346-17556: the fade to black, which stays over the 1.0 s before the
        // match opens
        let m = Self::main_interactor(cx, "UI_DropGameMode");
        interactor::set_interactable(cx, m, false);
        Manager::transition(
            cx,
            TransitionParams { kind: 0, level_options: "Lobby".into(), ..TransitionParams::default() },
        );
        self.lobby_mut().start_match();
    }

    fn on_team_event(&mut self, cx: &mut Cx<'_>, team: usize, kind: u32, arg: &dyn Any) {
        match kind {
            K_KICK => {
                if let Some(id) = arg.downcast_ref::<String>() {
                    Self::action(cx);
                    self.lobby_mut().kick_player(id);
                }
            }
            K_PROMOTE => {
                if let Some(id) = arg.downcast_ref::<String>() {
                    Self::action(cx);
                    self.lobby_mut().promote_new_leader(id);
                }
            }
            K_BOT_SETTINGS => {
                if let Some((id, character, difficulty, skin)) = arg.downcast_ref::<(String, String, i32, String)>() {
                    self.lobby_mut().update_settings_for_bot(id, character, *difficulty, skin);
                }
            }
            // SwitchTo1 @9904 / SwitchTo0 @10521: JoinTeam, then the row goes to the other column's box at once
            // (@9919 / @10536, padding Bottom 40), the two CachedNumPlayers follow and both columns redo their empty
            // slots without the bot buttons; Btn_StartMatch off, with no ToggleLoading (the cover is for kick,
            // promote, the bots and the Join buttons). The row stays in the old column's rows: at the echo that
            // column removes it and the new one makes its own. The free-for-all column's rows have no handler here.
            K_SWITCH => {
                let Some((id, row)) = arg.downcast_ref::<(String, WidgetId)>() else { return };
                if team > 1 {
                    return;
                }
                self.lobby_mut().join_team(id, 1 - team);
                let (this, other) = (Self::team(cx, team), Self::team(cx, 1 - team));
                cx.view.tree_mut().remove(*row);
                wd(cx, *row).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 40.0 };
                let bx = cx.view.tree().find_in(other, "CustomGamePlayers");
                cx.view.tree_mut().add(bx, *row);
                cx.visit::<CustomGameTeam, _>(this, |t, cx| {
                    t.cached_num_players = t.cached_num_players.saturating_sub(1);
                    t.update_empty_slot_visibility(cx, false, None);
                });
                cx.visit::<CustomGameTeam, _>(other, |t, cx| {
                    t.cached_num_players += 1;
                    t.update_empty_slot_visibility(cx, false, None);
                });
                Self::set_start_interactable(cx, false);
            }
            K_ADD_BOT => {
                if let Some((character, skin)) = arg.downcast_ref::<(String, String)>() {
                    Self::action(cx);
                    self.lobby_mut().add_bot(team.min(1), character, skin, 4);
                }
            }
            K_INVITE if !self.local() => self.show_invite_dialog(cx),
            _ => {}
        }
    }
}

impl Modal for CustomGame {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
        // (the port's own) the room code masked again each time the lobby opens
        self.code_shown = false;
        Self::set_start_interactable(cx, false);
        self.toggle_loading(cx, true, false);
        let v = find(cx, "VerticalBox_101");
        wd(cx, v).opacity = 0.0;
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let me = cx.w;
        let scroll = find(cx, "scrollContainer");
        let mut list: Vec<WidgetId> = LOBBY_DROPS.iter().map(|n| Self::main_interactor(cx, n)).collect();
        list.push(Self::hazards_interactor(cx));
        let sm = find(cx, "SelectMapsButton");
        list.push(cx.view.tree().find_in(sm, "UI_Interactor"));
        batch_set_scroll_box(cx, &list, scroll);
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
        for (n, id) in
            [("UI_DropGameMode", G_DROP_MODE), ("UI_DropRingouts", G_DROP_RINGOUTS), ("UI_DropTime", G_DROP_TIME)]
        {
            let d = Self::drop_menu(cx, n);
            cx.visit::<DropMenu, _>(d, |d, _| d.on_click.add(Handler { owner: me, id }));
        }
        let h = find(cx, "Toggle_Hazards");
        cx.visit::<ToggleSwitch, _>(h, |t, _| t.on_toggle_changed.add(Handler { owner: me, id: G_HAZARDS }));
        for (n, id) in [
            ("ResetToDefaultsButton", G_EXPERIMENTAL),
            ("CancelCustomGameButton", G_LEAVE),
            ("SelectMapsButton", G_SELECT_MAPS),
        ] {
            let b = find(cx, n);
            cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
        let st = find(cx, "Btn_StartMatch");
        cx.visit::<ButtonMain, _>(st, |b, _| b.on_click.add(Handler { owner: me, id: G_START }));
        for i in 0..3 {
            let t = Self::team(cx, i);
            cx.visit::<CustomGameTeam, _>(t, |t, _| {
                t.on_kick_player_clicked.add(Handler { owner: me, id: team_h(i, K_KICK) });
                t.on_promote_new_leader_clicked.add(Handler { owner: me, id: team_h(i, K_PROMOTE) });
                t.on_bot_settings_change_requested.add(Handler { owner: me, id: team_h(i, K_BOT_SETTINGS) });
                t.on_join_team_clicked.add(Handler { owner: me, id: team_h(i, K_SWITCH) });
            });
            for n in 1..=4 {
                let s = cx.view.tree().find_in(t, &format!("EmptySlot{n}"));
                cx.visit::<CustomGamePlayer, _>(s, |p, _| {
                    p.add_bot_dispatch.add(Handler { owner: me, id: team_h(i, K_ADD_BOT) });
                    p.invite_player_clicked.add(Handler { owner: me, id: team_h(i, K_INVITE) });
                });
            }
        }
        self.add_code_button(cx);
        self.add_code_view(cx);
        self.add_settings_button(cx);
        self.add_link_line(cx);
        self.show_link(cx);
        if self.lobby().online() {
            self.add_chat(cx);
        }
        cx.view.delay(0.2, Handler { owner: me, id: R_UNFOCUS_MODE });
        // a lobby made or joined (not the return to an open one after its match) ends a full party
        // Deviation: the build leaves the party only on an invite's join (PGI OnJoinCustomLobbyResult @109848
        // RemakeParty); the port leaves on making a lobby too (the user's rule; native StartCustomGameLobby unread).
        if self.lobby().net().is_some_and(|n| !n.is_open()) {
            Self::leave_full_party(cx);
        }
        self.lobby_mut().start();
        self.flush(cx);
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    /// Not a cancel (the sidebar's back button, "globalnavstepback"): straight to the leave step (@19316); a cancel
    /// would first give an open social panel its step back (not ported).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        self.leave(cx);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for CustomGame {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        for n in ["UI_PM_Experimental", "SpectatorDivider", "AddSpectatorsBox"] {
            set_vis(cx, n, Vis::Collapsed);
        }
        let ffa = Self::team(cx, 2);
        cx.visit::<CustomGameTeam, _>(ffa, |t, cx| t.as_grid(cx));
        let reset = find(cx, "ResetToDefaultsButton");
        cx.visit::<ButtonSecondary, _>(reset, |b, cx| b.init(cx, custom_text::EXPERIMENTAL_BUTTON));
        let sb = find(cx, "scrollContainer");
        cx.view.manager.register_scrollbox(sb);
        // Deviation: both scroll boxes clip to their bounds as the build's do, but their content (the inputs, the team
        // columns) fills the width, so a hover outline at its edge was cut. The clip reaches `CLIP_BLEED` past both
        // sides (the port's own).
        for n in ["scrollContainer", "AddPlayerScrollContainer"] {
            let b = find(cx, n);
            wd(cx, b).clip_bleed = CLIP_BLEED;
        }
        let me = cx.w;
        cx.view.manager.on_submit3.add(Handler { owner: me, id: G_SUBMIT3 });
        cx.view.manager.on_key_input.add(Handler { owner: me, id: G_KEY });
        // back from a match: the lobby it was started from (an online one only while its connection lasted)
        let kept_net = lobby::take_kept_net().filter(|k| k.alive());
        // (Lobby.kept of a local lobby: the session's, put there by the match's end or the pre-match's Back to Lobby;
        // taken, so a later main menu does not open the custom game again)
        let kept_local = cx.view.session.lobby.take();
        let join = lobby::take_pending_join();
        let (connected, my_id, can_join, compat, name) = {
            let o = &cx.view.session.online;
            (o.connected, o.me.clone(), o.can_join, o.compat.clone(), o.name.clone())
        };
        self.was_connected = connected;
        // (Lobby.kept is one lobby, local or online: the kept one is shown again unless an invitation's join waits)
        let mut chosen = match (kept_net, kept_local) {
            (Some(k), _) if join.is_none() => Lobby::Net(k),
            (None, Some(l)) if join.is_none() => Lobby::Local(l),
            (kept_net, _) => {
                if let Some(mut k) = kept_net {
                    k.leave();
                    for c in k.take_cmds() {
                        cx.online(c);
                    }
                }
                // (a code's join needs only the net process: a direct host's code is joined whatever the sign-in;
                // the port's own: so does a lobby of this player's, which the service hosts locally with no
                // connection up)
                if can_join {
                    Lobby::Net(Box::new(NetLobby::new(&my_id, join, &compat, &name)))
                } else {
                    // Unverified: the lobby's local name is the player's profile name in the build; the port has
                    // none yet (the online name stands for it when one is chosen).
                    Lobby::Local(Box::new(LocalLobby::new("Player")))
                }
            }
        };
        if let Lobby::Local(l) = &mut chosen
            && !name.is_empty()
        {
            l.set_local_name(&name);
        }
        let mut lobby = chosen;
        lobby.attach();
        self.lobby = Some(lobby);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        // CanvasPanel_0's slot ZOrders: the nav and the social panel 1, over the full-anchor page drawn after them
        // (at 0 the page took the social tab's clicks)
        slot_z_order(cx, &["WBP_GlobalNav_CustomGame", "UI_SocialPanel_CustomGame"], 1);
        Self::fill(cx);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        Self::fill(cx);
        if self.lobby.is_none() {
            return;
        }
        let mut state = (cx.view.manager.random() * 4_294_967_296.0) as u64 | 1;
        let mut random = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
        };
        let online = self.lobby().online();
        let view = if online { cx.view.session.online.clone() } else { OnlineView::default() };
        // Online going off ends a lobby on the chosen backend.
        let lost = self.was_connected && !view.connected && online;
        self.was_connected = view.connected;
        if lost && let Some(n) = self.lobby_mut().net_mut().filter(|n| n.on_chosen()) {
            n.connection_lost();
        }
        let events = self.lobby_mut().take_events(&view, &mut random);
        for e in events {
            if self.lobby.is_none() {
                return;
            }
            self.on_event(cx, e);
        }
        self.flush(cx);
        self.show_link(cx);
        if let Some(mut c) = self.chat.take() {
            c.tick(cx, dt);
            self.chat = Some(c);
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let sb = find(cx, "scrollContainer");
        cx.view.manager.unregister_scrollbox(sb);
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: G_SUBMIT3 });
        // (the lobby's settings go with it)
        Manager::dismiss_modal(cx, SETTINGS_MODAL);
        Manager::dismiss_modal(cx, EXPERIMENTAL_MODAL);
        let me = cx.w;
        cx.view.manager.on_key_input.remove(Handler { owner: me, id: G_KEY });
        if let Some(c) = self.chat.take() {
            c.destroy(cx);
        }
        if let Some(l) = self.lobby.as_mut() {
            l.detach();
            // LeaveCustomGameLobby, unless a match is starting: the lobby is kept for the return
            l.leave();
        }
        self.flush(cx);
        cx.view.release_pictures(PICTURES_LOBBY);
        // Restore Homescreen Context
        if let Some(h) = live(cx, "UI_Homescreen_v2").pop() {
            cx.visit::<Homescreen, _>(h, |h, cx| h.update_context(cx));
        }
        Manager::unfocus_all(cx, 0);
        for n in live(cx, "WBP_GlobalNav") {
            cx.visit::<GlobalNav, _>(n, |n, _| n.refresh_context_paused = false);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            R_UNFOCUS_MODE => {
                let m = Self::main_interactor(cx, "UI_DropGameMode");
                interactor::set_focused(cx, m, false);
            }
            R_LOADING_WAIT => self.loading_wait(cx),
            R_START => self.begin_match(cx),
            R_SHOW_CODE => self.show_code_text(cx),
            _ => {}
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        if self.lobby.is_none() {
            return;
        }
        match id {
            // OnSubmit3: the Start button, while no other modal is over the lobby
            G_SUBMIT3 => {
                let latest = Manager::latest_modal_id(cx.view);
                if latest.is_empty() || latest == self.base.modal_id {
                    self.start_clicked(cx);
                }
            }
            G_DROP_MODE => {
                if let Some((_, s)) = arg.downcast_ref::<(i32, String)>() {
                    self.set_game_mode_from_dropdown(cx, s);
                }
            }
            G_DROP_RINGOUTS => {
                if let Some((_, s)) = arg.downcast_ref::<(i32, String)>() {
                    let (style, stocks) = (self.lobby().team_style(), self.lobby().options().stocks);
                    if style != STYLE_FFA || stocks {
                        let n = s.parse::<u32>().unwrap_or(0);
                        self.lobby_mut().set_num_ringouts_for_win(n);
                    }
                }
            }
            G_DROP_TIME => {
                if let Some((_, s)) = arg.downcast_ref::<(i32, String)>() {
                    let n = s.parse::<u32>().unwrap_or(0);
                    self.lobby_mut().set_match_duration(n);
                }
            }
            G_HAZARDS => {
                let Some(&on) = arg.downcast_ref::<bool>() else { return };
                self.lobby_mut().set_allow_hazards(on);
            }
            G_EXPERIMENTAL => self.show_settings(cx, EXPERIMENTAL_MODAL, experimental_page()),
            G_LEAVE => self.leave(cx),
            G_LEAVE_CONFIRMED => {
                let id = self.base.modal_id.clone();
                Manager::dismiss_modal(cx, &id);
            }
            G_SELECT_MAPS => self.select_maps(cx),
            G_MAPS_SELECTED => {
                if let Some(maps) = arg.downcast_ref::<Vec<MapInfo>>() {
                    let names: Vec<String> = maps.iter().map(|m| m.name.clone()).collect();
                    self.lobby_mut().set_maps(&names);
                }
            }
            G_COPY_CODE if !self.local() => {
                let code = self.lobby().net().map_or("", NetLobby::code).to_string();
                if !code.is_empty() {
                    // (the port's own) in capitals, as shown: the join path reads a code in any case
                    cx.sink.clipboard(&code.to_uppercase());
                    // (the port's own) the copy said in a note, as the lobby's other notes
                    Self::note(cx, custom_text::ROOM_CODE_COPIED, "");
                }
            }
            G_EYE => {
                self.code_shown = !self.code_shown;
                cx.view.delay(0.0, Handler { owner: cx.w, id: R_SHOW_CODE });
            }
            G_ROOM_SETTINGS if !self.local() => self.show_settings(cx, SETTINGS_MODAL, room_page()),
            G_INVITED => {
                if let Some(players) = arg.downcast_ref::<Vec<super::social::SocialPlayerData>>() {
                    for p in players {
                        self.lobby_mut().invite(&p.id);
                    }
                }
            }
            G_KEY => {
                if let Some((_, key)) = arg.downcast_ref::<(i32, String)>()
                    && (key == "Enter" || key == "NumpadEnter")
                    && !cx.view.key_repeat
                {
                    self.open_chat(cx);
                }
            }
            id if (G_CHAT..G_CHAT + 0x10).contains(&id) => {
                if let Some(mut c) = self.chat.take() {
                    let a = c.handle(cx, id, arg);
                    if let ChatAction::Send(text) = a {
                        self.send_chat(cx, &mut c, &text);
                    }
                    self.chat = Some(c);
                }
            }
            G_START => self.start_clicked(cx),
            G_OPTION => {
                if let Some((key, value)) = arg.downcast_ref::<super::custom_settings::OptionRequest>() {
                    self.lobby_mut().set_option(*key, *value);
                }
            }
            id if id >= G_TEAM => {
                let (team, kind) = (((id - G_TEAM) / 16) as usize, (id - G_TEAM) % 16);
                self.on_team_event(cx, team, kind, arg);
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}
