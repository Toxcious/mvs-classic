//! The end-of-game widgets over the podium: the progression container and its
//! steps (match results, stat sheets and nametags, the rematch, the arcade's continue, play again, pull to pre-match).
//!
//! A queue match's session (online matchmaking: the rematch votes, the set's games, leaving the set) is the
//! program's: `QueueEnd` holds what the widgets read of it, `EogState` what they hand it, and the program calls
//! `EOGRematchStatus::update_vote_for_player` / `exit` as the votes and the set come.

use std::any::Any;

use super::arcade_progress::AXProgress;
use super::common::{apply_founder_name_treatment, color_for_index, fill_page, palette_entry};
use super::controls::buttons::{ButtonMain, ButtonSecondary};
use super::controls::interactor::{
    InteractorState, batch_set_context, set_context, set_focused, set_interactable, set_is_enabled,
};
use super::controls::{bind, find, ist, prop_bool, quad_color, quad_wobble, wd};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::{Manager, TransitionParams};
use crate::menu::view::MenuView;
use crate::results::{EndOfGame, PodiumPlayer, int_text, stat_rows};
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

// ---------- the module statics ----------

/// What a queue match's end session gives the widgets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueEnd {
    /// The wins that end the set (by mode).
    pub set_wins: i32,
    /// The queue's team style (the votes needed: 0 -> 2, 1..3 -> 4).
    pub team_style: usize,
}

/// The results screen's data and state.
#[derive(Debug, Default)]
pub struct EogState {
    /// The EndOfGameData the build's widgets read from the step's Init Data (UI_EOG_Data).
    pub data: Option<EndOfGame>,
    pub queue: Option<QueueEnd>,
    /// PGI_MatchmakingHandler EOGDataCache: whether the local player won each game of the running queue set, this one
    /// last.
    pub cache: Vec<bool>,
    /// PLAY AGAIN was taken on this screen.
    pub play_again: bool,
    /// The rematch vote the bar sent (the server's match.vote), taken by the queue's owner.
    pub rematch_vote: Option<bool>,
    /// The queue session was left (UI_EOG_PlayAgain).
    pub left_queue: bool,
    /// The widgets bound to the toast service that leave it with the screen.
    pub toast_owners: Vec<WidgetId>,
}

impl EogState {
    /// setEndOfGame.
    pub fn set(&mut self, data: Option<EndOfGame>, queue: Option<QueueEnd>) {
        self.data = data;
        self.queue = queue;
        self.play_again = false;
        self.rematch_vote = None;
        self.left_queue = false;
        self.toast_owners.clear();
    }

    /// Get Wins / Get Scores (DetermineScore) over the set's games: the games YourMatchData won are yours, every other
    /// one (a draw too) theirs.
    pub fn set_scores(&self) -> (i32, i32) {
        let yours = self.cache.iter().filter(|&&w| w).count() as i32;
        (yours, self.cache.len() as i32 - yours)
    }

    /// WasWonInTwoRounds: the cache's last index is 1 and its two games' YourMatchData.IsWinner are the same.
    pub fn won_in_two_rounds(&self) -> bool {
        self.cache.len() == 2 && self.cache[0] == self.cache[1]
    }
}

fn data(cx: &Cx<'_>) -> Option<EndOfGame> {
    cx.view.session.eog.data.clone()
}

/// A widget animation's length in seconds.
fn anim_length(cx: &Cx<'_>, name: &str) -> f64 {
    let class = cx.view.tree().user_class(cx.w).unwrap_or_default().to_string();
    cx.view.anim_length(&class, name)
}

fn set_text(cx: &mut Cx<'_>, name: &str, t: &str) {
    let w = find(cx, name);
    wd(cx, w).text = t.to_string();
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn game_text(cx: &Cx<'_>, key: &str) -> String {
    cx.view.game_text(key).to_string()
}

/// The UI_Interactor child of a button widget.
fn interactor_in(cx: &Cx<'_>, button: WidgetId) -> WidgetId {
    cx.view.tree().find_in(button, "UI_Interactor")
}

// ---------- UI_EOG_MatchResults ----------

// The header texts.
const MR_CUSTOM: &str = "CF6BBFCA4EE800E6621923A21EE95C86";
const MR_NORMAL: &str = "8775226742714B062EAD3E938465832E";
const MR_MODE: &str = "08CED9FA483935E662B86F940A8F02ED";
const MR_VICTORIOUS: &str = "135311634495EF34599536835723DF79";
const MR_WINS: &str = "F2A2554A4DD9EDDB676084961C99CC21";
const MR_YOUR_TEAM: &str = "59FC3B91409A546CF3D10A90A331295F";
const MR_ENEMY_TEAM: &str = "7141B64344BFFEDA6B8FE1824639532B";
const MR_NAME_VICTORIOUS: &str = "EA8C5FCE460AC663CA742C89CE041DA2";
const MR_NAME_RESULT: &str = "5400E24F4F535368DBF7039656503AFD";
const MR_TEAM_RESULT: &str = "8E3C823E4E017B01AEDB3793AFDCE68E";

/// UI_EOG_MatchResults (step "results"): the header. Init -> UpdateAppearance; BPI Start -> BPI Step, which runs
/// UpdateSetScore and completes the step at once (the widget stays). Init's RoundNumber is the length of the step's
/// EOG Data: the set's games in a queue match (this one last), one game in a custom one. Not ported: arcade and the
/// WinnerToSet animation, which only RematchAccepted plays (no caller in the build).
#[derive(Debug, Default)]
pub struct EOGMatchResults {
    pub on_complete: Event<()>,
}

impl EOGMatchResults {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGMatchResults::default())
    }

    pub fn update_context(&mut self, _cx: &mut Cx<'_>, _ctx: &str) {}

    pub fn start(&mut self, cx: &mut Cx<'_>) {
        cx.emit(self, |c| &c.on_complete, &());
    }
}

impl Controller for EOGMatchResults {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let Some(d) = data(cx) else { return };
        let Some(you) = d.players.get(d.me) else { return };
        let queue = cx.view.session.eog.queue.is_some();
        let round = if queue { cx.view.session.eog.cache.len().max(1) } else { 1 };
        // Get Winners Name and Team: the first winner of MatchData
        let name = d.players.iter().position(|p| p.is_winner).map_or("", |i| d.name(i)).to_string();
        let won_two = queue && cx.view.session.eog.won_in_two_rounds();
        // Result: IS VICTORIOUS! on round 3 or a set won in two rounds (WasWonInTwoRounds), else WINS
        let result = game_text(cx, if round == 3 || won_two { MR_VICTORIOUS } else { MR_WINS });
        let label = match d.team_style {
            0 => game_text(cx, MR_NAME_RESULT).replace("{Name}", &name).replace("{Result}", &result),
            2 => game_text(cx, MR_NAME_VICTORIOUS).replace("{Name}", &name),
            _ => game_text(cx, MR_TEAM_RESULT)
                .replace("{Team}", &game_text(cx, if you.is_winner { MR_YOUR_TEAM } else { MR_ENEMY_TEAM }))
                .replace("{Result}", &result),
        };
        set_text(cx, "roundLabel", "");
        let team = find(cx, "teamLabel");
        let colour = color_for_index(cx, if you.is_winner { 0 } else { 1 });
        let t = wd(cx, team);
        t.text = label;
        t.color = colour;
        set_vis(cx, "resultContainer", Vis::SelfHitTestInvisible);
        // Conditional Visibility(SetScoreVert, round 1 || free for all || arcade || local play, Collapsed,
        // SelfHitTestInvisible)
        let hide = round == 1 || d.team_style == 2 || d.local_play;
        set_vis(cx, "SetScoreVert", if hide { Vis::Collapsed } else { Vis::SelfHitTestInvisible });
        // UpdateSetScore (@3426): Get Scores over the set's games (a custom game's one game), yours in palette 0's UI
        // colour, theirs in 1's
        let (yours, theirs) = if queue {
            cx.view.session.eog.set_scores()
        } else {
            (i32::from(you.is_winner), i32::from(!you.is_winner))
        };
        let (blue, red) = (palette_entry(cx, 0), palette_entry(cx, 1));
        let b = find(cx, "BlueSetScoreLabel");
        let w = wd(cx, b);
        w.text = int_text(i64::from(yours));
        w.color = blue;
        let r = find(cx, "RedSetScoreLabel");
        let w = wd(cx, r);
        w.text = int_text(i64::from(theirs));
        w.color = red;
        // PopulateMatchInfo: bIsCustomGame -> "Custom", else "Bots" for queue type 5, else "Normal".
        // Unverified: a queue match the server filled with bots is taken as the Normal queue's (queue type 5 is a
        // queue of its own).
        let mode = game_text(cx, MR_MODE).replace("{mode}", &game_text(cx, if queue { MR_NORMAL } else { MR_CUSTOM }));
        set_text(cx, "ModeLabel_1", &mode);
        // Deviation: the build's QueueLabel is the queue config's display name (ParseMatchData of "1v1" / "2v2" /
        // "ffa", GameModes spec 06); the port types its own three texts, which no Game.locres key holds as such.
        let mut q = ["1 vs 1", "Teams", "Free For All"].get(usize::from(d.team_style)).copied().unwrap_or("");
        // Deviation: a Teams match whose sides differ in size (a custom Versus lobby's 2v1) is named "Versus", the
        // lobby's own name for it; the build has no such match.
        let side = |t: i32| d.players.iter().filter(|p| p.team_index == t).count();
        if d.team_style == 1 && side(0) != side(1) {
            q = crate::menu::custom_text::VERSUS;
        }
        set_text(cx, "QueueLabel", q);
    }
}

