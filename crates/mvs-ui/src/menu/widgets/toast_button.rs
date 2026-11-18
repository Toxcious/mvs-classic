//! The podium's toast buttons: UI_EOG_ToastButton, the widget of BP_Banner_PM's
//! ToastButton component, and its root UI_ButtonToast, and the banners the end-of-game step "toast"
//! (UI_CollectionToasts BPI Start) shows them on.

use std::any::Any;

use super::common::text;
use super::controls::interactor::set_interactable;
use super::controls::{bind, find, quad_color, quad_wobble, wd};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::toasts::Toasts;
use crate::results::{PodiumPlayer, int_text};
use crate::slate::{Vis, WidgetId};

/// UWidget::IsVisible: Visible, HitTestInvisible or SelfHitTestInvisible.
fn is_visible(v: Vis) -> bool {
    v != Vis::Collapsed && v != Vis::Hidden
}

const TEAL: Rgba = [0.194618, 0.571125, 0.552012, 1.0];
const GREY: Rgba = [0.341915, 0.341915, 0.341915, 1.0];

// ButtonToast's handler ids.
const BT_FOCUS: u32 = 1;
const BT_UNFOCUS: u32 = 2;
const BT_CLICK: u32 = 3;
// EOGToastButton's handler ids.
const ET_CLICK: u32 = 1;
const ET_ON_TOASTED: u32 = 2;
const ET_ON_INVENTORY_UPDATED: u32 = 3;
const ET_INVENTORY_UPDATED: u32 = 4;

/// `f` on the controller of `b` as a ButtonToast, deferred while it is busy (a call from inside its own click).
fn with_button(cx: &mut Cx<'_>, b: WidgetId, f: impl FnOnce(&mut ButtonToast, &mut Cx<'_>) + 'static) {
    cx.run(
        b,
        Box::new(move |c, cx| {
            let any: &mut dyn Any = c;
            if let Some(t) = any.downcast_mut::<ButtonToast>() {
                f(t, cx);
            }
        }),
    );
}

/// UI_ButtonToast: the give button (HorizontalBox_Give: the label, "+1", the toast icon), the owner's count
/// (HorizontalBox_Me, never seen: IsOwningPlayer hides the whole widget) and the pressed look (HorizontalBox_Disabled,
/// "TOASTED").
/// Not ported: Construct's UI_Interactor Is3DInteractor? (the port's interactors have no 3D flag; the world widget is
/// hit-tested on the screen point it is drawn at, menu/view.rs).
#[derive(Debug, Default)]
pub struct ButtonToast {
    pub on_click: Event<()>,
    pub on_click_with_source: Event<WidgetId>,
    is_owner: bool,
    /// The click's DoOnce (never reset).
    closed: bool,
    /// The pressed look was taken (a click's or ClickFeedback's).
    /// Deviation: it keeps the interactor off: the build's SetInteractable(false) @869 holds until the server's
    /// InventoryUpdated, which the port's count change (a spend's, at once) would otherwise stand for, turning a
    /// TOASTED button on again.
    pub pressed: bool,
    /// (the port's own) a click is taken only while a toast is left after the spends under way (EOGToastButton); the
    /// build's stock goes down inside the click, so a second banner's click in the same round trip cannot be.
    pub gated: bool,
}

