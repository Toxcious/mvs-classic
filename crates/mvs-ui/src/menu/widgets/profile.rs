//! The main menu's profile (modal `profile`; UI_Profile_v2, UI_ProfileStats, UI_ProfileStat, RankedLeaderboard,
//! UI_MatchHistoryEntry, UI_MatchBreakdown, UI_MatchBreakdownPlayer). Everything the
//! panel shows came from Hydra in the build (GameProfileManager.GetPlayerProfile, the HydraUserObject, the
//! leaderboards).
//!
//! Deviation: the profile is the stats file's (`own_profile`): the online matches' records and totals the player's own
//! game kept, under the player's own name. The level and XP are no account's (`profile_rules::shown_level`), and the
//! ranks stay "N/A" (no ranked play).
//!
//! ChildOfflineMode (BPI_GetChildOfflineMode @9900, the game instance's) is false: the stats file stands in for the
//! backend, so OnInitialized shows BottomOverlay (the stats and the tab bar), GuildContainer and LevelXPBar.
//!
//! Deviation: IsRankedAvailable?'s Feature Enabled, a Hydra feature flag, is taken
//! as false here (the port has no ranked play): the two ranked leaderboard tabs leave the bar (@666) and the RANKED
//! SEASON STATS block is collapsed (UI_ProfileStats @3225 hides it, keeping its room). nav.rs PROFILE_CLASS still
//! picks this class over UI_Profile.

use std::any::Any;

use mvs_protocol::profile::{HistoryMode, MatchRecordPlayer, ProfileWire};
use serde_json::json;

use super::common::{
    ModalBase, Quadrilateral, apply_founder_name_treatment, color_for_team, fill_page, palette_entry, text,
};
use super::controls::buttons::{ButtonSecondary, IconButton};
use super::controls::interactor::{batch_set_context, set_context, set_focused, set_interactable};
use super::controls::{bind, find, focus_frame, ist, prop_bool, wd};
use super::modals::can_show_modal;
use super::nav::{GlobalNav, GlobalSlideOut, PROFILE_CLASS, find_global_nav};
use super::section_bar::SectionBar;
use crate::bundle::{Rgba, RosterEntry};
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{Manager, Modal};
use crate::menu::own_profile::{OWN_ID, load_own_profile, my_profile_icon, watch_profile_icon};
use crate::menu::profile_rules::{
    HistoryEntry, MODE_SOLO, MODE_TEAMS, SPECTATOR_TEAM, breakdown_teams, history_entry, level_row, most_played,
    shown_level, win_rate,
};
use crate::menu::screens::lab_selectors::{CosmeticTile, CosmeticType, EquipperTile, GridItemList, TileData};
use crate::menu::scroll::scroll_list;
use crate::menu::view::MenuView;
use crate::menu::widgets::collection_chooser::CollectionChooser;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// IsRankedAvailable?'s Feature Enabled, taken as false (see the top of the file).
const RANKED_FEATURE_ENABLED: bool = false;
/// The account id the port answers with: any value other than the panel's empty AccountID (OpenProfile).
const OWN_ACCOUNT_ID: &str = "local";

// Deviation: the profile screen has a pencil button right of the name, which opens UI_SetUsername in its "change"
// mode (dialogs.rs). The build has no rename route here (Binding_ChangeUsernameButtonVisibility is bound to no
// widget) and the sidebar's profile slot opens this screen, so without it the name could only be chosen once. The
// button is a UI_ButtonSecondary with the edit icon, as the Collection's Edit Perks button (UI_CollectionPerks'
// buttonEdit: ui_icons_edit at 40 x 40, no label padding), square to the name's height (the port's
// own, in place of a CHANGE NAME button under the name). The name and the button share a HorizontalBox of
// the port's own in ScaleBox_0, so the button follows the name's end.
const RENAME_TIP: &str = crate::menu::custom_text::CHANGE_NAME;
const RENAME_ICON: &str = "ui_icons_edit";
const RENAME_SIZE: [f64; 2] = [64.0, 64.0];
const RENAME_GAP: f64 = 20.0;

/// ToggleListSize's timeline: 0.4 s of real time, Ease(EaseOut, exponent 10) between these heights.
// Deviation: without the ranked block the short list reaches up to the career
// stats (840, the room the block left), in place of the build's 550 under the block.
const LIST_SHORT: f64 = if RANKED_FEATURE_ENABLED { 550.0 } else { 840.0 };
const LIST_TALL: f64 = 1050.0;
const LIST_EASE_TIME: f64 = 0.4;
/// ToggleTrayCover @14631 / @1075.
const TRAY_FADE_TIME: f64 = 0.3;

fn ease_out(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * (1.0 - (1.0 - t).powi(10))
}

const CREAM: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const CREAM_FADED: Rgba = [0.89627, 0.973446, 0.871367, 0.4];
const FOCUS_ORANGE: Rgba = [1.0, 0.223228, 0.002428, 1.0];
const WHITE_TEXT: Rgba = [1.0; 4];
const BREAKDOWN_DRAW: Rgba = [0.262251, 0.366253, 0.396755, 1.0];

/// UI_AccountLevel_XPBar's OnClick @139-400: with no `profile` modal the profile is shown
/// (ShowModal-Generic(false, class, "profile")); an open one is left as it is (no toggle, no dismissal of other
/// modals there, unlike the profile slot's click).
pub fn show_profile_from_xp_bar(cx: &mut Cx<'_>) {
    if cx.view.manager.get_modal("profile").is_some() {
        return;
    }
    if can_show_modal(cx, PROFILE_CLASS, "profile") {
        Manager::show_modal(cx, PROFILE_CLASS, "profile", "", "");
    }
}

/// Conv_IntToText(v, false, true, 1, 324): with grouping.
fn int_text(v: f64) -> String {
    let n = v.trunc() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// GetWinRateText: "N/A" below 0, else "{percent}%" of Round(rate * 100).
fn win_rate_text(r: f64) -> String {
    if r < 0.0 { "N/A".to_string() } else { format!("{}%", (r * 100.0).round()) }
}

/// The interactor of the user widget `w` (its UI_Interactor child).
fn interactor_in(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

fn set_text(cx: &mut Cx<'_>, name: &str, t: &str) {
    let w = find(cx, name);
    wd(cx, w).text = t.to_string();
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn set_color(cx: &mut Cx<'_>, name: &str, c: Rgba) {
    let w = find(cx, name);
    wd(cx, w).color = c;
}

// ---------- what the history reads of the build's data ----------

/// The EX_TextConst keys of the history's Game strings, by what the graphs set them on: UI_MatchHistoryEntry
/// GetMapAndModeText (@411, @348, @278) and GetVictoryDefeatText (@253, @189, @127; @43 a spectated match's), the
/// spectated header @3810; UI_MatchBreakdown SetMapAndModeText (@360, @175, @86) and SetVictoryDefeatText (@166, @102,
/// @40; spectated @547, @951).
mod texts {
    pub const ENTRY_MODE: [&str; 3] =
        ["681AD7484AAEDE692897EF8A77BD8D8C", "3C0EF72945B5E7488210B38F5FC6BB8F", "0ABB0A3F43B89BD6A9FB1E8029AF6935"];
    pub const ENTRY_RESULT: [&str; 3] =
        ["05661FA34147BCA06D1E5EA30398DF7C", "45710C094051CDCEA6909C9A280217AC", "BC6C2BDA4AEC145828EFC7963CB845CD"];
    pub const ENTRY_SPECTATED_RESULT: &str = "B54B64AB48870D59DEE31AB189A15AD5";
    pub const ENTRY_TEAM: &str = "790DF82C42BDC057F6ED4988AC8F22F0";
    pub const BREAKDOWN_MODE: [&str; 3] =
        ["9634579C48E72A3603A7298D8F99C56A", "A86626344C92D1B4F14784A41FE40F6D", "C40967F5429C097943763CAE189982D1"];
    pub const BREAKDOWN_RESULT: [&str; 3] =
        ["623526EF4B240087F8DAAE83E78B2489", "4840E50A4EE9795EF959D7B27E417E4D", "B89176154112EAA6AB580D968E2BAA80"];
    pub const BREAKDOWN_USER_WINS: &str = "4E7586DD45A679083CB9649A9F372C52";
    pub const BREAKDOWN_TEAM_VICTORY: &str = "01E0822548BA24BCBDFBAEAF8997B6D2";
}

/// A mode's or a result's string; another value (EGameMode 3 / 4 / 5) the empty text.
fn mode_text(view: &MenuView, keys: &[&str; 3], mode: usize) -> String {
    keys.get(mode).map(|k| view.game_text(k).to_string()).unwrap_or_default()
}

/// Format(text, [{name: value}]).
fn format_arg(text: &str, name: &str, value: &str) -> String {
    text.replace(&format!("{{{name}}}"), value)
}

/// PGI_GlobalData GlobalCharacterData[slug]: the record names a fighter by the port's character key, whose
/// CharacterData is the fighter's slug; None when the map has none.
fn character_of(view: &MenuView, key: &str) -> Option<RosterEntry> {
    if key.is_empty() {
        return None;
    }
    let slug = view.fighters().get(key).map_or(key, |f| f.slug.as_str());
    let roster = view.roster();
    roster
        .iter()
        .find(|r| r.slug == slug)
        .or_else(|| roster.iter().find(|r| r.fighter.as_deref() == Some(key)))
        .cloned()
}

/// SetBrushFromSoftMaterial(CharacterSelectPortraitMaterial); none: the null material.
fn set_portrait(cx: &mut Cx<'_>, name: &str, c: Option<&RosterEntry>) {
    let w = find(cx, name);
    let x = wd(cx, w);
    x.tex = c.map(|c| c.portrait.clone()).filter(|p| !p.is_empty());
    x.mid = None;
}

/// The map's MapData DisplayName: the record names it by its MapData asset.
fn map_name(view: &MenuView, name: &str) -> String {
    view.map(name).map(|m| m.data.display_name).unwrap_or_default()
}

// ---------- UI_ProfileStat (one value with its title) ----------

/// UI_ProfileStat. PreConstruct @145 -> @81: the title, then (not at design time) Set Value of an empty text @10.
#[derive(Debug, Default)]
pub struct ProfileStat;

impl ProfileStat {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ProfileStat)
    }

    /// Set Value @31.
    pub fn set_value(&mut self, cx: &mut Cx<'_>, v: &str) {
        set_text(cx, "labelValue", v);
    }
}

impl Controller for ProfileStat {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let t = text(cx.view.tree().props(cx.w).get("Title"));
        set_text(cx, "labelTitle", &t);
        self.set_value(cx, "");
    }
}