// ---------- UI_EOG_StatItem ----------

/// UI_EOG_StatItem: Init(StatInfo).
#[derive(Debug, Default)]
pub struct EOGStatItem;

impl EOGStatItem {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGStatItem)
    }

    pub fn init(&mut self, cx: &mut Cx<'_>, title: &str, value: &str, highlight: Rgba) {
        set_text(cx, "titleLabel", title);
        set_text(cx, "valueLabel", value);
        let q = find(cx, "WBP_Quadrilateral_236");
        quad_color(cx, q, highlight);
        let w = cx.w;
        cx.view.play(w, "Enter", false, 1.0);
    }
}

impl Controller for EOGStatItem {}

// ---------- UI_EOG_Nametag ----------

/// UI_EOG_Nametag: the screen-space widget of a podium banner (BP_Banner_PM "EOG Nametag"), shown by the sequence's
/// ShowNametag. Populate (@2772) and UpdateAppearance (@123): the name, the team colour on the plate and the winner's
/// diamond, the "me" plate (never with spectator colours), the trophy overlay and 50 more right padding for a winner,
/// render scale 0.8 (winner) or 0.7, then the Enter animation. The name (@248-491): in Local Play
/// GetLocalPlayUsername(PlayerIndex)'s custom name, else its PlayerNumberLong; otherwise the account's username, then
/// Apply Founder Pack Name Treatment @624 (its User is no valid one in Local Play: untreated there). Not ported: the
/// platform indicator's SetUser (its image is collapsed for the own platform), the context menu of another account and
/// the balloon message (online), the spectator colours (a spectator is no player of the port's matches).
#[derive(Debug, Default)]
pub struct EOGNametag;

impl EOGNametag {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGNametag)
    }

    pub fn populate(&mut self, cx: &mut Cx<'_>, p: &PodiumPlayer, your_team: bool, is_me: bool) {
        let local_play = data(cx).is_some_and(|d| d.local_play);
        let name = if local_play {
            p.info.local_name.clone().unwrap_or_else(|| p.username.clone())
        } else {
            p.username.clone()
        };
        set_text(cx, "titleLabel", &name);
        if !local_play {
            let t = find(cx, "titleLabel");
            apply_founder_name_treatment(cx, t);
        }
        let colour = palette_entry(cx, if your_team { 0 } else { 1 });
        for n in ["teamColorBG", "WBP_Quadrilateral_1"] {
            let q = find(cx, n);
            quad_color(cx, q, colour);
        }
        let me_bg = find(cx, "meColorBG");
        let me_colour = palette_entry(cx, 2);
        quad_color(cx, me_bg, me_colour);
        wd(cx, me_bg).vis = if is_me { Vis::HitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "Overlay_Winner", if p.entry.is_winner { Vis::HitTestInvisible } else { Vis::Collapsed });
        if p.entry.is_winner {
            for n in ["teamColorBG", "meColorBG"] {
                let w = find(cx, n);
                wd(cx, w).pad.r += 50.0;
            }
        }
        let me = cx.w;
        let s = if p.entry.is_winner { 0.8 } else { 0.7 };
        wd(cx, me).transform.scale = [s, s];
        if !cx.view.is_animation_playing(me, "Enter") {
            cx.view.play(me, "Enter", false, 1.0);
        } else if !cx.view.is_animation_playing_forward(me, "Enter") {
            cx.view.reverse_animation(me, "Enter");
            cx.view.set_playback_speed(me, "Enter", 1.0);
        }
    }
}

impl Controller for EOGNametag {}

// ---------- UI_EOG_StatSheet ----------

/// UI_EOG_StatSheet: the world widget of a podium banner (BP_Banner_PM "Stat Sheet"). Populate (from the banner's
/// BPI_ShowNametag) stores the record; the banner's "ShowStats" event runs PopulateStats and shows the component. Not
/// ported: the context menu of another account (online).
#[derive(Debug, Default)]
pub struct EOGStatSheet {
    index: Option<usize>,
}

impl EOGStatSheet {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGStatSheet::default())
    }

    /// `index`: the record's place in MatchData.
    pub fn populate(&mut self, index: usize) {
        self.index = Some(index);
    }

    pub fn populate_stats(&mut self, cx: &mut Cx<'_>) {
        let (Some(d), Some(i)) = (data(cx), self.index) else { return };
        if i >= d.players.len() {
            return;
        }
        let box_ = find(cx, "statsContainer");
        for c in cx.view.tree().widget(box_).children.clone() {
            MenuView::discard(cx, c);
        }
        for row in stat_rows(&d, i) {
            let w = MenuView::create(cx, "UI_EOG_StatItem");
            {
                let x = wd(cx, w);
                x.pad = Margin { l: 0.0, t: 35.0, r: 0.0, b: 0.0 };
                x.h_align = HAlign::Right;
            }
            MenuView::add_child(cx, box_, w, None);
            // (PaletteForIndex(IsYourTeam ? 0 : 1).base, else transparent)
            let colour = row.highlight.map_or([0.0; 4], |h| color_for_index(cx, h));
            let title = game_text(cx, row.title);
            cx.visit::<EOGStatItem, _>(w, |c, cx| c.init(cx, &title, &row.value, colour));
        }
    }
}

impl Controller for EOGStatSheet {}

// ---------- UI_SteppedNextButton ----------

const SN_CLICK: u32 = 1;
const SN_SUBMIT3: u32 = 2;

/// UI_SteppedNextButton (step "next"): enabled by BPI Start; a click or the manager's submit key (no modal up,
/// visible, enabled) completes the step and disables the button. The button is disabled with SetIsEnabled, not
/// through its interactor, so UI_ButtonMain's Interactable stays true and the tree's DefaultShimmer stays on; its
/// interactor takes no click or hover while it is (the tree's buttonNext starts disabled).
#[derive(Debug, Default)]
pub struct SteppedNextButton {
    pub on_complete: Event<()>,
    enabled: bool,
}

