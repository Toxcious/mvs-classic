//! UI_PaginationPips and UI_PaginationPip.

use std::any::Any;

use serde_json::Value;

use super::buttons::ButtonSecondary;
use super::interactor::{batch_set_context, set_context, set_interactable};
use super::{bind, find, prop, prop_bool, prop_f64, wd};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::NULL_GUID;
use crate::menu::view::MenuView;
use crate::slate::{Margin, Vis, WidgetId};

/// FMath::Clamp (Kismet Clamp): Min when X < Min, else the smaller of X and Max (so Max wins when Max < Min).
fn clamp_int(x: i32, lo: i32, hi: i32) -> i32 {
    if x < lo {
        lo
    } else if x < hi {
        x
    } else {
        hi
    }
}

/// UI_Functions WrapInt: integer + increment, then below min -> max, then above max -> min.
fn wrap_int(v: i32, inc: i32, lo: i32, hi: i32) -> i32 {
    let mut r = v + inc;
    if r < lo {
        r = hi;
    }
    if r > hi {
        r = lo;
    }
    r
}

const H_LEFT: u32 = 1;
const H_RIGHT: u32 = 2;
const H_SWITCH_PAGE: u32 = 3;
const H_PIP_CLICKED: u32 = 4;

/// A page indicator: a pip per page (a "n/total" label past Max Pips), the Left / Right arrow buttons and an auto
/// timer that turns the page. Its owner reads `on_page_changed`, `active_index` and `auto_time_percentage`.
#[derive(Debug)]
pub struct PaginationPips {
    pub on_page_changed: Event<i32>,
    pub active_index: i32,
    pub page_count: i32,
    /// GetRealTimeSeconds at the last Update Auto Timer (only differences are read).
    pub last_change_time: f64,
    context: String,
    audio_spam_blocker: i32,
    /// Auto Timer Handle: seconds to the timer's run, < 0 = not set.
    timer: f64,
}