// ---------- UI_ProfileStats (CAREER STATS and RANKED SEASON STATS) ----------

/// What Update Appearance @337 reads of a profile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileValues {
    pub lifetime_matches_played: u64,
    pub lifetime_ringouts: u64,
    pub most_played_character_slug: String,
    pub teams_rank: i64,
    pub solo_rank: i64,
    pub teams_win_rate: f64,
    pub solo_win_rate: f64,
}

const PS_LEADERBOARDS: u32 = 1;

/// UI_ProfileStats.
#[derive(Debug, Default)]
pub struct ProfileStats {
    context: String,
}

impl ProfileStats {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ProfileStats::default())
    }

    /// Set User @10: the rank badges' SetUser (ProfileRankBadge: not ported, its tree stays) and its own
    /// GetPlayerProfile @180 (a second load with no history). Deviation: no second load; the page's answer is handed
    /// in and OnPlayerProfileLoaded @1634 runs Update Appearance with it.
    pub fn set_user(&mut self, cx: &mut Cx<'_>, p: &ProfileValues) {
        self.update_appearance(cx, p);
    }

    /// Update Appearance @337.
    fn update_appearance(&mut self, cx: &mut Cx<'_>, p: &ProfileValues) {
        // @543: GlobalCharacterData's DisplayName of the slug, "N/A" when the map has none
        let character =
            character_of(cx.view, &p.most_played_character_slug).map_or_else(|| "N/A".to_string(), |c| c.display_name);
        let rank = |r: i64| if r > 0 { int_text(r as f64) } else { "N/A".to_string() };
        let values = [
            ("UI_ProfileStat_Matches", int_text(p.lifetime_matches_played as f64)),
            ("UI_ProfileStat_Ringouts", int_text(p.lifetime_ringouts as f64)),
            ("UI_ProfileStat_Character", character),
            ("UI_ProfileStat_NormalRankTeams", rank(p.teams_rank)),
            ("UI_ProfileStat_NormalRank1v1", rank(p.solo_rank)),
            ("UI_ProfileStat_NormalWinRateTeams", win_rate_text(p.teams_win_rate)),
            ("UI_ProfileStat_NormalWinRate1v1", win_rate_text(p.solo_win_rate)),
        ];
        for (n, v) in values {
            let w = find(cx, n);
            cx.visit::<ProfileStat, _>(w, |s, cx| s.set_value(cx, &v));
        }
    }

    /// Update Context @1732: the rank badges' (not ported) and the leaderboards button's interactor.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let b = find(cx, "buttonNormalLeaderboards");
        let i = ButtonSecondary::interactor(cx, b);
        set_context(cx, i, ctx);
    }
}

impl Controller for ProfileStats {
    /// OnInitialized @3178: EnterAnim, and the ranked block by IsRankedAvailable?'s Feature Enabled (4 / 2).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
        // Deviation: Collapsed rather than the build's Hidden without ranked, so the block's room goes too
        set_vis(cx, "containerRanked", if RANKED_FEATURE_ENABLED { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        // buttonNormalLeaderboards' OnClick @1932: ShowModal-Generic(UI_Leaderboards, "leaderboards"), a class the
        // menu bundle lacks (can_show_modal warns once)
        let b = find(cx, "buttonNormalLeaderboards");
        let h = Handler { owner: me, id: PS_LEADERBOARDS };
        cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(h));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == PS_LEADERBOARDS && can_show_modal(cx, "UI_Leaderboards", "leaderboards") {
            Manager::show_modal(cx, "UI_Leaderboards", "leaderboards", "", "");
        }
    }
}

// ---------- UI_MatchHistoryEntry (one match of the history) ----------

const MH_FOCUS: u32 = 1;
const MH_UNFOCUS: u32 = 2;
const MH_CLICK: u32 = 3;

/// UI_MatchHistoryEntry.
#[derive(Debug, Default)]
pub struct MatchHistoryEntry {
    pub entry: Option<HistoryEntry>,
    /// The ProfileV2 that holds it.
    pub parent_menu: Option<WidgetId>,
    was_spectator: bool,
}

impl MatchHistoryEntry {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MatchHistoryEntry::default())
    }

    /// PopulateData @3980 -> @2019.
    pub fn populate_data(&mut self, cx: &mut Cx<'_>, e: HistoryEntry) {
        let r = e.record.clone();
        self.was_spectator = e.player_stats.team == SPECTATOR_TEAM;
        if self.was_spectator {
            // @555-1726: the winning team's players found in GlobalCharacterData, the first on CharacterPortrait
            // (@1940), the others on SpectatedAllyPortait, shown (@1403, @1470)
            let mut first = true;
            for p in &r.players {
                if p.team != r.winning_team {
                    continue;
                }
                let Some(c) = character_of(cx.view, &p.character) else { continue };
                if first {
                    set_portrait(cx, "CharacterPortrait", Some(&c));
                } else {
                    set_portrait(cx, "SpectatedAllyPortait", Some(&c));
                    set_vis(cx, "SpectatedAllyPortait", Vis::SelfHitTestInvisible);
                }
                first = false;
            }
        } else if let Some(c) = character_of(cx.view, &e.player_stats.character) {
            // @2288 / @347: the owner's character; one the map lacks leaves the brush (a PrintString only, @458)
            set_portrait(cx, "CharacterPortrait", Some(&c));
        }
        // @2484: GetMapAndModeText (the mode by GameMode @411 / @348 / @278, the map's DisplayName @709) and
        // GetVictoryDefeatText (played @253 / @189 / @127, spectated @43)
        let mode = mode_text(cx.view, &texts::ENTRY_MODE, r.mode.0 as usize);
        set_text(cx, "GameModeText", &mode);
        let map = map_name(cx.view, &r.map);
        set_text(cx, "MapText", &map);
        let result = if self.was_spectator {
            cx.view.game_text(texts::ENTRY_SPECTATED_RESULT).to_string()
        } else {
            mode_text(cx.view, &texts::ENTRY_RESULT, e.match_result)
        };
        set_text(cx, "VictoryDefeatText", &result);
        self.entry = Some(e);
        self.result_colours(cx);
        // @3543-3941: a spectated match's header "Team {id}", WinningTeamId + 1
        if self.was_spectator {
            let t = format_arg(cx.view.game_text(texts::ENTRY_TEAM), "id", &(r.winning_team + 1).to_string());
            set_text(cx, "Spectate_VictoryTextHeader", &t);
            set_vis(cx, "Spectate_VictoryTextHeader", Vis::SelfHitTestInvisible);
        }
    }

    /// @2715-3397 (and @4616-5298): spectated Color for Team(WinningTeamId); MatchResult 0 palette 0, 1 palette 1
    /// (.UI), 2 cream.
    fn result_colours(&self, cx: &mut Cx<'_>) {
        let Some(e) = &self.entry else { return };
        let c = if self.was_spectator {
            color_for_team(cx, e.record.winning_team, None)
        } else {
            match e.match_result {
                0 => palette_entry(cx, 0),
                1 => palette_entry(cx, 1),
                _ => CREAM,
            }
        };
        set_color(cx, "VictoryDefeatText", c);
        set_color(cx, "Spectate_VictoryTextHeader", c);
    }
}