impl SteppedNextButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SteppedNextButton::default())
    }

    /// BPI Init (@52): SetLabel(NodeDisplayName) unless that text is culture invariant (@208); the custom list's
    /// "NEXT" is, so the button keeps its tree label (the localised "NEXT"). The shortcut is UI_Manager's OnSubmit3
    /// (@98 / @167: the function OnSubmit3 bound to it), not OnSubmit (a left click on any focused interactor).
    pub fn init(&mut self, cx: &mut Cx<'_>, display_name: &str, culture_invariant: bool) {
        let me = cx.w;
        cx.view.manager.on_submit3.add(Handler { owner: me, id: SN_SUBMIT3 });
        if !culture_invariant {
            let b = find(cx, "buttonNext");
            cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, display_name));
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let b = find(cx, "buttonNext");
        let i = interactor_in(cx, b);
        set_context(cx, i, ctx);
    }

    /// BPI Start @504: buttonNext.SetIsEnabled(true).
    pub fn start(&mut self, cx: &mut Cx<'_>) {
        self.enabled = true;
        let b = find(cx, "buttonNext");
        set_is_enabled(cx, b, true);
    }

    fn can_submit(&self, cx: &Cx<'_>) -> bool {
        !Manager::is_modal_active(&*cx.view)
            && cx.view.visible_in_hierarchy(cx.w)
            && !cx.view.is_destroyed(cx.w)
            && self.enabled
    }

    fn step(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: SN_SUBMIT3 });
        self.enabled = false;
        // BPI Step @10: buttonNext.SetIsEnabled(false)
        let b = find(cx, "buttonNext");
        set_is_enabled(cx, b, false);
        cx.emit(self, |c| &c.on_complete, &());
    }
}

impl Controller for SteppedNextButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let b = find(cx, "buttonNext");
        cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(Handler { owner: me, id: SN_CLICK }));
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: SN_SUBMIT3 });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if matches!(id, SN_CLICK | SN_SUBMIT3) && self.can_submit(cx) {
            self.step(cx);
        }
    }
}

// ---------- UI_ButtonYesNo ----------

const YN_FOCUS: u32 = 1;
const YN_UNFOCUS: u32 = 2;
const YN_CLICK: u32 = 3;

/// UI_ButtonYesNo: PreConstruct shows the check (IsYesButton) or the cross and fills the quad orange or dark;
/// OnInitialized hides the outline; the interactor's focus shows it wobbling with FocusedAnim looping at speed 0.5,
/// its unfocus hides it; its click is OnClick.
#[derive(Debug, Default)]
pub struct ButtonYesNo {
    pub on_click: Event<i32>,
}

impl ButtonYesNo {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonYesNo::default())
    }

    fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }
}

impl Controller for ButtonYesNo {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let yes = prop_bool(cx, "IsYesButton");
        set_vis(cx, "iconYes", if yes { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        set_vis(cx, "iconNo", if yes { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        let f = find(cx, "bgFill");
        quad_color(cx, f, if yes { [0.982251, 0.520996, 0.000304, 1.0] } else { [0.016807, 0.024158, 0.046665, 1.0] });
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "bgOutline", Vis::Hidden);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, YN_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, YN_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, YN_CLICK);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        let me = cx.w;
        if let Some(s) = ist(cx, i) {
            s.on_focus.remove(Handler { owner: me, id: YN_FOCUS });
            s.on_unfocus.remove(Handler { owner: me, id: YN_UNFOCUS });
            s.on_click.remove(Handler { owner: me, id: YN_CLICK });
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let me = cx.w;
        let outline = find(cx, "bgOutline");
        match id {
            YN_FOCUS => {
                wd(cx, outline).vis = Vis::SelfHitTestInvisible;
                quad_wobble(cx, outline, true);
                // PlayAnimation(FocusedAnim, 0, 0 loops, Forward, 0.5, bRestoreState true) @34; StopAnimation @10
                // restores
                cx.view.play_animation(me, "FocusedAnim", false, 0.5, 0, false, true);
            }
            YN_UNFOCUS => {
                wd(cx, outline).vis = Vis::Hidden;
                quad_wobble(cx, outline, false);
                cx.view.stop_animation(me, "FocusedAnim");
            }
            YN_CLICK => {
                let p = arg.downcast_ref::<i32>().copied().unwrap_or(0);
                cx.emit(self, |c| &c.on_click, &p);
            }
            _ => {}
        }
    }
}

// ---------- UI_EOG_RematchStatus ----------

const GREEN: Rgba = [0.008023, 0.693872, 0.258183, 1.0];
/// The seconds the rematch vote runs.
const REMATCH_LIMIT: f64 = 12.0;
/// GetVoteResults: the votes needed by the team style (0 -> 2, 1..3 -> 4).
const VOTES_NEEDED: [usize; 4] = [2, 4, 4, 4];

/// The votes a rematch needs: the team style's (`VOTES_NEEDED`), every player of the match. Deviation (the port's
/// big matches): a match of more than four needs every one of its players' votes; the build's table stops at four,
/// every player of its largest match.
fn votes_needed(team_style: usize, players: usize) -> usize {
    VOTES_NEEDED.get(team_style).copied().unwrap_or(4).max(players)
}
const RS_DECLINED: &str = "DB9EB7BD42CA912FC3669BB4F06053FA";
const RS_FINAL_ROUND: &str = "B5A1EC854483F6512C6838B06C3E65CE";
const RS_ROUND: &str = "A304F86046D31CC30EEFBFA80895DF75";
const RS_ACCEPTED: &str = "E056BBBA4FA98A95CEBE4995B8D0D08E";
const RS_DECLINED_OWN: &str = "7415534E4361E78502F06A9CFD713EE1";
const RS_DECLINED_REMATCH: &str = "3E0C5B0A43FABC5CD5508080BDD5E165";

const RS_YES: u32 = 1;
const RS_NO: u32 = 2;
const RS_SUBMIT3: u32 = 3;
const RS_ENABLE: u32 = 4;
const RS_BIND_SUBMIT: u32 = 5;
const RS_EXIT_DECLINED: u32 = 6;
const RS_EXIT_OWN: u32 = 7;
const RS_COMPLETE: u32 = 8;

/// UI_EOG_RematchStatus (step "rematch" of the Matchmade list, "Rematch Timer"): the rematch vote's bar. BPI Init:
/// Init(the last game, 12): a 12 s limit. BPI Start (@6147): a set a side has won (Get Wins, the higher of the two
/// scores, at the queue's MaximumWins) completes the step at once; else Enter (@2846). The bar counts down while
/// Counting Down (Tick @3622); Yes / No (or the manager's OnSubmit3, a yes) vote once. Every vote that comes
/// (`update_vote_for_player`, UpdateVoteForPlayer @4611) is kept by slot: a decline shows "SOMEONE DECLINED!" and
/// exits after 1.0 s, all accepted (the votes needed by the team style) shows "STARTING ROUND {number}" and stays. The
/// limit's end exits with no vote sent. Exit plays ExitAnim, then completes.
/// Not ported: TriggerScreenShake(1.0) on the manager (the menus' screen shake is not ported), the arcade label, the
/// analytics (RecordRematchResponse*), UpdateQuestAndAchievement and Complete's ClearCharacterAndSkinSelections (the
/// backend's), the podiums' BPI_HideBalloonMessage (no balloons). The queue session is the program's
/// (`EogState::rematch_vote` is the vote sent; the program calls `update_vote_for_player` and `set_decided`).
#[derive(Debug, Default)]
pub struct EOGRematchStatus {
    pub on_complete: Event<()>,
    limit: f64,
    current: f64,
    counting: bool,
    played_enter: bool,
    voted: bool,
    /// The 4 / 3 / 2 / 1 s DoOnce gates.
    ticked: Vec<i32>,
    declined_once: bool,
    exited: bool,
    submit_bound: bool,
    /// Done has run (a vote, a decline seen, all accepted).
    is_done: bool,
    /// AccountToWidgetIndex: slot -> accepted.
    votes: Vec<(usize, bool)>,
    /// Enter has bound the session's listeners (votes, the set): the program hands them on from then.
    listening: bool,
}

impl EOGRematchStatus {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGRematchStatus::default())
    }

