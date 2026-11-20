//! The friends panel and its lists (web/src/menu/widgets/social.ts): UI_SocialPlayer, UI_SocialList,
//! UI_SocialPanel and UI_SocialInviteModal, offline.
//!
//! Offline is what the oracle draws with no online service (online.connected false, the online list empty): the panel
//! hides itself, the invite modal shows its "no friends online" line. Online it lists the relay's online list
//! (`Session::online`, redrawn when it changes), sends the party invite and asks for the online list again while open.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::common::{ModalBase, apply_founder_name_treatment, enum_index, fill_page, margin, rgba, text};
use super::controls::buttons::{ButtonMain, ButtonSecondary, IconButton};
use super::controls::interactor::{
    FocusedCommon, InteractorState, batch_set_context, batch_set_scroll_box, set_context, set_focused, set_interactable,
};
use super::controls::tooltip::close_all_tooltips;
use super::controls::{bind, find, prop, prop_bool, wd};
use super::dialogs::{NotificationToast, TextInput};
use super::nav::live;
use super::social_context_menu::show_social_actions_menu;
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra::picture_name;
use crate::menu::manager::{Manager, Modal, NULL_GUID};
use crate::menu::online::OnlineCmd;
use crate::menu::online::PartyCmd;
use crate::menu::online::PresenceStatus;
use crate::menu::online_read::OnlineSeen;
use crate::menu::own_profile::{profile_icon_by_asset, watch_profile_icon};
use crate::menu::scroll::{scroll_list, scroll_to_start, set_scroll_bar_visibility, set_scrollbar_padding};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// The online service is signed in (online.ts connected).
fn online_connected(cx: &Cx<'_>) -> bool {
    cx.view.session.online.connected
}

/// An interactor event of its state (the binds of a table).
type InteractorEvent = fn(&mut InteractorState) -> &mut Event<i32>;

// ---------- UI_SocialPlayer ----------

/// The build's UI_SocialPlayerData, as far as the port has it: a player met recently or one of the relay's online
/// list (online.ts OnlinePlayer).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SocialPlayerData {
    pub id: String,
    pub name: String,
    pub busy: bool,
    /// On the online list now (its presence is known).
    pub present: bool,
    /// The profile icon's asset name the player's lobby last showed.
    pub icon: String,
    /// (the port's own) What they are doing, from their presence (None: not known).
    pub status: Option<PresenceStatus>,
}

/// (the port's own) The words of the line under a row's name.
pub fn status_words(s: PresenceStatus) -> &'static str {
    use crate::menu::custom_text as t;
    match s {
        PresenceStatus::Menus => t::STATUS_MENUS,
        PresenceStatus::Lobby => t::STATUS_LOBBY,
        PresenceStatus::Queue => t::STATUS_QUEUE,
        PresenceStatus::OneVOne => t::STATUS_1V1,
        PresenceStatus::TwoVTwo => t::STATUS_2V2,
        PresenceStatus::Ffa => t::STATUS_FFA,
        PresenceStatus::Match => t::STATUS_MATCH,
    }
}

/// Whether a row's player is shown busy (the dot orange): by their status when they sent one (anything but the
/// menus), else by their presence's busy (a client of before the status).
pub fn shown_busy(d: &SocialPlayerData) -> bool {
    d.status.map_or(d.busy, PresenceStatus::busy)
}

/// UI_Functions IsPlayerInvitable? @0-1775: the party joinable (PartyManager.IsPartyJoinable), no custom game widget
/// and no custom game lobby, not in a match (GetGameStateType != 3), and the player's friend object valid. The
/// oracle: online play on, the party not full and this player its leader (or no party yet), no custom lobby open
/// (its modal customGame), and the player on the online list now, not busy and not in the party.
/// Unverified: IsPartyJoinable is native; read as "not full and this player may invite".
/// Deviation: with a custom lobby up the build shows no "+" (IsPlayerInvitable? is false there); the port's "+" in
/// Quick Invite Mode shows for a player on the online list who is not busy and not a member of the lobby, and
/// PN_PLAYER_INVITE sends the lobby invite then (the social actions menu's Invite to Custom Game does the same). The
/// lobby's size is not known here, so a full lobby still shows it.
pub fn is_player_invitable(cx: &Cx<'_>, d: &SocialPlayerData) -> bool {
    let o = &cx.view.session.online;
    if !o.connected {
        return false;
    }
    let Some(now) = o.listed_player(&d.id) else { return false };
    if now.busy {
        return false;
    }
    if cx.view.manager.get_modal("customGame").is_some() {
        return lobby_invite_open(cx, &d.id);
    }
    !o.party_full()
        && (o.party.is_none() || o.party_leading())
        && !o.party.as_ref().is_some_and(|p| p.members.iter().any(|m| m.id == d.id))
}

/// A custom lobby of the online session is up and `id` is not one of its members.
pub(crate) fn lobby_invite_open(cx: &Cx<'_>, id: &str) -> bool {
    let lobby = cx.view.session.online.session.as_ref().and_then(|s| s.lobby.as_ref());
    lobby.is_some_and(|l| !l.teams.iter().flatten().any(|p| p.account_id == id))
}

/// SetStyle by UI_SocialPlayerStyle (Default, Compact, Default Invitable, Default YesNo, Default No, Compact
/// Invitable): playerInfoContainer, UI_Interactor, yesNoContainer, BtnYes, btnInviteContainer.
const PLAYER_STYLE_VIS: [[Vis; 5]; 6] = {
    use Vis::{Collapsed as C, Hidden as D, HitTestInvisible as H, SelfHitTestInvisible as S, Visible as V};
    [[H, S, C, C, D], [C, D, C, C, D], [H, S, C, C, V], [H, S, V, V, D], [H, S, V, D, D], [C, D, C, C, V]]
};

/// The style's row of PLAYER_STYLE_VIS (another style: Default's).
fn style_vis(style: i32) -> [Vis; 5] {
    usize::try_from(style).ok().and_then(|s| PLAYER_STYLE_VIS.get(s)).copied().unwrap_or(PLAYER_STYLE_VIS[0])
}

/// Status Color For Player: online and joinable, online and busy.
const STATUS_ONLINE: Rgba = [0.0, 1.0, 0.168627, 1.0];
const STATUS_BUSY: Rgba = [0.991102, 0.571125, 0.057805, 1.0];
/// (the port's own) The quick invite "+"'s gap from the row's right end (update_invitable).
const INVITE_MARGIN: f64 = 12.0;
/// (the port's own) The status line under the name: its widget's name, the build's body face and its size.
const STATUS_LINE: &str = "PresenceStatusLine";
const STATUS_FACE: &str = "NormsCond-Medium";
const STATUS_SIZE: f64 = 14.0;

const SP_FOCUS: u32 = 1;
const SP_UNFOCUS: u32 = 2;
const SP_CLICK: u32 = 3;
const SP_YES: u32 = 4;
const SP_NO: u32 = 5;
const SP_INVITE: u32 = 6;

/// UI_SocialPlayer.
/// Deviation: a player with no profile icon shows the design's default (the build: the platform's), there is no
/// platform indicator or founder name treatment, and Select fires `selected`, on which the social panel opens the
/// social actions menu (the build's row opens it itself).
#[derive(Debug, Default)]
pub struct SocialPlayer {
    pub data: Option<SocialPlayerData>,
    pub style: i32,
    pub yes_clicked: Event<SocialPlayerData>,
    pub no_clicked: Event<SocialPlayerData>,
    pub invite_clicked: Event<SocialPlayerData>,
    /// The row's Select while Enable Context Menu is on: the row and its player.
    pub selected: Event<(WidgetId, SocialPlayerData)>,
    /// UI_GridItemBase: Shows Selected State, Toggle Selected State, the selection and SelectedEvent.
    pub shows_selected_state: bool,
    pub toggle_selected_state: bool,
    pub is_selected: bool,
    pub selected_event: Event<WidgetId>,
    /// Enable Context Menu as the row has it now (the invite modal turns it off).
    pub enable_context_menu: bool,
    sent_to: String,
    /// The tree's icon brush (the platform default's stand-in).
    tree_icon: Option<Option<String>>,
    /// btnInviteContainer's place in the build's data: translation X, alignment and padding.
    build_place: Option<(f64, HAlign, Margin)>,
}

