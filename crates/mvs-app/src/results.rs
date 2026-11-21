//! The results screen after a match
//! over the podium level, then where the match's end goes.
//!
//! Deviation: the podium level is the render track's (`PodiumLevel`: its methods panic while unported). The screen
//! asks the renderer for the level under `catch_unwind`; when the level is not there the screen runs over black and
//! the host plays the level's two events itself ("ShowUI" a second after the screen opens, "ShowStats" a second
//! after that), so the end-of-game steps run on without the podium's sequence. With the level there the renderer's
//! events drive the screen, in order.
//!
//! A queue match's end: the set's games in the session's cache, the
//! Matchmade list, the bar's vote and PLAY AGAIN's leave sent, the votes and the set handed to the bar once its Enter
//! has run, the rematch's pm.begin opening the PreMatch level a second later, the set ending in the rematch's wait
//! taking the screen to the main menu with its note.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;

use mvs_render::Renderer;
use mvs_render::gpu::{TexDesc, TexKind, TexViews, Texture};
use mvs_render::menuscene::format::EventArg;
use mvs_render::menuscene::{MenuEvent, MenuLevel, ScenePoint, SceneWidget};
use mvs_ui::element::{ElementList, Ui};
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::manager::{Manager, TransitionParams};
use mvs_ui::menu::online::{KickNote, OnlineCmd};
use mvs_ui::menu::scene::SceneCmd;
use mvs_ui::menu::screens::pre_match::setup::PreMatchStart;
use mvs_ui::menu::screens::results::{self, PodiumPoints, ResultsScreen};
use mvs_ui::menu::widgets::end_of_game::{EOGRematchStatus, QueueEnd};
use mvs_ui::menu::{MenuExit, MenuInput, MenuScreen};
use mvs_ui::results::EndOfGame;
use mvs_ui::slate::WidgetId;
use mvs_ui::text::Fonts;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

use crate::app::{Frame, Go, MatchEnd, MenuReturn, Shell, go_of};
use crate::menu_audio::MenuAudio;
use crate::menus::{MenuHost, menu_run, menu_view};
use crate::{Error, Result};

const NO_UI: &str = "the results screen needs the UI (--no-ui)";
const NO_STORE: &str = "no store is open";
/// The host's own "ShowUI" and "ShowStats" while the podium level is not there, s after the screen opens.
const SYNTH_SHOW_UI: f64 = 1.0;
const SYNTH_SHOW_STATS: f64 = 2.0;

/// What the results screen is opened with: the match's EndOfGameData and where the match's end goes after it.
#[derive(Debug)]
pub struct ResultsStart {
    pub eog: EndOfGame,
    pub then: MatchEnd,
    /// `--shot-results`: the frame this many seconds after the screen opens written as `<out>/results.png`, then the
    /// screen left.
    pub shot: Option<f64>,
    /// Where the shot goes.
    pub out: PathBuf,
}

/// The results screen running over the podium.
pub struct ResultsHost {
    screen: MenuScreen,
    state: ResultsScreen,
    then: Option<MatchEnd>,
    /// The screen's clock, s.
    clock: f64,
    /// The podium level asked for did not come (its methods are unported): the host plays its events.
    no_podium: bool,
    /// The synthetic events still to come: (due time, the "PodiumEvent" argument).
    synth: Vec<(f64, &'static str)>,
    shot: Option<f64>,
    /// V is pressed at this time (None: no press due), released the frame after.
    press: Option<f64>,
    release: bool,
    /// The shot is written: the screen goes on the next frame.
    shot_taken: bool,
    out: PathBuf,
    /// The level's effects (the light beams, the banners' elements), made when the level opens.
    fx: Option<mvs_render::fx::Fx>,
    /// The world space widgets' render targets (the winner text, the stat sheets), each at its widget's size.
    targets: Vec<(SceneWidget, Texture)>,
    /// The elements of a target's paint.
    target_list: ElementList,
    /// A queue match's end: its set's queue (None: not a queue match).
    queue: Option<mvs_protocol::online::QueueMode>,
    /// The set's votes handed to the bar, and whether the set's outcome was.
    votes_seen: usize,
    set_seen: bool,
    /// The rematch's PreMatch level opens at this screen time (the rematch began; Delay(1.0)).
    rematch_at: Option<f64>,
    /// The PreMatch level was opened on the rematch.
    rematch: bool,
    /// The set ended in the rematch's wait.
    ended_seen: bool,
}

impl std::fmt::Debug for ResultsHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultsHost").field("clock", &self.clock).field("no_podium", &self.no_podium).finish()
    }
}