    pub fn init(&mut self) {
        self.limit = REMATCH_LIMIT;
    }

    /// GetInteractors: [buttonNo, buttonYes]
    fn interactors(cx: &Cx<'_>) -> [WidgetId; 2] {
        let no = find(cx, "buttonNo");
        let yes = find(cx, "buttonYes");
        [interactor_in(cx, no), interactor_in(cx, yes)]
    }

    fn toggle_interactors(cx: &mut Cx<'_>, on: bool) {
        for i in Self::interactors(cx) {
            set_interactable(cx, i, on);
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        batch_set_context(cx, &Self::interactors(cx), ctx);
    }

    /// BPI Start.
    pub fn start(&mut self, cx: &mut Cx<'_>) {
        let (yours, theirs) = cx.view.session.eog.set_scores();
        // (the port's own) the set's wins by its queue: a Free For All or Chaos set is one game
        let wins = cx.view.session.eog.queue.as_ref().map_or(2, |q| q.set_wins);
        if yours.max(theirs) >= wins {
            cx.emit(self, |c| &c.on_complete, &());
            return;
        }
        self.enter(cx);
    }

    fn enter(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "EnterAnim2", false, 2.0);
        // (EnterAnim2's Finished @3465). (the port's own) Once Done has run (a decline that came during the
        // animation), the buttons stay off and OnSubmit3 unbound: a vote replayed before the end of EnterAnim2 would
        // otherwise turn them back on after "SOMEONE DECLINED!" or "STARTING ROUND n"
        let t = anim_length(cx, "EnterAnim2") / 2.0;
        cx.view.delay(t, Handler { owner: me, id: RS_ENABLE });
        if self.limit > 0.0 {
            self.current = self.limit;
            self.counting = true;
            let f = find(cx, "timerFIll");
            wd(cx, f).opacity = 1.0;
            set_vis(cx, "actionsContainer", Vis::Visible);
            cx.post("ui_rematch_enter");
            cx.post("vo_announcer_rematch");
            self.played_enter = true;
        }
        self.listening = true;
    }

    /// Enter has run: the votes and the set that came are handed on (the screen binds onVotes / onSet there,
    /// and is given what came before).
    pub fn listening(&self) -> bool {
        self.listening
    }

    fn unbind_submit(&mut self, cx: &mut Cx<'_>) {
        if !self.submit_bound {
            return;
        }
        self.submit_bound = false;
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: RS_SUBMIT3 });
    }

    /// ButtonClicked (@7587): once.
    fn button_clicked(&mut self, cx: &mut Cx<'_>, yes: bool) {
        if self.voted {
            return;
        }
        self.voted = true;
        self.make_choice(cx, yes);
        cx.post(if yes { "ui_rematch_accept" } else { "ui_rematch_decline" });
    }

    /// MakeChoice: the server's match.vote (once), the label, SelectAnim, Done; a decline also shows DeclinedFeedback
    /// with "DECLINED REMATCH!" and exits on a 1.5 s timer.
    fn make_choice(&mut self, cx: &mut Cx<'_>, accept: bool) {
        cx.view.session.eog.rematch_vote = Some(accept);
        let me = cx.w;
        let t = game_text(cx, if accept { RS_ACCEPTED } else { RS_DECLINED_OWN });
        set_text(cx, "StatusLabel", &t);
        cx.view.stop_animation(me, "WarningBounce");
        // PlayAnimationForward @620, bRestoreState
        cx.view.play_animation(me, "SelectAnim", false, 1.0, 1, false, true);
        self.done(cx);
        if accept {
            return;
        }
        self.declined_feedback(cx);
        let t = game_text(cx, RS_DECLINED_REMATCH);
        set_text(cx, "StatusLabel", &t);
        cx.view.delay(1.5, Handler { owner: me, id: RS_EXIT_OWN });
    }

    /// Done (@4344): out of the input, the bar dimmed green; the count goes on.
    fn done(&mut self, cx: &mut Cx<'_>) {
        self.is_done = true;
        self.unbind_submit(cx);
        set_vis(cx, "actionsContainer", Vis::Collapsed);
        let f = find(cx, "timerFIll");
        let w = wd(cx, f);
        w.opacity = 0.6;
        w.color = GREEN;
        Self::toggle_interactors(cx, false);
    }

    /// UpdateVoteForPlayer @4611 (the player state's OnRematchVoteReceived): the vote of `slot`.
    pub fn update_vote_for_player(&mut self, cx: &mut Cx<'_>, slot: usize, yes: bool) {
        match self.votes.iter_mut().find(|v| v.0 == slot) {
            Some(v) => v.1 = yes,
            None => self.votes.push((slot, yes)),
        }
        if !yes {
            // (@2219: DoOnce, DeclinedFeedback, Delay 1.0, Exit)
            if self.declined_once {
                return;
            }
            self.declined_feedback(cx);
            let me = cx.w;
            cx.view.delay(1.0, Handler { owner: me, id: RS_EXIT_DECLINED });
            return;
        }
        let ts = cx.view.session.eog.queue.as_ref().map_or(0, |q| q.team_style);
        let players = cx.view.session.eog.data.as_ref().map_or(0, |d| d.players.len());
        let needed = votes_needed(ts, players);
        if self.votes.len() < needed {
            return;
        }
        if self.votes.iter().filter(|v| v.1).count() >= needed {
            self.all_accepted_feedback(cx);
        }
    }

    /// DeclinedFeedback (@4892).
    fn declined_feedback(&mut self, cx: &mut Cx<'_>) {
        self.declined_once = true;
        let t = game_text(cx, RS_DECLINED);
        set_text(cx, "StatusLabel", &t);
        self.done(cx);
        let f = find(cx, "timerFIll");
        let w = wd(cx, f);
        w.opacity = 1.0;
        w.color = [0.040915, 0.049707, 0.215861, 1.0];
        self.counting = false;
        cx.post("ui_general_prompt");
    }

    /// AllAcceptedFeedback (@5169): the next game's number is the cache's length + 1.
    fn all_accepted_feedback(&mut self, cx: &mut Cx<'_>) {
        let n = cx.view.session.eog.cache.len() + 1;
        let t = if n == 3 {
            game_text(cx, RS_FINAL_ROUND)
        } else {
            game_text(cx, RS_ROUND).replace("{number}", &int_text(n as i64))
        };
        set_text(cx, "StatusLabel", &t);
        self.done(cx);
        let f = find(cx, "timerFIll");
        let w = wd(cx, f);
        w.opacity = 1.0;
        w.color = GREEN;
        self.counting = false;
        cx.post("ui_menu_matchsettings_select");
        cx.post("vo_announcer_rematch_accept");
    }

    /// The set decided with no rematch (`!s.rematch && !declinedOnce && exit()`).
    pub fn set_decided(&mut self, cx: &mut Cx<'_>) {
        if !self.declined_once {
            self.exit(cx);
        }
    }

    /// Exit (@4887, once): ExitAnim, then Complete (with no enter played, nothing more).
    pub fn exit(&mut self, cx: &mut Cx<'_>) {
        if self.exited {
            return;
        }
        self.exited = true;
        self.unbind_submit(cx);
        if !self.played_enter {
            return;
        }
        let me = cx.w;
        cx.view.play(me, "ExitAnim", false, 1.0);
        let t = anim_length(cx, "ExitAnim");
        cx.view.delay(t, Handler { owner: me, id: RS_COMPLETE });
    }
}

