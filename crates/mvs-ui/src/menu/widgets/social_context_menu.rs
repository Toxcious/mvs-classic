//! UI_SocialContextMenu: a player's social actions menu, a modal per player opened at the caller's point
//! (UI_Functions ShowSocialActionsMenu), with the actions UI_Functions SocialActionsForPlayer gives that player. The
//! port keeps Invite to Party (1), Invite to Custom Game (12) and Leave Party (9); it has no friends, profiles,
//! reports, blocks, guilds, console sessions or online Lab, so the other buttons never show.

use std::any::Any;

use super::common::ModalBase;
use super::controls::buttons::IconButton;
use super::controls::interactor::{batch_set_context, set_focused};
use super::controls::{bind, find, wd};
use super::social::{SocialPlayer, SocialPlayerData, is_player_invitable, lobby_invite_open};
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{Manager, Modal};
use crate::menu::online::{OnlineCmd, PartyCmd};
use crate::slate::{Vis, WidgetId};

/// The menu's class.
pub const CLASS: &str = "UI_SocialContextMenu";

/// ESocialContextAction's byte values the port runs (the enum asset).
pub const INVITE_TO_PARTY: u8 = 1;
pub const LEAVE_PARTY: u8 = 9;
pub const INVITE_TO_CUSTOM_GAME: u8 = 12;

/// Every button of the tree with its ESocialContextAction (Setup Buttons @4573: the labelled buttons, then the
/// UI_IconButton children of buttonContainer1 and buttonContainer2).
const BUTTONS: [(&str, u8); 17] = [
    ("buttonInviteToParty", INVITE_TO_PARTY),
    ("buttonInviteToCustomGame", INVITE_TO_CUSTOM_GAME),
    ("buttonInviteToLab", 13),
    ("buttonSessionInvite", 14),
    ("buttonLeave", LEAVE_PARTY),
    ("buttonProfile", 4),
    ("buttonAddFriend", 2),
    ("buttonPlatformProfile", 15),
    ("buttonPromote", 5),
    ("buttonGuild", 6),
    ("buttonBlock", 3),
    ("buttonUnfriend", 7),
    ("buttonReport", 10),
    ("buttonUnblock", 8),
    ("buttonGuildPromote", 16),
    ("buttonGuildDemote", 17),
    ("buttonGuildKickMember", 18),
];
/// The labelled buttons: BUTTONS before this index. The rest are the icon buttons of the HorizontalBox.
const LABELLED: usize = 5;

/// ShowSocialActionsMenu @556-1032: a point within this of the viewport's bottom moves up by MOVE_UP.
const BOTTOM_MARGIN: f64 = 300.0;
const MOVE_UP: f64 = 450.0;
/// FocusFirstButton's Delay @4363.
const FOCUS_DELAY: f64 = 0.1;

/// What the actions of a player depend on, as the port has it (SocialActionsForPlayer's tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionInputs {
    /// IsPlayerInvitable? and not CanInviteToSession? (always false on PC): the party invite.
    pub party_invitable: bool,
    /// CanInvitePlayerToCustomGame?: a custom lobby is up and the player is not in it.
    pub lobby_invitable: bool,
    /// PartyManager.IsInParty(the player's AccountID).
    pub in_my_party: bool,
}

/// UF SocialActionsForPlayer (line 3306) with the port's actions, in the tests' order (1, 12, 9):
/// - 1 @256-435: IsPlayerInvitable? and not CanInviteToSession?;
/// - 12 @676-963: CanInvitePlayerToCustomGame? and not CanInvitePlayerToLab? (no online Lab) and not
///   CanInviteToSession?;
/// - 9 @5972-6286: the player is in my party.
pub fn social_actions(i: ActionInputs) -> Vec<u8> {
    let mut v = Vec::new();
    if i.party_invitable {
        v.push(INVITE_TO_PARTY);
    }
    if i.lobby_invitable {
        v.push(INVITE_TO_CUSTOM_GAME);
    }
    if i.in_my_party {
        v.push(LEAVE_PARTY);
    }
    v
}

/// The button loop @1650-2196: a button shows when its action is in the list, not omitted, and in the override
/// (an empty override lets every action through).
pub fn button_shown(action: u8, actions: &[u8], omit: &[u8], overrides: &[u8]) -> bool {
    actions.contains(&action) && !omit.contains(&action) && (overrides.is_empty() || overrides.contains(&action))
}

