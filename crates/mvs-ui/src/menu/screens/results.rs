//! The results screen after a match: the podium level under the end-of-game
//! widgets (UI_CanvasProgressionContainer's steps), left for the main menu or the PreMatch level.
//!
//! The podium level is asked for through the scene commands (`SceneCmd::Level`, `SeqCmd::Podium`); what its sequence
//! tells (`MenuEvent::Podium`: PodiumEvent, ShowNametag, VictoryVO) comes back through `podium_event`, and the
//! program gives the places of the banners (`PodiumPoints`) each frame to `place_widgets`. The world space widgets
//! (the winner text, the stat sheets) are target widgets the program paints and hands to the scene
//! (`scene_widgets`).
//!
//! Not ported: VictoryVO (the winners' victory line posted at their preview actor: it needs the roster's VO tables),
//! the podium's ambience event (it has no stop key through `MenuSink`). A queue match's
//! session (its votes, the set, the rematch's pm.begin, the set ending in the rematch's wait) is the program's: it
//! hands the widgets what comes (`EOGRematchStatus`) and opens the level.

use mvs_render::menuscene::format::EventArg;
use mvs_render::menuscene::{MenuLevel, PodiumRecord, PodiumStart, ScenePoint, SceneWidget, SeqCmd};

use crate::menu::arcade::num_arcade_levels;
use crate::menu::controller::{Cx, MenuRun};
use crate::menu::manager::{Manager, TransitionParams};
use crate::menu::scene::SceneCmd;
use crate::menu::screen::MenuScreen;
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::wd;
use crate::menu::widgets::end_of_game::{CanvasProgressionContainer, EOGNametag, EOGStatSheet, QueueEnd, StepList};
use crate::menu::widgets::toast_button::{EOGToastButton, add_podium_toast_banner, reset_podium_toasts};
use crate::results::{EndOfGame, podium_order};
use crate::slate::WidgetId;

/// WBP_3dTeam's Small Text keys.
const TEAM_WINNER: &str = "A955A7E741E0BCEB3C250CAD0AE612A1";
const TEAM_FINAL_ROUND: &str = "B17EBE724C8A08E2A14242AE9D7598EF";
const TEAM_ROUND: &str = "0FF230AF4730F9CCD16B74A8DE4E5AAF";
const TEAM_ARCADE_STAGE: &str = "84F0AC594A2CF293331FED8FD87A401A";

/// The classes of the podium level's widgets (the asset's `text.widget` and `banner.*.widget`).
const WINNER_TEXT: &str = "WBP_3dTeam";
const NAMETAG: &str = "UI_EOG_Nametag";
const TOAST_BUTTON: &str = "UI_EOG_ToastButton";
const STAT_SHEET: &str = "UI_EOG_StatSheet";

/// What the program knows of the podium level: a banner's place in world space and where it stands on the screen.
pub trait PodiumPoints {
    /// `Renderer::menu_point`.
    fn point(&self, p: ScenePoint) -> Option<[f64; 3]>;
    /// `Renderer::menu_screen_place`: the projected point in pixels of the output and the distance from the view
    /// origin.
    fn screen_place(&self, at: [f64; 3]) -> Option<([f64; 2], f64)>;
}

/// A screen-space widget of a banner, at its projected place.
#[derive(Debug)]
struct Tag {
    loc: [f64; 3],
    /// The index of the widget in the view's world widgets.
    world: usize,
    shown: bool,
    /// The toast button's widget (its tag shows once the toast step has turned it on).
    toast: Option<WidgetId>,
}

/// The results screen.
#[derive(Debug)]
pub struct ResultsScreen {
    /// UI_CanvasProgressionContainer.
    pub widget: WidgetId,
    /// The record indices by list, your own record first (SortPodiumDataResults).
    winners: Vec<usize>,
    losers: Vec<usize>,
    tags: Vec<Tag>,
    /// WBP_3dTeam's target widget.
    winner_text: Option<WidgetId>,
    /// The stat sheets' target widgets by banner (the sequence call's index and list).
    sheets: Vec<(WidgetId, usize, bool)>,
    /// "ShowStats" has shown the stat sheets.
    stats_shown: bool,
}