impl Controller for EOGRematchStatus {
    /// OnInitialized (@2769): ToggleInteractors(false), UpdateContext(NewGuid).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        Self::toggle_interactors(cx, false);
        let ctx = cx.view.manager.new_guid();
        self.update_context(cx, &ctx);
        let me = cx.w;
        let yes = find(cx, "buttonYes");
        let no = find(cx, "buttonNo");
        cx.visit::<ButtonYesNo, _>(yes, |b, _| b.on_click.add(Handler { owner: me, id: RS_YES }));
        cx.visit::<ButtonYesNo, _>(no, |b, _| b.on_click.add(Handler { owner: me, id: RS_NO }));
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.unbind_submit(cx);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if !self.counting {
            return;
        }
        self.current = (self.current - dt).max(0.0);
        let f = find(cx, "timerFIll");
        wd(cx, f).transform.scale[0] = self.current / self.limit;
        let near = [4, 3, 2, 1].into_iter().find(|&n| (self.current - f64::from(n)).abs() <= 0.1);
        if let Some(n) = near {
            if self.ticked.contains(&n) {
                return;
            }
            self.ticked.push(n);
            cx.post("ui_countdown_tick");
            let me = cx.w;
            cx.view.play(me, "WarningBounce", false, 1.0);
            return;
        }
        if self.current.abs() > 1e-6 {
            return;
        }
        cx.post("ui_rematch_warning");
        self.counting = false;
        self.exit(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            RS_YES => self.button_clicked(cx, true),
            RS_NO => self.button_clicked(cx, false),
            RS_SUBMIT3 => self.make_choice(cx, true),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        let me = cx.w;
        match id {
            RS_ENABLE => {
                if self.is_done {
                    return;
                }
                Self::toggle_interactors(cx, true);
                cx.view.delay(0.25, Handler { owner: me, id: RS_BIND_SUBMIT });
            }
            RS_BIND_SUBMIT => {
                if self.is_done {
                    return;
                }
                let yes = find(cx, "buttonYes");
                let i = interactor_in(cx, yes);
                set_focused(cx, i, false);
                cx.view.manager.on_submit3.add(Handler { owner: me, id: RS_SUBMIT3 });
                self.submit_bound = true;
            }
            RS_EXIT_DECLINED | RS_EXIT_OWN => self.exit(cx),
            RS_COMPLETE => cx.emit(self, |c| &c.on_complete, &()),
            _ => {}
        }
    }
}

// ---------- UI_ArcadeEOGContinue ----------

// Set Button Text And Visibility's, Update Retries Text's and Update Countdown Timer's texts (@564-926, @0-645,
// @211): NEXT ROUND, RETRY, NEXT; Retries Left: {retries}; Continuing in {seconds}...
const EC_NEXT_ROUND: &str = "D2658F5D410F32AFB27DDFA7D2672AE8";
const EC_RETRY: &str = "264A07624BBA70A4BE1A52A40E77E427";
const EC_NEXT: &str = "4F6DE0604D9836A7C81C77955F22288D";
const EC_RETRIES: &str = "2BE3A07642CBCCC640BED784403F2B1D";
const EC_CONTINUING: &str = "8A32905041E06FD6F0FF418E2507DED0";
/// The CDO's Seconds To Auto Proceed.
const AUTO_PROCEED_S: i32 = 10;
/// Unverified: the server's VoteToReplay Delay(0.5) (PGM @15429) before StartNextArcadeMatch, and the client's
/// Delay(1.0) of Matchmaking_HandleOnRematchStarted (MH@3649) before the PreMatch level opens; the travel itself is
/// native.
const NEXT_MATCH_S: f64 = 0.5 + 1.0;

const AC_CONTINUE: u32 = 1;
const AC_END_SERIES: u32 = 2;
const AC_SUBMIT3: u32 = 3;
const AC_NEXT_MATCH: u32 = 4;

/// The arcade's continue step (Arcade list "arcadeui", RemoveOnCompletion false). GetArcadeInfo reads the round as
/// played (the round and continues before the result the app's loop has applied); Series Ended (@128-486: no continue
/// left and lost, or the run finished and won) is the run's `over` after that result. The party's rematch votes (the
/// player state's delegate @1055, On Rematch Vote Received @1888) have no counterpart: the port is solo.
/// Continue, the series not ended (@1487-1706): Submit Rematch Vote(0) stands for the server's StartNextArcadeMatch,
/// which the app's loop has run already; the port opens the PreMatch level for the run's next round itself, after
/// NEXT_MATCH_S. The canvas does not step, as in the build.
/// End Series (@315-641): ClearCharacterAndSkinSelections (no counterpart: the next pre-match picks again), then BPI
/// Step -> On Complete: the list goes on to PLAY AGAIN and the PreMatch level.
/// Unverified: PLAY AGAIN after a run lands on a new run of the same difficulty (whether ResetMatchFlow keeps the
/// arcade's match config is native); `ArcadeState::restart` asks for it.
#[derive(Debug)]
pub struct ArcadeEOGContinue {
    pub on_complete: Event<()>,
    seconds: i32,
    /// Time to the auto proceed timer's next run; < 0 = not set.
    timer: f64,
    series_ended: bool,
    enabled: bool,
    submitted: bool,
}

impl Default for ArcadeEOGContinue {
    fn default() -> ArcadeEOGContinue {
        ArcadeEOGContinue {
            on_complete: Event::default(),
            seconds: AUTO_PROCEED_S,
            timer: -1.0,
            series_ended: false,
            enabled: false,
            submitted: false,
        }
    }
}