impl SocialPlayer {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SocialPlayer::default())
    }

    /// The row's UI_Interactor of the row widget `w`.
    pub fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    /// The row's interactors: its own, btnInvite's, BtnYes' and BtnNo's.
    pub fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let i = Self::interactor_of(cx, cx.w);
        let mut v = vec![i];
        for n in ["btnInvite", "BtnYes", "BtnNo"] {
            let b = find(cx, n);
            v.push(ButtonSecondary::interactor(cx, b));
        }
        v
    }

    pub fn set_data(&mut self, cx: &mut Cx<'_>, d: SocialPlayerData) {
        self.data = Some(d);
        self.update_appearance(cx);
    }

    /// The checkmark of a selected row. Deviation: read literally the build's row clears Shows Selected State in its
    /// own UpdateAppearance, so the checkmark would not come back on a third click; not seen running, and the port
    /// keeps the checkmark with the selection.
    pub fn set_selected(&mut self, cx: &mut Cx<'_>, on: bool) {
        self.is_selected = on;
        let s = find(cx, "selectedIndicator");
        wd(cx, s).vis = if on && self.shows_selected_state { Vis::HitTestInvisible } else { Vis::Hidden };
    }

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let Some(d) = self.data.clone() else { return };
        let name = find(cx, "username");
        wd(cx, name).text = d.name.clone();
        // @469: a founder's Apply Founder Pack Name Treatment (every player here), not ApplyNormalNameTreatment
        apply_founder_name_treatment(cx, name);
        // Status Color For Player: online and joinable green, busy orange. @753-975: statusIndicator by
        // OnlineStatus, shown for an online player; here a player whose presence is known (on the online list now),
        // else Hidden (Deviation: a recently met player's state is not known). Deviation: busy is the port's
        // status (anything but In Menus: a custom lobby too), not the platform's OnlineStatus.
        let busy = shown_busy(&d);
        let color = if busy { STATUS_BUSY } else { STATUS_ONLINE };
        let c = find(cx, "statusColor");
        wd(cx, c).color = color;
        let s = find(cx, "statusIndicator");
        wd(cx, s).vis = if d.present { Vis::HitTestInvisible } else { Vis::Hidden };
        // (the port's own) the status line under the name, in the dot's colour; none for a player not on the list
        // now or one whose client sends no status
        let line = Self::status_line(cx);
        let x = wd(cx, line);
        match d.status.filter(|_| d.present) {
            Some(st) => {
                x.text = status_words(st).to_owned();
                x.color = color;
                x.vis = Vis::HitTestInvisible;
            }
            None => x.vis = Vis::Collapsed,
        }
        // @1082-1393: GetPreferredProfileIconPath (the Hydra user's ProfileIconPath, else the platform's default
        // icon) loaded; a ProfileIconData's TextureRef into `icon`, then icon SelfHitTestInvisible. The tree's icon
        // stands for the platform default (Deviation), set again since a row is reused for another player.
        // The row's own icon from the profile icons' catalogue by `d.icon`.
        let img = find(cx, "icon");
        let tree = self.tree_icon.get_or_insert_with(|| cx.view.tree().widget(img).tex.clone()).clone();
        let own = profile_icon_by_asset(cx.view, &d.icon).and_then(|i| i.icon.as_deref().and_then(picture_name));
        let x = wd(cx, img);
        x.tex = own.or(tree);
        x.vis = Vis::SelfHitTestInvisible;
    }

    /// (the port's own) The row's status line: a text block made once into playerInfoContainer (the VerticalBox
    /// that holds the name's row), under the name. The build's row has the name alone.
    fn status_line(cx: &mut Cx<'_>) -> WidgetId {
        if let Some(w) = cx.view.tree().find_opt(cx.w, STATUS_LINE) {
            return w;
        }
        let info = find(cx, "playerInfoContainer");
        let bundle = cx.view.bundle.clone();
        let t = cx.view.tree_mut();
        let w = t.create_plain(&bundle, "TextBlock", STATUS_LINE);
        let x = t.widget_mut(w);
        x.typeface = Some(STATUS_FACE.to_owned());
        x.font_size = STATUS_SIZE;
        x.h_align = HAlign::Left;
        x.v_align = VAlign::Top;
        x.vis = Vis::Collapsed;
        MenuView::add_child(cx, info, w, None);
        w
    }

    pub fn set_style(&mut self, cx: &mut Cx<'_>, style: i32) {
        self.style = style;
        let v = style_vis(style);
        let p = find(cx, "playerInfoContainer");
        wd(cx, p).vis = v[0];
        let i = Self::interactor_of(cx, cx.w);
        wd(cx, i).vis = v[1];
        let (yn, li) = (find(cx, "yesNoContainer"), find(cx, "loadingIndicator"));
        let loading_collapsed = cx.view.tree().widget(li).vis == Vis::Collapsed;
        wd(cx, yn).vis = if loading_collapsed { v[2] } else { Vis::Collapsed };
        let y = find(cx, "BtnYes");
        wd(cx, y).vis = v[3];
        self.update_invitable(cx);
    }

    /// UpdateInvitable @4051-4349 -> @1432: btnInviteContainer by the style (Visible for the invitable styles 2 and
    /// 5, else Hidden), Collapsed when the player is not invitable. (The port's) a row whose "Invite Sent" still plays
    /// for this player keeps its button hidden through a list refresh.
    fn update_invitable(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        if let Some(d) = &self.data
            && d.id == self.sent_to
            && cx.view.is_animation_playing(me, "InviteSentAnim")
        {
            return;
        }
        let v = style_vis(self.style);
        let bx = find(cx, "btnInviteContainer");
        let invitable = self.data.as_ref().is_some_and(|d| is_player_invitable(cx, d));
        // Deviation: the build's data puts the box at the row's left edge moved 80 px left (RenderTransform X -80,
        // OverlaySlot HAlign_Left), outside mainPanel's and content's ClipToBounds, so it never draws or takes the
        // mouse there. For the invitable styles 2 and 5 the port draws it inside the row's right end instead
        // (right-aligned, INVITE_MARGIN from the edge, clear of the name and the icon on the left); the other styles
        // keep the build's.
        let place = *self.build_place.get_or_insert_with(|| {
            let w = cx.view.tree().widget(bx);
            (w.transform.translation[0], w.h_align, w.pad)
        });
        let inside = self.style == 2 || self.style == 5;
        let x = wd(cx, bx);
        x.vis = if invitable { v[4] } else { Vis::Collapsed };
        x.transform.translation[0] = if inside { 0.0 } else { place.0 };
        x.h_align = if inside { HAlign::Right } else { place.1 };
        x.pad = if inside { Margin { l: 0.0, t: 0.0, r: INVITE_MARGIN, b: 0.0 } } else { place.2 };
        // (inside the row it paints last in mainPanel, over the row's own interactor, so the mouse reaches it)
        if inside && let Some(p) = cx.view.tree().widget(bx).parent {
            let kids = &mut cx.view.tree_mut().widget_mut(p).children;
            if kids.last() != Some(&bx) {
                kids.retain(|&c| c != bx);
                kids.push(bx);
            }
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
    }

    /// The answer of BtnYes / BtnNo: the event, then the loading look.
    fn answer(&mut self, cx: &mut Cx<'_>, yes: bool) {
        if let Some(d) = self.data.clone() {
            if yes {
                cx.emit(self, |c| &c.yes_clicked, &d);
            } else {
                cx.emit(self, |c| &c.no_clicked, &d);
            }
        }
        let yn = find(cx, "yesNoContainer");
        wd(cx, yn).vis = Vis::Collapsed;
        let li = find(cx, "loadingIndicator");
        wd(cx, li).vis = Vis::SelfHitTestInvisible;
    }
}

impl Controller for SocialPlayer {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let i = Self::interactor_of(cx, me);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, SP_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, SP_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, SP_CLICK);
        for (n, id) in [("BtnYes", SP_YES), ("BtnNo", SP_NO)] {
            let b = find(cx, n);
            cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
        let b = find(cx, "btnInvite");
        cx.visit::<IconButton, _>(b, |b, _| b.on_click.add(Handler { owner: me, id: SP_INVITE }));
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        self.enable_context_menu = prop_bool(cx, "Enable Context Menu");
        self.set_style(cx, enum_index(cx.view.tree().props(me).get("Style")));
        set_interactable(cx, i, self.enable_context_menu);
        // (the port's own) the icons' catalogue, fetched once: a row drawn before it came in is drawn again
        if watch_profile_icon(cx) {
            self.update_appearance(cx);
        }
        // Online (R7): the icons' catalogue, fetched once: a row drawn before it came in is drawn again.
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            SP_FOCUS | SP_UNFOCUS => {
                let on = id == SP_FOCUS;
                let q = find(cx, "WBP_Quadrilateral");
                wd(cx, q).vis = if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
                let c = rgba(&prop(cx, if on { "UsernameHighlightedColor" } else { "UsernameUnhighlightedColor" }));
                let u = find(cx, "username");
                wd(cx, u).color = c;
            }
            // UI_GridItemBase.Select, then UI_SocialPlayer's (the social actions menu)
            SP_CLICK => {
                let on = if self.toggle_selected_state { !self.is_selected } else { true };
                self.set_selected(cx, on);
                let me = cx.w;
                cx.emit(self, |c| &c.selected_event, &me);
                if self.enable_context_menu
                    && let Some(d) = self.data.clone()
                {
                    cx.emit(self, |c| &c.selected, &(me, d));
                }
            }
            SP_YES => self.answer(cx, true),
            SP_NO => self.answer(cx, false),
            SP_INVITE => {
                self.sent_to = self.data.as_ref().map(|d| d.id.clone()).unwrap_or_default();
                if let Some(d) = self.data.clone() {
                    cx.emit(self, |c| &c.invite_clicked, &d);
                }
                let b = find(cx, "btnInviteContainer");
                wd(cx, b).vis = Vis::Hidden;
                let me = cx.w;
                cx.view.play(me, "InviteSentAnim", false, 1.0);
            }
            _ => {}
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.stop_all_animations(me);
    }
}

// ---------- UI_SocialList ----------

const LIST_LABEL: Rgba = [0.262251, 0.366253, 0.396755, 1.0];
const LIST_LABEL_FOCUS: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const LIST_BACKING: Rgba = [0.016807, 0.024158, 0.046665, 1.0];
const LIST_BACKING_FOCUS: Rgba = [0.194618, 0.651406, 0.630757, 1.0];

/// SocialListCollapseStateSave's IsCollapsed by SaveSlot.
/// Deviation: kept for the run (the oracle kept it in localStorage); the settings have no slot for it yet.
fn collapse_data() -> &'static Mutex<HashMap<String, bool>> {
    static D: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(HashMap::new()))
}