/// PostShow @5131-5408: the ActionsOverride of a caller's slug (`leaderboards` and `customGame`: View Career and
/// the platform profile only).
pub fn overrides_for(slug: &str) -> Vec<u8> {
    if slug == "leaderboards" || slug == "customGame" { vec![4, 15] } else { Vec::new() }
}

/// ShowSocialActionsMenu's place rule @556-1032: below `view_h - 300` the point moves up 450; X is never clamped.
pub fn menu_place(at: [f64; 2], view_h: f64) -> [f64; 2] {
    if at[1] > view_h - BOTTOM_MARGIN { [at[0], at[1] - MOVE_UP] } else { at }
}

/// The inputs of `d` from the online state and the open modals.
/// Unverified: CanInvitePlayerToCustomGame? also needs IsPartyJoinable and the native CustomGameManager.CanInvitePlayer
/// (question N4); here the lobby up and the player not one of its members, as the social row's "+" in a lobby.
fn inputs(cx: &Cx<'_>, d: &SocialPlayerData) -> ActionInputs {
    let o = &cx.view.session.online;
    let lobby_up = cx.view.manager.get_modal("customGame").is_some();
    ActionInputs {
        // IsPlayerInvitable? @532: no custom game widget and no lobby
        party_invitable: !lobby_up && is_player_invitable(cx, d),
        lobby_invitable: lobby_up && o.connected && lobby_invite_open(cx, &d.id),
        // Unverified: IsInParty is native (N3); here a joined member other than this player
        in_my_party: o.party.as_ref().is_some_and(|p| p.me != d.id && p.members.iter().any(|m| m.id == d.id)),
    }
}

/// The player `id` as the menu shows them: the online list's entry, else the party's member, else the id alone.
pub fn player_data(cx: &Cx<'_>, id: &str) -> SocialPlayerData {
    let o = &cx.view.session.online;
    if let Some(p) = o.listed_player(id) {
        return SocialPlayerData::from(p);
    }
    let member = o.party.as_ref().and_then(|p| p.members.iter().find(|m| m.id == id));
    match member {
        Some(m) => SocialPlayerData {
            id: m.id.clone(),
            name: m.name.clone(),
            icon: m.icon.clone(),
            ..SocialPlayerData::default()
        },
        None => SocialPlayerData { id: id.to_string(), ..SocialPlayerData::default() },
    }
}

/// A point of the window in device pixels as a viewport point (Slate units).
pub fn pixels_to_viewport(cx: &Cx<'_>, p: [f64; 2]) -> [f64; 2] {
    let dpi = cx.view.dpi;
    [p[0] / dpi, p[1] / dpi]
}

/// UF ShowSocialActionsMenu(AccountID, Position, Radial false, Location Slug): the menu of player `d` at `at`
/// (viewport units), its Omit Actions `omit` (BP_SocialMenu's [3, 7, 10]; else none). A menu of that player already
/// up: nothing (@37-114). Returns the modal shown.
/// Deviation: a menu with no button to show is not opened (the build always has View Career, which the port drops).
pub fn show_social_actions_menu(
    cx: &mut Cx<'_>,
    d: SocialPlayerData,
    at: [f64; 2],
    slug: &str,
    omit: &[u8],
) -> Option<WidgetId> {
    if d.id.is_empty() || cx.view.manager.get_modal(&d.id).is_some() {
        return None;
    }
    let actions = social_actions(inputs(cx, &d));
    let overrides = overrides_for(slug);
    if !BUTTONS.iter().any(|&(_, a)| button_shown(a, &actions, omit, &overrides)) {
        return None;
    }
    if !cx.view.bundle.widgets.contains_key(CLASS) {
        eprintln!("mvs: the social actions menu: no {CLASS} in the menu bundle");
        return None;
    }
    let id = d.id.clone();
    let w = Manager::show_modal(cx, CLASS, &id, slug, "")?;
    let omit = omit.to_vec();
    cx.visit::<SocialContextMenu, _>(w, |m, cx| m.init(cx, d, at, omit));
    Some(w)
}

const CM_DISMISS: u32 = 1;
const CM_FOCUS_FIRST: u32 = 2;
/// A button's click: CM_BUTTON + its index in BUTTONS.
const CM_BUTTON: u32 = 100;

