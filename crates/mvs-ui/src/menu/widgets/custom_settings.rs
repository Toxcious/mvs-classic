//! (the port's own) A custom lobby's settings: a page of lobby options
//! (mvs_protocol::lobby_rules `OptionKey`), a row each, drawn on UI_QuickMatchSettings' tree (the art, the panel, a
//! drop menu per row, Done and the close button; its own three menus and the hazards toggle stay collapsed). Two
//! pages: the room's (modal id `customGameSettings`, the lobby screen's ROOM SETTINGS button, an online lobby's),
//! the experimental rules (`customGameExperimental`, the button in Reset To Defaults' place) and the Chaos Wheel's
//! (`customGameWheel`, the CHAOS WHEEL button beside it: `wheel_page`, the rows of `WheelOptions`). The leader's rows ask
//! the lobby for the change at once (`on_option`); the others' show the values, greyed. The lobby screen hands the
//! modal its lobby's state and page (`bind`) and calls `refresh` at each change of the options or the leader.
//!
//! Deviation: the modal holds no Lobby; the lobby is the lobby screen's, which is busy while it calls this
//! modal, so the modal takes a `LobbyView` copy at `bind` / `refresh` and asks for a change through `on_option`.

use std::any::Any;

use mvs_protocol::lobby_rules::{
    LobbyOptions, MutatorSet, OptionKey, OptionValue, WHEEL_SECONDS_CHOICES, WHEEL_WARN_CHOICES, WheelOptions,
};
use serde_json::{Value, json};

use super::common::{ModalBase, fill_page};
use super::controls::bar_buttons::{P_ID, P_LABEL};
use super::controls::buttons::{ButtonMain, IconButton};
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{self, batch_set_context};
use super::controls::{find, wd};
use super::input_binding_modal::CapturePlayersFocus;
use super::quick_match_settings::{MODE_BG, MODE_FG};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::custom_text as text;
use crate::menu::manager::Modal;
use crate::slate::{Vis, WidgetId};

/// A page's words: a Game string key (the menu's text table) or the port's own text (custom_text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Words {
    Game(&'static str),
    Own(String),
}

/// One of the Chaos Wheel's choices, a part of `WheelOptions` (`wheel_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelPick {
    On(bool),
    Seconds(u32),
    Warn(u32),
    /// A rate by its place in `mvs_mutators::ChaosRate::ALL`.
    Chaos(u8),
    Two(bool),
    /// The pool's quick picks: every mutator, the Mild ones, all but the Chaos ones.
    All,
    Mild,
    NoChaos,
    /// One mutator of `mvs_mutators::ALL`, ticked or unticked.
    Mutator(usize),
}

/// A drop menu entry: a lobby option's value, or a Chaos Wheel choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Value(OptionValue),
    Wheel(WheelPick),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub key: OptionKey,
    pub label: Words,
    /// The values in the order the drop menu lists them, with their names.
    pub values: Vec<(Pick, Words)>,
    /// A checklist (the mutators, the wheel's pool): the values are sets of one, ticked or not; the empty set (the
    /// quick picks) sets the whole list.
    pub multi: bool,
}

#[derive(Clone, Debug)]
pub struct SettingsPage {
    pub title: Words,
    pub rows: Vec<Row>,
    /// The art on the left: the lobby's mode art (None), or a texture of its own.
    pub art: Option<&'static str>,
}

fn on_off() -> Vec<(Pick, Words)> {
    vec![
        (Pick::Value(OptionValue::Bool(false)), Words::Own(text::OFF.into())),
        (Pick::Value(OptionValue::Bool(true)), Words::Own(text::ON.into())),
    ]
}

fn own(s: impl Into<String>) -> Words {
    Words::Own(s.into())
}