const SL_CLICK: u32 = 1;
const SL_FOCUS: u32 = 2;
const SL_UNFOCUS: u32 = 3;

/// UI_SocialList.
/// Deviation: every entry shows (Players Per Page and the list's paging through its pips are not ported), and
/// entries keep the caller's order (the build puts an entry that has a Hydra user in front).
#[derive(Debug)]
pub struct SocialList {
    pub players: Vec<SocialPlayerData>,
    pub style: i32,
    pub is_collapsed: bool,
    context: String,
    scroll_box: Option<WidgetId>,
    /// An entry was set (the entry's row, the list).
    pub player_added_to_list: Event<(WidgetId, WidgetId)>,
    pub on_list_expanded: Event<WidgetId>,
}

impl SocialList {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SocialList {
            players: Vec::new(),
            style: 0,
            is_collapsed: false,
            context: NULL_GUID.to_string(),
            scroll_box: None,
            player_added_to_list: Event::default(),
            on_list_expanded: Event::default(),
        })
    }

    fn header_interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }

    fn collapsible(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "IsCollabsable")
    }

    fn header_text(cx: &Cx<'_>) -> String {
        text(cx.view.tree().props(cx.w).get("Header Text"))
    }

    fn empty_text(cx: &Cx<'_>) -> String {
        text(cx.view.tree().props(cx.w).get("Empty Text"))
    }

    /// The entries in use (cached ones past the count stay collapsed).
    pub fn player_widgets(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        let c = find(cx, "container");
        cx.view.tree().widget(c).children.iter().take(self.players.len()).copied().collect()
    }

    /// (the port's own) The empty line's text: the list's Empty Text, or the reason the list could not be filled.
    pub fn set_empty_text(&mut self, cx: &mut Cx<'_>, t: Option<&str>) {
        let t = t.map_or_else(|| Self::empty_text(cx), str::to_string);
        let e = find(cx, "labelEmpty");
        wd(cx, e).text = t;
        self.update_labels(cx);
    }

    /// (the port's own) Replaces the list's Empty Text, the line it shows with no rows.
    pub fn set_empty_default(&mut self, cx: &mut Cx<'_>, t: &str) {
        let me = cx.w;
        cx.view.tree_mut().set_prop(me, "Empty Text", serde_json::json!({ "CultureInvariantString": t }));
        self.set_empty_text(cx, None);
    }

    pub fn set_header_text(&mut self, cx: &mut Cx<'_>, t: &str) {
        let me = cx.w;
        cx.view.tree_mut().set_prop(me, "Header Text", serde_json::json!({ "CultureInvariantString": t }));
        let h = find(cx, "labelHeader");
        wd(cx, h).text = t.to_string();
        self.update_labels(cx);
    }

    fn set_header_parts(cx: &mut Cx<'_>, shown: bool) {
        let v = if shown { Vis::Visible } else { Vis::Collapsed };
        for w in [Self::header_interactor(cx), find(cx, "arrow"), find(cx, "HeaderBacking")] {
            wd(cx, w).vis = v;
        }
        let h = find(cx, "HeaderSizebox");
        wd(cx, h).h_override = if shown { 72.0 } else { 28.0 };
    }

    /// Populate: the entry widgets are kept and reused (Cache Entries).
    pub fn populate(&mut self, cx: &mut Cx<'_>, players: &[SocialPlayerData]) {
        self.players = players.to_vec();
        let me = cx.w;
        let container = find(cx, "container");
        let pad = margin(cx.view.tree().props(me).get("Item Padding"), Margin::default());
        for (i, p) in players.iter().enumerate() {
            let kids = cx.view.tree().widget(container).children.clone();
            let w = match kids.get(i) {
                Some(&w) => {
                    wd(cx, w).vis = Vis::SelfHitTestInvisible;
                    w
                }
                None => {
                    let w = MenuView::create(cx, "UI_SocialPlayer");
                    let x = wd(cx, w);
                    x.h_align = HAlign::Fill;
                    x.v_align = VAlign::Fill;
                    x.pad = pad;
                    // AddChildToGrid(entry, the container's child count, 0)
                    x.row = kids.len() as i32;
                    MenuView::add_child(cx, container, w, None);
                    let ctx = self.context.clone();
                    cx.visit::<SocialPlayer, _>(w, |c, cx| c.update_context(cx, &ctx));
                    if let Some(sb) = self.scroll_box {
                        let i = SocialPlayer::interactor_of(cx, w);
                        batch_set_scroll_box(cx, &[i], sb);
                    }
                    w
                }
            };
            cx.visit::<SocialPlayer, _>(w, |c, cx| c.set_data(cx, p.clone()));
            cx.emit(self, |c| &c.player_added_to_list, &(w, me));
        }
        for w in cx.view.tree().widget(container).children.clone().into_iter().skip(players.len()) {
            wd(cx, w).vis = Vis::Collapsed;
        }
        self.update_labels(cx);
    }

    fn update_labels(&mut self, cx: &mut Cx<'_>) {
        let (empty, header) = (find(cx, "labelEmpty"), find(cx, "labelHeader"));
        wd(cx, empty).vis =
            if self.players.is_empty() && !self.is_collapsed { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        if self.style == 1 || self.style == 5 {
            wd(cx, empty).vis = Vis::Collapsed;
            wd(cx, header).vis = Vis::Collapsed;
        } else {
            wd(cx, header).vis = Vis::SelfHitTestInvisible;
        }
        if Self::empty_text(cx).is_empty() {
            wd(cx, empty).vis = Vis::Collapsed;
            let b = find(cx, "bottomSpacer");
            wd(cx, b).vis = Vis::Collapsed;
        }
        if Self::header_text(cx).is_empty() {
            wd(cx, header).vis = Vis::Collapsed;
        }
    }

    /// SetMode(Social Panel Mode).
    pub fn set_mode(&mut self, cx: &mut Cx<'_>, mode: i32) {
        let head = find(cx, "HeaderSizebox");
        if mode == 0 {
            wd(cx, head).vis = Vis::Collapsed;
            return;
        }
        wd(cx, head).vis = if prop_bool(cx, "HideHeader") { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        let c = Self::collapsible(cx);
        Self::set_header_parts(cx, c);
    }

    pub fn set_style(&mut self, cx: &mut Cx<'_>, style: i32) {
        self.style = style;
        self.update_labels(cx);
        for w in self.player_widgets(cx) {
            cx.visit::<SocialPlayer, _>(w, |c, cx| c.set_style(cx, style));
        }
    }

    pub fn set_collapsed(&mut self, cx: &mut Cx<'_>, collapsed: bool) {
        if !Self::collapsible(cx) {
            return;
        }
        let v = if collapsed { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        for n in ["container", "labelEmpty", "bottomSpacer", "paginationContainer"] {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        }
        self.is_collapsed = collapsed;
        if !collapsed {
            let me = cx.w;
            cx.emit(self, |c| &c.on_list_expanded, &me);
        }
        let a = find(cx, "arrow");
        wd(cx, a).transform.angle = if collapsed { 0.0 } else { 180.0 };
        self.update_labels(cx);
    }

    pub fn set_scroll_box(&mut self, cx: &mut Cx<'_>, sb: WidgetId) {
        self.scroll_box = Some(sb);
        let mut list = vec![Self::header_interactor(cx)];
        list.extend(self.player_widgets(cx).into_iter().map(|w| SocialPlayer::interactor_of(cx, w)));
        batch_set_scroll_box(cx, &list, sb);
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let h = Self::header_interactor(cx);
        set_context(cx, h, ctx);
        let c = find(cx, "container");
        for w in cx.view.tree().widget(c).children.clone() {
            cx.visit::<SocialPlayer, _>(w, |p, cx| p.update_context(cx, ctx));
        }
    }
}

impl Controller for SocialList {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let p = find(cx, "UI_PaginationPips");
        wd(cx, p).vis = Vis::Collapsed;
        self.update_labels(cx);
        // SetInitialCollapseStatus
        let slot = super::controls::prop_str(cx, "SaveSlot");
        let saved = collapse_data().lock().map(|d| d.get(&slot).copied().unwrap_or(false)).unwrap_or(false);
        self.set_collapsed(cx, saved);
        let c = Self::collapsible(cx);
        Self::set_header_parts(cx, c);
        let i = Self::header_interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, SL_CLICK);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, SL_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, SL_UNFOCUS);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let h = Self::header_text(cx);
        self.set_header_text(cx, &h);
        let e = find(cx, "labelEmpty");
        wd(cx, e).text = Self::empty_text(cx);
        let hs = find(cx, "HeaderSizebox");
        wd(cx, hs).vis = if prop_bool(cx, "HideHeader") { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            SL_CLICK => {
                let c = !self.is_collapsed;
                self.set_collapsed(cx, c);
                let slot = super::controls::prop_str(cx, "SaveSlot");
                if !slot.is_empty()
                    && let Ok(mut d) = collapse_data().lock()
                {
                    d.insert(slot, self.is_collapsed);
                }
            }
            SL_FOCUS | SL_UNFOCUS => {
                let (label, backing) =
                    if id == SL_FOCUS { (LIST_LABEL_FOCUS, LIST_BACKING_FOCUS) } else { (LIST_LABEL, LIST_BACKING) };
                for n in ["labelHeader", "arrow"] {
                    let w = find(cx, n);
                    wd(cx, w).color = label;
                }
                let b = find(cx, "HeaderBacking");
                wd(cx, b).color = backing;
            }
            _ => {}
        }
    }
}

// ---------- UI_SocialPanel ----------