impl Controller for MatchHistoryEntry {
    /// The interactor's OnFocus @4017, OnUnfocus @4272 and OnClick @4012.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_in(cx, cx.w);
        bind(cx, i, |s| &mut s.on_focus, MH_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, MH_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, MH_CLICK);
    }

    /// Construct @5445.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let (fc, i) = (find(cx, "UI_FocusedCommon"), interactor_in(cx, cx.w));
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            MH_FOCUS => {
                for n in ["VictoryDefeatText", "MapText", "GameModeText", "Spectate_VictoryTextHeader"] {
                    set_color(cx, n, FOCUS_ORANGE);
                }
            }
            MH_UNFOCUS => {
                for n in ["VictoryDefeatText", "MapText", "Spectate_VictoryTextHeader"] {
                    set_color(cx, n, WHITE_TEXT);
                }
                set_color(cx, "GameModeText", CREAM_FADED);
                self.result_colours(cx);
            }
            // @15: IsRankedAvailable?'s Feature Enabled (taken as true) -> UI_Profile_v2.OpenMatchBreakdown
            MH_CLICK => {
                if let (Some(e), Some(p)) = (self.entry.clone(), self.parent_menu) {
                    let me = cx.w;
                    cx.visit::<ProfileV2, _>(p, |p, cx| p.open_match_breakdown(cx, e, me));
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_MatchBreakdownPlayer (one player of the breakdown) ----------

const MP_FOCUS: u32 = 1;
const MP_UNFOCUS: u32 = 2;

/// UI_MatchBreakdownPlayer.
#[derive(Debug, Default)]
pub struct MatchBreakdownPlayer {
    is_me: bool,
}

impl MatchBreakdownPlayer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MatchBreakdownPlayer::default())
    }

    /// The line's button (btnPlayerContextMenu), an interactor.
    pub fn button(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "btnPlayerContextMenu")
    }

    /// PopulateData @227 -> @15. `me`: the account id of the page's own player (GetClientGUID).
    pub fn populate_data(&mut self, cx: &mut Cx<'_>, p: &MatchRecordPlayer, e: &HistoryEntry, me: &str) {
        self.is_me = !p.bot && p.id == me;
        // @15 / @2178: the name, then shown. Deviation: LoadHydraUser is not asked: the record's name at the match is
        // shown (the build's fallback when the user does not load, @2714)
        set_vis(cx, "UsernameText", Vis::Hidden);
        set_text(cx, "UsernameText", &p.name);
        set_vis(cx, "UsernameText", Vis::SelfHitTestInvisible);
        // @284
        let c = character_of(cx.view, &p.character);
        set_portrait(cx, "CharacterPortrait", c.as_ref());
        // @572: UI_UserPlatformIndicator's UpdateForTargetPlatform: not ported (the tree's indicator stays); @626
        let ffa = e.record.mode == HistoryMode::FREE_FOR_ALL;
        set_vis(cx, "FFAHorizontalBox", if ffa { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        // @726
        set_text(cx, "DamageDealtText", &int_text(f64::from(p.damage)));
        set_text(cx, "DeathsText", &int_text(f64::from(p.deaths)));
        set_text(cx, "RingoutsText", &int_text(f64::from(p.ringouts)));
        set_text(cx, "FFAScoreNumber", &int_text(f64::from(p.score)));
        // @1139: the entry's own player palette 2 (Me), else team 0 / 1 / 2 / 3 -> palette 0 / 1 / 3 / 4
        let team = find(cx, "TeamColor");
        if p.name == e.player_stats.name {
            let c = palette_entry(cx, 2);
            cx.visit::<Quadrilateral, _>(team, |q, cx| q.update_color(cx, c));
        } else if (0..=3).contains(&p.team) {
            let c = palette_entry(cx, [0, 1, 3, 4][p.team as usize]);
            cx.visit::<Quadrilateral, _>(team, |q, cx| q.update_color(cx, c));
        }
        // @1719
        let b = Self::button(cx, cx.w);
        set_interactable(cx, b, !self.is_me);
    }
}

impl Controller for MatchBreakdownPlayer {
    /// btnPlayerContextMenu's OnFocus @2591 (-> @1959) and OnUnfocus @2596. Its OnClick @2217 opens
    /// ShowSocialActionsMenu at the button's top left + (50, 50): not ported (no social actions).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button(cx, cx.w);
        bind(cx, b, |s| &mut s.on_focus, MP_FOCUS);
        bind(cx, b, |s| &mut s.on_unfocus, MP_UNFOCUS);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            MP_FOCUS => {
                if !self.is_me {
                    set_vis(cx, "WBP_Quadrilateral", Vis::SelfHitTestInvisible);
                }
            }
            MP_UNFOCUS => set_vis(cx, "WBP_Quadrilateral", Vis::Collapsed),
            _ => {}
        }
    }
}

// ---------- UI_MatchBreakdown (the popup of one match) ----------

const MB_CLOSE: u32 = 1;

/// UI_MatchBreakdown.
#[derive(Debug, Default)]
pub struct MatchBreakdown {
    /// The ProfileV2 that holds it.
    pub parent_menu: Option<WidgetId>,
    pub entry: Option<HistoryEntry>,
    context: String,
    player_margin: Option<Margin>,
    yours: Vec<MatchRecordPlayer>,
}