impl PaginationPips {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PaginationPips {
            on_page_changed: Event::default(),
            active_index: 0,
            page_count: 0,
            last_change_time: 0.0,
            context: NULL_GUID.to_string(),
            audio_spam_blocker: 0,
            timer: -1.0,
        })
    }

    fn auto_timer(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "Auto Timer", -1.0)
    }

    fn max_pips(cx: &Cx<'_>) -> i32 {
        prop_f64(cx, "Max Pips", 100.0) as i32
    }

    /// Init(Page Count, Active Page Index) @2827: Page Count, the pips rebuilt (@1976), then SetActiveIndex.
    pub fn init(&mut self, cx: &mut Cx<'_>, page_count: i32, active: i32) {
        self.page_count = page_count;
        self.rebuild(cx);
        self.set_active_index(cx, active);
    }

    /// @1976: up to Max Pips, the pips container shown (SelfHitTestInvisible), the label Collapsed, the container
    /// cleared and a UI_PaginationPip per page added (padding 5 / 0 / 5 / 0, Index, interactable by Clickable Pips,
    /// OnClick -> OnPipClicked when clickable), then UpdateContext(Context) @1952; past Max Pips the container
    /// Collapsed and the label SelfHitTestInvisible.
    fn rebuild(&mut self, cx: &mut Cx<'_>) {
        let (boxw, label) = (find(cx, "pipsContainer"), find(cx, "pageLabel"));
        if self.page_count > Self::max_pips(cx) {
            wd(cx, boxw).vis = Vis::Collapsed;
            wd(cx, label).vis = Vis::SelfHitTestInvisible;
            return;
        }
        wd(cx, boxw).vis = Vis::SelfHitTestInvisible;
        wd(cx, label).vis = Vis::Collapsed;
        for c in cx.view.tree().widget(boxw).children.clone() {
            MenuView::discard(cx, c);
        }
        let clickable = prop_bool(cx, "Clickable Pips");
        let me = cx.w;
        for i in 0..self.page_count {
            let w = MenuView::create(cx, "UI_PaginationPip");
            MenuView::add_child(cx, boxw, w, None);
            wd(cx, w).pad = Margin { l: 5.0, t: 0.0, r: 5.0, b: 0.0 };
            let iw = cx.view.tree().find_in(w, "UI_Interactor");
            cx.visit::<PaginationPip, _>(w, |p, _| {
                p.index = i;
                if clickable {
                    p.on_click.add(Handler { owner: me, id: H_PIP_CLICKED });
                }
            });
            set_interactable(cx, iw, clickable);
        }
        let ctx = self.context.clone();
        self.update_context(cx, &ctx);
    }

    fn pips(cx: &Cx<'_>) -> Vec<WidgetId> {
        let b = find(cx, "pipsContainer");
        cx.view.tree().widget(b).children.clone()
    }

    /// SetActiveIndex @2889: Active Index = Clamp(index, 0, Page Count - 1), pageLabel "{current}/{total}"
    /// (current = Active Index + 1), each pip's SetActive(its place == Active Index) up to Max Pips (@110), then
    /// PageChanged(Active Index) and Update Auto Timer (@3573).
    pub fn set_active_index(&mut self, cx: &mut Cx<'_>, index: i32) {
        self.active_index = clamp_int(index, 0, self.page_count - 1);
        let l = find(cx, "pageLabel");
        wd(cx, l).text = format!("{}/{}", self.active_index + 1, self.page_count);
        if self.page_count <= Self::max_pips(cx) {
            let active = self.active_index;
            for (i, c) in Self::pips(cx).into_iter().enumerate() {
                // (a child that is not a pip is passed over)
                cx.visit::<PaginationPip, _>(c, |p, cx| p.set_active(cx, i as i32 == active));
            }
        }
        let a = self.active_index;
        cx.emit(self, |c| &c.on_page_changed, &a);
        self.update_auto_timer(cx);
    }

    /// IncrementPage(Direction) @3616: SetActiveIndex(WrapInt(Active Index, Direction, 0, Page Count - 1)), and
    /// ui_menu_global_hover_tab_switch_light when the result is not the last one that posted it (AudioSpamBlocker).
    pub fn increment_page(&mut self, cx: &mut Cx<'_>, direction: i32) {
        let r = wrap_int(self.active_index, direction, 0, self.page_count - 1);
        self.set_active_index(cx, r);
        if r == self.audio_spam_blocker {
            return;
        }
        self.audio_spam_blocker = r;
        cx.post("ui_menu_global_hover_tab_switch_light");
    }

    /// Update Auto Timer @3940: with Auto Timer > 0, Last Change Time = GetRealTimeSeconds and the timer set again
    /// (K2_SetTimerDelegate(OnAutoTimerChange, Auto Timer, not looping)).
    fn update_auto_timer(&mut self, cx: &mut Cx<'_>) {
        let t = Self::auto_timer(cx);
        if t > 0.0 {
            self.last_change_time = cx.view.now();
            self.timer = t;
        }
    }

    /// AutoTimePercentage: Clamp((GetRealTimeSeconds - Last Change Time) / Auto Timer, 0, 1), 0 with no Auto Timer.
    pub fn auto_time_percentage(&self, cx: &Cx<'_>) -> f64 {
        let t = Self::auto_timer(cx);
        if t > 0.0 { ((cx.view.now() - self.last_change_time) / t).clamp(0.0, 1.0) } else { 0.0 }
    }

    /// UpdateContext @2298: Context, then BatchSetContext(GetInteractors): Right's and Left's interactors, then each
    /// pip's.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let mut list = vec![
            cx.view.tree().find_in(find(cx, "Right"), "UI_Interactor"),
            cx.view.tree().find_in(find(cx, "Left"), "UI_Interactor"),
        ];
        for c in Self::pips(cx) {
            if cx.view.tree().user_class(c) == Some("UI_PaginationPip") {
                list.push(cx.view.tree().find_in(c, "UI_Interactor"));
            }
        }
        batch_set_context(cx, &list, ctx);
    }
}