/// The podium's places from the renderer.
struct Points<'a>(Option<&'a Renderer>);

impl PodiumPoints for Points<'_> {
    fn point(&self, p: ScenePoint) -> Option<[f64; 3]> {
        // (a stubbed point is no podium)
        guarded(|| self.0?.menu_point(p)).flatten()
    }

    fn screen_place(&self, at: [f64; 3]) -> Option<([f64; 2], f64)> {
        guarded(|| self.0?.menu_screen_place(at)).flatten()
    }
}

/// A renderer call that may be one of the unported menuscene stubs (`stub!`: "not implemented: menuscene/...").
/// Its panic is the answer "not there"; any other panic goes on.
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(p) => {
            let text = p.downcast_ref::<String>().map(String::as_str).or_else(|| p.downcast_ref::<&str>().copied());
            if text.is_some_and(|t| t.starts_with("not implemented: menuscene/")) { None } else { resume_unwind(p) }
        }
    }
}

impl ResultsHost {
    /// The results screen shown; its scene commands (the podium level) go to the renderer on
    /// the first frame.
    pub fn new(shell: &mut Shell, start: ResultsStart) -> Result<ResultsHost> {
        let queue = shell.online.as_ref().and_then(|h| h.borrow().eog_queue());
        let mut view = menu_view(shell)?;
        view.resize(shell.swap.size);
        // A queue match's end: the set's games (the build's EOGDataCache: a set's first game starts it, a match out of
        // a set empties it).
        if queue.is_some() {
            let won = start.eog.players.get(start.eog.me).is_some_and(|p| p.is_winner);
            view.session.eog.cache.push(won);
        } else {
            view.session.eog.cache.clear();
        }
        let queue_end = queue.map(|(mode, ts)| QueueEnd {
            set_wins: i32::try_from(mvs_protocol::queue_rules::set_wins(mode)).unwrap_or(2),
            team_style: usize::from(ts),
        });
        let Shell { ui, settings, audio, music, window, .. } = shell;
        let ui = ui.as_mut().ok_or_else(|| Error::Match(NO_UI.into()))?;
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = menu_run(ui, settings, &mut sink);
        let mut screen = MenuScreen::new(view, &mut run);
        let state = results::show(&mut screen, &mut run, &start.eog, queue_end);
        Ok(ResultsHost {
            screen,
            state,
            then: Some(start.then),
            clock: 0.0,
            no_podium: false,
            synth: Vec::new(),
            shot: start.shot,
            shot_taken: false,
            press: shell.results_press,
            release: false,
            out: start.out,
            fx: None,
            targets: Vec::new(),
            target_list: ElementList::default(),
            queue: queue.map(|q| q.0),
            votes_seen: 0,
            set_seen: false,
            rematch_at: None,
            rematch: false,
            ended_seen: false,
        })
    }