impl ArcadeEOGContinue {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ArcadeEOGContinue::default())
    }

    /// [Button_EndSeries, Button_Continue] interactors.
    fn interactors(cx: &Cx<'_>) -> [WidgetId; 2] {
        let end = find(cx, "Button_EndSeries");
        let cont = find(cx, "Button_Continue");
        [interactor_in(cx, end), interactor_in(cx, cont)]
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        batch_set_context(cx, &Self::interactors(cx), ctx);
    }

    /// BPI Start @919: both buttons enabled, Set Button Text And Visibility; not ended: the 1.0 s looping timer @154.
    pub fn start(&mut self, cx: &mut Cx<'_>) {
        self.set_enabled(cx, true);
        self.set_button_text_and_visibility(cx);
        if !self.series_ended {
            self.timer = 1.0;
        }
    }

    /// Button_EndSeries and Button_Continue .SetIsEnabled(b) (@315 / @1096 / @1287).
    fn set_enabled(&mut self, cx: &mut Cx<'_>, b: bool) {
        self.enabled = b;
        for name in ["Button_EndSeries", "Button_Continue"] {
            let w = find(cx, name);
            set_is_enabled(cx, w, b);
        }
    }

    fn won(cx: &Cx<'_>) -> bool {
        data(cx).is_some_and(|d| d.players.get(d.me).is_some_and(|p| p.is_winner))
    }

    /// Set Button Text And Visibility.
    fn set_button_text_and_visibility(&mut self, cx: &mut Cx<'_>) {
        let played = cx.view.session.arcade.round_played().cloned();
        self.series_ended = cx.view.session.arcade.run.as_ref().is_some_and(|r| r.over);
        set_vis(cx, "Button_EndSeries", if self.series_ended { Vis::Collapsed } else { Vis::Visible }); // @505
        let key = if self.series_ended {
            EC_NEXT
        } else if Self::won(cx) {
            EC_NEXT_ROUND
        } else {
            EC_RETRY
        };
        let label = game_text(cx, key);
        let b = find(cx, "Button_Continue");
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
        // @1186-1349: the rival of the last game's fighter (CharacterDataByFighter[...].ArcadeRivalSlug: the run's)
        let p = find(cx, "UI_AXProgress");
        let slug = played.and_then(|r| r.rival_slug);
        cx.visit::<AXProgress, _>(p, |c, cx| c.update_rival_portrait(cx, slug.as_deref()));
        self.update_retries_text(cx, false);
        self.update_countdown_timer(cx);
    }

    /// Update Retries Text(IsRetrying) @0-788.
    fn update_retries_text(&mut self, cx: &mut Cx<'_>, retrying: bool) {
        let continues = cx.view.session.arcade.round_played().map_or(0, |r| r.continues);
        let unlimited = continues == -1;
        // (@352-645: "Retries Left: {retries}" is formatted either way, with GetEmptyText's argument when unlimited)
        let n = if unlimited { String::new() } else { (continues - i32::from(retrying)).to_string() };
        let t = game_text(cx, EC_RETRIES).replace("{retries}", &n);
        set_text(cx, "Text_Retries", &t);
        set_vis(cx, "InfiniteSymbol", if unlimited { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
    }

    /// Update Countdown Timer @211-460.
    fn update_countdown_timer(&mut self, cx: &mut Cx<'_>) {
        let t = game_text(cx, EC_CONTINUING).replace("{seconds}", &self.seconds.to_string());
        set_text(cx, "Text_AutoContinue", &t);
        let v = if self.seconds >= 0 && !self.series_ended { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "Text_AutoContinue", v);
    }

    /// Countdown To Auto Proceed @1773 -> @15.
    fn countdown_to_auto_proceed(&mut self, cx: &mut Cx<'_>) {
        self.seconds -= 1;
        self.update_countdown_timer(cx);
        if self.seconds <= 0 {
            self.timer = -1.0;
            self.continue_run(cx);
        } else {
            let me = cx.w;
            cx.view.play(me, "CountdownPulse", false, 1.0);
        }
    }

    fn can_submit(&self, cx: &Cx<'_>) -> bool {
        self.enabled && !cx.view.is_destroyed(cx.w) && cx.view.visible_in_hierarchy(cx.w)
    }

    /// Continue @1287. (the port's own) once: the build's second vote changes nothing on the server.
    fn continue_run(&mut self, cx: &mut Cx<'_>) {
        if self.submitted {
            return;
        }
        self.submitted = true;
        self.set_enabled(cx, false);
        if self.series_ended {
            self.end_series(cx);
            return;
        }
        let retrying = !Self::won(cx);
        self.update_retries_text(cx, retrying);
        self.timer = -1.0;
        set_vis(cx, "Text_AutoContinue", Vis::Collapsed);
        let me = cx.w;
        cx.view.delay(NEXT_MATCH_S, Handler { owner: me, id: AC_NEXT_MATCH });
    }

    /// End Series @315-641.
    fn end_series(&mut self, cx: &mut Cx<'_>) {
        self.set_enabled(cx, false);
        self.timer = -1.0;
        if let Some(r) = cx.view.session.arcade.round_played().cloned() {
            cx.view.session.arcade.restart = Some(r.difficulty);
        }
        cx.emit(self, |c| &c.on_complete, &());
    }
}

impl Controller for ArcadeEOGContinue {
    /// BPI Init @660-914; the UI manager's OnSubmit3 bound @775.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let cont = find(cx, "Button_Continue");
        let end = find(cx, "Button_EndSeries");
        cx.visit::<ButtonMain, _>(cont, |b, _| b.on_click.add(Handler { owner: me, id: AC_CONTINUE }));
        cx.visit::<ButtonSecondary, _>(end, |b, _| b.on_click.add(Handler { owner: me, id: AC_END_SERIES }));
        cx.view.manager.on_submit3.add(Handler { owner: me, id: AC_SUBMIT3 });
        self.set_enabled(cx, false);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: AC_SUBMIT3 });
    }

    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if self.timer < 0.0 {
            return;
        }
        self.timer -= dt;
        if self.timer > 0.0 {
            return;
        }
        self.timer += 1.0;
        self.countdown_to_auto_proceed(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            // Button_Continue's OnClick @1250: CanSubmit? (Button_EndSeries enabled and the widget shown), then
            // Continue's body
            AC_CONTINUE => {
                if self.can_submit(cx) {
                    self.continue_run(cx);
                }
            }
            // Btn_EndArcade's OnClick @262: CanSubmit?, Submit Rematch Vote(1), then End Series' body
            AC_END_SERIES => {
                if !self.can_submit(cx) || self.submitted {
                    return;
                }
                self.submitted = true;
                self.end_series(cx);
            }
            // OnSubmit3 @1754: Continue
            AC_SUBMIT3 => self.continue_run(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == AC_NEXT_MATCH {
            Manager::open_level_now(cx, "PreMatch", "Lobby");
        }
    }
}

// ---------- UI_EOG_PlayAgain ----------

const PA_DONE: u32 = 1;

/// UI_EOG_PlayAgain (in the build the "eog" container's "PLAY AGAIN", AddToContainer false): no widget and no BPI
/// Start; its BPI Step (@136): Transition (type 0, no level, the options "Lobby"), ResetMatchFlow, Delay(0.5), On
/// Complete. ResetMatchFlow here: out of the queue's record (`EogState::left_queue`). Unverified: the native
/// ResetMatchFlow's own effects are not read. The port runs it from the Matchmade list's NEXT (the canvas's
/// playAgain step).
#[derive(Debug, Default)]
pub struct EOGPlayAgain {
    pub on_complete: Event<()>,
    stepped: bool,
}

impl EOGPlayAgain {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGPlayAgain::default())
    }

    pub fn step(&mut self, cx: &mut Cx<'_>) {
        if self.stepped {
            return;
        }
        self.stepped = true;
        cx.view.session.eog.play_again = true;
        Manager::transition(cx, TransitionParams::default());
        if cx.view.session.eog.queue.is_some() {
            cx.view.session.eog.left_queue = true;
        }
        let me = cx.w;
        cx.view.delay(0.5, Handler { owner: me, id: PA_DONE });
    }
}

impl Controller for EOGPlayAgain {
    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == PA_DONE {
            cx.emit(self, |c| &c.on_complete, &());
        }
    }
}

// ---------- UI_EOG_PullToPreMatch ----------

/// UI_EOG_PullToPreMatch (AddToContainer false), BPI Start (@856): every VictoryCelebrationActor's
/// CleanupForPreMatchTransition (the podium goes with the screen), BPI_ClearReferences, BPI_OpenLevel(PreMatch) (@691),
/// On Complete. The PreMatch level is the queue's on the match's mode.
#[derive(Debug, Default)]
pub struct EOGPullToPreMatch {
    pub on_complete: Event<()>,
}

impl EOGPullToPreMatch {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGPullToPreMatch::default())
    }

    pub fn start(&mut self, cx: &mut Cx<'_>) {
        Manager::open_level_now(cx, "PreMatch", "Lobby");
        cx.emit(self, |c| &c.on_complete, &());
    }
}

impl Controller for EOGPullToPreMatch {}

// ---------- UI_CollectionToasts ----------

const CT_CHANGED: u32 = 1;
const CT_DECREMENTED: u32 = 2;
/// The build's toast icon without the "+" (UI_ButtonToast's, UI_ShopCard_Toasts').
const TOAST_ICON: &str = "ui_icons_toast";

/// UI_CollectionToasts: the toasts left, as the EOG step
/// "toast" (the Matchmade and Custom lists). OnInitialized @2207: UpdateToastCount @15-909 (the count, no grouping),
/// then the count's change (InventoryUpdated -> UpdateToastCount) and OnToastsDecremented (-> ToastsDecremented
/// @2355-2748: the label's own number less one) are bound (@951-1285). The port's count is there at once: the
/// build's Delay(0) / Delay(0.1) waits for the InventoryManager and the hydra manager do not apply. The button
/// (@2231) opens the toasts' shop, which the port has not: it is drawn as a plain count (`plain_count`).
/// BPI Start @2794: On Complete first (the canvas goes on to its next step; the widget stays, RemoveOnCompletion
/// false), HorizontalBox_0's overlay slot padding (0, 60, 60, 0), then every podium banner's BPI_ShowToastButton with
/// the step's Context. The step's widget leaves the toast service with the results screen (its tree is not
/// destructed): `EogState::toast_owners`. Not ported: the Collection's stats page use of this widget.
#[derive(Debug, Default)]
pub struct CollectionToasts {
    pub on_complete: Event<()>,
    context: String,
}