/// Shows the results: the podium's level asked through the scene commands, the end-of-game widgets
/// over it; `queue` a queue match's set (setEndOfGame's session; None: not a queue match). The audio goes through the
/// view's sink.
pub fn show(screen: &mut MenuScreen, run: &mut MenuRun<'_>, eog: &EndOfGame, queue: Option<QueueEnd>) -> ResultsScreen {
    screen.with_cx(run, |cx| {
        let matchmade = queue.is_some();
        cx.view.session.eog.set(Some(eog.clone()), queue);
        reset_podium_toasts(cx);
        // the match's end left the fader black
        cx.view.manager.fader = 1.0;
        // VictoryCelebrationActor Init New EOG UI (@16600..17226): the progression's root step (the canvas) is created
        // and added to the viewport, Update Context(NewGuid), SetRootContextForLevel(the root's Context),
        // SetHandleInput(false) (the waitforsequence step turns it on again at the podium's ShowUI), BPI Start.
        let ctx = cx.view.manager.new_guid();
        let root = MenuView::add(cx, "UI_CanvasProgressionContainer");
        // (an arcade round: the Arcade list, IsArcadeModeActive; a queue match: the Matchmade one; local play: the
        // Local list, BPI_GetIsLocalPlay)
        let arcade = eog.arcade.clone().or_else(|| cx.view.session.arcade.round_played().cloned());
        let list = if arcade.is_some() {
            StepList::Arcade
        } else if matchmade {
            StepList::Matchmade
        } else if eog.local_play {
            StepList::Local
        } else {
            StepList::Custom
        };
        cx.visit::<CanvasProgressionContainer, _>(root, |c, _| {
            c.use_list(list);
            c.update_context(&ctx);
        });
        cx.view.manager.set_root_context_for_level(&ctx);
        cx.view.manager.set_handle_input(false);
        cx.visit::<CanvasProgressionContainer, _>(root, |c, cx| c.start(cx));

        // SortPodiumDataResults: your own record first in its list, the others in MatchData order.
        let order = podium_order(eog);
        let (winners, losers): (Vec<usize>, Vec<usize>) = order.into_iter().partition(|&i| eog.players[i].is_winner);
        let Some(me) = eog.players.get(eog.me) else {
            return ResultsScreen::new(root, winners, losers, None);
        };
        // A custom game's MatchFlowID is not LocalPlay and the local player is no spectator, so a banner's ColorType
        // is 1 on your team and 2 on another (Animate EOG Banner Entrance @104).
        let record = |i: usize| {
            let info = eog.info(i);
            PodiumRecord {
                key: info.character,
                skin: info.skin,
                color: u8::from(eog.players[i].team_index != me.team_index),
                banner: info.banner,
            }
        };
        // The level plays its sequences from its first frame; BPI_PodiumEvent goes to the UI steps.
        cx.scene(SceneCmd::Level(MenuLevel::Podium));
        cx.scene(SceneCmd::Sequence(SeqCmd::Podium(Box::new(PodiumStart {
            team_style: eog.team_style,
            winners: winners.iter().map(|&i| record(i)).collect(),
            losers: losers.iter().map(|&i| record(i)).collect(),
            you_won: me.is_winner,
        }))));
        let text = winner_text(cx, eog, &winners);
        // PodiumGameMode BeginPlay: PlayPodiumVictoryMusic = BPI_SetMusicMapState(5), the global switch
        // mus_map_state-mus_victory, and the state state_gameflow_victorypodium.
        cx.sink.set_switch("mus_map_state", "mus_victory");
        cx.sink.set_state("state_gameflow", "state_gameflow_victorypodium");
        cx.view.show_cursor(true);
        Manager::transition(
            cx,
            TransitionParams { next_level: String::new(), kind: 1, delay: 0.0, force: true, ..Default::default() },
        );
        ResultsScreen::new(root, winners, losers, Some(text))
    })
}

impl ResultsScreen {
    fn new(widget: WidgetId, winners: Vec<usize>, losers: Vec<usize>, winner_text: Option<WidgetId>) -> ResultsScreen {
        ResultsScreen { widget, winners, losers, tags: Vec::new(), winner_text, sheets: Vec::new(), stats_shown: false }
    }
}

