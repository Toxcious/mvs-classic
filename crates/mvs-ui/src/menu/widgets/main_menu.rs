//! UI_MainMenu: the main menu's widget, the home screen over its scene, and the
//! online notices it takes: the invitations' toasts (a custom game's, a party's), the name modal asked again when the
//! name is taken, the session's question, the host's notes and the note of an online match that stopped early.
//!
//! EnterPreMatch's seamless branch (`fadeToPrematch`, `addWidgetToCanvas`: the queue's screen inside the main menu's)
//! is not here: the queue's Confirm sends OnlineCmd::Queue and the program opens the PreMatch level, as the arcade's
//! pre-match does (Deviation).
//! OnResourcesLoaded (Init Homescreen(true) and the first visit's music switches) needs the auth handler's Online
//! Initialized?, which stays false offline: it does not run.

use std::any::Any;

use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{DialogId, DialogParams, Manager, NotificationParams};
use crate::menu::online::{LobbyInvite, OnlineCmd, OnlineEvent, OnlineNotice, PartyCmd};
use crate::menu::online_text::{ask_text, notice_text};
use crate::menu::widgets::common::fill_page;
use crate::menu::widgets::controls::slot_z_order;
use crate::menu::widgets::custom_game::CustomGame;
use crate::menu::widgets::home::Homescreen;
use crate::slate::{Vis, WidgetId};
use mvs_net::service::api::InviteKind;

/// Most invitations kept for a main menu to come (Deviation, the port's own, against invite floods): held by
/// inviter, the newest of each, this many at the most (the oldest let go).
const QUEUED_INVITES: usize = 5;
/// The custom game invite toast's time on screen (OpenCustomGameInvite's notification: 15 s); a second invite of one
/// inviter while its first toast is up is dropped (Deviation: the build makes a second toast).
const INVITE_SCREEN_TIME: f64 = 15.0;
/// Seconds the port's own server notes stay up (the notification class default is 4).
const NOTICE_SCREEN_TIME: f64 = 12.0;

/// PandaGameInstance's party invite toast (@51385-54649): title Format("Party Invite: {mode}"), {mode} by the
/// invite's MultiqueueConfigs[0] ("2v2 Normal"), buttons Accept and Decline (Game.locres keys).
const PARTY_INVITE_TITLE: &str = "574B07F54A59F6F281E768B9410B6745";
const PARTY_MODE_2V2_NORMAL: &str = "59B333BD487777A7F49F76A22650DDBF";
const PARTY_ACCEPT: &str = "D8CE09314444BBC2ACD4AD81565E0A4F";
const PARTY_DECLINE: &str = "4D7FCA0C49B918F3716DC6BE217136D7";

/// The title of the port's own server notes: DisplayError's notification title "Error!" (Game.locres key).
pub(crate) const NOTICE_TITLE: &str = "1A0583A247EF3E084A21CFA94A8966F7";

/// "Party Disbanded" (Game.locres), PGI OpenPartyDisbandedNotification's title.
const PARTY_DISBANDED_TITLE: &str = "583B9D914E26BA7FC8AB47AE6E1070F4";

/// OpenCustomGameInvite's toast: its title "Custom Game Invite" and its button "Join" (Game.locres keys).
const INVITE_TITLE: &str = "2310459D491D8208EE15C3B448CF21E2";
const INVITE_JOIN: &str = "ECB3BE4D429FCD7D8286FA91AE40D7B9";

const MM_NAME: u32 = 1;
const MM_ASK_AGAIN: u32 = 2;
const MM_DEQUEUE: u32 = 3;
const MM_NOTE: u32 = 4;
const MM_ASK_YES: u32 = 5;
const MM_ASK_NO: u32 = 6;
const MM_REOPEN_LOBBY: u32 = 7;
/// (the port's own) A desync note's OPEN FOLDER.
const MM_OPEN_FOLDER: u32 = 8;
/// The first handler id of an invite toast's four events (action 1, action 2, timed out, dismissed).
const MM_TOAST: u32 = 0x100;