impl MatchBreakdown {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MatchBreakdown::default())
    }

    /// PopulateData @63.
    pub fn populate_data(&mut self, cx: &mut Cx<'_>, me: &str) {
        self.populate_match_players(cx, me);
        self.set_map_and_mode_text(cx);
        self.set_victory_defeat_text(cx);
        let ctx = self.context.clone();
        self.update_context(cx, &ctx);
    }

    /// PopulateMatchPlayers (profile_rules::breakdown_teams), @1012: both containers cleared, a line per player.
    fn populate_match_players(&mut self, cx: &mut Cx<'_>, me: &str) {
        let Some(e) = self.entry.clone() else { return };
        let (yours, theirs) = breakdown_teams(&e);
        self.yours = yours.clone();
        for (box_name, list) in [("YourTeamContainer", yours), ("TheirTeamContainer", theirs)] {
            let bx = find(cx, box_name);
            for c in cx.view.tree().widget(bx).children.clone() {
                MenuView::discard(cx, c);
            }
            for p in &list {
                let w = MenuView::create(cx, "UI_MatchBreakdownPlayer");
                cx.visit::<MatchBreakdownPlayer, _>(w, |l, cx| l.populate_data(cx, p, &e, me));
                MenuView::add_child(cx, bx, w, None);
                if let Some(m) = self.player_margin {
                    wd(cx, w).pad = m;
                }
            }
        }
    }

    /// SetMapAndModeText: the mode @360 / @175 / @86. Unverified: the map is PGI_GlobalData
    /// GetMapNameFromPath(MapSlug) @676 (native, not decoded; the build reads the slug as a map path there): the port
    /// shows the entry's MapData DisplayName.
    fn set_map_and_mode_text(&self, cx: &mut Cx<'_>) {
        let Some(r) = self.entry.as_ref().map(|e| &e.record) else { return };
        let mode = mode_text(cx.view, &texts::BREAKDOWN_MODE, r.mode.0 as usize);
        let map = map_name(cx.view, &r.map);
        set_text(cx, "Mode", &mode);
        set_text(cx, "Map", &map);
    }

    /// SetVictoryDefeatText: played @166 / @102 / @40; spectated by GameMode 0 and 2 "{username} Wins" (YourTeam[0])
    /// @547, 1 and 3 "Team {id} Victory" (WinningTeamId + 1) @951. The colour @1337-1855.
    fn set_victory_defeat_text(&self, cx: &mut Cx<'_>) {
        let Some(e) = &self.entry else { return };
        let r = &e.record;
        if e.player_stats.team == SPECTATOR_TEAM {
            let t = if r.mode.0 == 0 || r.mode.0 == 2 {
                let name = self.yours.first().map_or("", |p| p.name.as_str());
                format_arg(cx.view.game_text(texts::BREAKDOWN_USER_WINS), "username", name)
            } else {
                format_arg(cx.view.game_text(texts::BREAKDOWN_TEAM_VICTORY), "id", &(r.winning_team + 1).to_string())
            };
            set_text(cx, "VictoryText", &t);
            let c = color_for_team(cx, r.winning_team, None);
            set_color(cx, "VictoryText", c);
            return;
        }
        let t = mode_text(cx.view, &texts::BREAKDOWN_RESULT, e.match_result);
        set_text(cx, "VictoryText", &t);
        let c = match e.match_result {
            0 => color_for_team(cx, 0, None),
            1 => color_for_team(cx, 1, None),
            _ => BREAKDOWN_DRAW,
        };
        set_color(cx, "VictoryText", c);
    }

    /// GetInteractors: Btn_Back's, then each line's button (TheirTeamContainer's, then YourTeamContainer's).
    pub fn get_interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let mut list = vec![interactor_in(cx, find(cx, "Btn_Back"))];
        for n in ["TheirTeamContainer", "YourTeamContainer"] {
            let bx = find(cx, n);
            for &c in &cx.view.tree().widget(bx).children {
                if cx.view.tree().user_class(c) == Some("UI_MatchBreakdownPlayer") {
                    list.push(MatchBreakdownPlayer::button(cx, c));
                }
            }
        }
        list
    }

    /// Update Context @356.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let list = Self::get_interactors(cx);
        batch_set_context(cx, &list, ctx);
    }

    /// Close @462 / OnCancel @351 / Btn_Back -> @15: ParentMenu.ClosePopupMenu(Open, reverse).
    pub fn close(&self, cx: &mut Cx<'_>) {
        if let Some(p) = self.parent_menu {
            cx.visit::<ProfileV2, _>(p, |p, cx| p.close_popup_menu(cx));
        }
    }
}

impl Controller for MatchBreakdown {
    /// Btn_Back's OnClick -> @15.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "Btn_Back");
        let h = Handler { owner: cx.w, id: MB_CLOSE };
        cx.visit::<IconButton, _>(b, |b, _| b.on_click.add(h));
    }

    /// Construct @147: PlayerMargin = the slot padding of YourTeamContainer's child 0.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let bx = find(cx, "YourTeamContainer");
        if let Some(&first) = cx.view.tree().widget(bx).children.first() {
            self.player_margin = Some(cx.view.tree().widget(first).pad);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == MB_CLOSE {
            self.close(cx);
        }
    }
}

// ---------- RankedLeaderboard (the two leaderboard tabs) ----------

/// RankedLeaderboard.
#[derive(Debug, Default)]
pub struct RankedLeaderboard {
    /// The do-once's IsClosed (its Has Been Initd is set on the first call and only steers that call).
    closed: bool,
}

impl RankedLeaderboard {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(RankedLeaderboard::default())
    }

    /// Load @241 -> @102: a do-once that starts open. The first call marks it initialised (@216) and its pop_flow
    /// resumes at @122: the loading panel and LoadLeaderboard (Hydra: the port has no answer, so the spinner stays);
    /// later calls do nothing (IsClosed).
    pub fn load(&mut self, cx: &mut Cx<'_>) {
        if self.closed {
            return;
        }
        self.closed = true;
        Self::set_panel(cx, "panelLoading");
    }

    /// Update Context: the entries' (none: the entry box stays empty).
    pub fn update_context(&mut self, _cx: &mut Cx<'_>, _ctx: &str) {}

    fn set_panel(cx: &mut Cx<'_>, name: &str) {
        let sw = find(cx, "mainSwitcher");
        let t = cx.view.tree();
        if let Some(at) = t.widget(sw).children.iter().position(|&c| t.widget(c).name == name) {
            wd(cx, sw).active = at as i32;
        }
    }
}

impl Controller for RankedLeaderboard {
    /// OnInitialized @246: the loading panel (its spinner).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        Self::set_panel(cx, "panelLoading");
    }
}

// ---------- UI_Profile_v2 (the modal `profile`) ----------

/// The tab bar's IDs (case-insensitive) and the switcher's panels (@16463).
fn tab_panel(id: &str) -> Option<&'static str> {
    match id.to_lowercase().as_str() {
        "matchhistory" => Some("panelMatchHistory"),
        "spectatehistory" => Some("panelSpectateHistory"),
        "teamsleaderboard" => Some("RankedLeaderboardTeams"),
        "1vs1leaderboard" => Some("RankedLeaderboard1v1"),
        _ => None,
    }
}

const P_SCROLL: u32 = 1;
const P_SUBMIT2: u32 = 2;
const P_TAB: u32 = 3;
const P_CHOOSER_SHOWN: u32 = 4;
const P_CHOOSER_HIDDEN: u32 = 5;
const P_EQUIPPER_SELECTED: u32 = 6;
const P_RENAME: u32 = 7;
const P_POST_SHOW: u32 = 10;
const P_LOADED: u32 = 11;
const P_EDIT: u32 = 12;
const P_CLOSE_WAIT: u32 = 13;

/// A WBP_GlobalSlideOut panel: the header "Career", the name, the level and its XP bar, UI_ProfileStats, and the tab
/// bar over the match history, spectate history and the two ranked leaderboards, and the profile icon equipper with
/// its UI_CollectionChooser of the icons, and UI_MatchBreakdown over the history. Not ported: the social actions menu
/// of a breakdown line, the guild, party and follow actions (Hydra), the platform indicator's SetUser (its image
/// stays Collapsed) and the analytics calls (Record UIFlow "match_breakdown").
#[derive(Default)]
pub struct ProfileV2 {
    base: ModalBase,
    guid: String,
    is_loading_profile: bool,
    profile_loaded: bool,
    account_id: String,
    toggle_more_history: bool,
    moving: bool,
    move_start: f64,
    bound: bool,
    rename: Option<WidgetId>,
    /// ToggleTrayCover's fade (@15961, the loop @863): its direction and start, None when it is not running.
    cover_show: bool,
    cover_start: Option<f64>,
    /// The profile shown (the stats file's, or an empty one), and the answer on its way.
    profile: Option<ProfileWire>,
    pending: Option<ProfileWire>,
    /// The entries in list order.
    entries: Vec<WidgetId>,
    /// EntryPadding (@163) and the popup state: UI_Base IsPopupOpen / LastFocused, OpenedMatchBreakdownInteractor.
    entry_padding: Margin,
    popup_open: bool,
    last_focused: Option<WidgetId>,
    opened_interactor: Option<WidgetId>,
    /// The equipper's data (UI_EquipperItem's `data`, which the tile does not hand out).
    equipper_tile: Option<EquipperTile>,
    /// The name and icon last drawn: the port's stores have no change events (identity.onChanged,
    /// onHydraPrefsChanged), their writes are seen on the next tick.
    seen_name: String,
    seen_icon: String,
}

impl std::fmt::Debug for ProfileV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileV2").field("guid", &self.guid).field("account_id", &self.account_id).finish()
    }
}