/// The Chaos Wheel's page: on or off, the window, the warning, the Chaos events' rate, two at once, and the pool (a
/// checklist of `mvs_mutators::ALL` under its quick picks, each named with its tier).
pub fn wheel_page() -> SettingsPage {
    let w = |p: WheelPick, s: Words| (Pick::Wheel(p), s);
    let row = |label: &str, values: Vec<(Pick, Words)>, multi: bool| Row {
        key: OptionKey::Wheel,
        label: own(label),
        values,
        multi,
    };
    let mut pool = vec![
        w(WheelPick::All, own(text::POOL_ALL)),
        w(WheelPick::Mild, own(text::POOL_MILD)),
        w(WheelPick::NoChaos, own(text::POOL_NO_CHAOS)),
    ];
    for (i, m) in mvs_mutators::ALL.iter().enumerate() {
        pool.push(w(WheelPick::Mutator(i), own(format!("{} ({})", text::wheel_name(m.key), text::tier_name(m.tier)))));
    }
    SettingsPage {
        title: own(text::CHAOS_WHEEL),
        rows: vec![
            row(
                text::CHAOS_WHEEL,
                vec![w(WheelPick::On(false), own(text::OFF)), w(WheelPick::On(true), own(text::ON))],
                false,
            ),
            row(
                text::WHEEL_TIME,
                WHEEL_SECONDS_CHOICES.iter().map(|&s| w(WheelPick::Seconds(s), own(text::seconds(s)))).collect(),
                false,
            ),
            row(
                text::WHEEL_WARNING,
                WHEEL_WARN_CHOICES.iter().map(|&s| w(WheelPick::Warn(s), own(text::seconds(s)))).collect(),
                false,
            ),
            row(
                text::WHEEL_EVENTS,
                (0u8..4).map(|r| w(WheelPick::Chaos(r), own(text::WHEEL_RATES[usize::from(r)]))).collect(),
                false,
            ),
            row(
                text::WHEEL_TWO,
                vec![w(WheelPick::Two(false), own(text::OFF)), w(WheelPick::Two(true), own(text::ON))],
                false,
            ),
            row(text::WHEEL_POOL, pool, true),
        ],
        art: Some("GameMode_AXHard_BG"),
    }
}

/// The pool's left-out bits of every mutator whose tier `keep` refuses.
fn off_where(keep: impl Fn(mvs_mutators::Tier) -> bool) -> u64 {
    mvs_mutators::ALL.iter().enumerate().filter(|(_, m)| !keep(m.tier)).fold(0, |o, (i, _)| o | (1 << i))
}

/// The options after the choice `p`; None where it is refused (a pool left empty).
pub fn wheel_after(o: WheelOptions, p: WheelPick) -> Option<WheelOptions> {
    use mvs_mutators::Tier;
    let n = match p {
        WheelPick::On(on) => WheelOptions { on, ..o },
        WheelPick::Seconds(seconds) => WheelOptions { seconds, ..o },
        WheelPick::Warn(warn) => WheelOptions { warn, ..o },
        WheelPick::Chaos(chaos) => WheelOptions { chaos, ..o },
        WheelPick::Two(two) => WheelOptions { two, ..o },
        WheelPick::All => WheelOptions { off: 0, ..o },
        WheelPick::Mild => WheelOptions { off: off_where(|t| t == Tier::Mild), ..o },
        WheelPick::NoChaos => WheelOptions { off: off_where(|t| t != Tier::Chaos), ..o },
        WheelPick::Mutator(i) => o.with(i, !o.has(i)),
    };
    n.valid().then_some(n)
}

/// A wheel choice's id in its drop menu.
fn wheel_id(p: WheelPick) -> String {
    match p {
        WheelPick::On(b) => format!("on{}", u8::from(b)),
        WheelPick::Seconds(s) => format!("s{s}"),
        WheelPick::Warn(s) => format!("w{s}"),
        WheelPick::Chaos(r) => format!("c{r}"),
        WheelPick::Two(b) => format!("t{}", u8::from(b)),
        WheelPick::All => "all".into(),
        WheelPick::Mild => "mild".into(),
        WheelPick::NoChaos => "nochaos".into(),
        WheelPick::Mutator(i) => format!("m{i}"),
    }
}

