//! The pre-match screen: UI_PreMatch over the PreMatch level; its children; what it
//! is given and ends with.
//!
//! A custom game runs the widget in rematch mode (bIsRematch and bIsCustomGame both true), an arcade round in the
//! arcade's (the run in `Session::arcade`, its match made at the screen's end). The queue's seamless entry over the
//! main menu's scene and the online rematch path are not here yet.

pub mod card_row;
pub mod child_widgets;
pub mod path_plate;
pub mod setup;
pub mod widget;

/// The session calls of the widget.
pub(super) mod widget_api {
    pub use super::widget::{last_chosen, session, set_session};
}

use mvs_net::service::api::{PartyMember, PartyView};
use mvs_protocol::online::PARTY_MAX;
use mvs_render::menuscene::{MenuEvent, MenuLevel};

use crate::menu::controller::MenuRun;
use crate::menu::lobby::loadout::BotPool;
use crate::menu::lobby::{LocalLobby, PreMatchPick, SetupSource};
use crate::menu::pictures::{roster_pictures, skin_pictures};
use crate::menu::scene::SceneCmd;
use crate::menu::screen::MenuScreen;
use crate::menu::view::MenuView;
use crate::slate::WidgetId;
use setup::{PreMatchCall, PreMatchEnd, PreMatchSetup, PreMatchStart};

/// The picture owner of the screen's holds.
const PICTURES: &str = "prematch";

/// The pre-match screen: UI_PreMatch's widget and the lobby it ends the match with.
#[derive(Debug)]
pub struct PreMatchScreen {
    pub widget: WidgetId,
    lobby: Option<Box<LocalLobby>>,
    setup: PreMatchSetup,
    partner: PartnerBadge,
    /// The match's path (pm.path): this player's far notice.
    path: path_plate::PathPlate,
}

/// The level's partner for PartnerNameplate_PM: what BeginPlay's UpdatePlayerBadge or UpdateHomescreenParty and each
/// later OnPartyChanged left on it, until PlayersFound hides it.
#[derive(Debug, Default)]
struct PartnerBadge {
    /// The party as last seen (None: not seen yet, the level's BeginPlay to come).
    party: Option<Option<PartyView>>,
    shown: Option<PartyMember>,
    players_found: bool,
    /// BP_Banner_PM_Player2's Enable Social Menu as last called: the partner click target on.
    click: bool,
}

impl PartnerBadge {
    /// PlayTransition(`name`)'s Enable Social Menu (PreMatchScreen::partner_transition).
    fn transition(&mut self, name: &str, party: Option<&PartyView>) {
        match name {
            "Matchmaking" => self.click = false,
            "CancelMatchmaking" => self.click = party.is_some_and(|p| p.members.len() >= PARTY_MAX),
            _ => {}
        }
    }
}

/// UpdateHomescreenParty and OnPartyChanged (@22253, @24851-25217, alone @26040): the party's member other than this
/// player (the loop leaves the last one on the nameplate), none alone.
fn homescreen_partner(party: Option<&PartyView>) -> Option<PartyMember> {
    let p = party?;
    p.members.iter().rfind(|m| m.id != p.me).cloned()
}

/// UpdatePlayerBadge @37271 (BeginPlay with bIsRematch, @725-761): a full party (GetPartyData IsPartyFull @37503)
/// takes its JoinedMembers loop @15852 -> @13416-15342, the member other than this player; else the rematch players'
/// loop @10312 -> @7860-10223: each player of this player's team with another account id (@8908-9058), its
/// FetchHydraUser's Username and ProfileIconPath (@9241-9872). The last one found stays on the nameplate.
// Unverified: the build's FetchHydraUser of a bot's account id (@9241) is not read; the port takes humans alone.
// Unverified: a team-mate's ProfileIconPath (FetchHydraUser @9872) is read as the icon asset its game sent
// (net_cosmetics.icon); a team-mate with none (an offline custom game's) shows no icon.
fn player_badge_partner(setup: &PreMatchSetup, party: Option<&PartyView>) -> Option<PartyMember> {
    if let Some(p) = party.filter(|p| p.members.len() >= PARTY_MAX) {
        return p.members.iter().rfind(|m| m.id != p.me).cloned();
    }
    let me = setup.players.get(setup.me)?;
    setup.players.iter().rfind(|p| p.team == me.team && p.account_id != me.account_id && !p.is_bot).map(|p| {
        PartyMember {
            id: p.account_id.clone(),
            name: p.username.clone(),
            icon: p.net_cosmetics.as_ref().map(|c| c.icon.asset.clone()).unwrap_or_default(),
        }
    })
}