impl ProfileV2 {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ProfileV2::default())
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }

    fn bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_SectionBar_Lists")
    }

    fn scroll_box(cx: &Cx<'_>) -> WidgetId {
        find(cx, "listsScrollbox")
    }

    fn chooser(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_CollectionChooser")
    }

    fn equipper(cx: &Cx<'_>) -> WidgetId {
        find(cx, "profileIconEquipper")
    }

    fn breakdown(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_MatchBreakdown_New")
    }

    /// The profile's own account id (the record's player id of the page's player).
    fn owner_id(&self) -> String {
        self.profile.as_ref().map(|p| p.id.clone()).filter(|i| !i.is_empty()).unwrap_or_else(|| OWN_ID.to_string())
    }

    fn equipper_interactor(cx: &Cx<'_>) -> WidgetId {
        interactor_in(cx, Self::equipper(cx))
    }

    /// IsMe?: the profile shown is the player's own (the only one the port opens).
    fn is_me(&self) -> bool {
        self.account_id == OWN_ACCOUNT_ID
    }

    fn chooser_shown(cx: &mut Cx<'_>) -> bool {
        let ch = Self::chooser(cx);
        cx.visit::<CollectionChooser, _>(ch, |c, _| c.shown()).unwrap_or(false)
    }

    /// OpenProfile @5782.
    fn open_profile(&mut self, cx: &mut Cx<'_>, account_id: &str, username: &str) {
        if account_id != self.account_id && !self.is_loading_profile {
            self.account_id = account_id.to_string();
            self.load_profile_from_hydra(cx, username);
        } else {
            self.update_context_and_focus(cx, true);
        }
    }

    /// LoadProfileFromHydra @5125. The equipper's icon texture (20210225_CN_AT_Icon_Profile_PButler @5408) is not in
    /// the menu textures: the tree's stays.
    fn load_profile_from_hydra(&mut self, cx: &mut Cx<'_>, _username: &str) {
        self.is_loading_profile = true;
        self.update_bindings(cx);
        set_vis(cx, "Content", Vis::Hidden);
        set_vis(cx, "ProfileIcon", Vis::Hidden);
        self.toggle_editing(cx, false);
        let id = self.base.modal_id.clone();
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.open_slide_out(cx, &id));
        // GetPlayerProfile's answer -> OnLoadProfileResult @5522 (own_profile::load_own_profile). The spinner shows
        // while it is out: the answer comes on the next frame.
        self.pending = Some(load_own_profile(cx));
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: P_LOADED });
    }

    fn on_load_profile_result(&mut self, cx: &mut Cx<'_>, profile: ProfileWire) {
        self.profile = Some(profile.clone());
        self.profile_loaded = true;
        self.is_loading_profile = false;
        set_vis(cx, "Content", Vis::SelfHitTestInvisible);
        set_vis(cx, "ProfileIcon", Vis::SelfHitTestInvisible);
        self.populate_data(cx, &profile);
    }

    /// PopulateData @6406.
    fn populate_data(&mut self, cx: &mut Cx<'_>, profile: &ProfileWire) {
        self.populate_match_history(cx, profile);
        // SetGuildInfo: guild.ToggleGuildSystem is off (DefaultEngine.ini [SystemSettings] L119
        // guild.ToggleGuildSystem=False) -> Collapsed
        set_vis(cx, "GuildContainer", Vis::Collapsed);
        // SetChangeUsernameText: the profile's Username (the player's own name: see the top of the file)
        let name = cx.prefs.profile.name.clone();
        self.seen_name.clone_from(&name);
        set_text(cx, "TextUsername", &name);
        // @937 Apply Founder Pack Name Treatment(TextUsername, the Hydra user)
        let tu = find(cx, "TextUsername");
        apply_founder_name_treatment(cx, tu);
        let t = &profile.totals;
        let values = ProfileValues {
            lifetime_matches_played: t.matches,
            lifetime_ringouts: t.ringouts,
            most_played_character_slug: most_played(t),
            teams_rank: 0,
            solo_rank: 0,
            teams_win_rate: win_rate(t, MODE_TEAMS),
            solo_win_rate: win_rate(t, MODE_SOLO),
        };
        let ps = find(cx, "UI_ProfileStats");
        cx.visit::<ProfileStats, _>(ps, |s, cx| s.set_user(cx, &values));
        self.set_level_info(cx);
        self.set_xp_bar_percentage(cx);
        self.update_profile_icon(cx);
        self.update_context_and_focus(cx, true);
    }

    /// PopulateMatchHistory: EntryPadding from MatchHistoryContainer's child 0 (@163), both lists cleared (@212,
    /// @248; the tree's seven placeholder entries each), then per match of the profile in its order (newest first) an
    /// entry (@478) with the list's scroll box (@531), its data (@752) and this panel as its parent (@797), into the
    /// spectate list for a spectator (@965) else the match list, with EntryPadding (@1086, @2320); each list's last
    /// entry 60 below it (@1472, @1976), MATCH HISTORY selected (@2367), with no spectated match the SPECTATE HISTORY
    /// tab removed (@2512), then the entries chained up and down (@2146, the push_flow the function runs last).
    fn populate_match_history(&mut self, cx: &mut Cx<'_>, profile: &ProfileWire) {
        let me = cx.w;
        let mc = find(cx, "MatchHistoryContainer");
        let sc = find(cx, "SpectateHistoryContainer");
        if let Some(&first) = cx.view.tree().widget(mc).children.first() {
            self.entry_padding = cx.view.tree().widget(first).pad;
        }
        for bx in [sc, mc] {
            for c in cx.view.tree().widget(bx).children.clone() {
                MenuView::discard(cx, c);
            }
        }
        let sb = Self::scroll_box(cx);
        let owner = self.owner_id();
        let mut list = Vec::new();
        self.entries.clear();
        for r in &profile.matches {
            let w = MenuView::create(cx, "UI_MatchHistoryEntry");
            let i = interactor_in(cx, w);
            if let Some(s) = ist(cx, i) {
                s.scroll_box = Some(sb);
            }
            list.push(i);
            let h = history_entry(r, &owner);
            let spectator = h.player_stats.team == SPECTATOR_TEAM;
            cx.visit::<MatchHistoryEntry, _>(w, |e, cx| {
                e.populate_data(cx, h);
                e.parent_menu = Some(me);
            });
            MenuView::add_child(cx, if spectator { sc } else { mc }, w, None);
            wd(cx, w).pad = self.entry_padding;
            self.entries.push(w);
        }
        for bx in [mc, sc] {
            if let Some(&last) = cx.view.tree().widget(bx).children.last() {
                wd(cx, last).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 60.0 };
            }
        }
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index_by_id(cx, "matchHistory"));
        if cx.view.tree().widget(sc).children.is_empty() {
            cx.visit::<SectionBar, _>(bar, |b, cx| b.remove_option(cx, "spectateHistory"));
        }
        self.setup_manual_navigation(cx, &list);
        // (the portraits the entries draw, held while the panel is up)
        let mut pics = Vec::new();
        for r in &profile.matches {
            for p in &r.players {
                if let Some(c) = character_of(cx.view, &p.character)
                    && !c.portrait.is_empty()
                    && !pics.contains(&c.portrait)
                {
                    pics.push(c.portrait);
                }
            }
        }
        cx.view.hold_pictures("profile", pics.iter().map(String::as_str));
        self.update_bindings(cx);
    }

    /// UI_Functions Setup Manual Navigation(Interactors, Orientation 1): the focusable ones in order, each Up to the
    /// one before it and Down to the one after it (no wrap).
    fn setup_manual_navigation(&self, cx: &mut Cx<'_>, list: &[WidgetId]) {
        let valid: Vec<WidgetId> = list.iter().copied().filter(|&i| Manager::is_focusable(cx, i)).collect();
        for (k, &i) in valid.iter().enumerate() {
            let up = (k > 0).then(|| valid[k - 1]);
            let down = valid.get(k + 1).copied();
            if let Some(s) = ist(cx, i) {
                if up.is_some() {
                    s.manual_up = up;
                }
                if down.is_some() {
                    s.manual_down = down;
                }
            }
        }
    }

    /// OpenMatchBreakdown @6734: the focused interactor kept (@7071), the breakdown's entry and PopulateData (@7139),
    /// OpenPopupMenu(breakdown, Open, forward) (@7175), and each breakdown interactor Left to the entry's (@3594).
    pub fn open_match_breakdown(&mut self, cx: &mut Cx<'_>, e: HistoryEntry, source: WidgetId) {
        self.opened_interactor = cx.view.manager.get_currently_focused(0);
        let b = Self::breakdown(cx);
        let me = cx.w;
        let owner = self.owner_id();
        cx.visit::<MatchBreakdown, _>(b, |m, cx| {
            m.entry = Some(e);
            m.populate_data(cx, &owner);
            m.parent_menu = Some(me);
        });
        self.open_popup_menu(cx);
        let src = interactor_in(cx, source);
        let list = cx.visit::<MatchBreakdown, _>(b, |_, cx| MatchBreakdown::get_interactors(cx)).unwrap_or_default();
        for i in list {
            if let Some(s) = ist(cx, i) {
                s.manual_left = Some(src);
            }
        }
    }

    /// UI_Base OpenPopupMenu @1306: Visible, IsPopupOpen, ParentMenu, LastFocused = the focused interactor, the
    /// animation forward.
    fn open_popup_menu(&mut self, cx: &mut Cx<'_>) {
        let b = Self::breakdown(cx);
        wd(cx, b).vis = Vis::Visible;
        self.popup_open = true;
        self.last_focused = cx.view.manager.get_currently_focused(0);
        cx.view.play(b, "Open", false, 1.0);
    }

    /// UI_Base ClosePopupMenu @3054: IsPopupOpen false, the animation reversed (PlayMode 1); at its end (@3314)
    /// Collapsed, OnPopupMenuClosed and SetCurrentFocus(LastFocused). LastFocused is the interactor OpenMatchBreakdown
    /// kept (both read the focus at the open), which OnPopupMenuClosed has focused already: the port focuses it once.
    pub fn close_popup_menu(&mut self, cx: &mut Cx<'_>) {
        let b = Self::breakdown(cx);
        self.popup_open = false;
        cx.view.play(b, "Open", true, 1.0);
        let me = cx.w;
        cx.view.delay(0.0, Handler { owner: me, id: P_CLOSE_WAIT });
    }

    fn close_wait(&mut self, cx: &mut Cx<'_>) {
        let b = Self::breakdown(cx);
        if cx.view.is_animation_playing(b, "Open") {
            let me = cx.w;
            cx.view.delay(0.0, Handler { owner: me, id: P_CLOSE_WAIT });
            return;
        }
        if self.popup_open {
            return;
        }
        wd(cx, b).vis = Vis::Collapsed;
        self.on_popup_menu_closed(cx);
        if let Some(last) = self.last_focused
            && Some(last) != self.opened_interactor
            && Manager::is_focusable(cx, last)
        {
            set_focused(cx, last, false);
        }
    }

    /// OnPopupMenuClosed @7259: the interactor that opened it focused again, else SetPreferredFocus.
    fn on_popup_menu_closed(&mut self, cx: &mut Cx<'_>) {
        match self.opened_interactor {
            Some(i) if Manager::is_focusable(cx, i) => set_focused(cx, i, false),
            _ => self.set_preferred_focus(cx),
        }
    }

    /// SetPreferredFocus @5986: the first match entry's interactor, else the slide-out's dismiss button.
    fn set_preferred_focus(&self, cx: &mut Cx<'_>) {
        let mc = find(cx, "MatchHistoryContainer");
        let first = cx.view.tree().widget(mc).children.first().copied();
        match first {
            Some(f) if cx.view.tree().user_class(f) == Some("UI_MatchHistoryEntry") => {
                let i = interactor_in(cx, f);
                set_focused(cx, i, false);
            }
            _ => {
                let d = cx.view.tree().find_in(Self::slide_out(cx), "dismissButton");
                set_focused(cx, d, true);
            }
        }
    }

    /// Set Level Info @12999: Level max(level, 1) (@13597), the gold "x{count}" (@14374) of GetLevelData(that level),
    /// the reward of the level being worked on; the level the port shows: profile_rules::shown_level.
    fn set_level_info(&self, cx: &mut Cx<'_>) {
        let levels = crate::menu::own_profile::account_levels(cx.view).to_vec();
        let level = shown_level(&levels).0.max(1);
        set_text(cx, "LevelNumber", &level.to_string());
        set_text(cx, "Text_GoldCount", &format!("x{}", level_row(&levels, level).gold));
    }

    /// SetXPBarPercentage: WBP_QuadProgress's Percentage XP / the row's XP (@1186), Number "{Number} XP" of what is
    /// left (@2516), the row LevelUpData[max(level - 1, 0)].
    fn set_xp_bar_percentage(&self, cx: &mut Cx<'_>) {
        let levels = crate::menu::own_profile::account_levels(cx.view).to_vec();
        let (level, xp) = shown_level(&levels);
        let row = level_row(&levels, level.max(1));
        let q = find(cx, "WBP_QuadProgress");
        let pct = crate::menu::profile_rules::xp_fraction(xp, row.xp) as f32;
        let w = wd(cx, q);
        if let Some(quad) = &mut w.quad {
            match quad.iter_mut().find(|e| e.0 == "Percentage") {
                Some(e) => e.1 = pct,
                None => quad.push(("Percentage".to_string(), pct)),
            }
        }
        if let Some(m) = &mut w.mid {
            m.set_scalar("Percentage", pct);
        }
        set_text(cx, "Number", &format!("{} XP", int_text(row.xp - xp)));
    }

    /// UpdateProfileIcon @11406: push_flow of Delay(0.1) -> @3438 ToggleEditing(IsMe?); the icon's path
    /// (GetMyProfileIcon's for IsMe?, else the profile's ProfileIconPath: the port opens only its own) loaded as a
    /// ProfileIconData. Found: a new UI_EquipperItemData of it (InitEquipperItemDataFromProfileIcon with the own
    /// slug, so Selected) into the equipper (@12320), then each item data of the chooser's primary list Selected =
    /// (its slug == "", the empty string literal @4350) and UpdateAppearance, and the chooser's ToggleLoading(false).
    /// Not found: ToggleLoading(true) and ToggleEditing(false) (@12370). The build drew no icon offline (the live
    /// capture with no backend shows no pencil); the port's store holds one (own_profile, a Deviation).
    fn update_profile_icon(&mut self, cx: &mut Cx<'_>) {
        self.seen_icon = cx.prefs.hydra.profile_icon.slug.clone();
        let icon = if self.is_me() { my_profile_icon(cx) } else { None };
        let ch = Self::chooser(cx);
        if let Some(item) = icon {
            let tile = CosmeticTile {
                kind: CosmeticType::ProfileIcon,
                item,
                selected: true,
                tag: "profileIcon".to_string(),
                character: String::new(),
            };
            self.equipper_tile = Some(EquipperTile::Cosmetic(tile.clone()));
            let eq = Self::equipper(cx);
            cx.visit_dyn(eq, |c, cx| {
                if let Some(t) = c.as_grid_item() {
                    t.set_data(cx, Some(TileData::Cosmetic(tile)));
                }
            });
            // Unverified: the tiles' Selected is set on the tiles' own data (the list keeps its copy, which the
            // chooser's own populate wrote)
            let list = find_in_chooser(cx, ch);
            let items = cx.visit::<GridItemList, _>(list, |l, _| {
                l.items().into_iter().map(|w| (w, l.record(w).and_then(|r| r.0))).collect::<Vec<_>>()
            });
            for (w, data) in items.unwrap_or_default() {
                let Some(TileData::Cosmetic(mut d)) = data else { continue };
                d.selected = d.item.slug.is_empty();
                cx.visit_dyn(w, |c, cx| {
                    if let Some(t) = c.as_grid_item() {
                        t.set_data(cx, Some(TileData::Cosmetic(d)));
                    }
                });
            }
            cx.visit::<CollectionChooser, _>(ch, |c, cx| c.toggle_loading(cx, false));
        } else {
            cx.visit::<CollectionChooser, _>(ch, |c, cx| c.toggle_loading(cx, true));
            self.toggle_editing(cx, false);
        }
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: P_EDIT });
    }

    /// The equipper's SelectedEvent @12478: an open chooser closes (Hide, ToggleTrayCover(false)); else, on the own
    /// profile, Init(the equipper's data, no character, close tab at 25, "profileIcon"), Show, ToggleTrayCover(true).
    fn on_equipper_selected(&mut self, cx: &mut Cx<'_>) {
        let ch = Self::chooser(cx);
        if Self::chooser_shown(cx) {
            cx.visit::<CollectionChooser, _>(ch, |c, cx| c.hide(cx));
            self.toggle_tray_cover(cx, false);
            return;
        }
        if !self.is_me() {
            return;
        }
        let data = self.equipper_tile.clone();
        cx.visit::<CollectionChooser, _>(ch, |c, cx| {
            c.init(cx, data, None, 25.0, "profileIcon");
            c.show(cx);
        });
        self.toggle_tray_cover(cx, true);
    }

    /// ToggleTrayCover(ShowCover) @15961: TrayCoverProfile Visible / Collapsed, then from the game time 0.3 s of
    /// opacity Ease(0, 1, alpha, EaseInOut, exponent 2) (1 - that to hide) in a Delay(0) loop (@863).
    fn toggle_tray_cover(&mut self, cx: &mut Cx<'_>, show: bool) {
        set_vis(cx, "TrayCoverProfile", if show { Vis::Visible } else { Vis::Collapsed });
        self.cover_show = show;
        self.cover_start = Some(cx.now());
    }

    fn tick_tray_cover(&mut self, cx: &mut Cx<'_>) {
        let Some(start) = self.cover_start else { return };
        let a = ((cx.now() - start) / TRAY_FADE_TIME).clamp(0.0, 1.0);
        let k = if a < 0.5 { 0.5 * (2.0 * a).powi(2) } else { 1.0 - 0.5 * (2.0 * (1.0 - a)).powi(2) };
        let t = find(cx, "TrayCoverProfile");
        wd(cx, t).opacity = if self.cover_show { k } else { 1.0 - k };
        if a >= 1.0 {
            self.cover_start = None;
        }
    }

    /// ToggleEditing @12838: the edit mark (4 / 2) and the equipper's interactor.
    fn toggle_editing(&self, cx: &mut Cx<'_>, can_edit: bool) {
        set_vis(cx, "Overlay_EditIcon", if can_edit { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        let i = Self::equipper_interactor(cx);
        set_interactable(cx, i, can_edit);
    }

    /// The visibility bindings, set where their inputs change: LoadingIndicator (IsLoadingProfile: Visible /
    /// Collapsed) and the match history's "No matches" text TextBlock_2 (both lists empty and the profile loaded; the
    /// spectate list's TextBlock has no binding).
    fn update_bindings(&self, cx: &mut Cx<'_>) {
        set_vis(cx, "LoadingIndicator", if self.is_loading_profile { Vis::Visible } else { Vis::Collapsed });
        let (mc, sc) = (find(cx, "MatchHistoryContainer"), find(cx, "SpectateHistoryContainer"));
        let t = cx.view.tree();
        let none = self.profile_loaded && t.widget(mc).children.is_empty() && t.widget(sc).children.is_empty();
        set_vis(cx, "TextBlock_2", if none { Vis::Visible } else { Vis::Collapsed });
    }

    /// Update Context(Set Focus) on the panel's GUID.
    fn update_context_and_focus(&mut self, cx: &mut Cx<'_>, set_focus: bool) {
        let g = self.guid.clone();
        self.base.context.clone_from(&g);
        let eq = Self::equipper_interactor(cx);
        set_context(cx, eq, &g);
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.update_context(cx, &g));
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, &g, None));
        for n in ["RankedLeaderboardTeams", "RankedLeaderboard1v1"] {
            let w = find(cx, n);
            cx.visit::<RankedLeaderboard, _>(w, |l, cx| l.update_context(cx, &g));
        }
        let ps = find(cx, "UI_ProfileStats");
        cx.visit::<ProfileStats, _>(ps, |s, cx| s.update_context(cx, &g));
        if let Some(r) = self.rename {
            let i = ButtonSecondary::interactor(cx, r);
            set_context(cx, i, &g);
        }
        // @0-407: GetInteractors (the match entries, the equipper, the spectate entries) and the breakdown's
        let list: Vec<WidgetId> = self.entries.iter().map(|&e| interactor_in(cx, e)).collect();
        batch_set_context(cx, &list, &g);
        let b = Self::breakdown(cx);
        cx.visit::<MatchBreakdown, _>(b, |m, cx| m.update_context(cx, &g));
        if set_focus {
            self.set_preferred_focus(cx);
        }
    }

    /// Port's own (RENAME_ICON): TextUsername moves into a HorizontalBox in ScaleBox_0 with the pencil after it, and
    /// the name shown follows a change (identity's OnUsernameChanged, seen on the tick).
    fn add_rename_button(&mut self, cx: &mut Cx<'_>) {
        let w = MenuView::create(cx, "UI_ButtonSecondary");
        let (sb, name) = (find(cx, "ScaleBox_0"), find(cx, "TextUsername"));
        let bundle = cx.view.bundle.clone();
        let t = cx.view.tree_mut();
        t.set_prop(w, "buttonLabel", json!({ "CultureInvariantString": "" }));
        t.set_prop(w, "Tooltip", json!({ "CultureInvariantString": RENAME_TIP }));
        t.set_prop(w, "icon", json!(RENAME_ICON));
        t.set_prop(w, "IconSizeW", json!(40));
        t.set_prop(w, "IconSizeH", json!(40));
        t.set_prop(w, "OverrideSize", json!({ "X": RENAME_SIZE[0], "Y": RENAME_SIZE[1] }));
        t.set_prop(w, "LabelPadding", json!({ "Left": 0, "Top": 0, "Right": 0, "Bottom": 0 }));
        let row = t.create_plain(&bundle, "HorizontalBox", "PortNameRow");
        t.insert(sb, row, None);
        t.insert(row, name, None);
        {
            let x = t.widget_mut(name);
            x.h_align = HAlign::Left;
            x.v_align = VAlign::Center;
            x.fill = 0.0;
        }
        let x = t.widget_mut(w);
        x.name = "RenameButton".to_string();
        x.h_align = HAlign::Left;
        x.v_align = VAlign::Center;
        x.fill = 0.0;
        x.pad = Margin { l: RENAME_GAP, t: 0.0, r: 0.0, b: 0.0 };
        MenuView::add_child(cx, row, w, None);
        let h = Handler { owner: cx.w, id: P_RENAME };
        cx.visit::<ButtonSecondary, _>(w, |b, _| b.on_click.add(h));
        self.rename = Some(w);
    }

    fn unbind_submit(&mut self, cx: &mut Cx<'_>) {
        if !self.bound {
            return;
        }
        self.bound = false;
        let me = cx.w;
        cx.view.manager.on_submit2.remove(Handler { owner: me, id: P_SUBMIT2 });
    }

    /// ToggleListSize @16165 (the manager's OnSubmit2): "VIEW MORE" and the list eased back to 550, or "VIEW LESS"
    /// and eased out to 1050, over 0.4 s of real time; a press while it moves jumps it to its end (alpha 1).
    fn toggle_list_size(&mut self, cx: &mut Cx<'_>) {
        let l = find(cx, "UI_InputLabel_ListSize");
        let label = cx.view.tree().find_in(l, "textLabel");
        wd(cx, label).text = if self.toggle_more_history { "VIEW MORE" } else { "VIEW LESS" }.to_string();
        if self.moving {
            self.move_start = f64::NEG_INFINITY;
            return;
        }
        self.moving = true;
        self.move_start = cx.now();
    }

    /// OnScroll @10817: in the panel's context and with the chooser hidden.
    fn on_scroll(&self, cx: &mut Cx<'_>, player: i32, amount: f64) {
        if cx.view.manager.get_current_context(player) != self.guid {
            return;
        }
        let ch = Self::chooser(cx);
        if cx.view.tree().widget(ch).shown() {
            return;
        }
        let sb = Self::scroll_box(cx);
        scroll_list(cx.view.tree_mut(), sb, amount);
    }
}