/// What the main menus keep across screens: the invitations that came with no
/// main menu up, the toasts shown, the party toasts up.
#[derive(Debug, Default)]
pub struct MainMenuState {
    /// The main menu that is up (MainMenu.live; AccessorLibrary GetMainMenu).
    pub live: Option<WidgetId>,
    /// OnReceivedCustomGameInvite: kept under the inviter while no main menu is up (QueuedCustomGameInvites).
    queued: Vec<LobbyInvite>,
    /// The invite toasts' keys (kind and inviter) and when each was shown (s).
    shown: Vec<(String, f64)>,
    /// The party invite toasts up, for JoinParty's DismissAllPartyInvites (@47896): toast id, its widget.
    party_toasts: Vec<(u32, WidgetId)>,
    next_toast: u32,
    /// The queue the main menu's Confirm asked for, taken by the program with the PreMatch
    /// level it opens; `last_queue` the queues of the last Confirm (a queue match's PLAY AGAIN searches them again).
    pub queue: Option<Vec<mvs_protocol::online::QueueMode>>,
    pub last_queue: Option<Vec<mvs_protocol::online::QueueMode>>,
    /// Why the last online match stopped before its end, in words; the main menu shows it once (Lobby.note).
    pub note: String,
    /// (the port's own) A join link's code: the main menu that is up opens the lobby screen on it, as the join box
    /// does (kept while none is up: a match, the pre-match, the title).
    pub link_join: Option<String>,
}

impl MainMenuState {
    /// An invitation with no main menu up: a party's is let go (@51298: Should Show Invite? and
    /// CanShowNotifications, else nothing kept), a custom game's kept under its inviter.
    pub fn keep(&mut self, i: LobbyInvite) {
        if i.kind == InviteKind::Party {
            return;
        }
        self.queued.retain(|q| q.from_id != i.from_id);
        self.queued.push(i);
        if self.queued.len() > QUEUED_INVITES {
            self.queued.remove(0);
        }
    }
}

/// What a toast's handler ids stand for.
#[derive(Debug)]
enum ToastKind {
    Custom { lobby: String },
    Party { lobby: String },
}

/// UI_MainMenu.
#[derive(Debug, Default)]
pub struct MainMenu {
    /// The question shown and its dialog (None: none), and the last question answered (not shown again).
    ask: Option<(u32, DialogId)>,
    answered: Option<u32>,
    /// The invite toasts' handler ids.
    toasts: Vec<(u32, ToastKind)>,
    /// The code of the invitation whose lobby screen opens at the next MM_REOPEN_LOBBY.
    reopen: Option<String>,
    /// The folder of the last desync note's report, which its OPEN FOLDER opens.
    desync_folder: String,
}