    /// The queue set's end session against the screen:
    /// what the widgets handed (the vote, out of the set) sent, what the session heard handed to them.
    fn queue_news(
        &mut self,
        hub: &std::rc::Rc<std::cell::RefCell<crate::online::hub::OnlineHub>>,
        run: &mut MenuRun<'_>,
    ) {
        let eog = &mut self.screen.view.session.eog;
        let vote = eog.rematch_vote.take();
        let left = std::mem::take(&mut eog.left_queue);
        let play_again = eog.play_again;
        let v = {
            let mut h = hub.borrow_mut();
            let now = h.clock();
            if let Some(yes) = vote {
                h.run(OnlineCmd::Vote(yes), &|_| None, now);
            }
            if left {
                h.run(OnlineCmd::LeaveSet, &|_| None, now);
            }
            let v = h.queue_end.as_ref().map(|q| q.view().clone());
            // (a rematch PLAY AGAIN beat is let go: its pm.leave went with PLAY AGAIN; the hub's open pre-match would
            // otherwise be entered on a later PreMatch level)
            if play_again
                && v.as_ref().is_none_or(|v| v.rematch_begun)
                && let Some(pm) = &mut h.hub.current
                && pm.open()
                && pm.begin.rematch.is_some()
            {
                pm.close();
            }
            let Some(v) = v else { return };
            v
        };
        // (the bar's listeners bind at its Enter and are given what came before)
        if let Some(bar) = Self::rematch_bar(&self.screen.view) {
            let (seen, set_seen) = (self.votes_seen, self.set_seen);
            let done = self.screen.with_cx(run, |cx| {
                cx.visit::<EOGRematchStatus, _>(bar, |b, cx| {
                    if !b.listening() {
                        return (seen, set_seen);
                    }
                    for &(slot, yes) in v.votes.iter().skip(seen) {
                        b.update_vote_for_player(cx, slot as usize, yes);
                    }
                    let decided = v.set.as_ref().is_some_and(|s| !s.rematch);
                    if decided && !set_seen {
                        b.set_decided(cx);
                    }
                    (v.votes.len(), set_seen || decided)
                })
            });
            if let Some((s, d)) = done {
                self.votes_seen = s;
                self.set_seen = d;
            }
        }
        if play_again {
            return;
        }
        // The rematch (PGI_MatchmakingHandler @29588..29602: CleanupForRematch, Delay(1.0), then the PreMatch level,
        // @562: UnfocusAll, Disable All Interactors, BPI_ClearReferences)
        if v.rematch_begun && !self.rematch && self.rematch_at.is_none() {
            self.rematch_at = Some(self.clock + 1.0);
        }
        if self.rematch_at.is_some_and(|t| self.clock >= t) {
            self.rematch_at = None;
            self.rematch = true;
            self.screen.with_cx(run, |cx| {
                for p in 0..4 {
                    Manager::unfocus_all(cx, p);
                }
                Manager::dismiss_all_modals(cx);
                Manager::open_level_now(cx, "PreMatch", "Lobby");
            });
        }
        // The set ended in the rematch's wait: the main menu with the pre-match leave's note (OnMatchAbandonedByPlayer
        // MH@31771, MH@34468; PM@51907 with no name), by the ExitToMainMenu transition
        if let Some((why, name)) = &v.ended
            && !self.ended_seen
        {
            self.ended_seen = true;
            self.rematch_at = None;
            self.rematch = false;
            let note = if !name.is_empty() {
                KickNote::LeftNamed(name.clone())
            } else if *why == mvs_protocol::messages::PmEndWhy::Error {
                KickNote::Error
            } else {
                KickNote::Left
            };
            self.screen.view.session.main_menu.note = mvs_ui::menu::online_text::kick_text(&note);
            self.screen.with_cx(run, |cx| {
                for p in 0..4 {
                    Manager::unfocus_all(cx, p);
                }
                Manager::transition(
                    cx,
                    TransitionParams {
                        next_level: "New_MainMenu".into(),
                        kind: 0,
                        delay: 0.0,
                        force: true,
                        level_options: "Lobby".into(),
                        ..TransitionParams::default()
                    },
                );
            });
        }
    }