const SOCIAL_HEADER: Rgba = [0.127438, 0.165132, 0.991102, 1.0];
const SOCIAL_HEADER_FOCUS: Rgba = [0.138432, 0.623961, 0.658375, 1.0];
/// All Lists, in PreConstruct's order.
const SOCIAL_LISTS: [&str; 9] = [
    "socialListOnline",
    "socialListRecent",
    "socialListRequestsReceived",
    "socialListRequestsSent",
    "socialListSearchResults",
    "socialListOffline",
    "socialListBlocked",
    "socialListPlatformOnline",
    "socialListPlatformOffline",
];
const SOCIAL_FRIEND_LISTS: [&str; 5] = [
    "socialListOnline",
    "socialListOffline",
    "socialListRecent",
    "socialListPlatformOnline",
    "socialListPlatformOffline",
];
const SOCIAL_RIGHT_LISTS: [&str; 3] = ["socialListRequestsReceived", "socialListRequestsSent", "socialListBlocked"];
const SOCIAL_MODE_WIDGETS: [&[&str]; 3] = [
    &["headerSmall", "hoverBox", "headerSmallSizebox"],
    &["headerMedium", "buttonBGDismissMedium"],
    &["headerLarge", "contentRight", "blurBox", "buttonBGDismissLarge", "SizeBox_Hack", "Offline", "socialListBlocked"],
];
/// Deviation: the panel lists the relay's online list (socialListOnline: everyone listed, there are no friends) under
/// the header ONLINE_LIST_HEADER. The recently played, platform and offline lists and the Large mode's right column
/// (search, the request lists, Refresh) stay collapsed, and Large is 500 wide (the build: 1250, with the right
/// column's 754; the oracle's width, the user's choice).
const SOCIAL_UNUSED: [&str; 4] = ["RecentlyPlayed", "PlatformOnline", "Offline", "contentRight"];
/// (the port's own) The online list is asked for again every LISTED_EVERY_S while the panel is open (Medium or
/// Large), and as it opens. The build's lists follow the backend's friend events (OnFriendOnline, the
/// PopulateTimerHandle delays); a list of everyone has none.
const LISTED_EVERY_S: f64 = 10.0;
const LARGE_WIDTH: f64 = crate::bundle::SOCIAL_LARGE_WIDTH;
const SOCIAL_ANIMS: [&str; 5] = [
    "AnimTransitionSmallToHidden",
    "AnimTransitionLargeToSmall",
    "AnimTransitionMediumToSmall",
    "AnimTransitionMediumToLarge",
    "AnimTransitionSmallToMedium",
];
const SCROLL_BOXES: [&str; 3] = ["scrollBoxLeft", "scrollBoxRight", "scrollBoxSearchResults"];

const PN_HOVER_FOCUS: u32 = 1;
const PN_HOVER_UNFOCUS: u32 = 2;
const PN_HOVER_CLICK: u32 = 3;
const PN_MEDIUM_FOCUS: u32 = 4;
const PN_MEDIUM_UNFOCUS: u32 = 5;
const PN_MEDIUM_CLICK: u32 = 6;
const PN_LARGE_FOCUS: u32 = 7;
const PN_LARGE_UNFOCUS: u32 = 8;
const PN_LARGE_CLICK: u32 = 9;
const PN_DISMISS_LARGE: u32 = 10;
const PN_DISMISS_MEDIUM_FOCUS: u32 = 11;
const PN_DISMISS_MEDIUM_CLICK: u32 = 12;
const PN_X_FOCUS: u32 = 13;
const PN_X_UNFOCUS: u32 = 14;
const PN_DISMISS_SEARCH: u32 = 15;
const PN_INVITE_FOCUS: u32 = 16;
const PN_INVITE_UNFOCUS: u32 = 17;
const PN_INVITE_CLICK: u32 = 18;
const PN_PLAYER_ADDED: u32 = 19;
const PN_PLAYER_FOCUSED: u32 = 20;
const PN_PLAYER_SELECTED: u32 = 21;
const PN_PLAYER_INVITE: u32 = 22;
const PN_CANCEL: u32 = 23;
const PN_INTERACTOR_FOCUSED: u32 = 24;
const PN_MODAL_DISMISSED: u32 = 25;
const PN_SCROLL: u32 = 26;
const PN_SHORTCUT: u32 = 27;
const PN_APPEARANCE: u32 = 28;

/// UI_SocialPanel: the friends panel on the right edge: Small (a strip of icons), Medium (names) and Large, with the
/// online list (socialListOnline). The panel is hidden while online play is off (not signed in), as the
/// home screen hides it in the build's offline mode.
/// Deviation, not ported: search, the requests / blocked tabs (listBar), the refresh button's reload, the request
/// badges and the UIFlow analytics. The other lists stay empty and are not shown (SOCIAL_UNUSED).
#[derive(Debug)]
pub struct SocialPanel {
    pub mode: i32,
    previous_mode: i32,
    quick_invite_mode: bool,
    solo_context: String,
    regular_context: String,
    mode_change_timer: f64,
    rest_vis: Vis,
    /// Populate Hid the panel (no connection).
    hidden_offline: bool,
    /// The last ask of the online list while open (view seconds).
    listed_at: f64,
    /// The online lists and the party as the rows were last drawn from (online.onChanged, party.onChange).
    seen: OnlineSeen,
}