fn pick_id(p: Pick) -> String {
    match p {
        Pick::Value(v) => option_text(v),
        Pick::Wheel(w) => wheel_id(w),
    }
}

/// The choice a single wheel row shows for `o`, by the row's kind (its first entry).
fn wheel_now(kind: WheelPick, o: WheelOptions) -> WheelPick {
    match kind {
        WheelPick::On(_) => WheelPick::On(o.on),
        WheelPick::Seconds(_) => WheelPick::Seconds(o.seconds),
        WheelPick::Warn(_) => WheelPick::Warn(o.warn),
        WheelPick::Chaos(_) => WheelPick::Chaos(o.chaos),
        WheelPick::Two(_) => WheelPick::Two(o.two),
        other => other,
    }
}

/// The pool checklist's label: All, the one mutator's name, or the count.
fn pool_label(o: WheelOptions) -> String {
    let n = (0..mvs_mutators::ALL.len()).filter(|&i| o.has(i)).count();
    match n {
        _ if o.off == 0 => text::POOL_ALL.to_string(),
        1 => (0..mvs_mutators::ALL.len())
            .find(|&i| o.has(i))
            .map_or_else(String::new, |i| text::wheel_name(mvs_mutators::ALL[i].key).to_string()),
        n => text::mutator_count(n),
    }
}

/// The pool checklist's ticked ids: each mutator in it.
fn pool_ticked(o: WheelOptions) -> Vec<String> {
    (0..mvs_mutators::ALL.len()).filter(|&i| o.has(i)).map(|i| wheel_id(WheelPick::Mutator(i))).collect()
}

/// The room's page.
pub fn room_page() -> SettingsPage {
    SettingsPage {
        title: Words::Game("2EFD913A40EFFF6563B156AE62312528"), // Settings
        rows: vec![
            Row {
                key: OptionKey::Public,
                label: Words::Game("C51E56ED4E673B922BDE2AAFB938F8F2"), // Privacy
                values: vec![
                    (Pick::Value(OptionValue::Bool(false)), Words::Game("1164D2E24901624F6C0BF48A216689B8")), // Private
                    (Pick::Value(OptionValue::Bool(true)), Words::Game("37F7D5A94886770D20288BA8D91BCB3D")),  // Public
                ],
                multi: false,
            },
            // (the port's own) only the players a member invited may join (invitedOnly)
            Row {
                key: OptionKey::InvitedOnly,
                label: Words::Own(text::INVITED_ONLY.into()),
                values: on_off(),
                multi: false,
            },
        ],
        art: None,
    }
}

/// The mutators' checklist: Off (the empty set), then each of LOBBY_MUTATORS.
fn mutators_row() -> Row {
    Row {
        key: OptionKey::Mutators,
        label: Words::Own(text::MUTATOR.into()),
        values: text::MUTATORS
            .iter()
            .enumerate()
            .map(|(i, t)| (Pick::Value(OptionValue::Set(MutatorSet::default().with(i, true))), Words::Own((*t).into())))
            .collect(),
        multi: true,
    }
}

/// The experimental rules, the port's own (not in the beta); the art is the arcade's hard card background. The
/// Chaos Wheel has a page of its own (`wheel_page`).
pub fn experimental_page() -> SettingsPage {
    SettingsPage {
        title: Words::Own(text::EXPERIMENTAL_TITLE.into()),
        rows: vec![
            Row { key: OptionKey::Stocks, label: Words::Own(text::STOCK_MODE.into()), values: on_off(), multi: false },
            mutators_row(),
        ],
        art: Some("GameMode_AXHard_BG"),
    }
}

/// The Lab's experimental rules: the mutators and the Chaos Wheel's rows (the Lab has no stocks).
pub fn lab_experimental_page() -> SettingsPage {
    let mut rows = vec![mutators_row()];
    rows.extend(wheel_page().rows);
    SettingsPage { rows, ..experimental_page() }
}