impl CollectionToasts {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CollectionToasts::default())
    }

    /// Update Context: the step's Context, and the button's interactor.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let b = find(cx, "UI_ButtonSecondary");
        let i = interactor_in(cx, b);
        batch_set_context(cx, &[i], ctx);
    }

    pub fn start(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.session.eog.toast_owners.push(me);
        cx.emit(self, |c| &c.on_complete, &());
        let b = find(cx, "HorizontalBox_0");
        wd(cx, b).pad = Margin { l: 0.0, t: 60.0, r: 60.0, b: 0.0 };
        let ctx = self.context.clone();
        super::toast_button::show_podium_toast_buttons(cx, &ctx);
    }

    /// (the port's own, a user request) The count alone: the button (@2231, which opens the toasts' shop the port
    /// has not: it did nothing) drawn as a bare ui_icons_toast beside "Available", with no backing, border, focus
    /// look or "+" (ui_icons_addtoast), its interactor collapsed: never focused, never hit.
    // Deviation: the build draws a UI_ButtonSecondary with ui_icons_addtoast that opens the toasts' shop.
    fn plain_count(cx: &mut Cx<'_>) {
        let b = find(cx, "UI_ButtonSecondary");
        let tree = cx.view.tree();
        let parts = ["QuadBacking", "QuadBorder", "UI_FocusedCommon", "UI_Interactor", "ScaleBox_Label"]
            .map(|n| tree.find_in(b, n));
        let icon = tree.find_in(b, "iconImage");
        let [size, row, icon_box] = ["SizeBox_0", "HorizontalBox_0", "IconSize"].map(|n| tree.find_in(b, n));
        for w in parts {
            wd(cx, w).vis = Vis::Collapsed;
        }
        wd(cx, icon).tex = Some(TOAST_ICON.into());
        // Deviation: the icon sits by the count, one unit. The build's button box
        // is its OverrideSize, 140 wide, the 72 icon centred in it past the 40 + 40 label padding: about 34 empty
        // between "Available" and the icon besides VerticalBox_0's own 15. Here the box is the icon's width with no
        // padding, so the 15 alone is left.
        let icon_w = wd(cx, icon_box).w_override;
        wd(cx, size).w_override = icon_w;
        let r = wd(cx, row);
        r.pad.l = 0.0;
        r.pad.r = 0.0;
    }

    /// Conv_IntToText(n, false, false, 1, 324) @699: no grouping, at least one digit.
    fn update_toast_count(cx: &mut Cx<'_>) {
        let n = cx.view.session.toasts.count();
        set_text(cx, "amountLabel", &n.to_string());
    }
}

impl Controller for CollectionToasts {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        Self::plain_count(cx);
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        Self::update_toast_count(cx);
        let me = cx.w;
        let t = &mut cx.view.session.toasts;
        t.on_changed.add(Handler { owner: me, id: CT_CHANGED });
        t.on_decremented.add(Handler { owner: me, id: CT_DECREMENTED });
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let t = &mut cx.view.session.toasts;
        t.on_changed.remove(Handler { owner: me, id: CT_CHANGED });
        t.on_decremented.remove(Handler { owner: me, id: CT_DECREMENTED });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            CT_CHANGED => Self::update_toast_count(cx),
            CT_DECREMENTED => {
                let l = find(cx, "amountLabel");
                let n: i32 = wd(cx, l).text.trim().parse().unwrap_or(0);
                wd(cx, l).text = int_text(i64::from(n - 1));
            }
            _ => {}
        }
    }
}

// ---------- UI_CanvasProgressionContainer ----------

/// What a step of a list does when it has no widget on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// UI_WaitForPodiumUIEvent ("waitforsequence"): completes on the podium's ShowUI.
    Wait,
    /// UI_AutoPartyPrompt: the port has no parties, so it completes at once.
    AutoParty,
    /// UI_EOG_ReturnHome.
    ReturnHome,
    /// The cut "eog" container's PLAY AGAIN and UI_EOG_PullToPreMatch.
    PlayAgain,
}

#[derive(Clone, Copy, Debug)]
struct CanvasStep {
    /// "": no widget of the port's on the canvas (kind says what the step does).
    cls: &'static str,
    name: &'static str,
    remove: bool,
    kind: Option<Kind>,
    /// The NodeDisplayName is culture invariant (UI_SteppedNextButton keeps its tree label).
    invariant: bool,
}

const fn widget(cls: &'static str, name: &'static str, remove: bool, invariant: bool) -> CanvasStep {
    CanvasStep { cls, name, remove, kind: None, invariant }
}

const fn bare(kind: Kind) -> CanvasStep {
    CanvasStep { cls: "", name: "", remove: false, kind: Some(kind), invariant: false }
}

/// "toast": the toasts left and the banners' buttons (RemoveOnCompletion false; BPI Start completes it at once).
const TOAST_STEP: CanvasStep = widget("UI_CollectionToasts", "", false, false);
const RESULTS_STEP: CanvasStep = widget("UI_EOG_MatchResults", "", false, false);
/// The Local list (local play): no toast step. NodeDisplayName of "next": the culture invariant string "NEXT".
const LOCAL_STEPS: [CanvasStep; 4] =
    [bare(Kind::Wait), RESULTS_STEP, widget("UI_SteppedNextButton", "NEXT", true, true), bare(Kind::ReturnHome)];
/// The Custom list: the Local one with "toast" after the results.
const CUSTOM_STEPS: [CanvasStep; 5] = [
    bare(Kind::Wait),
    RESULTS_STEP,
    TOAST_STEP,
    widget("UI_SteppedNextButton", "NEXT", true, true),
    bare(Kind::ReturnHome),
];
/// The Matchmade list: "rematch" is the vote's bar (RemoveOnCompletion), "autoparty" completes at once, "next" is the
/// localised "NEXT". Deviation: the build's "eog" step (UI_TabbedProgressionContainer, "EOG Progression",
/// GenerateOwnContext: the progression pages and their prompt PLAY AGAIN) is cut: NEXT runs its PLAY AGAIN
/// (UI_EOG_PlayAgain's step) and then UI_EOG_PullToPreMatch, as the container would have.
const MATCHMADE_STEPS: [CanvasStep; 7] = [
    bare(Kind::Wait),
    RESULTS_STEP,
    widget("UI_EOG_RematchStatus", "Rematch Timer", true, false),
    TOAST_STEP,
    bare(Kind::AutoParty),
    widget("UI_SteppedNextButton", "NEXT", true, false),
    bare(Kind::PlayAgain),
];
/// The Arcade list (an arcade round): no results header, no rematch, no NEXT: the continue widget, then (Deviation, as
/// the Matchmade list's) the cut "eog" container's PLAY AGAIN and UI_EOG_PullToPreMatch.
const ARCADE_STEPS: [CanvasStep; 3] =
    [bare(Kind::Wait), widget("UI_ArcadeEOGContinue", "", false, false), bare(Kind::PlayAgain)];

/// Which list of steps a match's end has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepList {
    Custom,
    Matchmade,
    Arcade,
    Local,
}

impl StepList {
    fn steps(self) -> &'static [CanvasStep] {
        match self {
            StepList::Custom => &CUSTOM_STEPS,
            StepList::Matchmade => &MATCHMADE_STEPS,
            StepList::Arcade => &ARCADE_STEPS,
            StepList::Local => &LOCAL_STEPS,
        }
    }
}

