//! The Gleamium shop (UI_CurrencyShop, the modal `currencyShop`) and its promo.
//! Opened by the shop's get-more button (shop.rs) and by the sidebar's currency labels (nav.rs). With no backend it
//! shows its loading cover and keeps it (Unverified): the offers, purchases and the Founder's Pack are dead.

use std::any::Any;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use super::common::{ModalBase, text};
use super::controls::buttons::IconButton;
use super::controls::interactor::{InteractorState, batch_set_context, set_focused};
use super::controls::{bind, find, focus_frame, prop, wd};
use super::nav::GlobalSlideOut;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::Modal;
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Vis, WidgetId, resolve};
use crate::text::Justify;

fn warn_once(id: &str, what: &str) {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut w = W.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.insert(id.to_string()) {
        eprintln!("menu: {what} is not ported (no backend)");
    }
}

/// FMath::InterpEaseInOut (UKismetMathLibrary::Ease, EEasingFunc 7).
fn ease_in_out(a: f64, exp: f64) -> f64 {
    if a < 0.5 { 0.5 * (2.0 * a).powf(exp) } else { 1.0 - 0.5 * (2.0 * (1.0 - a)).powf(exp) }
}

const H_EXIT: u32 = 1;
const R_AFTER_OPEN: u32 = 1;
const R_FADE: u32 = 2;
const R_DISMISS: u32 = 3;

/// UI_CurrencyShop. Not ported: the GleamiumPurchased binding (a purchase path), the offers, the Founder's Pack page.
#[derive(Debug, Default)]
pub struct CurrencyShop {
    base: ModalBase,
    /// EventClosed.
    pub event_closed: Event<()>,
    closed: bool,
    fade_start: f64,
}

impl CurrencyShop {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CurrencyShop::default())
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }

    fn promo(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_CurrencyShopPromo")
    }

    fn exit(cx: &Cx<'_>) -> WidgetId {
        find(cx, "buttonExit")
    }

    fn clear_offers(cx: &mut Cx<'_>) {
        let c = find(cx, "OffersContainer");
        for k in cx.view.tree().widget(c).children.clone() {
            MenuView::discard(cx, k);
        }
    }

    /// @1600: Repopulate Offers, the exit's context, then the promo's 0.3 s fade (@1673 -> @33, a Delay(0) per frame).
    fn after_open(&mut self, cx: &mut Cx<'_>) {
        self.repopulate_offers(cx);
        let e = cx.view.tree().find_in(Self::exit(cx), "UI_Interactor");
        set_focused(cx, e, true);
        self.fade_start = cx.now();
        self.fade(cx);
    }

    /// @33-461: alpha = clamp((game time - start) / 0.3, 0, 1); opacity = Ease(0, 1, alpha, EaseInOut, 2).
    fn fade(&mut self, cx: &mut Cx<'_>) {
        let a = ((cx.now() - self.fade_start) / 0.3).clamp(0.0, 1.0);
        let promo = Self::promo(cx);
        if a < 1.0 {
            wd(cx, promo).opacity = ease_in_out(a, 2.0);
            cx.view.delay(0.0, Handler { owner: cx.w, id: R_FADE });
        } else {
            wd(cx, promo).opacity = 1.0;
        }
    }

    /// Repopulate Offers @16414: ToggleLoading(true), ClearChildren, LoadOffers(OnOffersLoaded) (native). Unverified:
    /// with no backend LoadOffers is taken never to answer, so the cover stays.
    fn repopulate_offers(&mut self, cx: &mut Cx<'_>) {
        self.toggle_loading(cx, true);
        Self::clear_offers(cx);
    }

    /// ToggleLoading: LoadingIndicator Visible or Hidden. The cover (Visible, after buttonExit) takes the mouse as in
    /// the build: the exit works by its focus or Escape, and the slide-out's dismiss area closes the shop too.
    fn toggle_loading(&mut self, cx: &mut Cx<'_>, on: bool) {
        let l = find(cx, "LoadingIndicator");
        wd(cx, l).vis = if on { Vis::Visible } else { Vis::Hidden };
    }

    /// UpdateContext @5956 (GetInteractors: the offers' interactors and the exit's; the promo is not among them). The
    /// footer's alignment follows the platform (2 or 3: right); GetLocalPlatform is neither here: centred.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let e = cx.view.tree().find_in(Self::exit(cx), "UI_Interactor");
        batch_set_context(cx, &[e], ctx);
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.update_context(cx, ctx));
        let hb = find(cx, "HorizontalBox_0");
        wd(cx, hb).h_align = HAlign::Center;
        let tb = find(cx, "TextBlock");
        wd(cx, tb).justify = Justify::Center;
    }

    /// SetSlideOutPreviousContext(Context, Enable Full Screen) @0: the slide-out's SetPreviousContext, then
    /// EnableFullScreen (CanvasRoot's padding: left 0 when full, else 127).
    pub fn set_slide_out_previous_context(&mut self, cx: &mut Cx<'_>, ctx: &str, full_screen: bool) {
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, _| s.set_previous_context(ctx));
        GlobalSlideOut::set_global_nav_available(cx, so, !full_screen);
    }

    /// PostDismiss and Destruct -> @2077: once, ShopExit (native) and EventClosed @3277.
    fn close_tail(&mut self, cx: &mut Cx<'_>) {
        if self.closed {
            return;
        }
        self.closed = true;
        cx.emit(self, |c| &c.event_closed, &());
    }
}