/// The level's bIsRematch (PGIMatchmakingHandler): a custom game's start, an arcade run's rematch; a queue's
/// pre-match and an arcade run's first entry are not.
fn is_rematch(setup: &PreMatchSetup) -> bool {
    match &setup.arcade {
        Some(run) => run.rematch,
        None => setup.net.as_ref().is_none_or(|n| n.queue.is_none()),
    }
}

/// A bot's name is its character's DisplayName, from the same data on every peer; a character
/// the menu data does not have keeps the lobby's name.
/// Deviation: in the build the name is the backend's User.Username of the bot, which no file of it holds.
pub fn bot_username(
    fighters: &indexmap::IndexMap<String, crate::bundle::FighterInfo>,
    roster: &[crate::bundle::RosterEntry],
    character: Option<&str>,
    lobby_name: &str,
) -> String {
    let named = character.filter(|c| !c.is_empty()).and_then(|c| {
        fighters
            .get(c)
            .map(|f| f.display_name.clone())
            .or_else(|| roster.iter().find(|r| r.fighter.as_deref() == Some(c)).map(|r| r.display_name.clone()))
    });
    named.filter(|n| !n.is_empty()).unwrap_or_else(|| lobby_name.to_string())
}

/// Shows the pre-match screen: UI_PreMatch added over the level's fade (BPI_AddPreMatchUI: Create and
/// AddToViewport(0)), the set-up handed in, the PreMatch level asked through the scene commands.
///
/// An open-level path arrives under the old level's fade to black (the custom lobby's Start): the build keeps
/// UI_Global's one fader across OpenLevel, and UI_PreMatch's first SetState fades in from it (PM@57292-57603), so the
/// first build (Construct's Delay(0.2), PM@61163) is never seen.
pub fn show(screen: &mut MenuScreen, run: &mut MenuRun<'_>, start: PreMatchStart) -> PreMatchScreen {
    let PreMatchStart { mut setup, lobby } = start;
    screen.with_cx(run, |cx| {
        cx.view.manager.fader = 1.0;
        // (an arcade round's bot keeps the run's name)
        let arcade = setup.arcade.is_some();
        for p in &mut setup.players {
            if p.is_bot && !arcade {
                p.username = bot_username(cx.view.fighters(), cx.view.roster(), p.character.as_deref(), &p.username);
            }
        }
        widget_api::set_session(Some(setup.clone()));
        // The skins of the fighter the grid opens on (GetLastChosenCharacter) and the tiles stay for the screen's
        // life: the grid enters with them in. The shadow under the grid is a picture of UI_PreMatch's tree.
        let last = widget_api::last_chosen(cx.prefs);
        let first = cx
            .view
            .roster()
            .iter()
            .find(|r| Some(&r.slug) == last.as_ref() || (r.fighter.is_some() && r.fighter == last))
            .or_else(|| cx.view.roster().first())
            .and_then(|r| r.fighter.clone());
        let mut names = roster_pictures(cx.view);
        names.extend(skin_pictures(cx.view, first.as_deref()));
        names.push("UI_CharacterGrid_Shadow".to_string());
        cx.view.hold_pictures(PICTURES, names.iter().map(String::as_str));
        // (the lobby's team style before the level opens, the level and the previews read it)
        cx.scene(SceneCmd::TeamStyle(Some(setup.team_style)));
        cx.scene(SceneCmd::Level(MenuLevel::PreMatch));
        let widget = MenuView::add(cx, "UI_PreMatch");
        if let Some(w) = cx.view.controller::<widget::PreMatch>(widget) {
            w.set_level(true);
        }
        cx.view.show_cursor(true);
        PreMatchScreen {
            widget,
            lobby,
            setup,
            partner: PartnerBadge::default(),
            path: path_plate::PathPlate::default(),
        }
    })
}