/// The world space widgets the scene shows this frame and their target widgets (the winner
/// text from the start, the stat sheets once "ShowStats" ran): the program paints each (`MenuView::paint_target`)
/// and hands the picture to `Renderer::menu_widget`.
pub fn scene_widgets(state: &ResultsScreen) -> Vec<(SceneWidget, WidgetId)> {
    let mut out: Vec<(SceneWidget, WidgetId)> =
        state.winner_text.map(|w| (SceneWidget::WinnerText, w)).into_iter().collect();
    if state.stats_shown {
        out.extend(state.sheets.iter().map(|&(w, index, winner)| (SceneWidget::StatSheet { index, winner }, w)));
    }
    out
}

/// WBP_3dTeam (level Blueprint @3069, @3766; the widget's Small Text binding): the big text is the first winner's
/// name (your own record first), or "WINNER" in a team match; the small one "Winner" in free for all, else the round's
/// number: Small Text formats "Round {Number}" with the rounds played (EOGDataArray's length), "Winner" for a 1 vs 1
/// set won in two or at its third round, "Final Round" for a team set's third. A queue set's games take that rule as
/// it is.
/// Deviation: a custom game is one match and has no rounds: the build's "Round 1" is not shown. 1 vs 1 takes the
/// build's own set-end text "Winner", a team match (the big text is "WINNER" already) none: the line keeps its room.
/// The widget is a target widget the scene draws as `SceneWidget::WinnerText`.
fn winner_text(cx: &mut Cx<'_>, eog: &EndOfGame, winners: &[usize]) -> WidgetId {
    let n = cx.view.session.eog.cache.len();
    let queue = cx.view.session.eog.queue.is_some();
    let won_two = cx.view.session.eog.won_in_two_rounds();
    let t = |cx: &Cx<'_>, k: &str| cx.view.game_text(k).to_string();
    let set_text = |cx: &Cx<'_>| {
        if eog.local_play {
            String::new()
        } else if eog.team_style == 2 || (eog.team_style == 0 && (n == 3 || won_two)) {
            t(cx, TEAM_WINNER)
        } else if n == 3 && eog.team_style == 1 {
            t(cx, TEAM_FINAL_ROUND)
        } else {
            t(cx, TEAM_ROUND).replace("{Number}", &n.to_string())
        }
    };
    // WBP_3dTeam's arcade arm first (@1716-2365): "Stage {Number} of {NumRounds}", ArcadeRound and GetNumArcadeLevels
    let arcade = eog.arcade.clone().or_else(|| cx.view.session.arcade.round_played().cloned());
    let small = if let Some(a) = arcade {
        t(cx, TEAM_ARCADE_STAGE)
            .replace("{Number}", &a.round.to_string())
            .replace("{NumRounds}", &num_arcade_levels(a.difficulty).to_string())
    } else if queue && n > 0 {
        set_text(cx)
    } else if eog.team_style == 1 {
        " ".to_string()
    } else {
        t(cx, TEAM_WINNER)
    };
    // Unverified: "WINNER" is the port's own literal; the build's team name text is not read.
    let big = if eog.team_style == 1 {
        "WINNER".to_string()
    } else {
        winners.first().map_or(String::new(), |&i| eog.name(i).to_string())
    };
    let w = MenuView::create(cx, WINNER_TEXT);
    let b = cx.view.tree().find_in(w, "bigText");
    wd(cx, b).text = big;
    let s = cx.view.tree().find_in(w, "smallText");
    wd(cx, s).text = small;
    MenuView::add_target_widget(cx, w);
    w
}

/// A podium event of the level's sequence director (`MenuEvent::Podium { call, args }`).
pub fn podium_event(
    screen: &mut MenuScreen,
    run: &mut MenuRun<'_>,
    state: &mut ResultsScreen,
    points: &dyn PodiumPoints,
    call: &str,
    args: &[EventArg],
) {
    screen.with_cx(run, |cx| match call {
        "PodiumEvent" => {
            let name = match args.first() {
                Some(EventArg::Str(s)) => s.clone(),
                Some(EventArg::Num(n)) => n.to_string(),
                _ => String::new(),
            };
            let root = state.widget;
            cx.visit::<CanvasProgressionContainer, _>(root, |c, cx| c.podium_event(cx, &name));
            // BP_Banner_PM BPI_PodiumEvent "ShowStats" (@5917): PopulateStats, then the Stat Sheet component is shown.
            if name == "ShowStats" {
                for &(sheet, ..) in &state.sheets {
                    cx.visit::<EOGStatSheet, _>(sheet, |s, cx| s.populate_stats(cx));
                }
                state.stats_shown = true;
            }
        }
        "ShowNametag" => {
            let (Some(EventArg::Num(i)), Some(EventArg::Bool(w))) = (args.first(), args.get(1)) else { return };
            show_nametag(cx, state, points, *i as usize, *w);
        }
        // Not ported: VictoryVO, the winners' victory line.
        _ => {}
    });
}