impl SocialPanel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SocialPanel {
            mode: 0,
            previous_mode: 0,
            quick_invite_mode: false,
            solo_context: NULL_GUID.to_string(),
            regular_context: NULL_GUID.to_string(),
            mode_change_timer: 0.0,
            rest_vis: Vis::SelfHitTestInvisible,
            hidden_offline: false,
            listed_at: f64::NEG_INFINITY,
            seen: OnlineSeen::default(),
        })
    }

    fn use_shortcut(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Use Shortcut Binding")
    }

    fn lists(cx: &Cx<'_>) -> Vec<WidgetId> {
        SOCIAL_LISTS.iter().map(|n| find(cx, n)).collect()
    }

    fn list_name(cx: &Cx<'_>, l: WidgetId) -> String {
        cx.view.tree().widget(l).name.clone()
    }

    /// An interactor of the panel by name (a UI_Interactor widget).
    fn interactor(cx: &Cx<'_>, name: &str) -> WidgetId {
        find(cx, name)
    }

    /// WBP_Invite's UI_Interactor (GetInteractors @0: in the list after buttonBGDismissMedium).
    fn invite_interactor(cx: &Cx<'_>) -> WidgetId {
        let w = find(cx, "WBP_Invite");
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    /// GetInteractors.
    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let sub = |n: &str| ButtonSecondary::interactor(cx, find(cx, n));
        vec![
            Self::interactor(cx, "buttonHeaderMedium"),
            Self::interactor(cx, "buttonHeaderLarge"),
            sub("inputSearch"),
            sub("UI_ButtonSecondary"),
            Self::interactor(cx, "buttonSearchX"),
            Self::interactor(cx, "buttonBGDismissLarge"),
            Self::interactor(cx, "buttonBGDismissMedium"),
            Self::interactor(cx, "hoverBox"),
            sub("Btn_Refresh"),
            Self::interactor(cx, "buttonBGDismissSearch"),
            Self::invite_interactor(cx),
        ]
    }

    /// WBP_Invite, the quick invite button ("+"): its OnInitialized @2220 (UI_FocusedCommon on its interactor), the
    /// interactor's focus @1513 / unfocus @1701 and click @1422 (every UI_SocialPanel's SetMode(1, true, true): the
    /// Medium panel in Quick Invite Mode). Collapsed until its visibility is worked out (quick_invite_visibility).
    /// Not ported: Construct's PartyManager binds and tooltip (@15-916; the visibility is read each frame instead),
    /// the FadeOut animation's stop and the hover quad's opacity (@1569-1625).
    fn init_quick_invite(cx: &mut Cx<'_>) {
        let w = find(cx, "WBP_Invite");
        wd(cx, w).vis = Vis::Collapsed;
        let i = Self::invite_interactor(cx);
        let fc = cx.view.tree().find_in(w, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, PN_INVITE_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, PN_INVITE_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, PN_INVITE_CLICK);
    }

    /// WBP_Invite PartyChanged @2270-2944: Collapsed when the party has two joined (NumJoined >= 2), while
    /// matchmaking or a rematch, with the social panel open (SetPanelOpen, set by SetMode @8343 / @8855 / @10403) or
    /// the custom game open (WBP_CustomGame's CustomGameOpen: its modal customGame); else SelfHitTestInvisible
    /// (visibility 4). DetermineVisibility(Show) @2174: only with the panel's Show Quick Invite Button (SetMode(0)
    /// @10440), else Collapsed.
    fn quick_invite_visibility(&self, cx: &Cx<'_>) -> Vis {
        if !prop_bool(cx, "Show Quick Invite Button") || self.mode != 0 || !online_connected(cx) {
            return Vis::Collapsed;
        }
        let o = &cx.view.session.online;
        if o.party_full() || o.queued() || cx.view.manager.get_modal("customGame").is_some() {
            return Vis::Collapsed;
        }
        Vis::SelfHitTestInvisible
    }

    /// Large at the oracle's width: the area left of the panel that dismisses it. The two transitions that end at the
    /// build's 1250 (AnimTransitionMediumToLarge and AnimTransitionLargeToSmall, mainPanel's RightCurve keys) are
    /// clamped to LARGE_WIDTH when the bundle is read (`Bundle::narrow_social_panel`).
    fn narrow_large(cx: &mut Cx<'_>) {
        let b = find(cx, "buttonBGDismissLarge");
        wd(cx, b).pad = Margin { l: 1.0, t: 1.0, r: LARGE_WIDTH, b: 1.0 };
    }

    fn ask_listed(&mut self, cx: &mut Cx<'_>) {
        self.listed_at = cx.view.now();
        if online_connected(cx) {
            cx.online(OnlineCmd::Who);
        }
    }

    /// Populate, with the online list where the build's online friends go (socialListOnline); ends as
    /// OnPlatformPopulated does. The panel shows while online play is on. (The port's) an online list that could not
    /// be asked shows why in its empty line.
    pub fn populate(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.seen = OnlineSeen::of(&cx.view.session.online);
        if !online_connected(cx) {
            if self.mode != 0 {
                self.set_mode(cx, 0, false, true);
            }
            if cx.view.tree().widget(me).vis != Vis::Collapsed {
                wd(cx, me).vis = Vis::Hidden;
                self.hidden_offline = true;
            }
            return;
        }
        // (only its own Hidden is undone: a screen that Collapsed the panel, the pre-match's ExitGlobalNav, keeps it
        // so, before or after the panel was hidden)
        if self.hidden_offline {
            if cx.view.tree().widget(me).vis == Vis::Hidden {
                wd(cx, me).vis = self.rest_vis;
            }
            self.hidden_offline = false;
        }
        let o = &cx.view.session.online;
        let listed: Vec<SocialPlayerData> = o.listed.iter().map(SocialPlayerData::from).collect();
        let why = o.listed_why_text();
        for l in Self::lists(cx) {
            let name = Self::list_name(cx, l);
            let players: &[SocialPlayerData] = match name.as_str() {
                "socialListOnline" => &listed,
                _ => &[],
            };
            cx.visit::<SocialList, _>(l, |l, cx| l.populate(cx, players));
            if name == "socialListOnline" {
                cx.visit::<SocialList, _>(l, |l, cx| l.set_empty_text(cx, (!why.is_empty()).then_some(why.as_str())));
            }
        }
        // Deviation: OnPlatformPopulated ends in SetMode(Mode, false, true) (@16385 / @21364), dropping Quick Invite
        // Mode, but the build populates on platform events only; the port repopulates on every change of the online
        // list it asks for as the panel opens and while it is open, which would take the rows' "+" away a frame
        // after the quick invite's click. The mode is kept.
        let (m, q) = (self.mode, self.quick_invite_mode);
        self.set_mode(cx, m, q, true);
    }

    fn style_for_list(&self, name: &str) -> i32 {
        match name {
            "socialListRequestsReceived" => 3,
            "socialListRequestsSent" => 4,
            n if !SOCIAL_FRIEND_LISTS.contains(&n) => 0,
            _ => match (self.mode, self.quick_invite_mode) {
                (0, true) => 5,
                (0, false) => 1,
                (1, true) => 2,
                _ => 0,
            },
        }
    }

    fn scroll_box_for_list(cx: &Cx<'_>, name: &str) -> WidgetId {
        let n = if SOCIAL_FRIEND_LISTS.contains(&name) {
            "scrollBoxLeft"
        } else if SOCIAL_RIGHT_LISTS.contains(&name) {
            "scrollBoxRight"
        } else {
            "scrollBoxSearchResults"
        };
        find(cx, n)
    }

    /// SetMode(Mode, Quick Invite Mode, Force).
    pub fn set_mode(&mut self, cx: &mut Cx<'_>, mode: i32, quick_invite_mode: bool, force: bool) {
        if !force {
            // TimeIntervalCheck(ModeChangeTimer, 0.1)
            let now = cx.view.now();
            let d = now - self.mode_change_timer;
            if (0.0..0.1).contains(&d) {
                return;
            }
            self.mode_change_timer = now;
            if mode == self.previous_mode {
                return;
            }
        }
        self.quick_invite_mode = quick_invite_mode;
        self.mode = mode;
        // (the port's) the online list asked for as the panel opens
        if mode != 0 {
            self.ask_listed(cx);
        }
        let prev = self.previous_mode;
        let left = find(cx, "scrollBoxLeft");
        if mode == 0 {
            for (box_name, list) in [("Online", "socialListOnline"), ("PlatformOnline", "socialListPlatformOnline")] {
                let l = find(cx, list);
                let any = cx.visit::<SocialList, _>(l, |l, _| !l.players.is_empty()).unwrap_or(false);
                let b = find(cx, box_name);
                wd(cx, b).vis = if any { Vis::SelfHitTestInvisible } else { Vis::Visible };
            }
            // @10337-10369 ScrollToStart, SetScrollBarVisibility(Collapsed)
            scroll_to_start(cx.view.tree_mut(), left);
            set_scroll_bar_visibility(cx.view.tree_mut(), left, Vis::Collapsed);
            if prev == 1 {
                self.animate_transition(cx, "AnimTransitionSmallToMedium", true);
            } else if prev == 2 {
                self.animate_transition(cx, "AnimTransitionLargeToSmall", false);
            }
        } else if mode == 1 {
            for n in ["Online", "PlatformOnline", "socialListPlatformOffline"] {
                let w = find(cx, n);
                wd(cx, w).vis = Vis::SelfHitTestInvisible;
            }
            // @9118-9287
            set_scrollbar_padding(cx.view.tree_mut(), left, Margin::default());
            set_scroll_bar_visibility(cx.view.tree_mut(), left, Vis::Visible);
            if prev == 0 {
                self.animate_transition(cx, "AnimTransitionSmallToMedium", false);
            } else if prev == 2 {
                self.animate_transition(cx, "AnimTransitionMediumToLarge", true);
            }
        } else {
            close_all_tooltips(cx); // @8417
            // @8454-8623
            set_scrollbar_padding(cx.view.tree_mut(), left, Margin { l: 10.0, t: 0.0, r: -16.0, b: 30.0 });
            set_scroll_bar_visibility(cx.view.tree_mut(), left, Vis::Visible);
            if prev == 1 {
                self.animate_transition(cx, "AnimTransitionMediumToLarge", false);
            } else if prev == 0 {
                self.animate_transition(cx, "AnimTransitionLargeToSmall", true);
            }
        }
        Self::dismiss_search_results(cx);
        self.update_appearance(cx);
        if mode == 2 {
            let ctx = self.solo_context.clone();
            Self::internal_update_context(cx, &ctx);
            self.focus_first(cx);
        } else {
            let ctx = self.regular_context.clone();
            self.update_context(cx, &ctx);
            if mode == 1 && !cx.view.manager.cursor_mode[0] {
                self.focus_first(cx);
            }
        }
        for l in Self::lists(cx) {
            let collapsed = cx
                .visit::<SocialList, _>(l, |l, cx| {
                    l.set_mode(cx, mode);
                    l.is_collapsed
                })
                .unwrap_or(false);
            wd(cx, l).vis = if mode == 0 && collapsed { Vis::Collapsed } else { Vis::Visible };
        }
        self.previous_mode = mode;
    }

    /// AnimateTransition(Animation, 1.0, Stop Others true, Reverse).
    fn animate_transition(&mut self, cx: &mut Cx<'_>, name: &str, reverse: bool) {
        let me = cx.w;
        cx.view.stop_all_animations(me);
        cx.view.play(me, name, reverse, 1.0);
    }

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        for n in SOCIAL_MODE_WIDGETS.iter().flat_map(|l| l.iter()) {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        let m = usize::try_from(self.mode).unwrap_or(0).min(2);
        for n in SOCIAL_MODE_WIDGETS[m] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Visible;
        }
        // UpdateListStyles
        for l in Self::lists(cx) {
            let s = self.style_for_list(&Self::list_name(cx, l));
            cx.visit::<SocialList, _>(l, |l, cx| l.set_style(cx, s));
        }
        if self.quick_invite_mode {
            let o = find(cx, "Offline");
            wd(cx, o).vis = Vis::Collapsed;
        }
        for n in SOCIAL_UNUSED {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
    }

    /// UpdateContext: the context of the screen the panel sits on (the home screen's Update Context calls it on
    /// SocialPanel_MainMenu).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.regular_context = ctx.to_string();
        Self::internal_update_context(cx, ctx);
    }

    fn internal_update_context(cx: &mut Cx<'_>, ctx: &str) {
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
        for l in Self::lists(cx) {
            cx.visit::<SocialList, _>(l, |l, cx| l.update_context(cx, ctx));
        }
    }

    fn focus_first(&mut self, cx: &mut Cx<'_>) {
        let input = find(cx, "inputSearch");
        if cx.visit::<TextInput, _>(input, |_, cx| TextInput::has_keyboard_focus(cx)).unwrap_or(false) {
            return;
        }
        let n = match self.mode {
            1 => "buttonHeaderMedium",
            2 => "buttonHeaderLarge",
            _ => return,
        };
        let i = Self::interactor(cx, n);
        set_focused(cx, i, false);
    }

    /// IsOpen?
    pub fn is_open(&self, cx: &Cx<'_>) -> bool {
        let me = cx.w;
        if !cx.view.visible_in_hierarchy(me) {
            return false;
        }
        self.mode != 0 || SOCIAL_ANIMS.iter().any(|a| cx.view.is_animation_playing(me, a))
    }

    /// HandleShortcut (UISubmit2).
    fn handle_shortcut(&mut self, cx: &mut Cx<'_>) {
        if self.is_open(cx) {
            return self.set_mode(cx, 0, false, false);
        }
        let me = cx.w;
        if !cx.view.visible_in_hierarchy(me) || Manager::is_dialog_active(cx.view) {
            return;
        }
        if Manager::is_modal_active(cx.view) {
            let Some(lobby) = cx.view.manager.get_modal("customGame") else { return };
            // Port's own: only the lobby's panel opens over the lobby (the build's main menu panel behind it is bound
            // to the same event).
            let mut w = Some(me);
            let mut inside = false;
            while let Some(x) = w {
                inside |= x == lobby;
                w = cx.view.tree().widget(x).parent;
            }
            if !inside {
                return;
            }
        }
        self.set_mode(cx, 2, false, false);
        cx.post("ui_menu_global_social_open");
    }

    fn dismiss_medium(&mut self, cx: &mut Cx<'_>) {
        if self.mode != 1 {
            return;
        }
        self.set_mode(cx, 0, false, true);
        cx.post("ui_menu_global_back_escape");
    }

    fn dismiss_search_results(cx: &mut Cx<'_>) {
        let s = find(cx, "searchResultsPanel");
        wd(cx, s).vis = Vis::Collapsed;
    }

    /// StepBack, also the manager's OnCancel.
    pub fn step_back(&mut self, cx: &mut Cx<'_>) {
        if !self.is_open(cx) {
            return;
        }
        if self.mode == 1 {
            self.set_mode(cx, 0, false, false);
        } else if self.mode == 2 {
            let s = find(cx, "searchResultsPanel");
            if cx.view.visible_in_hierarchy(s) {
                Self::dismiss_search_results(cx);
            } else {
                let m = if Self::use_shortcut(cx) { 0 } else { 1 };
                self.set_mode(cx, m, false, false);
            }
        }
    }

    /// OnInteractorFocused: the focus going to something outside the panel closes it.
    fn on_interactor_focused(&mut self, cx: &mut Cx<'_>, i: WidgetId) {
        if self.mode == 0 || Manager::is_dialog_active(cx.view) {
            return;
        }
        if Manager::is_modal_active(cx.view) && Manager::latest_modal_id(cx.view) != "customGame" {
            return;
        }
        let me = cx.w;
        let mut w = Some(i);
        while let Some(x) = w {
            if x == me {
                return;
            }
            let toast = cx.view.tree().user_class(x) == Some("UI_Notification")
                && cx.visit::<NotificationToast, _>(x, |_, _| ()).is_some();
            if toast {
                return;
            }
            w = cx.view.tree().widget(x).parent;
        }
        self.set_mode(cx, 0, false, false);
    }

    fn on_scroll(&mut self, cx: &mut Cx<'_>, p: i32, amount: f64) {
        let first = Self::interactors(cx)[0];
        if p != 0 || !self.is_open(cx) || !Manager::is_in_current_context(cx, first, false, 0) {
            return;
        }
        // UI_Functions.ScrollList
        let s = find(cx, "searchResultsPanel");
        if cx.view.visible_in_hierarchy(s) {
            let b = find(cx, "scrollBoxSearchResults");
            scroll_list(cx.view.tree_mut(), b, amount);
        } else {
            for n in ["scrollBoxLeft", "scrollBoxRight"] {
                let b = find(cx, n);
                scroll_list(cx.view.tree_mut(), b, amount);
            }
        }
    }

    /// BindSocialPlayerEvents.
    fn bind_social_player_events(&mut self, cx: &mut Cx<'_>, entry: WidgetId, list: WidgetId) {
        let me = cx.w;
        let i = SocialPlayer::interactor_of(cx, entry);
        let focus = Handler { owner: me, id: PN_PLAYER_FOCUSED };
        if let Some(s) = cx.view.manager.interactor_mut(i) {
            s.on_focus.remove(focus);
            s.on_focus.add(focus);
        }
        let name = Self::list_name(cx, list);
        let style = self.style_for_list(&name);
        cx.visit::<SocialPlayer, _>(entry, |p, cx| p.set_style(cx, style));
        let sb = Self::scroll_box_for_list(cx, &name);
        batch_set_scroll_box(cx, &[i], sb);
        cx.visit::<SocialPlayer, _>(entry, |p, _| {
            p.selected.remove(Handler { owner: me, id: PN_PLAYER_SELECTED });
            p.selected.add(Handler { owner: me, id: PN_PLAYER_SELECTED });
            p.invite_clicked.remove(Handler { owner: me, id: PN_PLAYER_INVITE });
            p.invite_clicked.add(Handler { owner: me, id: PN_PLAYER_INVITE });
        });
    }
}