impl PreMatchScreen {
    /// Whether a level is behind the screen (false: the screen times its face-off and clash itself).
    pub fn set_level(&self, screen: &mut MenuScreen, on: bool) {
        if let Some(w) = screen.view.controller::<widget::PreMatch>(self.widget) {
            w.set_level(on);
        }
    }

    /// The calls the widget left for the sound side since the last take.
    pub fn take_calls(&self, screen: &mut MenuScreen) -> Vec<PreMatchCall> {
        screen.view.controller::<widget::PreMatch>(self.widget).map(|w| w.take_calls()).unwrap_or_default()
    }

    /// A thing the level told the screen: a sequence's UI event, a world place the screen's widgets stand at, the
    /// face-off's end.
    /// (A sequence's Fade track is the scene's own: the post chain fades with the screen's fader.)
    pub fn scene_event(&self, screen: &mut MenuScreen, run: &mut MenuRun<'_>, ev: &MenuEvent) {
        let w = self.widget;
        screen.with_cx(run, |cx| match ev {
            MenuEvent::SequenceUi(name) => {
                cx.visit::<widget::PreMatch, _>(w, |p, cx| p.sequence_event(cx, name));
            }
            MenuEvent::WorldLocation { id, at } => {
                cx.visit::<widget::PreMatch, _>(w, |p, _| p.send_world_location(id, *at));
            }
            MenuEvent::TransitionComplete(name) => {
                cx.visit::<widget::PreMatch, _>(w, |p, cx| p.transition_complete(cx, name));
            }
            _ => {}
        });
    }

    /// The widget's stored world places projected with the level's camera of the moment (each frame).
    ///
    /// The banners' nametags (`plates`: the preview slot and the world place of each banner shown) are placed at
    /// their projected points, `place` giving a point in pixels and its distance (`Renderer::menu_screen_place`);
    /// a banner not shown has none. The match's path (`path_plate`) shows its far notice.
    pub fn project(
        &mut self,
        screen: &mut MenuScreen,
        run: &mut MenuRun<'_>,
        project: impl Fn([f64; 3]) -> Option<[f64; 2]>,
        plates: &[(usize, [f64; 3])],
        place: impl Fn([f64; 3]) -> Option<([f64; 2], f64)>,
    ) {
        let w = self.widget;
        let ids = screen.view.controller::<widget::PreMatch>(w).map(|p| p.plate_widgets()).unwrap_or_default();
        for (slot, &i) in ids.iter().enumerate() {
            let at = plates.iter().find(|p| p.0 == slot).and_then(|p| place(p.1));
            screen.view.place_world_widget(i, at);
        }
        self.path.take(&screen.view.session.online.events);
        if !self.path.idle() {
            // (this player's slot in the widget's own players: a queue's found match's, a rematch's set-up's)
            let me = screen.view.controller::<widget::PreMatch>(w).and_then(|p| p.preview_slots()).map(|s| s.1);
            let path = &mut self.path;
            screen.with_cx(run, |cx| path.frame(cx, me));
        }
        screen.with_cx(run, |cx| {
            cx.visit::<widget::PreMatch, _>(w, |p, _| {
                for (id, at) in p.world_points() {
                    p.set_projection(&id, project(at));
                }
            });
        });
    }