/// UI_SocialContextMenu: the player's card (UI_SocialPlayer, Enable Context Menu false), the action buttons, and
/// buttonDismiss under them filling the screen. Every click, a click outside and the pad's cancel close it.
/// Not ported: the UI_SocialPlayer card's own UpdateAppearance call beyond its data, the radial menu.
#[derive(Debug, Default)]
pub struct SocialContextMenu {
    base: ModalBase,
    data: Option<SocialPlayerData>,
    slug: String,
    omit: Vec<u8>,
    /// The actions the buttons were last shown for.
    actions: Vec<u8>,
}

impl SocialContextMenu {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SocialContextMenu::default())
    }

    fn button_interactor(cx: &Cx<'_>, name: &str) -> WidgetId {
        let b = find(cx, name);
        cx.view.tree().find_in(b, "UI_Interactor")
    }

    /// Init(social data) @3671, then the place (@1060-1217: the `menu` SizeBox's canvas slot at the point, its
    /// alignment 0) and UpdateAppearance.
    fn init(&mut self, cx: &mut Cx<'_>, d: SocialPlayerData, at: [f64; 2], omit: Vec<u8>) {
        self.omit = omit;
        let card = find(cx, "UI_SocialPlayer");
        let card_data = d.clone();
        cx.visit::<SocialPlayer, _>(card, |p, cx| {
            p.enable_context_menu = false;
            p.set_data(cx, card_data);
        });
        self.data = Some(d);
        let [x, y] = menu_place(at, cx.view.viewport_size()[1]);
        let menu = find(cx, "menu");
        let m = wd(cx, menu);
        m.offsets.l = x;
        m.offsets.t = y;
        self.set_available_actions(cx);
        self.update_appearance(cx);
    }

    /// SetAvailableActions @3879-4102 and the button loop @1650-2196.
    /// Deviation: with no icon button shown (the port never shows one) their row (the two WrapBoxes and the `div`
    /// between them) is collapsed; the build always shows View Career there.
    fn set_available_actions(&mut self, cx: &mut Cx<'_>) {
        let Some(d) = self.data.clone() else { return };
        self.actions = social_actions(inputs(cx, &d));
        let overrides = overrides_for(&self.slug);
        let mut icons = false;
        for (k, &(name, a)) in BUTTONS.iter().enumerate() {
            let on = button_shown(a, &self.actions, &self.omit, &overrides);
            icons |= on && k >= LABELLED;
            let b = find(cx, name);
            wd(cx, b).vis = if on { Vis::Visible } else { Vis::Collapsed };
        }
        let row = find(cx, "HorizontalBox_0");
        wd(cx, row).vis = if icons { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }

    /// UpdateAppearance @3740-3864: a new context over buttonDismiss and the buttons, buttonDismiss focused (the
    /// context only), then FocusFirstButton after its 0.1 s.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let g = cx.view.manager.new_guid();
        self.base.context = g.clone();
        let mut list = vec![find(cx, "buttonDismiss")];
        list.extend(BUTTONS.iter().map(|(n, _)| Self::button_interactor(cx, n)));
        batch_set_context(cx, &list, &g);
        let dismiss = find(cx, "buttonDismiss");
        set_focused(cx, dismiss, true);
        let me = cx.w;
        cx.view.delay(FOCUS_DELAY, Handler { owner: me, id: CM_FOCUS_FIRST });
    }

    /// FocusFirstButton @15-169.
    /// Deviation: the build focuses View Career (or the platform profile), which the port has not; the port focuses
    /// the first button shown, in the tree's order.
    fn focus_first_button(&mut self, cx: &mut Cx<'_>) {
        for &(name, _) in &BUTTONS {
            let b = find(cx, name);
            if cx.view.tree().widget(b).vis == Vis::Visible {
                let i = Self::button_interactor(cx, name);
                set_focused(cx, i, false);
                return;
            }
        }
    }

    /// UF PerformSocialAction @0-8650 for the port's actions. Not ported: the guard's IsBot? (no bot is on the
    /// online list or in a party).
    fn perform(&mut self, cx: &mut Cx<'_>, action: u8) {
        let Some(d) = self.data.clone() else { return };
        match action {
            // @1000-1323: IsPlayerInvitable?, then BPI_InviteToParty
            INVITE_TO_PARTY => {
                if is_player_invitable(cx, &d) && cx.view.manager.get_modal("customGame").is_none() {
                    cx.online(OnlineCmd::Party(PartyCmd::Invite { to: d.id.clone() }));
                }
            }
            // @7974-8217: BPI_InviteToCustomGame(account, false): the lobby page's own invite (its keeper message and
            // the online invite), as the social row's "+" in a lobby
            INVITE_TO_CUSTOM_GAME => {
                if let Some(page) = cx.view.manager.get_modal("customGame")
                    && lobby_invite_open(cx, &d.id)
                {
                    cx.visit::<super::custom_game::CustomGame, _>(page, |g, _| g.invite(&d.id));
                }
            }
            // @5305-5547: PartyManager.RemakeParty: this player leaves the party (the target stays where it is).
            // Unverified: what RemakeParty does for the leader (N1); the port's PartyLeave ends a leader's party for
            // its member and leaves a member's leader alone.
            LEAVE_PARTY => cx.online(OnlineCmd::Party(PartyCmd::Leave)),
            _ => {}
        }
    }
}