impl Controller for SocialPanel {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.rest_vis = cx.view.tree().widget(me).vis;
        self.solo_context = cx.view.manager.new_guid();
        let (fc, x) = (find(cx, "buttonSearchXFocused"), Self::interactor(cx, "buttonSearchX"));
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, x));
        // PlatformFriendsListTitle: no platform (the branch after Xbox, PlayStation, Epic and Steam).
        // Unverified: the two titles are the build's literal strings of that branch, not a localized line.
        for (n, t) in [
            ("socialListPlatformOnline", "ONLINE PLATFORM FRIENDS"),
            ("socialListPlatformOffline", "OFFLINE PLATFORM FRIENDS"),
        ] {
            let l = find(cx, n);
            cx.visit::<SocialList, _>(l, |l, cx| l.set_header_text(cx, t));
        }
        // (the port's own) the one list is the relay's online players, not the build's online WB friends: its header
        // and its empty line say players
        let l = find(cx, "socialListOnline");
        cx.visit::<SocialList, _>(l, |l, cx| {
            l.set_header_text(cx, crate::menu::custom_text::ONLINE_LIST_HEADER);
            l.set_empty_default(cx, crate::menu::custom_text::NO_PLAYERS_ONLINE);
        });
        Self::init_quick_invite(cx);
        // the tab bar's horizontal buttons are not in the bundle: the requests tab stays
        let lb = find(cx, "listBar");
        wd(cx, lb).vis = Vis::Collapsed;
        Self::narrow_large(cx);
        let binds: [(&str, InteractorEvent, u32); 14] = [
            ("hoverBox", |s| &mut s.on_focus, PN_HOVER_FOCUS),
            ("hoverBox", |s| &mut s.on_unfocus, PN_HOVER_UNFOCUS),
            ("hoverBox", |s| &mut s.on_click, PN_HOVER_CLICK),
            ("buttonHeaderMedium", |s| &mut s.on_focus, PN_MEDIUM_FOCUS),
            ("buttonHeaderMedium", |s| &mut s.on_unfocus, PN_MEDIUM_UNFOCUS),
            ("buttonHeaderMedium", |s| &mut s.on_click, PN_MEDIUM_CLICK),
            ("buttonHeaderLarge", |s| &mut s.on_focus, PN_LARGE_FOCUS),
            ("buttonHeaderLarge", |s| &mut s.on_unfocus, PN_LARGE_UNFOCUS),
            ("buttonHeaderLarge", |s| &mut s.on_click, PN_LARGE_CLICK),
            ("buttonBGDismissLarge", |s| &mut s.on_click, PN_DISMISS_LARGE),
            ("buttonBGDismissMedium", |s| &mut s.on_focus, PN_DISMISS_MEDIUM_FOCUS),
            ("buttonBGDismissMedium", |s| &mut s.on_click, PN_DISMISS_MEDIUM_CLICK),
            ("buttonSearchX", |s| &mut s.on_focus, PN_X_FOCUS),
            ("buttonSearchX", |s| &mut s.on_unfocus, PN_X_UNFOCUS),
        ];
        for (n, ev, id) in binds {
            let i = Self::interactor(cx, n);
            bind(cx, i, ev, id);
        }
        for n in ["buttonSearchX", "buttonBGDismissSearch"] {
            let i = Self::interactor(cx, n);
            bind(cx, i, |s: &mut InteractorState| &mut s.on_click, PN_DISMISS_SEARCH);
        }
    }

    /// RunPreConstruct.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for l in Self::lists(cx) {
            cx.visit::<SocialList, _>(l, |l, _| {
                let h = Handler { owner: me, id: PN_PLAYER_ADDED };
                l.player_added_to_list.remove(h);
                l.player_added_to_list.add(h);
            });
        }
        let m = enum_index(cx.view.tree().props(me).get("Mode"));
        self.set_mode(cx, m, false, true);
        let nav = !Self::use_shortcut(cx);
        let h = Self::interactor(cx, "hoverBox");
        if let Some(s) = cx.view.manager.interactor_mut(h) {
            s.navigable = nav;
        }
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for (f, i) in [("focusedHeaderMedium", "buttonHeaderMedium"), ("focusedHeaderLarge", "buttonHeaderLarge")] {
            let (f, i) = (find(cx, f), Self::interactor(cx, i));
            cx.visit::<FocusedCommon, _>(f, |f, cx| f.set_interactor(cx, i));
        }
        let m = &mut cx.view.manager;
        m.on_cancel.add(Handler { owner: me, id: PN_CANCEL });
        m.on_interactor_focused.add(Handler { owner: me, id: PN_INTERACTOR_FOCUSED });
        m.modal_dismissed.add(Handler { owner: me, id: PN_MODAL_DISMISSED });
        m.on_scroll.add(Handler { owner: me, id: PN_SCROLL });
        for n in SCROLL_BOXES {
            let b = find(cx, n);
            cx.view.manager.register_scrollbox(b);
        }
        let sc = Self::use_shortcut(cx);
        for n in ["inputLabelShortcutSmall", "inputLabelShortcutLarge"] {
            let w = find(cx, n);
            wd(cx, w).vis = if sc { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        }
        if sc {
            cx.view.manager.on_submit2.add(Handler { owner: me, id: PN_SHORTCUT });
        }
        self.populate(cx);
        let b = find(cx, "SizeBox_4");
        fill_page(cx, b);
    }

    /// Deviation: the panel's own 16:9 box follows the page like the main menu's, so it stays on the right edge.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_4");
        fill_page(cx, b);
        let invite = find(cx, "WBP_Invite");
        let vis = self.quick_invite_visibility(cx);
        if cx.view.tree().widget(invite).vis != vis {
            wd(cx, invite).vis = vis;
        }
        if !self.seen.same(&cx.view.session.online) {
            self.populate(cx);
        }
        if self.mode != 0 && cx.view.now() - self.listed_at >= LISTED_EVERY_S {
            self.ask_listed(cx);
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let m = &mut cx.view.manager;
        m.on_cancel.remove(Handler { owner: me, id: PN_CANCEL });
        m.on_interactor_focused.remove(Handler { owner: me, id: PN_INTERACTOR_FOCUSED });
        m.modal_dismissed.remove(Handler { owner: me, id: PN_MODAL_DISMISSED });
        m.on_scroll.remove(Handler { owner: me, id: PN_SCROLL });
        m.on_submit2.remove(Handler { owner: me, id: PN_SHORTCUT });
        for n in SCROLL_BOXES {
            let b = find(cx, n);
            cx.view.manager.unregister_scrollbox(b);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let header_bg = |cx: &mut Cx<'_>, n: &str, c: Rgba| {
            let w = find(cx, n);
            wd(cx, w).color = c;
        };
        match id {
            PN_HOVER_FOCUS => {
                if Self::use_shortcut(cx) {
                    let b = find(cx, "BG_Hover");
                    wd(cx, b).vis = Vis::SelfHitTestInvisible;
                } else {
                    self.set_mode(cx, 1, false, false);
                }
            }
            PN_HOVER_UNFOCUS => {
                if Self::use_shortcut(cx) {
                    let b = find(cx, "BG_Hover");
                    wd(cx, b).vis = Vis::Hidden;
                }
            }
            PN_HOVER_CLICK => {
                let m = if Self::use_shortcut(cx) { 2 } else { 1 };
                self.set_mode(cx, m, false, false);
            }
            PN_MEDIUM_FOCUS => header_bg(cx, "headerMediumBG", SOCIAL_HEADER_FOCUS),
            PN_MEDIUM_UNFOCUS => header_bg(cx, "headerMediumBG", SOCIAL_HEADER),
            PN_MEDIUM_CLICK => self.set_mode(cx, 2, false, false),
            PN_LARGE_FOCUS => header_bg(cx, "headerLargeBG", SOCIAL_HEADER_FOCUS),
            PN_LARGE_UNFOCUS => header_bg(cx, "headerLargeBG", SOCIAL_HEADER),
            PN_LARGE_CLICK => {
                let m = if Self::use_shortcut(cx) { 0 } else { 1 };
                self.set_mode(cx, m, false, false);
            }
            PN_DISMISS_LARGE => {
                self.set_mode(cx, 0, false, false);
                cx.post("ui_menu_global_back_escape");
            }
            // Rolloff Grace Period is 0: rolling off the Medium panel closes it at once
            PN_DISMISS_MEDIUM_FOCUS => {
                if !self.quick_invite_mode {
                    self.dismiss_medium(cx);
                }
            }
            PN_DISMISS_MEDIUM_CLICK => self.dismiss_medium(cx),
            PN_X_FOCUS => header_bg(cx, "Image_85", [0.006995, 0.008023, 0.022174, 1.0]),
            PN_X_UNFOCUS => header_bg(cx, "Image_85", [0.89627, 0.973446, 0.871367, 1.0]),
            PN_DISMISS_SEARCH => Self::dismiss_search_results(cx),
            PN_INVITE_FOCUS | PN_INVITE_UNFOCUS => {
                let on = id == PN_INVITE_FOCUS;
                let w = find(cx, "WBP_Invite");
                let (plus, quad) =
                    (cx.view.tree().find_in(w, "PlusImage_1"), cx.view.tree().find_in(w, "WBP_Quadrilateral_1"));
                let s = if on { 1.2 } else { 1.0 };
                wd(cx, plus).transform.scale = [s, s];
                wd(cx, quad).vis = if on { Vis::Hidden } else { Vis::SelfHitTestInvisible };
            }
            // every UI_SocialPanel's SetMode(1, true, true)
            PN_INVITE_CLICK => {
                let me = cx.w;
                self.set_mode(cx, 1, true, true);
                for p in live(cx, "UI_SocialPanel") {
                    if p != me {
                        cx.visit::<SocialPanel, _>(p, |p, cx| p.set_mode(cx, 1, true, true));
                    }
                }
            }
            PN_PLAYER_ADDED => {
                if let Some(&(entry, list)) = arg.downcast_ref::<(WidgetId, WidgetId)>() {
                    self.bind_social_player_events(cx, entry, list);
                }
            }
            PN_PLAYER_FOCUSED => {
                if self.mode == 0 {
                    self.set_mode(cx, 1, false, false);
                }
            }
            // UI_SocialPlayer Select @4460 -> @3242-3378: ShowSocialActionsMenu(the player, the row interactor's
            // viewport TopLeft + (-100, 70), slug '')
            PN_PLAYER_SELECTED => {
                if let Some((row, d)) = arg.downcast_ref::<(WidgetId, SocialPlayerData)>() {
                    let i = SocialPlayer::interactor_of(cx, *row);
                    let [x, y] = cx.view.local_to_viewport(i, 0.0, 0.0);
                    show_social_actions_menu(cx, d.clone(), [x - 100.0, y + 70.0], "", &[]);
                }
            }
            // OnPlayerInviteClicked @22997: with a custom game lobby PerformSocialAction(12) (the lobby's invite),
            // else PerformSocialAction(1) -> BPI_InviteToParty (PandaGameInstance InviteToParty @62690-63153:
            // PartyManager.InviteToParty). Not ported: InviteToParty's platform session invite, its matchmaking
            // cancel (the "+" shows only out of the queue) and its crossplay check.
            // Deviation: with a custom lobby up (the "+" shows then, is_player_invitable) the port sends the lobby
            // invite through the page's own (CustomGame::invite: the keeper's LobbyInvite and OnlineCmd::Invite), as
            // the invite modal's choice does.
            PN_PLAYER_INVITE => {
                if let Some(d) = arg.downcast_ref::<SocialPlayerData>() {
                    if let Some(page) = cx.view.manager.get_modal("customGame") {
                        if lobby_invite_open(cx, &d.id) {
                            let id = d.id.clone();
                            cx.visit::<super::custom_game::CustomGame, _>(page, |g, _| g.invite(&id));
                        }
                    } else {
                        cx.online(OnlineCmd::Party(PartyCmd::Invite { to: d.id.clone() }));
                    }
                }
            }
            PN_CANCEL => self.step_back(cx),
            PN_INTERACTOR_FOCUSED => {
                if let Some(&i) = arg.downcast_ref::<WidgetId>() {
                    self.on_interactor_focused(cx, i);
                }
            }
            PN_MODAL_DISMISSED => {
                let b = find(cx, "buttonBGDismissMedium");
                wd(cx, b).vis = Vis::Collapsed;
                let me = cx.w;
                cx.view.delay(0.25, Handler { owner: me, id: PN_APPEARANCE });
            }
            PN_SCROLL => {
                if let Some(&(p, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() {
                    self.on_scroll(cx, p, amount);
                }
            }
            PN_SHORTCUT => self.handle_shortcut(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == PN_APPEARANCE {
            self.update_appearance(cx);
        }
    }
}

// ---------- UI_SocialInviteModal ----------

const INVITE_LISTS: [&str; 3] = ["UI_SocialList_friends", "UI_SocialList_platform", "UI_SocialList_offline"];

const IM_INVITE: u32 = 1;
const IM_CANCEL: u32 = 2;
const IM_SCROLL: u32 = 3;
const IM_SELECTED: u32 = 4;

/// The lobby's Invite Player modal (modal socialInvite): the invitable players as rows with a checkmark, Invite and
/// Cancel. The online list's players (asked for as it shows and every LISTED_EVERY_S while up) stand in for the
/// invitable friends, in the one list titled ONLINE PLAYERS (UI_SocialList_friends); the platform and offline lists
/// are collapsed. Deviation: the build lists friends in three lists and says so in its titles and its empty line
/// (the port has no friends); the players met recently are not listed here.
/// Unverified: who can be invited is native (CanInvitePlayer); here everyone listed who is not busy and not in the
/// lobby (set_excluded). Not ported: the cap of 15 rows, the offline list's pages.
#[derive(Debug, Default)]
pub struct SocialInviteModal {
    base: ModalBase,
    /// The players chosen when Invite is clicked.
    pub selections_made_event: Event<Vec<SocialPlayerData>>,
    shown: bool,
    /// The lobby's members (set_excluded): not listed.
    excluded: Vec<String>,
    listed_at: f64,
    /// The online list as the rows were last drawn from (OnFriendOnline / OnFriendOffline).
    seen: OnlineSeen,
}

impl SocialInviteModal {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SocialInviteModal { listed_at: f64::NEG_INFINITY, ..SocialInviteModal::default() })
    }

    fn scroll(cx: &Cx<'_>) -> WidgetId {
        find(cx, "scrollContainer")
    }

    fn rows(cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut v = Vec::new();
        for n in INVITE_LISTS {
            let l = find(cx, n);
            v.extend(cx.visit::<SocialList, _>(l, |l, cx| l.player_widgets(cx)).unwrap_or_default());
        }
        v
    }

    /// The ids not to list (the lobby's members), the list drawn again when it is up.
    pub fn set_excluded(&mut self, cx: &mut Cx<'_>, ids: Vec<String>) {
        self.excluded = ids;
        if self.shown {
            self.refresh(cx);
        }
    }

    fn ask_listed(&mut self, cx: &mut Cx<'_>) {
        self.listed_at = cx.view.now();
        if online_connected(cx) {
            cx.online(OnlineCmd::Who);
        }
    }

    /// Refresh: the lists again with the selection kept, a new context and the focus on Cancel.
    fn refresh(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let selected: Vec<String> = self.selected_players(cx).into_iter().map(|p| p.id).collect();
        self.seen = OnlineSeen::of(&cx.view.session.online);
        let o = &cx.view.session.online;
        let players: Vec<SocialPlayerData> =
            o.listed.iter().filter(|p| !p.busy && !self.excluded.contains(&p.id)).map(SocialPlayerData::from).collect();
        let listed_text = o.listed_why_text();
        for (n, list) in
            [("UI_SocialList_friends", &players[..]), ("UI_SocialList_platform", &[]), ("UI_SocialList_offline", &[])]
        {
            let l = find(cx, n);
            cx.visit::<SocialList, _>(l, |l, cx| l.populate(cx, list));
        }
        let sc = Self::scroll(cx);
        for row in Self::rows(cx) {
            let i = SocialPlayer::interactor_of(cx, row);
            cx.visit::<SocialPlayer, _>(row, |r, cx| {
                r.set_style(cx, 0);
                r.enable_context_menu = false;
                r.shows_selected_state = true;
                r.toggle_selected_state = true;
                let h = Handler { owner: me, id: IM_SELECTED };
                r.selected_event.remove(h);
                r.selected_event.add(h);
                let on = r.data.as_ref().is_some_and(|d| selected.contains(&d.id));
                r.set_selected(cx, on);
            });
            batch_set_scroll_box(cx, &[i], sc);
            set_interactable(cx, i, true);
        }
        if !players.is_empty() {
            // UI_SocialInviteModal @5007-5135. Deviation: NoFriendsOnline is written at @512 alone (visibility 4):
            // in the build, once shown it stays under the lists when a later populate has rows (the modal opens
            // before the online list answers, so its line and the rows overlapped); here it is collapsed.
            let nf = find(cx, "NoFriendsOnline");
            wd(cx, nf).vis = Vis::Collapsed;
            wd(cx, sc).vis = Vis::SelfHitTestInvisible;
            for n in INVITE_LISTS {
                let w = find(cx, n);
                wd(cx, w).vis = if n == "UI_SocialList_friends" { Vis::Visible } else { Vis::Collapsed };
            }
        } else if !listed_text.is_empty() {
            // (the port's own) the online list could not be asked (no relay signed in): its empty line says why
            let nf = find(cx, "NoFriendsOnline");
            wd(cx, nf).vis = Vis::Collapsed;
            wd(cx, sc).vis = Vis::SelfHitTestInvisible;
            for n in INVITE_LISTS {
                let w = find(cx, n);
                wd(cx, w).vis = if n == "UI_SocialList_friends" { Vis::Visible } else { Vis::Collapsed };
            }
        } else {
            // (its title and hint say players: on_initialized)
            let nf = find(cx, "NoFriendsOnline");
            wd(cx, nf).vis = Vis::SelfHitTestInvisible;
            wd(cx, sc).vis = Vis::Hidden;
        }
        let f = find(cx, "UI_SocialList_friends");
        let empty = (players.is_empty() && !listed_text.is_empty()).then_some(listed_text.as_str());
        cx.visit::<SocialList, _>(f, |l, cx| l.set_empty_text(cx, empty));
        self.update_appearance(cx);
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        let c = find(cx, "buttonCancel");
        let ci = ButtonSecondary::interactor(cx, c);
        set_focused(cx, ci, false);
    }

    fn selected_players(&self, cx: &mut Cx<'_>) -> Vec<SocialPlayerData> {
        let mut v = Vec::new();
        for r in Self::rows(cx) {
            let vis = cx.view.visible_in_hierarchy(r);
            if let Some(Some(d)) =
                cx.visit::<SocialPlayer, _>(r, |p, _| if p.is_selected { p.data.clone() } else { None })
                && vis
            {
                v.push(d);
            }
        }
        v
    }

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let n = self.selected_players(cx).len();
        let b = find(cx, "buttonInvite");
        let i = cx.view.tree().find_in(b, "UI_Interactor");
        set_interactable(cx, i, n > 0);
        // Unverified: the labels are the oracle's literal strings, not a localized line of the build.
        let label = match n {
            0 => "Invite Players".to_string(),
            1 => "Invite Player".to_string(),
            n => format!("Invite {n} Players"),
        };
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let (bg, inv, can) = (find(cx, "buttonBG"), find(cx, "buttonInvite"), find(cx, "buttonCancel"));
        let list = [bg, cx.view.tree().find_in(inv, "UI_Interactor"), ButtonSecondary::interactor(cx, can)];
        batch_set_context(cx, &list, ctx);
        for n in INVITE_LISTS {
            let l = find(cx, n);
            cx.visit::<SocialList, _>(l, |l, cx| l.update_context(cx, ctx));
        }
    }
}