    /// The screen's end once the fade to the level is through (None: still up). `level`: the
    /// level the manager opened. A match start makes the lobby's match, with the picks, for the program
    /// (`lobby::take_start`); Back to Lobby keeps the lobby for the main menu (`Session::lobby`).
    pub fn take_end(&mut self, screen: &mut MenuScreen, run: &mut MenuRun<'_>, level: &str) -> PreMatchEnd {
        let exit = screen.view.controller::<widget::PreMatch>(self.widget).and_then(|w| w.take_exit());
        // (a Waves run's screen, taken whatever its end)
        let waves = std::mem::take(&mut screen.view.session.waves.choosing);
        widget_api::set_session(None);
        screen.view.release_pictures(PICTURES);
        let picks = match exit {
            Some(setup::Exit::StartMatch(p)) if level != "New_MainMenu" => p,
            // Wait in the Lab (BPI_TransitionToTraining): the queue's screen goes with the level, the search runs on
            // (the program keeps the queue and returns to this screen from the Lab)
            None if level == "TrainingContainer" => {
                return PreMatchEnd::Match { level: level.to_string(), picks: Vec::new() };
            }
            Some(setup::Exit::Kick(text)) => {
                // the main menu shows the text once as its "Error!" note
                if !text.is_empty() {
                    screen.view.session.main_menu.note = text;
                }
                if let Some(l) = self.lobby.take() {
                    screen.view.session.lobby = Some(l);
                }
                return PreMatchEnd::BackToLobby;
            }
            Some(setup::Exit::BackToLobby) | Some(setup::Exit::StartMatch(_)) | None => {
                if let Some(l) = self.lobby.take() {
                    screen.view.session.lobby = Some(l);
                }
                return PreMatchEnd::BackToLobby;
            }
        };
        if let Some(lobby) = self.lobby.take() {
            let m = screen.with_cx(run, |cx| start_match(cx, lobby, picks.clone()));
            crate::menu::lobby::with_carry(|c| c.start = Some(Box::new(m)));
        } else if self.setup.arcade.is_some() {
            // (the arcade's picks, then its match: the round's match with the local pick)
            let username = self.setup.players.get(self.setup.me).map(|p| p.username.clone()).unwrap_or_default();
            let m = screen.with_cx(run, |cx| {
                let bundle = cx.view.bundle.clone();
                let menu = bundle.menu.as_ref()?;
                cx.view.session.arcade.picks(menu, &picks);
                let drawable = |c: &str| cx.view.fighters().contains_key(c);
                cx.view.session.arcade.arcade_match(menu, &cx.prefs.hydra, &cx.prefs.input, &drawable, &username)
            });
            if let Some(m) = m {
                crate::menu::lobby::with_carry(|c| c.start = Some(Box::new(m)));
            }
        } else if waves {
            // (the port's own Waves run: its match with the local pick, `menu/waves.rs`)
            let m = screen.with_cx(run, |cx| crate::menu::waves::start_from_picks(cx, &picks));
            if let Some(m) = m {
                crate::menu::lobby::with_carry(|c| c.start = Some(Box::new(m)));
            }
        }
        PreMatchEnd::Match { level: level.to_string(), picks }
    }

    /// The partner PartnerNameplate_PM shows now (None: hidden), `party` the party of the moment: at the level's
    /// first frame UpdatePlayerBadge (a rematch) or UpdateHomescreenParty, at each party change after it
    /// OnPartyChanged (the level binds it at BeginPlay with no rematch test, @571-594), hidden from PlayersFound on
    /// (@27058) until a party change shows it again.
    pub fn partner(&mut self, screen: &mut MenuScreen, party: Option<&PartyView>) -> Option<PartyMember> {
        let found = screen.view.controller::<widget::PreMatch>(self.widget).is_some_and(|w| w.players_found_sent());
        let b = &mut self.partner;
        match &b.party {
            None => {
                b.shown = if is_rematch(&self.setup) {
                    player_badge_partner(&self.setup, party)
                } else {
                    homescreen_partner(party)
                };
                b.click = b.shown.is_some();
            }
            Some(seen) if seen.as_ref() != party => {
                b.shown = homescreen_partner(party);
                b.click = b.shown.is_some();
            }
            Some(_) => {}
        }
        b.party = Some(party.cloned());
        if found && !b.players_found {
            b.shown = None;
            // PlayersFound's Enable Social Menu(false) @13286
            b.click = false;
        }
        b.players_found = found;
        b.shown.clone()
    }