    /// The queue set's next screen at the PreMatch level: the rematch's pre-match on the
    /// server's pm.begin (the set's games kept), else PLAY AGAIN's new search on the last Confirm's queues (the set's
    /// games emptied).
    fn queue_next(&mut self, shell: &Shell, mode: mvs_protocol::online::QueueMode) -> Option<PreMatchStart> {
        let h = shell.online.as_ref()?;
        let session = &mut self.screen.view.session;
        if self.rematch {
            let h = h.borrow();
            return crate::menus::online_pre_match(&h, &shell.settings.profile.name, session);
        }
        session.eog.cache.clear();
        let modes = session.main_menu.last_queue.clone().unwrap_or_else(|| vec![mode]);
        let mut h = h.borrow_mut();
        let now = h.clock();
        h.run(OnlineCmd::Queue { modes: modes.clone() }, &|_| None, now);
        h.open_queue = None;
        Some(crate::menus::queue_pre_match(&h, &shell.settings, modes, None))
    }

    /// The widgets' targets: each world space widget the scene shows laid out at its desired
    /// size, painted into its render target (cleared to transparent, made again when the size changes) and handed
    /// to the level.
    fn paint_widgets(&mut self, r: &mut Renderer, ui: &mut Ui, fonts: &mut Fonts) -> mvs_render::Result<()> {
        for (sw, w) in results::scene_widgets(&self.state) {
            // a banner with no preview has no stat sheet; the winner text is painted once
            if !r.menu_shows(sw) || (sw == SceneWidget::WinnerText && self.targets.iter().any(|(x, _)| *x == sw)) {
                continue;
            }
            self.target_list.clear();
            let [tw, th] = self.screen.view.paint_target(fonts, &mut self.target_list, w);
            let k = match self.targets.iter().position(|(x, _)| *x == sw) {
                Some(k) if self.targets[k].1.desc.size[..2] == [tw, th] => k,
                at => {
                    let desc = TexDesc {
                        kind: TexKind::D2,
                        size: [tw, th, 1],
                        mips: 1,
                        format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        views: TexViews { srv: true, rtv: true, ..Default::default() },
                    };
                    let t = r.gpu().texture(&desc, &[])?;
                    match at {
                        Some(k) => {
                            self.targets[k].1 = t;
                            k
                        }
                        None => {
                            self.targets.push((sw, t));
                            self.targets.len() - 1
                        }
                    }
                }
            };
            let target = &self.targets[k].1;
            let rtv = target.rtv.as_ref().expect("a render target view");
            // SAFETY: a render target view of this device's texture; the clear colour is four floats.
            unsafe { r.gpu().ctx.ClearRenderTargetView(rtv, &[0.0; 4]) };
            ui.draw(r.gpu(), r.assets(), fonts, &self.target_list, target).map_err(|e| mvs_render::Error::Format {
                what: "results: a world widget".into(),
                why: e.to_string(),
            })?;
            guarded(|| r.menu_widget(sw, Some(target))).transpose()?;
        }
        Ok(())
    }