impl Controller for PaginationPips {
    /// OnInitialized @2404: the manager's OnRequestSwitchPage; Animated Arrows plays the two callouts, looping
    /// (@15, NumLoopsToPlay 0). The arrows' OnClick: Left @3935 -> IncrementPage(-1), Right @3930 -> IncrementPage(1).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.context = prop(cx, "Context").as_str().map_or_else(|| NULL_GUID.to_string(), str::to_string);
        cx.view.manager.on_request_switch_page.add(Handler { owner: me, id: H_SWITCH_PAGE });
        if prop_bool(cx, "Animated Arrows") {
            cx.view.play_animation(me, "AnimCalloutLeft", false, 1.0, 0, false, false);
            cx.view.play_animation(me, "AnimCalloutRight", false, 1.0, 0, false, false);
        }
        let (l, r) = (find(cx, "Left"), find(cx, "Right"));
        cx.visit::<ButtonSecondary, _>(l, |b, _| b.on_click.add(Handler { owner: me, id: H_LEFT }));
        cx.visit::<ButtonSecondary, _>(r, |b, _| b.on_click.add(Handler { owner: me, id: H_RIGHT }));
    }

    /// PreConstruct @2822 -> @687: for Left and Right, Show Input Label = !Hide Input Labels and Conditional
    /// Visibility(Show Buttons, SelfHitTestInvisible, Collapsed). The button's own PreConstruct (@531) read the
    /// variable already (a child's PreConstruct runs before its owner's), so the write changes nothing seen on the
    /// first construction.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let hide = prop_bool(cx, "Hide Input Labels");
        let show = prop_bool(cx, "Show Buttons");
        for n in ["Left", "Right"] {
            let b = find(cx, n);
            cx.view.tree_mut().set_prop(b, "Show Input Label", Value::Bool(!hide));
            wd(cx, b).vis = if show { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        }
    }

    /// Destruct @4188: the auto timer cleared. (The manager's delegate is let go with the widget in the build; the
    /// port removes it here.)
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.timer = -1.0;
        cx.view.manager.on_request_switch_page.remove(Handler { owner: cx.w, id: H_SWITCH_PAGE });
    }

    /// The world timer: OnAutoTimerChange @4168 -> IncrementPage(1) once the time has passed.
    /// Unverified: the timer manager's tick is taken as this frame's delta; it runs while the widget is constructed,
    /// seen or not, and with a modal up (the build checks nothing there either).
    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if self.timer < 0.0 {
            return;
        }
        self.timer -= dt;
        if self.timer > 0.0 {
            return;
        }
        self.timer = -1.0;
        self.increment_page(cx, 1);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_LEFT => self.increment_page(cx, -1),
            H_RIGHT => self.increment_page(cx, 1),
            // OnPipClicked(Index) @4209
            H_PIP_CLICKED => {
                if let Some(&i) = arg.downcast_ref::<i32>() {
                    self.set_active_index(cx, i);
                }
            }
            // OnRequestSwitchPage(Player Index, Direction) @2529: another player's request is passed over when Player
            // Index >= 0; IncrementPage only with Show Buttons, no Hide Input Labels and no Manual Shortcut Navigation
            // (an owner with Manual Shortcut Navigation pages itself).
            H_SWITCH_PAGE => {
                let Some(&(player, direction)) = arg.downcast_ref::<(i32, i32)>() else { return };
                let own = prop_f64(cx, "Player Index", -1.0) as i32;
                if player != own && own >= 0 {
                    return;
                }
                if prop_bool(cx, "Show Buttons")
                    && !prop_bool(cx, "Hide Input Labels")
                    && !prop_bool(cx, "Manual Shortcut Navigation")
                {
                    self.increment_page(cx, direction);
                }
            }
            _ => {}
        }
    }
}

const P_CLICK: u32 = 1;

#[derive(Debug, Default)]
pub struct PaginationPip {
    pub on_click: Event<i32>,
    pub index: i32,
}

impl PaginationPip {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PaginationPip::default())
    }

    /// SetActive(Active) @25: StopAllAnimations, activeIndicator SelfHitTestInvisible / Hidden, container's render
    /// scale 1 / 0.5, and when active ActiveAnim played once (@296).
    pub fn set_active(&mut self, cx: &mut Cx<'_>, active: bool) {
        let me = cx.w;
        cx.view.stop_all_animations(me);
        let a = find(cx, "activeIndicator");
        wd(cx, a).vis = if active { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        let c = find(cx, "container");
        wd(cx, c).transform.scale = if active { [1.0, 1.0] } else { [0.5, 0.5] };
        if active {
            cx.view.play(me, "ActiveAnim", false, 1.0);
        }
    }

    /// UpdateContext @348 (the owner's BatchSetContext sets the interactor directly).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let i = find(cx, "UI_Interactor");
        set_context(cx, i, ctx);
    }
}

impl Controller for PaginationPip {
    /// The interactor's OnClick @402: OnClick(Index). Destruct @435 (StopAnimationsAndLatentActions): the view
    /// drops a destructed owner's animations.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_click, P_CLICK);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == P_CLICK {
            let i = self.index;
            cx.emit(self, |c| &c.on_click, &i);
        }
    }
}