impl MainMenu {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MainMenu::default())
    }

    fn fill(cx: &mut Cx<'_>) {
        let b = cx.view.tree().find_in(cx.w, "SizeBox_0");
        fill_page(cx, b);
    }

    /// The name window again, with the build's "unavailable" line (the name service refused the stored name).
    pub fn ask_again(cx: &mut Cx<'_>, refused: bool) {
        if cx.view.manager.get_modal("chooseUsername").is_none() {
            Manager::show_modal(cx, "UI_SetUsername", "chooseUsername", "", if refused { "refused" } else { "taken" });
        }
    }

    /// The online events the main menu takes (the listeners bound in Construct and bound while it lives).
    pub fn on_event(&mut self, cx: &mut Cx<'_>, ev: &OnlineEvent) {
        match ev {
            OnlineEvent::Invited(i) => self.open_invite(cx, i),
            // (the port's own) a match that went out of sync: the note with OPEN FOLDER on the report's folder
            OnlineEvent::Notice(OnlineNotice::Desync { folder }) => self.desync_note(cx, folder),
            OnlineEvent::Notice(OnlineNotice::PartyDisbanded) => Self::party_disbanded(cx),
            // the server's line for a player it could not invite, or why it refused this game: DisplayError's
            // notification, held longer than the class default's 4 s so the two-sentence lines can be read
            OnlineEvent::Notice(n) => {
                let body = notice_text(n);
                if !body.is_empty() {
                    Self::notify(cx, body);
                }
            }
            OnlineEvent::NameTaken { refused } => Self::ask_again(cx, *refused),
            _ => {}
        }
    }

    /// The desync's note, held as long as the other notices, its OPEN FOLDER (action 1) on `folder`
    /// (MenuSink::open_folder).
    fn desync_note(&mut self, cx: &mut Cx<'_>, folder: &str) {
        self.desync_folder = folder.to_owned();
        let params = NotificationParams {
            title: cx.view.game_text(NOTICE_TITLE).to_string(),
            body: crate::menu::custom_text::DESYNC_NOTE.to_owned(),
            screen_time: NOTICE_SCREEN_TIME,
            button_label1: crate::menu::custom_text::OPEN_FOLDER.to_owned(),
            ..NotificationParams::default()
        };
        if let Some(n) = Manager::show_notification(cx, params)
            && let Some(note) = cx.view.notification(n)
        {
            note.on_action1().add(Handler { owner: cx.w, id: MM_OPEN_FOLDER });
        }
    }

    /// PGI OpenPartyDisbandedNotification (@49359): ShowSimpleNotification("Party Disbanded", no body), UF
    /// ShowSimpleNotification's params (screenTime 4.0, allowDismissal, no icon, no audio).
    fn party_disbanded(cx: &mut Cx<'_>) {
        let params = NotificationParams {
            title: cx.view.game_text(PARTY_DISBANDED_TITLE).to_string(),
            audio: None,
            ..NotificationParams::default()
        };
        Manager::show_notification(cx, params);
    }

    fn notify(cx: &mut Cx<'_>, body: String) {
        Manager::show_notification(
            cx,
            NotificationParams {
                title: cx.view.game_text(NOTICE_TITLE).to_string(),
                body,
                screen_time: NOTICE_SCREEN_TIME,
                ..NotificationParams::default()
            },
        );
    }

    /// PandaGameInstance OpenCustomGameInvite / OpenPartyInvite, once per kind and inviter while the toast is up.
    /// Deviation: Should Show Invite? (block list, friends-only preference) and the waiting cover of the join are not
    /// ported, the lobby screen's own loading cover shows instead.
    fn open_invite(&mut self, cx: &mut Cx<'_>, invite: &LobbyInvite) {
        let now = cx.now();
        let st = &mut cx.view.session.main_menu;
        st.shown.retain(|(_, at)| now - at < INVITE_SCREEN_TIME);
        let key = format!("{}:{}", if invite.kind == InviteKind::Party { "party" } else { "custom" }, invite.from_id);
        if st.shown.iter().any(|(k, _)| *k == key) {
            return;
        }
        st.shown.push((key, now));
        if invite.kind == InviteKind::Party {
            self.open_party_invite(cx, invite);
        } else {
            self.open_custom_game_invite(cx, invite);
        }
    }

    /// A toast's four handler ids, bound to the notification `n`.
    fn bind_toast(&mut self, cx: &mut Cx<'_>, n: WidgetId, kind: ToastKind, action2: bool) -> u32 {
        let st = &mut cx.view.session.main_menu;
        let id = MM_TOAST + st.next_toast * 4;
        st.next_toast += 1;
        let me = cx.w;
        if let Some(note) = cx.view.notification(n) {
            note.on_action1().add(Handler { owner: me, id });
            if action2 {
                note.on_action2().add(Handler { owner: me, id: id + 1 });
                note.on_timed_out().add(Handler { owner: me, id: id + 2 });
                note.on_dismissed().add(Handler { owner: me, id: id + 3 });
            }
        }
        self.toasts.push((id, kind));
        id
    }

    /// PandaGameInstance OpenPartyInvite's toast (@51308-54649): AccountToPartyID[inviter] = the party, then a toast
    /// titled "Party Invite: {mode}", no body, 15 s with its timer, not dismissable, no sound, tag "party_invite_" +
    /// the inviter's id, the inviter's row, Accept and Decline. Accept (OnPartyInviteAccept @91594 -> JoinParty
    /// @47896): the other party toasts dismissed (DismissAllPartyInvites), the custom game lobby left when one is
    /// open (@48295-48421), then the party joined. Decline, the timeout (@54960): an analytics event, the entry
    /// dropped; the inviter is told nothing.
    /// Deviation: the port's party invite carries no queue mode (the party is the net process's, not a backend party
    /// with its MultiqueueConfigs): {mode} reads "2v2 Normal", the queue a full party plays (WBP_MatchOptions
    /// @1248-1457 turns its other modes off; the port's queues are type 1, Normal).
    /// Not ported: Accept's matchmaking cancel (@91157; the queue's own follow of the party is the program's), the
    /// game-server-ready and placing checks, JoinParty's failure notification and CreateParty.
    fn open_party_invite(&mut self, cx: &mut Cx<'_>, invite: &LobbyInvite) {
        let title = cx.view.game_text(PARTY_INVITE_TITLE).replace("{mode}", cx.view.game_text(PARTY_MODE_2V2_NORMAL));
        let params = NotificationParams {
            title,
            screen_time: INVITE_SCREEN_TIME,
            allow_dismissal: false,
            audio: None,
            tag: format!("party_invite_{}", invite.from_id),
            button_label1: cx.view.game_text(PARTY_ACCEPT).to_string(),
            button_label2: cx.view.game_text(PARTY_DECLINE).to_string(),
            account_id: invite.from_id.clone(),
            show_timer: true,
            ..NotificationParams::default()
        };
        let Some(n) = Manager::show_notification(cx, params) else { return };
        let id = self.bind_toast(cx, n, ToastKind::Party { lobby: invite.lobby.clone() }, true);
        cx.view.session.main_menu.party_toasts.push((id, n));
    }

    /// PandaGameInstance OpenCustomGameInvite: a toast with the inviter's row, a 15 s timer bar and "Join" (no
    /// decline: it runs out). Join: JoinCustomGameLobby, then BPI_AddNewCustomGameUI (the lobby screen, in place of
    /// the one that is up).
    /// A second invitation to the same lobby makes a second toast in the build: UI_Manager ShowNotification @0-110
    /// and CreateAndAddNotification @0-161 make a UI_Notification per call and never look at the tag. Deviation: the
    /// port shows one per inviter at a time.
    fn open_custom_game_invite(&mut self, cx: &mut Cx<'_>, invite: &LobbyInvite) {
        let params = NotificationParams {
            title: cx.view.game_text(INVITE_TITLE).to_string(),
            screen_time: INVITE_SCREEN_TIME,
            allow_dismissal: false,
            audio: None,
            tag: invite.lobby.clone(),
            button_label1: cx.view.game_text(INVITE_JOIN).to_string(),
            account_id: invite.from_id.clone(),
            show_timer: true,
            ..NotificationParams::default()
        };
        let Some(n) = Manager::show_notification(cx, params) else { return };
        self.bind_toast(cx, n, ToastKind::Custom { lobby: invite.lobby.clone() }, false);
    }

    /// A toast's handler: Accept of a party's, Join of a custom game's, or the end of a party toast.
    fn toast_event(&mut self, cx: &mut Cx<'_>, id: u32) {
        let base = id - (id - MM_TOAST) % 4;
        let which = (id - MM_TOAST) % 4;
        let Some(at) = self.toasts.iter().position(|(i, _)| *i == base) else { return };
        let me = cx.w;
        let st = &mut cx.view.session.main_menu;
        if which != 0 {
            st.party_toasts.retain(|(i, _)| *i != base);
            return;
        }
        match &self.toasts[at].1 {
            ToastKind::Party { lobby } => {
                let lobby = lobby.clone();
                st.party_toasts.retain(|(i, _)| *i != base);
                let others: Vec<WidgetId> = std::mem::take(&mut st.party_toasts).into_iter().map(|(_, w)| w).collect();
                for w in others {
                    cx.visit_dyn(w, |c, cx| {
                        if let Some(n) = c.as_notification() {
                            n.exit(cx);
                        }
                    });
                }
                if cx.view.manager.get_modal("customGame").is_some() {
                    Manager::dismiss_modal(cx, "customGame");
                }
                cx.online(OnlineCmd::Party(PartyCmd::Join { code: lobby }));
            }
            ToastKind::Custom { lobby } => {
                let lobby = lobby.clone();
                // the lobby screen that is up moves its online lobby to the other one (CustomGame.joinInvited)
                let up = cx.view.manager.get_modal("customGame");
                if up.and_then(|w| cx.visit::<CustomGame, _>(w, |g, cx| g.join_invited(cx, &lobby))) == Some(true) {
                    return;
                }
                // else the lobby screen of that code in place of the one that is up (NetLobby.pendingJoin, set as
                // the screen opens: a screen that does not open leaves no join waiting)
                self.reopen = Some(lobby);
                Manager::dismiss_modal(cx, "customGame");
                cx.view.delay(0.0, Handler { owner: me, id: MM_REOPEN_LOBBY });
            }
        }
    }

    /// A join link's lobby screen: the one up is closed and the screen opens on the link's code (NetLobby.pendingJoin,
    /// set as it opens), the way a custom game invitation's Join reopens it.
    fn open_link_join(&mut self, cx: &mut Cx<'_>) {
        let Some(code) = cx.view.session.main_menu.link_join.take() else { return };
        self.reopen = Some(code);
        if cx.view.manager.get_modal("customGame").is_some() {
            Manager::dismiss_modal(cx, "customGame");
        }
        cx.view.delay(0.0, Handler { owner: cx.w, id: MM_REOPEN_LOBBY });
    }

    /// The session's question (online.onAsk): the manager's dialog over whatever menu the join started from, the
    /// question as its body, its two buttons the words of the question (yes on the left, focused first; no on the
    /// right); Back / Escape is no. A newer question or the session's end takes the dialog down unanswered; the
    /// menu's end answers no.
    fn sync_ask(&mut self, cx: &mut Cx<'_>) {
        let want = cx.view.session.online.ask.clone();
        if self.ask.as_ref().map(|a| a.0) == want.as_ref().map(|a| a.id) {
            return;
        }
        if let Some((_, d)) = self.ask.take() {
            self.exit_dialog(cx, d);
        }
        let Some(a) = want else {
            self.answered = None;
            return;
        };
        if self.answered == Some(a.id) {
            return;
        }
        let (body, yes, no) = ask_text(&a.what);
        let d = Manager::show_dialog(
            cx,
            DialogParams {
                body,
                button_left_label: yes.into(),
                button_right_label: no.into(),
                ..DialogParams::default()
            },
        );
        let me = cx.w;
        if let Some(dl) = cx.view.dialog(d) {
            dl.button_left_clicked().add(Handler { owner: me, id: MM_ASK_YES });
            dl.button_right_clicked().add(Handler { owner: me, id: MM_ASK_NO });
            dl.dialog_dismissed().add(Handler { owner: me, id: MM_ASK_NO });
        }
        self.ask = Some((a.id, d));
    }

    fn exit_dialog(&mut self, cx: &mut Cx<'_>, d: DialogId) {
        cx.visit_dyn(d.0, |c, cx| {
            if let Some(dl) = c.as_dialog() {
                dl.exit(cx);
            }
        });
    }

    fn answer(&mut self, cx: &mut Cx<'_>, yes: bool) {
        if let Some((id, _)) = self.ask.take() {
            self.answered = Some(id);
            cx.online(OnlineCmd::Answer { id, yes });
        }
    }
}