impl ButtonToast {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonToast::default())
    }

    pub fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }

    /// Init: the label and the interactor's id.
    pub fn init(&mut self, cx: &mut Cx<'_>, label: &str) {
        let l = find(cx, "label");
        wd(cx, l).text = label.to_string();
        let i = Self::interactor(cx);
        super::controls::set_id(cx, i, label);
    }

    /// OnClick @662: DoOnce, OnClick, OnClickWithSource, then the pressed look.
    fn click(&mut self, cx: &mut Cx<'_>) {
        let toasts = &cx.view.session;
        if self.closed
            || (self.gated && toasts.toasts.count() - i32::try_from(toasts.podium_toasts.pending).unwrap_or(0) <= 0)
        {
            return;
        }
        self.closed = true;
        cx.emit(self, |c| &c.on_click, &());
        let i = Self::interactor(cx);
        cx.emit(self, |c| &c.on_click_with_source, &i);
        self.pressed_look(cx);
    }

    /// ClickFeedback: the pressed look without the broadcasts (the DoOnce stays open).
    pub fn click_feedback(&mut self, cx: &mut Cx<'_>) {
        self.pressed_look(cx);
    }

    /// @755-869 then @15-289.
    fn pressed_look(&mut self, cx: &mut Cx<'_>) {
        self.pressed = true;
        let d = find(cx, "HorizontalBox_Disabled");
        wd(cx, d).vis = Vis::HitTestInvisible;
        let g = find(cx, "HorizontalBox_Give");
        wd(cx, g).vis = Vis::Collapsed;
        let s = find(cx, "shadow");
        wd(cx, s).vis = Vis::Collapsed;
        let i = Self::interactor(cx);
        set_interactable(cx, i, false);
        for q in ["QuadBacking", "QuadBorder"] {
            let q = find(cx, q);
            quad_color(cx, q, GREY);
        }
        let f = find(cx, "focusWobbler");
        wd(cx, f).vis = Vis::Hidden;
        quad_wobble(cx, f, false);
        cx.post("vo_announcer_toast");
    }

    /// IsOwningPlayer @1405-1546: the owner's look, and the widget Hidden.
    pub fn is_owning_player(&mut self, cx: &mut Cx<'_>) {
        let me = find(cx, "HorizontalBox_Me");
        wd(cx, me).vis = Vis::HitTestInvisible;
        let g = find(cx, "HorizontalBox_Give");
        wd(cx, g).vis = Vis::Collapsed;
        let s = find(cx, "shadow");
        wd(cx, s).vis = Vis::Collapsed;
        self.is_owner = true;
        let w = cx.w;
        wd(cx, w).vis = Vis::Hidden;
    }

    /// UpdateNumberOfToasts @1547-1780: no toast left or the owner's: Hidden; else Visible, Reset, interactable.
    pub fn update_number_of_toasts(&mut self, cx: &mut Cx<'_>, n: i32) {
        let l = find(cx, "label_3");
        wd(cx, l).text = int_text(i64::from(n));
        let w = cx.w;
        if n < 1 || self.is_owner {
            wd(cx, w).vis = Vis::Hidden;
            return;
        }
        wd(cx, w).vis = Vis::Visible;
        self.reset();
        let i = Self::interactor(cx);
        set_interactable(cx, i, !self.pressed);
    }

    /// Reset @1781: nothing.
    pub fn reset(&mut self) {}
}

impl Controller for ButtonToast {
    /// PreConstruct @304-661.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let hide = cx.view.tree().props(cx.w).get("HideBackground").and_then(|v| v.as_bool()).unwrap_or(false);
        let b = find(cx, "QuadBacking");
        wd(cx, b).opacity = if hide { 0.0 } else { 0.8 };
        quad_color(cx, b, TEAL);
        let border = find(cx, "QuadBorder");
        quad_color(cx, border, TEAL);
        let label = text(cx.view.tree().props(cx.w).get("buttonLabel"));
        if !label.is_empty() {
            self.init(cx, &label);
        }
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        bind(cx, i, |s| &mut s.on_focus, BT_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, BT_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, BT_CLICK);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            // OnFocus @927-1120, OnUnfocus @1121-1314: only while the shadow is visible (the give look)
            BT_FOCUS | BT_UNFOCUS => {
                let shadow = find(cx, "shadow");
                if !is_visible(wd(cx, shadow).vis) {
                    return;
                }
                let focus = id == BT_FOCUS;
                let f = find(cx, "focusWobbler");
                wd(cx, f).vis = if focus { Vis::SelfHitTestInvisible } else { Vis::Hidden };
                quad_wobble(cx, f, focus);
                if focus {
                    wd(cx, shadow).color = [0.0, 0.0, 0.0, 0.75];
                } else {
                    wd(cx, shadow).color = [0.0, 0.0, 0.0, 0.5];
                }
            }
            BT_CLICK => self.click(cx),
            _ => {}
        }
    }
}

/// UI_EOG_ToastButton: one podium banner's button. Populate (BP_Banner_PM BPI_ShowNametag) gives it the banner's
/// record; BPI_ShowToastButton (the EOG step "toast") puts it in the canvas's context. Its click gives a toast
/// (BPI_Toast) once; every button hears OnToasted (the toasted one takes the pressed look, and all go Hidden once no
/// toast is left) and the count's change (InventoryUpdated).
/// Not ported: the debug path (DebugModeEnabled, Debug_NumMatchToasts, ApplyToastAchievement) and the context menu's
/// checks (FollowEnabled?, InviteTo*Enabled?).
#[derive(Debug, Default)]
pub struct EOGToastButton {
    data: Option<PodiumPlayer>,
    is_me: bool,
    should_be_on: bool,
    /// The click's DoOnce (never reset).
    closed: bool,
    released: bool,
}