/// The set after a click on a checklist row's `value`: Off clears it, a mutator is ticked or unticked.
pub fn toggled(set: MutatorSet, value: MutatorSet) -> MutatorSet {
    if value.is_empty() { MutatorSet::default() } else { MutatorSet(set.0 ^ value.0) }
}

/// A checklist's label: Off, the one mutator's name, or the count.
pub fn mutators_label(set: MutatorSet) -> String {
    match set.len() {
        0 => text::MUTATORS[0].to_string(),
        1 => set.indexes().next().map_or_else(String::new, |n| text::MUTATORS[n].to_string()),
        n => text::mutator_count(n),
    }
}

/// A checklist's ticked rows' ids: Off for the empty set, else each mutator's.
fn ticked_ids(set: MutatorSet) -> Vec<String> {
    if set.is_empty() {
        return vec![option_text(OptionValue::Set(set))];
    }
    set.indexes().map(|n| option_text(OptionValue::Set(MutatorSet::default().with(n, true)))).collect()
}

/// What the modal reads of the lobby (`Lobby.amILeader()`, `teamStyle`, `options`).
#[derive(Clone, Copy, Debug)]
pub struct LobbyView {
    pub leader: bool,
    /// The mode art's ETeamStyle (0 1v1, 1 Teams, 2 Free For All).
    pub team_style: u8,
    pub options: LobbyOptions,
}

/// The leader's request of a row (the lobby's `setOption`).
pub type OptionRequest = (OptionKey, OptionValue);

const LONG_LIST: usize = 8;
const BUILD_ROWS: [&str; 4] = ["UI_DropMenu_TeamStyle", "UI_DropMenu_Ringouts", "UI_DropMenu_Time", "Toggle_Hazards"];

const H_DONE: u32 = 1;
const H_CLOSE: u32 = 2;
/// Row `i`'s drop menu click is `H_ROW + i`.
const H_ROW: u32 = 16;

/// A row's drop menu.
#[derive(Debug)]
struct MenuRow {
    key: OptionKey,
    menu: WidgetId,
    values: Vec<Pick>,
    multi: bool,
}

#[derive(Debug)]
pub struct CustomGameSettings {
    base: ModalBase,
    view: Option<LobbyView>,
    page: SettingsPage,
    menus: Vec<MenuRow>,
    /// (the port's own) the leader's change of an option, for the lobby screen.
    pub on_option: Event<OptionRequest>,
}

fn option_text(v: OptionValue) -> String {
    match v {
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Set(m) => m.0.to_string(),
        // (a wheel row's entries are `Pick::Wheel`, each with its own id)
        OptionValue::Wheel(_) => String::new(),
    }
}