    /// The scene commands the screen made, handed to the renderer in order.
    fn apply_commands(&mut self, renderer: Option<&mut Renderer>) {
        let cmds = self.screen.view.take_scene_cmds();
        if cmds.is_empty() {
            return;
        }
        let Some(r) = renderer.filter(|_| !self.no_podium) else { return };
        for cmd in cmds {
            let is_level = matches!(cmd, SceneCmd::Level(_));
            let done = guarded(|| match cmd {
                SceneCmd::Level(l) => {
                    // (the old level lets go of its effects on the player; they go with it when none follows)
                    let opened = r.set_menu_level(l, self.fx.as_mut().map(|f| &mut f.player));
                    if l == MenuLevel::None {
                        self.fx = None;
                    }
                    if opened.is_ok() && l != MenuLevel::None && self.fx.is_none() {
                        match mvs_render::fx::Fx::new(r.assets().store(), 1) {
                            Ok(fx) => self.fx = Some(fx),
                            Err(e) => eprintln!("mvs: results: the level's effects: {e}"),
                        }
                    }
                    opened
                }
                SceneCmd::Preview { slot, spec } => r.menu_preview(slot, spec),
                SceneCmd::Sequence(c) => r.menu_sequence(c),
                SceneCmd::Banner { slot, slug } => r.menu_banner(slot, slug.as_deref()),
                // (the pre-match's)
                SceneCmd::BannerIcon { .. } => Ok(()),
                SceneCmd::Fade(a) => {
                    r.set_post_fade(a);
                    Ok(())
                }
                SceneCmd::Hover { slot, on } => {
                    r.menu_hover(slot, on);
                    Ok(())
                }
                // (the main menu's Player1 line: not the results')
                SceneCmd::ClickVo => Ok(()),
                // (the pre-match's)
                SceneCmd::TeamStyle(_) => Ok(()),
            });
            match done {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    eprintln!("mvs: results: scene command: {e}");
                    if !is_level {
                        continue;
                    }
                    // (the level did not open)
                    self.no_podium = true;
                    self.synth =
                        vec![(self.clock + SYNTH_SHOW_UI, "ShowUI"), (self.clock + SYNTH_SHOW_STATS, "ShowStats")];
                    return;
                }
                None => {
                    // (the level's methods are unported)
                    self.no_podium = true;
                    self.synth =
                        vec![(self.clock + SYNTH_SHOW_UI, "ShowUI"), (self.clock + SYNTH_SHOW_STATS, "ShowStats")];
                    return;
                }
            }
        }
    }

    /// One frame of the screen and the podium (the menus' scene commands handed to the renderer); the next screen
    /// once the screen leaves.
    pub fn frame(&mut self, shell: &mut Shell, f: &Frame) -> Result<Go> {
        if let Some(size) = f.resized {
            self.screen.view.resize(size);
        }
        self.clock += f.dt;
        // (a scripted run's frame: the script's events and clicks, the window's dropped)
        let mut input = match shell.script.as_mut() {
            Some(s) => {
                shell.window.take_menu_input();
                let mut input = s.screen_input();
                let n = s.player.frame().saturating_sub(1);
                for (what, args) in s.player.take_dos() {
                    match what.as_str() {
                        "click" => {
                            let name = args.get(0).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                            s.clicks.push((name, args.get(1).and_then(serde_json::Value::as_u64)));
                        }
                        "queueState" => {
                            let hub = shell.online.as_ref().map(|h| h.borrow());
                            crate::menus::queue_state(hub.as_deref(), n);
                        }
                        _ => eprintln!("mvs: script: frame {n}: do {what} {args}: not run on the results screen"),
                    }
                }
                crate::menus::script_click(s, &self.screen, n, &mut input);
                input
            }
            None => MenuHost::live_input(shell),
        };
        if self.release {
            self.release = false;
            input.push(MenuInput::Key { key: "V", down: false, repeat: false, player: 0 });
        } else if self.press.is_some_and(|t| self.clock >= t) {
            self.press = None;
            self.release = true;
            input.push(MenuInput::Key { key: "V", down: true, repeat: false, player: 0 });
        }
        let exit;
        let left = self.shot_taken;
        {
            let Shell { ui, settings, audio, music, window, swap, renderer, perf, script, script_out, online, .. } =
                shell;
            music.tick = music.tick.wrapping_add(1);
            let ui = ui.as_mut().ok_or_else(|| Error::Match(NO_UI.into()))?;
            let mut list = std::mem::take(&mut ui.list);
            list.clear();
            let mut sink =
                MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
            let mut run = menu_run(ui, settings, &mut sink);

            // the level: the commands, its tick, what it told
            self.apply_commands(renderer.as_mut());
            let mut events = Vec::new();
            if !self.no_podium
                && let Some(r) = renderer.as_mut()
                && r.menu_level() != MenuLevel::None
            {
                match guarded(|| r.menu_tick(f.dt, self.fx.as_mut().map(|f| &mut f.player))) {
                    Some(Ok(())) => {}
                    Some(Err(e)) => eprintln!("mvs: results: podium tick: {e}"),
                    None => self.no_podium = true,
                }
                events = r.take_menu_events();
            }
            let clock = self.clock;
            self.synth.retain(|&(due, name)| {
                if clock < due {
                    return true;
                }
                events.push(MenuEvent::Podium { call: "PodiumEvent".into(), args: vec![EventArg::Str(name.into())] });
                false
            });
            for e in events {
                match e {
                    MenuEvent::Sound { event, .. } => run.sink.post(&event),
                    MenuEvent::Podium { call, args } => {
                        let points = Points(renderer.as_ref());
                        results::podium_event(&mut self.screen, &mut run, &mut self.state, &points, &call, &args);
                    }
                    MenuEvent::Fade(a) => self.screen.view.manager.fader = a,
                    MenuEvent::SequenceUi(_)
                    | MenuEvent::WorldLocation { .. }
                    | MenuEvent::PreviewSettled { .. }
                    | MenuEvent::TransitionComplete(_) => {}
                }
            }
            results::place_widgets(&mut self.screen, &mut self.state, &Points(renderer.as_ref()));

            // the frame's toasts of other players for the toast service (On Toast Received: the build's UI_Global
            // shows the notification over the podium); the menus' other events wait for the menus, and none of an
            // earlier screen is seen again
            self.screen.view.session.online.events =
                online.as_ref().map(|h| h.borrow_mut().take_toasts()).unwrap_or_default();
            if let Some(h) = online.as_ref() {
                if self.queue.is_some() {
                    self.queue_news(h, &mut run);
                }
                // (the banners' toasts, the podium's toasts: online.toast)
                let cmds = self.screen.view.take_online_cmds();
                if !cmds.is_empty() {
                    let mut h = h.borrow_mut();
                    let now = h.clock();
                    for c in cmds {
                        h.run(c, &|_| None, now);
                    }
                }
            }
            // (the frame after the shot's: the screen goes like any other exit)
            exit = if left { None } else { self.screen.frame(&mut run, &input, self.clock, &mut list) };
            if left || exit.is_some() {
                results::finish(&mut self.screen, &mut run);
            }
            window.set_cursor_hidden(ui.pointer.os_cursor_hidden());
            ui.ui.time = Some(self.clock);
            if !self.no_podium
                && exit.is_none()
                && let Some(r) = renderer.as_mut()
                && r.menu_level() != MenuLevel::None
                && let Err(e) = self.paint_widgets(r, &mut ui.ui, &mut ui.menu_fonts)
            {
                eprintln!("mvs: results: the world widgets: {e}");
            }

            // the draw: black, the podium over it, the elements over that, the picture of a shot due, the present
            let renderer = renderer.as_mut().ok_or_else(|| Error::Match(NO_STORE.into()))?;
            let back = swap.back_buffer();
            if let Some(rtv) = &back.rtv {
                // SAFETY: a render target view of the swap chain's back buffer, on this device's context.
                unsafe { renderer.gpu().ctx.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]) };
            }
            if !self.no_podium && renderer.menu_level() != MenuLevel::None {
                match guarded(|| renderer.menu_draw(self.fx.as_mut().map(|f| &mut f.player), Some(back))) {
                    Some(Ok(_)) => {}
                    Some(Err(e)) => eprintln!("mvs: results: podium draw: {e}"),
                    None => self.no_podium = true,
                }
            }
            let fps = crate::fps::line(settings.graphics.show_fps, perf.readout.as_ref());
            let ping = crate::fps::ping_line(online.as_ref().and_then(|h| h.borrow().ping_ms()));
            crate::fps::draw(&mut list, &mut ui.menu_fonts, swap.size, crate::fps::LAYER, fps.as_ref(), ping.as_ref());
            ui.ui.time = Some(self.clock);
            // (a shot's or a scripted run's picture waits for its pictures)
            ui.ui.stream_pictures = self.shot.is_none() && script.is_none();
            ui.ui.draw(renderer.gpu(), renderer.assets(), &mut ui.menu_fonts, &list, back)?;
            ui.list = list;
            if let Some(after) = self.shot.filter(|&a| !self.shot_taken && self.clock >= a) {
                let px = renderer.read_back(back)?;
                let [w, h] = swap.size;
                let path = self.out.join("results.png");
                mvs_render::image::write_png(&path, w, h, &px).map_err(Error::Match)?;
                eprintln!("mvs: {} (after {after} s)", path.display());
                self.shot_taken = true;
            }
            let hub = online.as_ref().map(|h| h.borrow());
            crate::app::script_frame(script, script_out, renderer, swap, false, hub.as_deref())?;
            drop(hub);
            swap.present(true)?;
        }

        if let Some(e) = &exit {
            eprintln!("mvs: results: the screen leaves: {e:?}");
        }
        let arcade = match &exit {
            Some(MenuExit::OpenLevel { level, .. }) if self.queue.is_none() => self.arcade_next(shell, level),
            _ => None,
        };
        // (a queue set's end: queue?.close() first, the rematch's pm let go when the screen did not take it, the
        // queue's session left on the way to the main menu)
        if exit.is_some()
            && self.queue.is_some()
            && let Some(h) = &shell.online
        {
            h.borrow_mut().close_queue_end();
        }
        let queue_next = match (&exit, self.queue) {
            (Some(MenuExit::OpenLevel { level, .. }), Some(mode)) if level == "PreMatch" => {
                self.queue_next(shell, mode)
            }
            _ => None,
        };
        if exit.is_some()
            && self.queue.is_some()
            && let Some(h) = &shell.online
        {
            let mut h = h.borrow_mut();
            if queue_next.is_none() {
                if let Some(pm) = &mut h.hub.current {
                    pm.close();
                }
                if !matches!(&exit, Some(MenuExit::OpenLevel { level, .. }) if level == "PreMatch") {
                    h.leave_queue_session();
                }
            }
        }
        let go = match exit {
            Some(MenuExit::Quit) => Go::Quit,
            // (the launcher, a program of its own, stays open beside the game)
            Some(MenuExit::Launcher) => Go::Quit,
            _ if arcade.is_some() => arcade.map_or(Go::Stay, |s| Go::PreMatch(Box::new(s))),
            _ if queue_next.is_some() => queue_next.map_or(Go::Stay, |s| Go::PreMatch(Box::new(s))),
            // New_MainMenu, or the PreMatch level of a custom game's
            Some(_) => self.leave(shell),
            None if left => self.leave(shell),
            None => Go::Stay,
        };
        if !matches!(go, Go::Stay) {
            shell.session = std::mem::take(&mut self.screen.view.session);
        }
        Ok(go)
    }

    /// An arcade run's next pre-match at the screen's end to `level`: a run asked for (the end of a
    /// run's PLAY AGAIN) is started, a finished run is left, and a run in progress going
    /// to the PreMatch level opens the pre-match screen of its next round (the next one or the retry).
    fn arcade_next(&mut self, shell: &Shell, level: &str) -> Option<PreMatchStart> {
        let view = &mut self.screen.view;
        if let Some(d) = view.session.arcade.restart.take() {
            // (Math.random() * 2^32 >>> 0)
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let seed = (view.manager.random() * 4_294_967_296.0) as u32;
            view.session.arcade.start(d, seed);
        }
        if view.session.arcade.run.as_ref().is_some_and(|r| r.over) {
            view.session.arcade.leave();
        }
        if level != "PreMatch" {
            return None;
        }
        let menu = view.bundle.menu.as_ref()?;
        let setup = view.session.arcade.pre_match_setup(menu, &shell.settings.profile.name)?;
        self.fx = None;
        Some(PreMatchStart { setup, lobby: None })
    }

    /// The rematch's bar (UI_EOG_RematchStatus) among the screen's widgets.
    fn rematch_bar(view: &mvs_ui::menu::view::MenuView) -> Option<WidgetId> {
        let tree = view.tree();
        view.layers()
            .iter()
            .flat_map(|l| tree.post_order(l.w))
            .find(|&w| tree.user_class(w) == Some("UI_EOG_RematchStatus"))
    }

    /// Where the match's end goes.
    fn leave(&mut self, shell: &Shell) -> Go {
        self.fx = None;
        match self.then.take() {
            Some(end) => go_of(shell, end),
            None => Go::Menu(MenuReturn::default()),
        }
    }
}