impl Modal for SocialInviteModal {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
        let me = cx.w;
        cx.view.manager.on_scroll.add(Handler { owner: me, id: IM_SCROLL });
        let sc = Self::scroll(cx);
        cx.view.manager.register_scrollbox(sc);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        self.shown = true;
        self.refresh(cx);
        self.ask_listed(cx);
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        let sc = Self::scroll(cx);
        cx.view.manager.unregister_scrollbox(sc);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for SocialInviteModal {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // One list: every online player. The platform and offline lists are never shown (refresh).
        let f = find(cx, "UI_SocialList_friends");
        cx.visit::<SocialList, _>(f, |l, cx| l.set_header_text(cx, crate::menu::custom_text::ONLINE_PLAYERS));
        // Deviation: the build's empty block names friends (its title and its hint); the port has none, so both say
        // players.
        let nf = find(cx, "NoFriendsOnline");
        for (n, t) in [
            ("TextBlock_135", crate::menu::custom_text::NO_PLAYERS_AVAILABLE),
            ("TextBlock", crate::menu::custom_text::INVITE_HINT),
        ] {
            let w = cx.view.tree().find_in(nf, n);
            wd(cx, w).text = t.to_string();
        }
        let sc = Self::scroll(cx);
        for n in INVITE_LISTS {
            let l = find(cx, n);
            cx.visit::<SocialList, _>(l, |l, cx| l.set_scroll_box(cx, sc));
        }
        let inv = find(cx, "buttonInvite");
        cx.visit::<ButtonMain, _>(inv, |b, _| b.on_click.add(Handler { owner: me, id: IM_INVITE }));
        let can = find(cx, "buttonCancel");
        cx.visit::<ButtonSecondary, _>(can, |b, _| b.on_click.add(Handler { owner: me, id: IM_CANCEL }));
        let bg = find(cx, "buttonBG");
        bind(cx, bg, |s: &mut InteractorState| &mut s.on_click, IM_CANCEL);
    }