impl EOGToastButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(EOGToastButton::default())
    }

    fn button(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonToast_C_0")
    }

    /// Account ID: the record's player id. (the port's own) A bot or an offline player has none (''): it is told
    /// apart by its slot, as the build's bots by their own AccountIDs.
    fn account_id(&self) -> String {
        match &self.data {
            Some(d) if !d.info.id.is_empty() => d.info.id.clone(),
            Some(d) => format!("#{}", d.entry.player_index),
            None => String::new(),
        }
    }

    /// Populate @1144-1281: the record, then UpdateAppearance; your own banner's also hears InventoryUpdated and takes
    /// IsOwningPlayer (@1448-1814).
    pub fn populate(&mut self, cx: &mut Cx<'_>, data: PodiumPlayer, _your_team: bool, is_me: bool) {
        self.data = Some(data);
        self.is_me = is_me;
        self.update_appearance(cx);
        if !is_me {
            return;
        }
        let h = Handler { owner: cx.w, id: ET_INVENTORY_UPDATED };
        cx.view.session.toasts.on_changed.add(h);
        let n = cx.view.session.toasts.count();
        let b = Self::button(cx);
        with_button(cx, b, move |c, cx| {
            c.update_number_of_toasts(cx, n);
            c.is_owning_player(cx);
        });
    }

    /// ShowToastButton @1851-2017.
    pub fn show_toast_button(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let b = Self::button(cx);
        let i = ButtonToast::interactor_of(cx, b);
        super::controls::interactor::set_context(cx, i, ctx);
        set_interactable(cx, i, !self.is_me);
        self.should_be_on = true;
    }

    /// HideToastButton @2029-2088.
    fn hide_toast_button(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button(cx);
        let i = ButtonToast::interactor_of(cx, b);
        set_interactable(cx, i, false);
        self.should_be_on = false;
    }

    /// Out of the page's toast service (the results screen's end: its trees are not destructed).
    pub fn release(&mut self, cx: &mut Cx<'_>) {
        self.released = true;
        let w = cx.w;
        let s = &mut cx.view.session.toasts;
        s.on_toasted.remove_owner(w);
        s.on_changed.remove_owner(w);
    }

    /// UpdateAppearance @3627-3938: the interactor, OnToasted and InventoryUpdated bound, then the count (@2220-2912).
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        if self.released {
            return;
        }
        let b = Self::button(cx);
        let pressed = cx.view.controller::<ButtonToast>(b).is_some_and(|t| t.pressed);
        let i = ButtonToast::interactor_of(cx, b);
        set_interactable(cx, i, self.should_be_on && !self.is_me && !pressed);
        let w = cx.w;
        let s = &mut cx.view.session.toasts;
        s.on_toasted.add(Handler { owner: w, id: ET_ON_TOASTED });
        s.on_changed.add(Handler { owner: w, id: ET_ON_INVENTORY_UPDATED });
        self.update_count(cx);
    }

    /// @2220-2912: the count on the button; none left and not your own: HideToastButton.
    fn update_count(&mut self, cx: &mut Cx<'_>) {
        let n = cx.view.session.toasts.count();
        let b = Self::button(cx);
        with_button(cx, b, move |c, cx| c.update_number_of_toasts(cx, n));
        if n <= 0 && !self.is_me {
            self.hide_toast_button(cx);
        }
    }

    /// The button's OnClick @2100 -> @29-832: DoOnce, BPI_Toast(the username, AccountID, !IsBot?(AccountID)), then the
    /// count.
    /// Deviation: the build's BPI_Toast runs inside the click; a spend resolved later through the stats file
    /// would put UI_ButtonToast's pressed look before OnToasted's ClickFeedback. Here the spend is at
    /// once (`Toasts::toast`), the other buttons hear it before the pressed look of this one.
    fn click(&mut self, cx: &mut Cx<'_>) {
        let Some(d) = &self.data else { return };
        if self.closed {
            return;
        }
        self.closed = true;
        let valid = !d.info.bot && !d.info.id.is_empty();
        let id = self.account_id();
        cx.view.session.podium_toasts.pending += 1;
        Toasts::toast(cx, &id, valid);
        cx.view.session.podium_toasts.pending -= 1;
        if !self.released {
            let n = cx.view.session.toasts.count();
            let b = Self::button(cx);
            with_button(cx, b, move |c, cx| c.update_number_of_toasts(cx, n));
        }
    }
}