impl Controller for MainMenu {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.session.main_menu.live = Some(me);
        // CanvasRoot's slot ZOrders (UI_MainMenu: Homescreenv2 and UI_Characters 0; ScaleBox_0, the nav and the social
        // panel 1), which the bundle does not carry into the paint order: a widget added at 0 (the pre-match,
        // AddWidgetToCanvas) paints under the last three. Their order among themselves is the child order as before.
        slot_z_order(cx, &["ScaleBox_0", "WBP_GlobalNav_MainMenu", "UI_SocialPanel_MainMenu"], 1);
        let home = cx.view.tree().find_in(cx.w, "Homescreenv2");
        cx.view.tree_mut().widget_mut(home).vis = Vis::Hidden;
        // Init Homescreen(false)
        cx.view.tree_mut().widget_mut(home).vis = Vis::Visible;
        cx.visit::<Homescreen, _>(home, |h, cx| h.init(cx));
        Self::fill(cx);
        // @1442: the name modal while the account has no name of its own (RequireManualUsernameUpdate)
        // Deviation: only while the online service is signed in. The menu tape harness runs with an identity name
        // set (no modal) and the native menus have no such name, so offline the modal would show on every start.
        if cx.prefs.profile.name.is_empty() && cx.view.session.online.connected {
            cx.view.delay(0.0, Handler { owner: me, id: MM_NAME });
        }
        // OnReceivedCustomGameInvite / DequeueCustomGameInvites: the invitations kept while none was up
        cx.view.delay(0.0, Handler { owner: me, id: MM_DEQUEUE });
        let o = &cx.view.session.online;
        if o.name_taken {
            cx.view.delay(0.0, Handler { owner: me, id: MM_ASK_AGAIN });
        }
        // back from an online match that stopped early: DisplayError's notification with the port's text
        if !cx.view.session.main_menu.note.is_empty() {
            cx.view.delay(0.0, Handler { owner: me, id: MM_NOTE });
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if cx.view.session.main_menu.live == Some(cx.w) {
            cx.view.session.main_menu.live = None;
        }
        if let Some((_, d)) = self.ask.take() {
            self.exit_dialog(cx, d);
            // the menu's end answers no
            if let Some(a) = cx.view.session.online.ask.clone() {
                cx.online(OnlineCmd::Answer { id: a.id, yes: false });
            }
        }
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        Self::fill(cx);
        self.sync_ask(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            MM_ASK_YES => self.answer(cx, true),
            MM_ASK_NO => self.answer(cx, false),
            MM_OPEN_FOLDER => cx.sink.open_folder(&self.desync_folder),
            i if i >= MM_TOAST => self.toast_event(cx, i),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            MM_NAME => {
                if cx.view.manager.get_modal("chooseUsername").is_none() {
                    Manager::show_modal(cx, "UI_SetUsername", "chooseUsername", "", "");
                }
            }
            MM_ASK_AGAIN => {
                let refused = cx.view.session.online.name_refused;
                Self::ask_again(cx, refused);
            }
            MM_DEQUEUE => {
                let queued = std::mem::take(&mut cx.view.session.main_menu.queued);
                for i in queued {
                    self.open_invite(cx, &i);
                }
            }
            MM_NOTE => {
                let note = std::mem::take(&mut cx.view.session.main_menu.note);
                if !note.is_empty() {
                    Self::notify(cx, note);
                }
            }
            MM_REOPEN_LOBBY => {
                let Some(code) = self.reopen.take() else { return };
                crate::menu::lobby::set_pending_join(Some(code));
                if Manager::show_modal(cx, "WBP_CustomGame", "customGame", "", "").is_none() {
                    crate::menu::lobby::set_pending_join(None);
                }
            }
            _ => {}
        }
    }
}