    /// ReorderLists (not a console): friends, platform, offline.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let sc = Self::scroll(cx);
        let order: Vec<WidgetId> = INVITE_LISTS.iter().map(|n| find(cx, n)).collect();
        let kids = &mut cx.view.tree_mut().widget_mut(sc).children;
        kids.sort_by_key(|c| order.iter().position(|o| o == c).map_or(-1, |p| p as i64));
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.shown && !self.seen.same(&cx.view.session.online) {
            self.refresh(cx);
        }
        if self.shown && cx.view.now() - self.listed_at >= LISTED_EVERY_S {
            self.ask_listed(cx);
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: IM_SCROLL });
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == IM_SELECTED {
            self.update_appearance(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            IM_INVITE => {
                let sel = self.selected_players(cx);
                cx.emit(self, |c| &c.selections_made_event, &sel);
                self.request_dismiss(cx, "", false);
            }
            IM_CANCEL => self.request_dismiss(cx, "", false),
            // The row's click emits this while the row's controller is checked out, so the rows cannot be visited
            // yet: the count is read a beat later.
            IM_SELECTED => {
                let me = cx.w;
                cx.view.delay(0.0, Handler { owner: me, id: IM_SELECTED });
            }
            // OnUIScroll: the scroll container by the amount.
            IM_SCROLL => {
                if let Some(&(_, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>() {
                    let sc = Self::scroll(cx);
                    scroll_list(cx.view.tree_mut(), sc, amount);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_beyond_the_table_is_the_default() {
        assert_eq!(style_vis(7), PLAYER_STYLE_VIS[0]);
        assert_eq!(style_vis(-1), PLAYER_STYLE_VIS[0]);
        assert_eq!(style_vis(2)[4], Vis::Visible);
    }

    #[test]
    fn the_dot_follows_the_status_and_an_old_client_by_busy() {
        let d = |busy, status| SocialPlayerData { busy, status, ..Default::default() };
        assert!(shown_busy(&d(false, Some(PresenceStatus::Lobby))));
        assert!(!shown_busy(&d(false, Some(PresenceStatus::Menus))));
        assert!(shown_busy(&d(true, None)) && !shown_busy(&d(false, None)));
        assert!(PresenceStatus::ALL.iter().all(|s| !status_words(*s).is_empty()));
    }
}