/// BPI_ShowNametag (level Blueprint @9368 -> the preview's banner): the banner's EOG Nametag component is shown and
/// its widget populated with the record, YourTeam (the record's TeamIndex is yours), IsMe (@10001: the record's
/// User.AccountID is yours and the match is not local play) and the banner's Spectator View (false): the plate takes
/// palette 0 / 1 by YourTeam and your own tag shows its "me" plate. The Stat Sheet's widget gets the record too
/// (shown by "ShowStats"), its highlight palette 0 / 1 the same way.
/// The banner's ToastButton widget gets Populate(record, YourTeam, IsMe) too (BAN @5703-5810); the component stays
/// hidden in game until the EOG step "toast" shows it (BPI_ShowToastButton).
/// The nametag and the toast button are in screen space: the widget sits at the component's projected location,
/// rounded to a pixel (`place_widgets`). The stat sheet is in world space: the scene shows its picture on the banner.
fn show_nametag(cx: &mut Cx<'_>, state: &mut ResultsScreen, points: &dyn PodiumPoints, index: usize, winner: bool) {
    let Some(eog) = cx.view.session.eog.data.clone() else { return };
    let Some(loc) = points.point(ScenePoint::Nametag { index, winner }) else { return };
    let list = if winner { &state.winners } else { &state.losers };
    let Some(&i) = list.get(index) else { return };
    if state.tags.iter().any(|t| t.loc == loc) {
        return;
    }
    let p = eog.player(i);
    let your_team = eog.players[i].team_index == eog.players[eog.me].team_index;
    let is_me = i == eog.me;
    let w = MenuView::create(cx, NAMETAG);
    let world = MenuView::add_world_widget(cx, w);
    state.tags.push(Tag { loc, world, shown: true, toast: None });
    cx.visit::<EOGNametag, _>(w, |c, cx| c.populate(cx, &p, your_team, is_me));
    // Deviation (the user's rule): a bot's banner has no toast button (the build's has one; a bot's toast reaches no
    // one and spends a toast)
    if !p.info.bot
        && let Some(tloc) = points.point(ScenePoint::ToastButton { index, winner })
    {
        let tw = MenuView::create(cx, TOAST_BUTTON);
        let world = MenuView::add_world_widget(cx, tw);
        state.tags.push(Tag { loc: tloc, world, shown: false, toast: Some(tw) });
        cx.visit::<EOGToastButton, _>(tw, |c, cx| c.populate(cx, p.clone(), your_team, is_me));
        add_podium_toast_banner(cx, tw);
    }
    let sw = MenuView::create(cx, STAT_SHEET);
    MenuView::add_target_widget(cx, sw);
    cx.visit::<EOGStatSheet, _>(sw, |c, _| c.populate(i));
    state.sheets.push((sw, index, winner));
}

/// The frame's part: the nametags and toast buttons at their banners' projected places (rounded
/// to a pixel; hidden off the screen or while a toast button waits for its step).
pub fn place_widgets(screen: &mut MenuScreen, state: &mut ResultsScreen, points: &dyn PodiumPoints) {
    let view = &mut screen.view;
    for w in view.session.podium_toasts.take_shown() {
        for t in &mut state.tags {
            if t.toast == Some(w) {
                t.shown = true;
            }
        }
    }
    for t in &state.tags {
        let at = if t.shown { points.screen_place(t.loc) } else { None };
        view.place_world_widget(t.world, at);
    }
}

/// The screen's removal: the widgets leave the toast service, the end-of-game state is
/// emptied.
pub fn finish(screen: &mut MenuScreen, run: &mut MenuRun<'_>) {
    screen.with_cx(run, |cx| {
        reset_podium_toasts(cx);
        for w in std::mem::take(&mut cx.view.session.eog.toast_owners) {
            let t = &mut cx.view.session.toasts;
            t.on_changed.remove_owner(w);
            t.on_decremented.remove_owner(w);
        }
        cx.view.session.eog.set(None, None);
    });
    screen.remove(run);
}