/// This frame's online events for the widgets: an invitation, a notice or a refused name to the main menu that is up
/// (an invitation with none is kept for the next), a toast of another player to the toast service. Called once per view
/// tick.
pub fn online_frame(cx: &mut Cx<'_>) {
    // (a join link's code, held by the session until a main menu is up)
    if cx.view.session.main_menu.link_join.is_some()
        && let Some(w) = cx.view.session.main_menu.live
    {
        cx.visit::<MainMenu, _>(w, |m, cx| m.open_link_join(cx));
    }
    if cx.view.session.online.events.is_empty() {
        return;
    }
    let events = cx.view.session.online.events.clone();
    for ev in &events {
        match ev {
            OnlineEvent::Toast { from, name } => {
                let count = cx.view.session.toasts.count();
                crate::menu::toasts::Toasts::received(cx, count, from, name);
            }
            OnlineEvent::Invited(_) | OnlineEvent::Notice(_) | OnlineEvent::NameTaken { .. } => {
                match cx.view.session.main_menu.live {
                    Some(w) => {
                        cx.visit::<MainMenu, _>(w, |m, cx| m.on_event(cx, ev));
                    }
                    None => {
                        if let OnlineEvent::Invited(i) = ev {
                            cx.view.session.main_menu.keep(i.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