impl CustomGameSettings {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CustomGameSettings {
            base: ModalBase::default(),
            view: None,
            page: room_page(),
            menus: Vec::new(),
            on_option: Event::default(),
        })
    }

    fn words(cx: &Cx<'_>, w: &Words) -> String {
        match w {
            Words::Game(k) => cx.view.game_text(k).to_string(),
            Words::Own(s) => s.clone(),
        }
    }

    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let t = cx.view.tree();
        vec![
            find(cx, "contextInteractor"),
            t.find_in(find(cx, "buttonDone"), "UI_Interactor"),
            t.find_in(find(cx, "buttonClose"), "UI_Interactor"),
        ]
    }

    /// The page's rows, made once (`bind`).
    fn make_rows(&mut self, cx: &mut Cx<'_>) {
        let like = find(cx, "UI_DropMenu_TeamStyle");
        let (parent, user, pad, h_align, v_align, fill, like_props) = {
            let t = cx.view.tree();
            let l = t.widget(like);
            (
                l.parent,
                t.user_class(like).map(str::to_string),
                l.pad,
                l.h_align,
                l.v_align,
                l.fill,
                t.props(like).clone(),
            )
        };
        let (Some(parent), Some(user)) = (parent, user) else { return };
        let done = find(cx, "buttonDone");
        let mut at = cx.view.tree().widget(parent).children.iter().position(|c| *c == done);
        let ctx = self.base.context.clone();
        for r in self.page.rows.clone() {
            let w = crate::menu::view::MenuView::create(cx, &user);
            let items: Vec<Value> = r
                .values
                .iter()
                .map(|(v, t)| json!({ P_LABEL: { "CultureInvariantString": Self::words(cx, t) }, P_ID: pick_id(*v) }))
                .collect();
            let n = items.len();
            let mut props = like_props.clone();
            if let Value::Object(o) = &mut props {
                o.insert("LabelText".into(), json!({ "CultureInvariantString": Self::words(cx, &r.label) }));
                o.insert("TopTooltipText".into(), Value::Null);
                o.insert("Items".into(), Value::Array(items));
                // a long list (the mutators) scrolls past LONG_LIST rows, by the drop menu's own scroll bar properties
                if n > LONG_LIST {
                    o.insert("EnableScrollBar".into(), Value::Bool(true));
                    o.insert("ShowScrollBarAfterCount".into(), json!(LONG_LIST));
                }
                for (k, v) in o.clone() {
                    cx.view.tree_mut().set_prop(w, &k, v);
                }
            }
            {
                let x = wd(cx, w);
                x.name = format!("Option_{:?}", r.key);
                x.pad = pad;
                x.h_align = h_align;
                x.v_align = v_align;
                x.fill = fill;
            }
            crate::menu::view::MenuView::add_child(cx, parent, w, at);
            at = at.map(|a| a + 1);
            cx.visit::<DropMenu, _>(w, |m, cx| m.initialize(cx));
            let me = cx.w;
            let row = self.menus.len() as u32;
            cx.visit::<DropMenu, _>(w, |m, _| m.on_click.add(Handler { owner: me, id: H_ROW + row }));
            self.menus.push(MenuRow {
                key: r.key,
                menu: w,
                values: r.values.iter().map(|(v, _)| *v).collect(),
                multi: r.multi,
            });
            cx.visit_dyn(w, |_, cx| DropMenu::update_context(cx, &ctx, Some(0)));
        }
    }

    /// The lobby whose settings these are, and the page of them shown.
    pub fn bind(&mut self, cx: &mut Cx<'_>, view: LobbyView, page: SettingsPage) {
        self.view = Some(view);
        if self.menus.is_empty() {
            self.page = page;
            let title = find(cx, "TextBlock_37");
            let t = Self::words(cx, &self.page.title);
            wd(cx, title).text = t;
            self.make_rows(cx);
            // a page's own art is no tree's texture: held while the modal is up
            if let Some(art) = self.page.art {
                cx.view.hold_pictures(&self.base.modal_id, [art]);
            }
        }
        self.refresh(cx, Some(view));
    }

    /// The rows again from the lobby's options; interactable for its leader alone.
    pub fn refresh(&mut self, cx: &mut Cx<'_>, view: Option<LobbyView>) {
        if view.is_some() {
            self.view = view;
        }
        let Some(v) = self.view else { return };
        for m in &self.menus {
            let value = v.options.get(m.key);
            let main = cx.view.tree().find_in(m.menu, "mainInteractor");
            match (value, m.values.first()) {
                (OptionValue::Wheel(o), _) if m.multi => {
                    let (ticked, label) = (pool_ticked(o), pool_label(o));
                    cx.visit::<DropMenu, _>(m.menu, |d, cx| d.set_checked(cx, ticked, &label));
                }
                (OptionValue::Wheel(o), Some(&Pick::Wheel(kind))) => {
                    let key = wheel_id(wheel_now(kind, o));
                    cx.visit::<DropMenu, _>(m.menu, |d, cx| d.set_active_by_id(cx, &key));
                }
                (OptionValue::Set(set), _) if m.multi => {
                    let (ticked, label) = (ticked_ids(set), mutators_label(set));
                    cx.visit::<DropMenu, _>(m.menu, |d, cx| d.set_checked(cx, ticked, &label));
                }
                _ => {
                    let key = option_text(value);
                    cx.visit::<DropMenu, _>(m.menu, |d, cx| d.set_active_by_id(cx, &key));
                }
            }
            interactor::set_interactable(cx, main, v.leader);
            if !v.leader {
                cx.visit::<DropMenu, _>(m.menu, |d, cx| d.close(cx));
            }
        }
        let style = usize::from(v.team_style).min(3);
        let art = self.page.art;
        let (bg, fg) = (find(cx, "TeamStyle_BG"), find(cx, "TeamStyle_FG"));
        wd(cx, bg).tex = art.or(MODE_BG[style]).map(str::to_string);
        // (an image with no texture draws white: a page's own art hides the mode's colour blend)
        let f = wd(cx, fg);
        f.vis = if art.is_some() { Vis::Hidden } else { Vis::SelfHitTestInvisible };
        if art.is_none() {
            f.tex = MODE_FG[style].map(str::to_string);
        }
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
        for m in &self.menus {
            cx.visit_dyn(m.menu, |_, cx| DropMenu::update_context(cx, ctx, Some(0)));
        }
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.update_context(cx, ctx));
    }
}