impl Modal for SocialContextMenu {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @5131-5408: the slug's ActionsOverride, the EnterAnim (@4418). Init runs from
    /// show_social_actions_menu, which has the player's data.
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        self.slug = instructions.to_string();
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @5098 -> @4480: DismissModal(Modal ID).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for SocialContextMenu {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// Setup Buttons @4573: each button's click to OnSocialButtonClick; buttonDismiss's click dismisses, and it is
    /// not navigable.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for (k, &(name, _)) in BUTTONS.iter().enumerate() {
            let b = find(cx, name);
            let id = CM_BUTTON + k as u32;
            cx.visit::<IconButton, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
        let dismiss = find(cx, "buttonDismiss");
        bind(cx, dismiss, |s| &mut s.on_click, CM_DISMISS);
        if let Some(s) = cx.view.manager.interactor_mut(dismiss) {
            s.navigable = false;
        }
    }

    /// OnJoinableChanged @4979: the list rebuilt when it changes while the menu is up. The port's stand-in for
    /// OnIsPartyJoinableUpdate: the actions read again each frame (the party, the online list, the lobby).
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let Some(d) = &self.data else { return };
        if social_actions(inputs(cx, d)) != self.actions {
            self.set_available_actions(cx);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == CM_FOCUS_FIRST {
            self.focus_first_button(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            CM_DISMISS => self.request_dismiss(cx, "", false),
            // OnSocialButtonClick @4735: PerformSocialAction, then RequestDismiss on both paths of @4931
            _ => {
                let Some(&(_, action)) = id.checked_sub(CM_BUTTON).and_then(|k| BUTTONS.get(k as usize)) else {
                    return;
                };
                self.perform(cx, action);
                self.request_dismiss(cx, "", false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stranger_out_of_a_lobby_gets_the_party_invite() {
        let a = social_actions(ActionInputs { party_invitable: true, ..ActionInputs::default() });
        assert_eq!(a, [INVITE_TO_PARTY]);
    }

    #[test]
    fn the_partner_gets_leave_party_and_in_a_lobby_the_lobby_invite() {
        let a = social_actions(ActionInputs { in_my_party: true, ..ActionInputs::default() });
        assert_eq!(a, [LEAVE_PARTY]);
        let a = social_actions(ActionInputs { lobby_invitable: true, in_my_party: true, ..ActionInputs::default() });
        assert_eq!(a, [INVITE_TO_CUSTOM_GAME, LEAVE_PARTY]);
    }

    #[test]
    fn nothing_applies_and_no_action_shows() {
        assert!(social_actions(ActionInputs::default()).is_empty());
    }

    #[test]
    fn omit_and_override_hide_a_listed_action() {
        let a = [INVITE_TO_PARTY, LEAVE_PARTY];
        assert!(button_shown(LEAVE_PARTY, &a, &[], &[]));
        assert!(!button_shown(LEAVE_PARTY, &a, &[LEAVE_PARTY], &[]));
        assert!(!button_shown(INVITE_TO_PARTY, &a, &[3, 7, 10], &overrides_for("customGame")));
        assert!(!button_shown(INVITE_TO_CUSTOM_GAME, &a, &[], &[]));
        assert!(overrides_for("").is_empty());
        assert_eq!(overrides_for("leaderboards"), [4, 15]);
    }

    #[test]
    fn a_point_near_the_bottom_moves_up() {
        assert_eq!(menu_place([100.0, 700.0], 1080.0), [100.0, 700.0]);
        assert_eq!(menu_place([100.0, 780.0], 1080.0), [100.0, 780.0]);
        assert_eq!(menu_place([-50.0, 781.0], 1080.0), [-50.0, 331.0]);
    }
}