/// The chooser's primary list.
fn find_in_chooser(cx: &Cx<'_>, ch: WidgetId) -> WidgetId {
    cx.view.tree().find_in(ch, "primaryList")
}

impl Modal for ProfileV2 {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @10747: Update Context(true), then Delay(0.01) -> @2674: with ShouldLoadYourProfile and no load
    /// running, OpenProfile(own Hydra GUID, own name) and the sidebar's exit button as Back (@3204).
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        self.update_context_and_focus(cx, true);
        let me = cx.w;
        cx.view.delay(0.01, Handler { owner: me, id: P_POST_SHOW });
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @11049: a visible breakdown closes (its Close), else an open chooser (@11142: Hide,
    /// ToggleTrayCover(false)), else OnSubmit2 unbound and CloseSlideOut(Modal ID).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        let b = Self::breakdown(cx);
        if cx.view.tree().widget(b).shown() {
            self.close_popup_menu(cx);
            return;
        }
        if Self::chooser_shown(cx) {
            let ch = Self::chooser(cx);
            cx.visit::<CollectionChooser, _>(ch, |c, cx| c.hide(cx));
            self.toggle_tray_cover(cx, false);
            return;
        }
        self.unbind_submit(cx);
        let id = self.base.modal_id.clone();
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.close_slide_out(cx, &id));
    }

    fn get_modal_context(&self) -> String {
        self.guid.clone()
    }
}