impl Modal for CustomGameSettings {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let title = find(cx, "TextBlock_37");
        let t = Self::words(cx, &self.page.title);
        wd(cx, title).text = t;
        let me = cx.w;
        cx.view.play(me, "EnterAnim", false, 1.0);
        let guid = cx.view.manager.new_guid();
        self.update_context(cx, &guid);
        let cap = find(cx, "UI_CapturePlayersFocus");
        cx.visit::<CapturePlayersFocus, _>(cap, |c, cx| c.focus(cx));
        let ctx = find(cx, "contextInteractor");
        interactor::set_focused(cx, ctx, true);
        self.refresh(cx, None);
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.pre_dismiss(cx);
    }

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.base.post_dismiss(cx);
    }

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for CustomGameSettings {
    /// Port: the 2560 x 1440 SizeBox_0 under the ScaleBox is widened or heightened to the page's aspect (fill_page,
    /// as the other full-screen menus), so the blur, the dimmer and the art reach the page's edges.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let b = find(cx, "SizeBox_0");
        fill_page(cx, b);
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        for n in BUILD_ROWS {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        let me = cx.w;
        let done = find(cx, "buttonDone");
        cx.visit::<ButtonMain, _>(done, |b, _| b.on_click.add(Handler { owner: me, id: H_DONE }));
        let close = find(cx, "buttonClose");
        cx.visit::<IconButton, _>(close, |b, _| b.on_click.add(Handler { owner: me, id: H_CLOSE }));
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if self.page.art.is_some() {
            cx.view.release_pictures(&self.base.modal_id);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_DONE | H_CLOSE => self.request_dismiss(cx, "", false),
            id if id >= H_ROW => {
                let Some(row) = self.menus.get((id - H_ROW) as usize) else { return };
                let Some((i, _)) = arg.downcast_ref::<(i32, String)>() else { return };
                let i = *i;
                let leader = self.view.is_some_and(|v| v.leader);
                if leader
                    && i >= 0
                    && let Some(&value) = row.values.get(i as usize)
                {
                    let key = row.key;
                    let value = match (value, self.view.map(|v| v.options.get(key))) {
                        (Pick::Value(OptionValue::Set(click)), Some(OptionValue::Set(now))) if row.multi => {
                            OptionValue::Set(toggled(now, click))
                        }
                        (Pick::Wheel(p), Some(OptionValue::Wheel(now))) => {
                            let Some(o) = wheel_after(now, p) else { return };
                            OptionValue::Wheel(o)
                        }
                        (Pick::Value(v), _) => v,
                        (Pick::Wheel(_), _) => return,
                    };
                    // (the page's copy takes the change at once: a second click before the lobby's answer toggles
                    // from it)
                    if let Some(v) = &mut self.view {
                        v.options.set(key, value);
                    }
                    cx.emit(self, |c| &c.on_option, &(key, value));
                }
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_experimental_page_lists_every_mutator() {
        let p = experimental_page();
        assert_eq!(p.rows[1].values.len(), mvs_protocol::lobby_rules::MUTATOR_CHOICES);
        assert_eq!(p.rows.iter().map(|r| r.key).collect::<Vec<_>>(), [OptionKey::Stocks, OptionKey::Mutators]);
        assert_eq!(p.art, Some("GameMode_AXHard_BG"));
        assert_eq!(room_page().rows.len(), 2);
        let lab = lab_experimental_page();
        assert_eq!(lab.rows.len(), 1 + wheel_page().rows.len());
        assert!(lab.rows[0].multi && lab.rows[0].key == OptionKey::Mutators);
    }

    #[test]
    fn the_wheel_page_sets_each_choice_and_keeps_a_pool() {
        let p = wheel_page();
        assert_eq!(p.rows.len(), 6);
        assert!(p.rows.iter().all(|r| r.key == OptionKey::Wheel));
        assert_eq!(p.rows[5].values.len(), 3 + mvs_mutators::ALL.len());
        let o = WheelOptions::default();
        let o = wheel_after(o, WheelPick::On(true)).expect("on");
        let o = wheel_after(o, WheelPick::Seconds(45)).expect("a window");
        let o = wheel_after(o, WheelPick::Chaos(0)).expect("a rate");
        assert_eq!((o.on, o.seconds, o.rate()), (true, 45, mvs_mutators::ChaosRate::Off));
        assert_eq!(wheel_id(wheel_now(WheelPick::Seconds(10), o)), "s45");
        // the quick picks and a single mutator
        let mild = wheel_after(o, WheelPick::Mild).expect("mild");
        assert!(mvs_mutators::ALL.iter().enumerate().all(|(i, m)| mild.has(i) == (m.tier == mvs_mutators::Tier::Mild)));
        let one = wheel_after(mild, WheelPick::Mutator(1)).expect("one out");
        assert!(!one.has(1));
        assert_eq!(pool_label(wheel_after(one, WheelPick::All).expect("all")), text::POOL_ALL);
        // the last mutator cannot be unticked
        let last = (1..mvs_mutators::ALL.len()).fold(o, |o, i| o.with(i, false));
        assert_eq!(pool_label(last), text::wheel_name(mvs_mutators::ALL[0].key));
        assert_eq!(wheel_after(last, WheelPick::Mutator(0)), None);
    }

    #[test]
    fn a_checklist_click_ticks_unticks_or_clears() {
        let one = |n| MutatorSet::default().with(n, true);
        let s = toggled(toggled(MutatorSet::default(), one(2)), one(6));
        assert_eq!(s, MutatorSet((1 << 2) | (1 << 6)));
        assert_eq!(mutators_label(s), text::mutator_count(2));
        assert_eq!(ticked_ids(s), ["4", "64"]);
        let s = toggled(s, one(2));
        assert_eq!(mutators_label(s), text::MUTATORS[6]);
        assert_eq!(toggled(s, MutatorSet::default()), MutatorSet::default());
        assert_eq!(mutators_label(MutatorSet::default()), text::MUTATORS[0]);
        assert_eq!(ticked_ids(MutatorSet::default()), ["0"]);
    }

    #[test]
    fn option_values_name_the_menu_ids() {
        assert_eq!(option_text(OptionValue::Bool(true)), "true");
        assert_eq!(option_text(OptionValue::Set(MutatorSet::default().with(3, true))), "8");
    }
}