impl ButtonToast {
    /// The interactor of the button widget `b`.
    fn interactor_of(cx: &Cx<'_>, b: WidgetId) -> WidgetId {
        cx.view.tree().find_in(b, "UI_Interactor")
    }
}

impl Controller for EOGToastButton {
    /// OnInitialized @3959 and Construct @3943: UpdateAppearance.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let b = Self::button(cx);
        if let Some(t) = cx.view.controller::<ButtonToast>(b) {
            t.gated = true;
            t.on_click.add(Handler { owner: cx.w, id: ET_CLICK });
        }
        self.update_appearance(cx);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.update_appearance(cx);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.release(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            ET_CLICK => self.click(cx),
            // OnInventoryUpdated @3317: Reset, UpdateAppearance
            ET_ON_INVENTORY_UPDATED => self.update_appearance(cx),
            // InventoryUpdated @3368 (your own banner's)
            ET_INVENTORY_UPDATED => self.update_count_only(cx),
            // OnToasted @2105: the count, then the toasted banner's ClickFeedback (@2927-2975)
            ET_ON_TOASTED => {
                self.update_count(cx);
                if arg.downcast_ref::<String>().is_some_and(|s| *s == self.account_id()) {
                    let b = Self::button(cx);
                    with_button(cx, b, |c, cx| c.click_feedback(cx));
                }
            }
            _ => {}
        }
    }
}

impl EOGToastButton {
    fn update_count_only(&mut self, cx: &mut Cx<'_>) {
        let n = cx.view.session.toasts.count();
        let b = Self::button(cx);
        with_button(cx, b, move |c, cx| c.update_number_of_toasts(cx, n));
    }
}

/// The podium's banners (GetAllActorsWithInterface(BPI_Podium) of UI_CollectionToasts BPI Start @1751-2174): each
/// one's BPI_ShowToastButton (BAN @6449-6590: SetHiddenInGame(false), then ShowToastButton). The results screen adds
/// a banner when its nametag shows.
/// Unverified: a banner added after the step has run is shown at once (the build's banner actors are there from the
/// start, so the step reaches every one; whether a nametag can show after the step is not read).
#[derive(Debug, Default)]
pub struct PodiumToasts {
    banners: Vec<WidgetId>,
    shown_context: Option<String>,
    /// The toasts being spent (EOGToastButton clicks whose toast() has not returned).
    pub pending: u32,
    /// The banners shown since the screen last took them (addPodiumToastBanner's `shown` callback).
    shown: Vec<WidgetId>,
}

impl PodiumToasts {
    /// The banners shown since the last call, by their EOGToastButton's widget.
    pub fn take_shown(&mut self) -> Vec<WidgetId> {
        std::mem::take(&mut self.shown)
    }
}

/// The results screen's start and end: the podium's buttons leave the toast service (the screen's trees are not
/// destructed when it goes).
pub fn reset_podium_toasts(cx: &mut Cx<'_>) {
    let banners = std::mem::take(&mut cx.view.session.podium_toasts.banners);
    for b in banners {
        cx.visit::<EOGToastButton, _>(b, |c, cx| c.release(cx));
    }
    let p = &mut cx.view.session.podium_toasts;
    p.shown_context = None;
    p.shown.clear();
    p.pending = 0;
}

/// A banner's button; shown at once when the step has run.
pub fn add_podium_toast_banner(cx: &mut Cx<'_>, button: WidgetId) {
    cx.view.session.podium_toasts.banners.push(button);
    if let Some(ctx) = cx.view.session.podium_toasts.shown_context.clone() {
        show_banner(cx, button, &ctx);
    }
}

fn show_banner(cx: &mut Cx<'_>, b: WidgetId, ctx: &str) {
    cx.view.session.podium_toasts.shown.push(b);
    cx.visit::<EOGToastButton, _>(b, |c, cx| c.show_toast_button(cx, ctx));
}

/// UI_CollectionToasts BPI Start's showPodiumToastButtons.
pub fn show_podium_toast_buttons(cx: &mut Cx<'_>, ctx: &str) {
    cx.view.session.podium_toasts.shown_context = Some(ctx.to_string());
    for b in cx.view.session.podium_toasts.banners.clone() {
        show_banner(cx, b, ctx);
    }
}