impl Modal for CurrencyShop {
    /// PreShow @6864: the five design-time offers go, the promo starts transparent. Unverified: the slide-out's
    /// SlideoutLoaded is not in the port's GlobalSlideOut; its EnforceLoadingWait branch is not read.
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
        Self::clear_offers(cx);
        let promo = Self::promo(cx);
        wd(cx, promo).opacity = 0.0;
    }

    /// PostShow @7001. Not ported: @7276 ShowModal(UI_FoundersPackPage, "founderpack", "promo") (Deviation); @5653 ->
    /// @3351 ShopEnter(Context) (native CurrencyShopScreenWidget, nothing to do here).
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        let id = self.base.modal_id.clone();
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.open_slide_out(cx, &id));
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        if instructions != "founderpack" {
            cx.view.delay(0.1, Handler { owner: cx.w, id: R_AFTER_OPEN });
        } else {
            warn_once("founderpack", "UI_FoundersPackPage");
        }
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, cx: &mut Cx<'_>) {
        self.close_tail(cx);
    }

    /// RequestDismiss @7451: Delay(0) -> @2004 CloseSlideOut.
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        cx.view.delay(0.0, Handler { owner: cx.w, id: R_DISMISS });
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for CurrencyShop {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// The buttonExit OnClick binding @17987 -> @15: RequestDismiss("", true).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let e = Self::exit(cx);
        cx.visit::<IconButton, _>(e, |b, _| b.on_click.add(Handler { owner: me, id: H_EXIT }));
    }

    /// PreConstruct @17408: ClipBounds 2100 x 0.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let so = Self::slide_out(cx);
        let clip = cx.view.tree().find_in(so, "Overlay_ClipBounds");
        let o = &mut wd(cx, clip).offsets;
        o.r = 2100.0;
        o.b = 0.0;
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.close_tail(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == H_EXIT {
            self.request_dismiss(cx, "", true);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            R_AFTER_OPEN => self.after_open(cx),
            R_FADE => self.fade(cx),
            R_DISMISS => {
                let id = self.base.modal_id.clone();
                let so = Self::slide_out(cx);
                cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.close_slide_out(cx, &id));
            }
            _ => {}
        }
    }
}

const H_PROMO_FOCUS: u32 = 1;
const H_PROMO_UNFOCUS: u32 = 2;
const H_PROMO_CLICK: u32 = 3;

/// UI_CurrencyShopPromo: the Founder's Pack banner. Its instance on UI_CurrencyShop is disabled (bIsEnabled false) and
/// Hidden, so it is never seen nor hit: the interactors under a disabled widget take no input (the interactor's
/// inherited enable, `register_with_manager`).
#[derive(Debug, Default)]
pub struct CurrencyShopPromo;

impl CurrencyShopPromo {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CurrencyShopPromo)
    }

    fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }
}

impl Controller for CurrencyShopPromo {
    /// OnInitialized @477: FocusedCommon.SetInteractor; the interactor's focus / unfocus play OnFocus forward / back at
    /// 2x (@164, @532); its click @379 opens the target modal (UI_FoundersPackPage, dead).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_PROMO_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_PROMO_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, H_PROMO_CLICK);
    }

    /// PreConstruct @212.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let promo_text = text(cx.view.tree().props(cx.w).get("Promotion Text"));
        let button_text = text(cx.view.tree().props(cx.w).get("Button Text"));
        let image = prop(cx, "Promotion Image").as_str().filter(|s| !s.is_empty()).map(|n| resolve(&cx.view.bundle, n));
        let t = find(cx, "TextBlock_75");
        wd(cx, t).text = promo_text;
        if let Some(tex) = image {
            let bg = find(cx, "BG");
            wd(cx, bg).tex = Some(tex);
        }
        let b = find(cx, "buttonBuy");
        let l = cx.view.tree().find_in(b, "label");
        wd(cx, l).text = button_text;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_PROMO_FOCUS => cx.view.play(cx.w, "OnFocus", false, 1.0),
            H_PROMO_UNFOCUS => cx.view.play(cx.w, "OnFocus", true, 2.0),
            H_PROMO_CLICK => {
                let id =
                    cx.view.tree().props(cx.w).get("Promotion Modal Id").and_then(Value::as_str).map(str::to_string);
                warn_once("promo", &format!("the promotion's {} modal", id.unwrap_or_default()));
            }
            _ => {}
        }
    }
}