/// A step's completion is the handler `CV_DONE` + its place in the list.
const CV_DONE: u32 = 100;
const CV_AGAIN_DONE: u32 = 1;
const CV_HOME: u32 = 2;

/// `ctl` created as the step and started: Update Context, On Complete bound to the canvas, Start.
macro_rules! start_step {
    ($cx:expr, $w:expr, $ty:ty, $ctx:expr, $h:expr) => {{
        $cx.visit::<$ty, _>($w, |c, cx| {
            c.update_context(cx, $ctx);
            c.on_complete.add($h);
            c.start(cx);
        });
    }};
}

/// UI_CanvasProgressionContainer (step "canvas") with its child steps (PGI_GlobalData EndOfGameProgressions): the
/// Custom list (a custom game), the Local one (local play) or the Matchmade one (a queue match), the results screen
/// chooses. Begin Next Step removes a finished RemoveOnCompletion step, creates the next one (a child of MainOverlay,
/// Fill / Fill, SelfHitTestInvisible, when it is on the container), and runs Init, Update Context and Start on it;
/// after the last one the container completes.
/// The canvas is the list's root (ParentID empty): VictoryCelebrationActor @16600..17226 creates it, adds it to the
/// viewport, Init, Update Context(NewGuid), SetRootContextForLevel(that context), AssignInputToWidget,
/// SetHandleInput(false), then BPI Start (the results screen). A GenerateOwnContext node gets a new context of its
/// own; none of the lists has one left.
/// Not on this canvas: bannersetup (the records go to the banners: the results screen). The stat sheets and the
/// toast buttons are on the podium banners; the step "toast" shows the toasts left and turns the buttons on.
#[derive(Debug)]
pub struct CanvasProgressionContainer {
    pub on_complete: Event<()>,
    current: isize,
    active: Option<WidgetId>,
    context: String,
    waiting: bool,
    list: StepList,
}

impl CanvasProgressionContainer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CanvasProgressionContainer {
            on_complete: Event::default(),
            current: -1,
            active: None,
            context: String::new(),
            waiting: false,
            list: StepList::Custom,
        })
    }

    /// The list of the match's context (before start).
    pub fn use_list(&mut self, list: StepList) {
        self.list = list;
    }

    pub fn update_context(&mut self, ctx: &str) {
        self.context = ctx.to_string();
    }

    pub fn start(&mut self, cx: &mut Cx<'_>) {
        if self.current == -1 {
            self.begin_next_step(cx);
        }
    }

    /// BPI_PodiumEvent from the podium's sequence director: ShowUI on the waitforsequence step
    /// (UI_WaitForPodiumUIEvent @449): SetHandleInput(true), then On Complete. (ShowStats goes to the banners.)
    pub fn podium_event(&mut self, cx: &mut Cx<'_>, name: &str) {
        if name == "ShowUI" && self.waiting {
            self.waiting = false;
            cx.view.manager.set_handle_input(true);
            self.begin_next_step(cx);
        }
    }

    fn begin_next_step(&mut self, cx: &mut Cx<'_>) {
        let steps = self.list.steps();
        let done = usize::try_from(self.current).ok().and_then(|i| steps.get(i));
        if let (Some(done), Some(active)) = (done, self.active)
            && done.remove
        {
            MenuView::remove_child(cx, active);
        }
        self.active = None;
        self.current += 1;
        let Some(step) = usize::try_from(self.current).ok().and_then(|i| steps.get(i)).copied() else { return };
        if step.cls.is_empty() {
            match step.kind {
                Some(Kind::Wait) => self.waiting = true,
                Some(Kind::AutoParty) => self.begin_next_step(cx),
                Some(Kind::PlayAgain) => self.play_again(cx),
                _ => self.return_home(cx),
            }
            return;
        }
        let me = cx.w;
        let w = MenuView::create(cx, step.cls);
        self.active = Some(w);
        {
            let x = wd(cx, w);
            x.h_align = HAlign::Fill;
            x.v_align = VAlign::Fill;
            x.vis = Vis::SelfHitTestInvisible;
        }
        let overlay = find(cx, "MainOverlay");
        MenuView::add_child(cx, overlay, w, None);
        let ctx = self.context.clone();
        let h = Handler { owner: me, id: CV_DONE + u32::try_from(self.current).unwrap_or(0) };
        match step.cls {
            "UI_EOG_MatchResults" => start_step!(cx, w, EOGMatchResults, &ctx, h),
            "UI_SteppedNextButton" => {
                cx.visit::<SteppedNextButton, _>(w, |c, cx| c.init(cx, step.name, step.invariant));
                start_step!(cx, w, SteppedNextButton, &ctx, h);
            }
            "UI_EOG_RematchStatus" => {
                cx.visit::<EOGRematchStatus, _>(w, |c, _| c.init());
                start_step!(cx, w, EOGRematchStatus, &ctx, h);
            }
            "UI_ArcadeEOGContinue" => start_step!(cx, w, ArcadeEOGContinue, &ctx, h),
            "UI_CollectionToasts" => start_step!(cx, w, CollectionToasts, &ctx, h),
            other => panic!("menu: no progression step {other}"),
        }
    }

    /// The cut "eog" container's two children (AddToContainer false: no widget on a panel): PLAY AGAIN's step, and on
    /// its completion UI_EOG_PullToPreMatch's Start (the PreMatch level for a new search); the list ends there.
    fn play_again(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let again = MenuView::create(cx, "UI_EOG_PlayAgain");
        cx.visit::<EOGPlayAgain, _>(again, |c, cx| {
            c.on_complete.add(Handler { owner: me, id: CV_AGAIN_DONE });
            c.step(cx);
        });
    }

    /// UI_EOG_PlayAgain's completion: UI_EOG_PullToPreMatch's Start, then the list's end.
    fn again_done(&mut self, cx: &mut Cx<'_>) {
        let pull = MenuView::create(cx, "UI_EOG_PullToPreMatch");
        cx.emit(self, |c| &c.on_complete, &());
        cx.visit::<EOGPullToPreMatch, _>(pull, |c, cx| c.start(cx));
    }

    /// UI_EOG_ReturnHome: Complete, UnfocusAll, Delay(0.2), ExitToMainMenu = ResetMatchFlow and a type 0 transition to
    /// New_MainMenu with the level options "Lobby".
    fn return_home(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.emit(self, |c| &c.on_complete, &());
        for p in 0..4 {
            Manager::unfocus_all(cx, p);
        }
        cx.view.delay(0.2, Handler { owner: me, id: CV_HOME });
    }
}

impl Controller for CanvasProgressionContainer {
    /// Deviation: the results fill the window at any aspect ratio.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == CV_AGAIN_DONE {
            self.again_done(cx);
        } else if id >= CV_DONE && isize::try_from(id - CV_DONE).is_ok_and(|i| i == self.current) {
            self.begin_next_step(cx);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == CV_HOME {
            Manager::transition(
                cx,
                TransitionParams {
                    next_level: "New_MainMenu".into(),
                    level_options: "Lobby".into(),
                    ..Default::default()
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_scores_count_wins() {
        let mut s = EogState { cache: vec![true, false, true], ..EogState::default() };
        assert_eq!(s.set_scores(), (2, 1));
        assert!(!s.won_in_two_rounds());
        s.cache = vec![false, false];
        assert!(s.won_in_two_rounds());
    }

    #[test]
    fn lists_end_in_their_last_steps() {
        assert_eq!(StepList::Custom.steps().len(), 5);
        assert_eq!(StepList::Local.steps().len(), 4);
        assert_eq!(StepList::Matchmade.steps().last().and_then(|s| s.kind), Some(Kind::PlayAgain));
        assert_eq!(StepList::Arcade.steps()[1].cls, "UI_ArcadeEOGContinue");
        // the matchmade list's rematch step is removed on completion, the toast step is not
        assert!(StepList::Matchmade.steps()[2].remove && !TOAST_STEP.remove);
    }
}