impl Controller for ProfileV2 {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// OnInitialized @8805.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.guid = cx.view.manager.new_guid();
        self.base.context.clone_from(&self.guid);
        cx.view.manager.on_scroll.add(Handler { owner: me, id: P_SCROLL });
        let sb = Self::scroll_box(cx);
        cx.view.manager.register_scrollbox(sb);
        cx.view.manager.on_submit2.add(Handler { owner: me, id: P_SUBMIT2 });
        self.bound = true;
        // @8860-9718: the chooser in a context of its own (UpdateContext(NewGuid, 0)); its ShowEvent -> FocusFirst
        // (@15304), its HideEvent -> the equipper's interactor SetFocused(false) (@15341), the equipper's SelectedEvent
        // -> @12478; the chooser hidden, the equipper's hover sharpness 0.005, the chooser's primary list 5 columns
        let ch = Self::chooser(cx);
        let g = cx.view.manager.new_guid();
        let (shown, hidden) = (Handler { owner: me, id: P_CHOOSER_SHOWN }, Handler { owner: me, id: P_CHOOSER_HIDDEN });
        cx.visit::<CollectionChooser, _>(ch, |c, cx| {
            c.update_context(cx, &g, 0);
            c.show_event.add(shown);
            c.hide_event.add(hidden);
            c.hide(cx);
        });
        let eq = Self::equipper(cx);
        cx.visit_dyn(eq, |c, _| {
            if let Some(t) = c.as_grid_item() {
                t.state().selected_event.add(Handler { owner: me, id: P_EQUIPPER_SELECTED });
            }
        });
        let hover = cx.view.tree().find_in(eq, "HoverIndicator");
        cx.visit::<Quadrilateral, _>(hover, |q, cx| q.update_sharpness(cx, 0.005));
        let list = find_in_chooser(cx, ch);
        cx.view.tree_mut().set_prop(list, "Force Grid Columns", json!(5));
        // OnAccountReloaded (@12823) -> UpdateProfileIcon (OnProfileIconUpdated, @12822, is a bare pop_flow): the port
        // has no account reload and runs it on a change of its Hydra preferences, seen on the tick, in its place
        // (Unverified: when the build's reload comes after set_profile_icon is native, unknown); the catalogue's
        // arrival seeds an empty icon
        self.seen_icon = cx.prefs.hydra.profile_icon.slug.clone();
        watch_profile_icon(cx);
        let bar = Self::bar(cx);
        let tab = Handler { owner: me, id: P_TAB };
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(tab));
        // @666: without Feature Enabled the two leaderboard tabs leave the bar (@755, @809)
        if !RANKED_FEATURE_ENABLED {
            cx.visit::<SectionBar, _>(bar, |b, cx| {
                b.remove_option(cx, "teamsLeaderboard");
                b.remove_option(cx, "1vs1Leaderboard");
            });
        }
        // @9800: Conditional Visibility by ChildOfflineMode (false: 1 / 4, 2 / 4, 1 / 4) and the offline text with it
        // and no load running (4 / 1)
        set_vis(cx, "BottomOverlay", Vis::SelfHitTestInvisible);
        set_vis(cx, "LevelXPBar", Vis::SelfHitTestInvisible);
        set_vis(cx, "GuildContainer", Vis::SelfHitTestInvisible);
        set_vis(cx, "OfflineModeText", Vis::Collapsed);
        let sz = find(cx, "tabbedListsSizeBox");
        wd(cx, sz).h_override = LIST_SHORT;
        self.add_rename_button(cx);
    }

    /// PreConstruct @14822-15114: the slide-out 1400 wide, 75 to the left, sheared -6.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let so = Self::slide_out(cx);
        let clip = cx.view.tree().find_in(so, "Overlay_ClipBounds");
        let c = wd(cx, clip);
        c.offsets.r = 1400.0;
        c.offsets.b = 0.0;
        c.offsets.l = -75.0;
        c.offsets.t = 0.0;
        c.transform.shear[0] = -6.0;
    }

    /// Deviation: the profile fills the window: SizeBox_3.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let s = find(cx, "SizeBox_3");
        fill_page(cx, s);
    }

    /// The timeline's Delay(0) loop (@15 / @2023), once a frame while it moves.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.tick_tray_cover(cx);
        let s = find(cx, "SizeBox_3");
        fill_page(cx, s);
        // identity.onChanged: the name shown follows a change
        if self.profile_loaded && cx.prefs.profile.name != self.seen_name {
            self.seen_name.clone_from(&cx.prefs.profile.name);
            let n = self.seen_name.clone();
            set_text(cx, "TextUsername", &n);
        }
        // onHydraPrefsChanged
        if self.profile_loaded && cx.prefs.hydra.profile_icon.slug != self.seen_icon {
            self.update_profile_icon(cx);
            // a pick in the chooser: its UpdateSelections follows the listener
            let ch = Self::chooser(cx);
            cx.visit::<CollectionChooser, _>(ch, |c, cx| {
                if c.take_icon_pick() {
                    c.update_all_selections(cx);
                }
            });
        }
        if !self.moving {
            return;
        }
        let t = ((cx.now() - self.move_start) / LIST_EASE_TIME).clamp(0.0, 1.0);
        let (a, b) = if self.toggle_more_history { (LIST_TALL, LIST_SHORT) } else { (LIST_SHORT, LIST_TALL) };
        let sz = find(cx, "tabbedListsSizeBox");
        wd(cx, sz).h_override = ease_out(a, b, t);
        if t < 1.0 {
            return;
        }
        self.toggle_more_history = !self.toggle_more_history;
        self.moving = false;
    }

    /// Destruct @15130: UnregisterScrollbox (and the port drops the delegates the destroyed widget held and the
    /// portraits it held).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let sb = Self::scroll_box(cx);
        cx.view.manager.unregister_scrollbox(sb);
        cx.view.release_pictures("profile");
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: P_SCROLL });
        self.unbind_submit(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            P_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() {
                    self.on_scroll(cx, player, amount);
                }
            }
            P_SUBMIT2 => self.toggle_list_size(cx),
            // the tab bar's OnClick @16463 (SetActiveWidgetAndFocus: the panel; the leaderboards Load)
            P_TAB => {
                let Some((_, tab)) = arg.downcast_ref::<(i32, String)>() else { return };
                let Some(name) = tab_panel(tab) else { return };
                let sw = find(cx, "tabbedListsSwitcher");
                let t = cx.view.tree();
                if let Some(at) = t.widget(sw).children.iter().position(|&c| t.widget(c).name == name) {
                    wd(cx, sw).active = at as i32;
                }
                if name.starts_with("RankedLeaderboard") {
                    let w = find(cx, name);
                    cx.visit::<RankedLeaderboard, _>(w, |l, cx| l.load(cx));
                }
            }
            P_CHOOSER_SHOWN => {
                let ch = Self::chooser(cx);
                cx.visit::<CollectionChooser, _>(ch, |c, cx| c.focus_first(cx));
            }
            P_CHOOSER_HIDDEN => {
                let i = Self::equipper_interactor(cx);
                set_focused(cx, i, false);
            }
            P_EQUIPPER_SELECTED => self.on_equipper_selected(cx),
            P_RENAME if cx.view.manager.get_modal("chooseUsername").is_none() => {
                Manager::show_modal(cx, "UI_SetUsername", "chooseUsername", "", "change");
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            P_POST_SHOW => {
                if !prop_bool(cx, "ShouldLoadYourProfile") || self.is_loading_profile {
                    return;
                }
                let name = cx.prefs.profile.name.clone();
                self.open_profile(cx, OWN_ACCOUNT_ID, &name);
                if let Some(n) = find_global_nav(cx) {
                    cx.visit::<GlobalNav, _>(n, |n, cx| n.customize_exit_button(cx, 1, "Back"));
                }
            }
            P_LOADED => {
                if let Some(p) = self.pending.take() {
                    self.on_load_profile_result(cx, p);
                }
            }
            P_EDIT => {
                let me = self.is_me();
                self.toggle_editing(cx, me);
            }
            P_CLOSE_WAIT => self.close_wait(cx),
            _ => {}
        }
    }
}