    /// The level's PlayTransition(`name`) for the partner click target: "Matchmaking" ends in
    /// BP_Banner_PM_Player2.Enable Social Menu(false) (@27769), "CancelMatchmaking" in Enable Social
    /// Menu(IsPartyFull) (@27543); the next OnPartyChanged sets it again (`partner`).
    pub fn partner_transition(&mut self, name: &str, party: Option<&PartyView>) {
        self.partner.transition(name, party);
    }

    /// The partner click target is on (BP_Banner_PM_Player2's Enable Social Menu as last called).
    pub fn partner_click_on(&self) -> bool {
        self.partner.click
    }

    pub fn setup(&self) -> &PreMatchSetup {
        &self.setup
    }
}

/// The lobby's match with the picks and this machine's loadout.
fn start_match(
    cx: &mut crate::menu::controller::Cx<'_>,
    lobby: Box<LocalLobby>,
    picks: Vec<PreMatchPick>,
) -> crate::menu::lobby::LobbyMatch {
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
    lobby.start_with_picks(picks, &src, &mut random)
}

#[cfg(test)]
mod tests {
    use super::*;
    use setup::PreMatchPlayer;

    fn member(id: &str) -> PartyMember {
        PartyMember { id: id.into(), name: format!("{id} name"), icon: format!("{id} icon") }
    }

    fn party(ids: &[&str], me: &str) -> PartyView {
        PartyView { leader: ids[0].into(), members: ids.iter().map(|i| member(i)).collect(), me: me.into() }
    }

    fn player(id: &str, team: u8, bot: bool) -> PreMatchPlayer {
        PreMatchPlayer {
            account_id: id.into(),
            username: format!("{id} name"),
            team,
            is_bot: bot,
            ..Default::default()
        }
    }

    fn two_v_two(mate_bot: bool) -> PreMatchSetup {
        let players = vec![player("me", 0, false), player("foe", 1, false), player("mate", 0, mate_bot)];
        PreMatchSetup { players, me: 0, team_style: 1, ..Default::default() }
    }

    #[test]
    fn badge_takes_the_full_party_else_the_human_team_mate() {
        let s = two_v_two(false);
        assert!(is_rematch(&s));
        // a full party: its other member, whoever the team-mate is
        let full = party(&["me", "pal"], "me");
        assert_eq!(player_badge_partner(&s, Some(&full)).map(|m| m.id), Some("pal".into()));
        // no party: the team's other human, no icon without its cosmetics
        let m = player_badge_partner(&s, None).unwrap();
        assert_eq!((m.id.as_str(), m.name.as_str(), m.icon.as_str()), ("mate", "mate name", ""));
        // a bot team-mate: none (Unverified)
        assert_eq!(player_badge_partner(&two_v_two(true), None), None);
        // the main menu's rule: the member other than me, none alone
        assert_eq!(homescreen_partner(Some(&full)).map(|m| m.id), Some("pal".into()));
        assert_eq!(homescreen_partner(Some(&party(&["me"], "me"))), None);
        assert_eq!(homescreen_partner(None), None);
    }

    #[test]
    fn matchmaking_turns_the_click_target_off_and_a_cancel_turns_it_on_for_a_full_party() {
        let full = party(&["me", "pal"], "me");
        let mut b = PartnerBadge { click: true, ..PartnerBadge::default() };
        b.transition("PlayersFound", Some(&full));
        assert!(b.click, "another transition leaves it");
        b.transition("Matchmaking", Some(&full));
        assert!(!b.click, "Matchmaking: off");
        b.transition("CancelMatchmaking", Some(&full));
        assert!(b.click, "a cancel in a full party: on");
        b.transition("CancelMatchmaking", Some(&party(&["me"], "me")));
        assert!(!b.click, "a cancel alone: off");
    }
}
