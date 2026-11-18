//! The promotions box of the home screen: UI_PromoTickerSmall and
//! UI_PromoTickerItemSmall; the large promotions modal its click opens (UI_PromoTickerLarge, from the Blueprint) and
//! the UI_PromoParent children it shows.

use std::any::Any;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use serde_json::json;

use super::common::ModalBase;
use super::controls::bar_buttons::{P_ICON, P_ID, P_LABEL, P_TOOLTIP};
use super::controls::buttons::ButtonSecondary;
use super::controls::interactor::{FocusedCommon, InteractorState, batch_set_context, set_focused};
use super::controls::pips::PaginationPips;
use super::controls::{bind, find, wd};
use super::modals::show_modal_generic;
use super::nav::live;
use super::section_bar::SectionBar;
use super::social::SocialPanel;
use crate::bundle::{Bundle, WData};
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{Manager, Modal};
use crate::menu::scroll::{ScrollDestination, scroll_into_view};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// What UI_PromoTickerItemSmall reads of a Promotion (the struct's other fields are not read by the two classes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Promotion {
    pub slug: String,
    /// A UI_PromoParent child class by name ("" = none).
    pub pre_determined_widget: String,
    pub title_small: String,
    pub small_image_url: String,
}

/// The part of the page's width one thumbnail covers: the box's sizeContainer, 500 of the 1920 wide layout (the mip
/// a hold loads before any draw).
pub const PROMO_TILE: f64 = 500.0 / 1920.0;

/// The menu bundle's promotion class `cls`: its class defaults' Title and Thumbnail (a soft texture of the bundle).
fn promo_widget<'a>(b: &'a Bundle, cls: &str) -> Option<(&'a str, &'a str)> {
    let w = b.menu.as_ref()?.promo_widgets.get(cls)?;
    Some((w.get("title")?.as_str()?, w.get("thumbnail")?.as_str()?))
}

/// (the port's own) The thumbnails the main menu's promotions box draws (a promotion's PreDeterminedWidget class's
/// Thumbnail). Held before the main menu's first frame.
pub fn promo_pictures(b: &Bundle) -> Vec<String> {
    local_promotions(b)
        .iter()
        .filter_map(|p| promo_widget(b, &p.pre_determined_widget).map(|(_, t)| t.to_string()))
        .collect()
}

/// (the port's own) The promotions the ticker shows: three of the build's own UI_PromoParent children named as
/// PreDeterminedWidget, standing in for the backend's (the build waits for ClientHydraManager's PromotionsManager and
/// its GetCachedPromotions, which never come without the backend).
pub fn local_promotions(b: &Bundle) -> Vec<Promotion> {
    let Some(Value::Array(list)) = b.menu.as_ref().map(|m| &m.promotions) else { return Vec::new() };
    list.iter()
        .map(|p| Promotion {
            slug: p.get("slug").and_then(Value::as_str).unwrap_or_default().to_string(),
            pre_determined_widget: p.get("widget").and_then(Value::as_str).unwrap_or_default().to_string(),
            ..Promotion::default()
        })
        .collect()
}

fn warn_once(key: &str, what: &str) {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut w = W.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.insert(key.to_string()) {
        eprintln!("menu: {what}");
    }
}

/// A node of a class's tree in the bundle by name.
fn tree_node<'a>(n: &'a WData, name: &str) -> Option<&'a WData> {
    if n.n == name {
        return Some(n);
    }
    n.c.iter().find_map(|c| tree_node(c, name))
}

// ---------- UI_PromoTickerSmall ----------

const H_PAGE: u32 = 1;
const H_CLICK: u32 = 2;
const H_SWITCH_PAGE: u32 = 3;

/// UI_PromoTickerSmall.
#[derive(Debug)]
pub struct PromoTickerSmall {
    promotions: Vec<Promotion>,
    last_scale: f64,
    /// (the port's own, for the checks) The child index of the last ScrollWidgetIntoView.
    pub scrolled_to: i32,
}

impl PromoTickerSmall {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PromoTickerSmall { promotions: Vec::new(), last_scale: -1.0, scrolled_to: -1 })
    }

    /// Populate(Promotions) @3020: Promotions, scrollContainer cleared (the designer's placeholders go), per
    /// promotion a UI_PromoTickerItemSmall with Init(promotion, (sizeContainer.WidthOverride, HeightOverride)) added
    /// to scrollContainer, its slot centred both ways; then UI_PaginationPips.Init(count, 0) @3938 and the pips
    /// SelfHitTestInvisible with more than one, else Hidden (@4081). The items' ImageFailedEvent (-> OnImageFailed,
    /// which reloads the promotions) is not bound: no image is downloaded in the port.
    pub fn populate(&mut self, cx: &mut Cx<'_>, promotions: Vec<Promotion>) {
        let sc = find(cx, "scrollContainer");
        for c in cx.view.tree().widget(sc).children.clone() {
            MenuView::discard(cx, c);
        }
        // the SizeBox's WidthOverride / HeightOverride properties (500, 275; only the width's override is on, which
        // the read does not look at): nothing writes them at run time, so the class's tree has them
        let dims =
            cx.view.bundle.widgets.get("UI_PromoTickerSmall").and_then(|c| tree_node(&c.root, "sizeContainer")).map_or(
                [0.0, 0.0],
                |n| {
                    let f = |k| n.p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    [f("WidthOverride"), f("HeightOverride")]
                },
            );
        for p in &promotions {
            let w = MenuView::create(cx, "UI_PromoTickerItemSmall");
            cx.visit::<PromoTickerItemSmall, _>(w, |i, cx| i.init(cx, p.clone(), dims));
            MenuView::add_child(cx, sc, w, None);
            // SetHorizontalAlignment(2) / SetVerticalAlignment(2)
            let x = wd(cx, w);
            x.h_align = HAlign::Center;
            x.v_align = VAlign::Center;
        }
        let n = promotions.len() as i32;
        self.promotions = promotions;
        let pips = find(cx, "UI_PaginationPips");
        cx.visit::<PaginationPips, _>(pips, |p, cx| p.init(cx, n, 0));
        wd(cx, pips).vis = if n > 1 { Vis::SelfHitTestInvisible } else { Vis::Hidden };
    }

    /// CanPaginate?: no modal active, no dialog active, and FindOrCreateSocialPanel(false) (the newest
    /// UI_SocialPanel seen) not open.
    fn can_paginate(cx: &mut Cx<'_>) -> bool {
        if Manager::is_modal_active(cx.view) || Manager::is_dialog_active(cx.view) {
            return false;
        }
        let social = live(cx, "UI_SocialPanel").into_iter().rev().find(|&w| cx.view.visible_in_hierarchy(w));
        !social.is_some_and(|s| cx.visit::<SocialPanel, _>(s, |p, cx| p.is_open(cx)).unwrap_or(false))
    }

    /// UpdateContext @2745: BatchSetContext([UI_Interactor]), then the pips' UpdateContext (the home screen's
    /// Update Context calls it).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let i = find(cx, "UI_Interactor");
        batch_set_context(cx, &[i], ctx);
        let pips = find(cx, "UI_PaginationPips");
        pips_context(cx, pips, ctx);
    }
}

/// The pips' UpdateContext, run when the pips' own call returns if they are busy: a page change (the pips'
/// PageChanged) reaches the tickers' UpdateAppearance -> UpdateContext inside the pips' call, where the build's
/// graph calls them again at once (a re-entrant call the port's controllers do not take; same context string).
fn pips_context(cx: &mut Cx<'_>, pips: WidgetId, ctx: &str) {
    let ctx = ctx.to_string();
    cx.run(
        pips,
        Box::new(move |c, cx| {
            let any: &mut dyn Any = c;
            if let Some(p) = any.downcast_mut::<PaginationPips>() {
                p.update_context(cx, &ctx);
            }
        }),
    );
}

impl Controller for PromoTickerSmall {
    /// OnInitialized @2869: the focused common's interactor, then @1839 the wait for ClientHydraManager, its
    /// PromotionsManager and the PGI auth handler's resources (Delay 0 / 0.1 at a time), the delegates bound
    /// (@574-1017: the manager's OnRequestSwitchPage among them) and Populate(GetCachedPromotions) @1305 when it has
    /// any, else ReloadPromotions (LoadPromotionsFromGlobalConfig / GlobalMessages). The port binds the delegates and
    /// populates from the local list at once (local_promotions). The pips' PageChanged and the interactor's OnClick
    /// are component bound events.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let (fc, i) = (find(cx, "UI_FocusedCommon"), find(cx, "UI_Interactor"));
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        let pips = find(cx, "UI_PaginationPips");
        cx.visit::<PaginationPips, _>(pips, |p, _| p.on_page_changed.add(Handler { owner: me, id: H_PAGE }));
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, H_CLICK);
        cx.view.manager.on_request_switch_page.add(Handler { owner: me, id: H_SWITCH_PAGE });
        let list = local_promotions(&cx.view.bundle);
        self.populate(cx, list);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_request_switch_page.remove(Handler { owner: me, id: H_SWITCH_PAGE });
    }

    /// Tick @5422: timerBar's render scale (1 - UI_PaginationPips.AutoTimePercentage, 1). Written only when it
    /// changes.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let pips = find(cx, "UI_PaginationPips");
        let Some(p) = cx.visit::<PaginationPips, _>(pips, |p, cx| p.auto_time_percentage(cx)) else { return };
        let s = 1.0 - p;
        if s == self.last_scale {
            return;
        }
        self.last_scale = s;
        let bar = find(cx, "timerBar");
        wd(cx, bar).transform.scale[0] = s;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // the pips' PageChanged(Page Index) @2919: scrollContainer.ScrollWidgetIntoView(GetChildAt(index),
            // animated, EDescendantScrollDestination 0 = IntoView, padding 0). GetChildAt out of range is None:
            // nothing scrolls.
            H_PAGE => {
                let Some(&index) = arg.downcast_ref::<i32>() else { return };
                let sc = find(cx, "scrollContainer");
                let Some(&child) = usize::try_from(index).ok().and_then(|i| cx.view.tree().widget(sc).children.get(i))
                else {
                    return;
                };
                self.scrolled_to = index;
                scroll_into_view(cx.view.tree_mut(), sc, child, true, ScrollDestination::IntoView, 0.0);
            }
            // the interactor's OnClick @4776: with promotions, ShowLargePromoAtIndex(the pips' Active Index) @4925:
            // a valid index opens UI_PromoTickerLarge as a generic modal (promoTicker, DismissOthers false) @4995 and
            // Inits it with the list and the index @5236
            H_CLICK => {
                let pips = find(cx, "UI_PaginationPips");
                let i = cx.visit::<PaginationPips, _>(pips, |p, _| p.active_index).unwrap_or(-1);
                if i < 0 || i as usize >= self.promotions.len() {
                    return;
                }
                let Some(m) = show_modal_generic(cx, false, "UI_PromoTickerLarge", "promoTicker", "") else { return };
                let list = self.promotions.clone();
                cx.visit::<PromoTickerLarge, _>(m, |l, cx| l.init(cx, list, i));
            }
            // OnRequestSwitchPage(Direction, Player Index) @5343: CanPaginate? then the pips' IncrementPage(Direction)
            H_SWITCH_PAGE => {
                let Some(&(_, direction)) = arg.downcast_ref::<(i32, i32)>() else { return };
                if Self::can_paginate(cx) {
                    let pips = find(cx, "UI_PaginationPips");
                    cx.visit::<PaginationPips, _>(pips, |p, cx| p.increment_page(cx, direction));
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_PromoTickerItemSmall ----------

/// UI_PromoTickerItemSmall.
#[derive(Debug, Default)]
pub struct PromoTickerItemSmall {
    promotion: Promotion,
}

impl PromoTickerItemSmall {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PromoTickerItemSmall::default())
    }

    /// Init(Promotion, Dimensions) @135: Promotion, UpdateAppearance, sizeContainer's width and height overrides.
    pub fn init(&mut self, cx: &mut Cx<'_>, p: Promotion, dims: [f64; 2]) {
        self.promotion = p;
        self.update_appearance(cx);
        let s = find(cx, "sizeContainer");
        let s = wd(cx, s);
        s.w_override = dims[0];
        s.h_override = dims[1];
    }

    /// UpdateAppearance @333: with a PreDeterminedWidget, the image's loading look and LoadAssetClass of it (->
    /// OnLoaded @978); else the title from TitleSmall, the loading look and DownloadImage(SmallImageUrl) (@500),
    /// which the port cannot do (no backend): warned once, the item stays loading. A class that is not in the
    /// bundle's promo widgets is warned and stays loading (a class that does not load: the cast @1081 fails).
    /// Port: the class loads at once (the thumbnail is asked for and drawn when the renderer has it).
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let cls = self.promotion.pre_determined_widget.clone();
        if !cls.is_empty() {
            Self::toggle_image_loading(cx, true);
            let b = cx.view.bundle.clone();
            let Some((title, thumb)) = promo_widget(&b, &cls) else {
                warn_once(
                    &format!("promo {cls}"),
                    &format!("promo item: {cls} is not in the menu bundle's promo widgets"),
                );
                return;
            };
            cx.view.load_texture(thumb);
            Self::on_loaded(cx, &b, title, thumb);
            return;
        }
        let t = self.promotion.title_small.clone();
        Self::update_title(cx, &t);
        Self::toggle_image_loading(cx, true);
        warn_once("promodownload", "promo item: DownloadImage(SmallImageUrl) is not ported (no backend)");
    }

    /// OnLoaded @978: the class's object (SpawnObject, cast to UI_PromoParent): Update Title(its Title),
    /// thumbnail.SetBrushFromTexture(its Thumbnail, bMatchSize = SizeX != SizeY), the loading look off.
    fn on_loaded(cx: &mut Cx<'_>, b: &Bundle, title: &str, tex: &str) {
        Self::update_title(cx, title);
        let th = find(cx, "thumbnail");
        let info = b.textures.get(tex);
        let x = wd(cx, th);
        x.tex = Some(info.map_or(tex, |t| t.entry()).to_string());
        x.mid = None;
        if let Some(t) = info.filter(|t| t.w != t.h) {
            x.brush_w = f64::from(t.w);
            x.brush_h = f64::from(t.h);
        }
        Self::toggle_image_loading(cx, false);
    }

    /// ToggleImageLoading(Is Loading) @859: imageLoadingIndicator SelfHitTestInvisible / Hidden, thumbnail the
    /// other.
    fn toggle_image_loading(cx: &mut Cx<'_>, loading: bool) {
        let (on, off) = (Vis::SelfHitTestInvisible, Vis::Hidden);
        let i = find(cx, "imageLoadingIndicator");
        wd(cx, i).vis = if loading { on } else { off };
        let t = find(cx, "thumbnail");
        wd(cx, t).vis = if loading { off } else { on };
    }

    /// Update TItle @1475: titleLabel's text, Hidden when empty, else SelfHitTestInvisible.
    fn update_title(cx: &mut Cx<'_>, t: &str) {
        let l = find(cx, "titleLabel");
        let l = wd(cx, l);
        l.text = t.to_string();
        l.vis = if t.is_empty() { Vis::Hidden } else { Vis::SelfHitTestInvisible };
    }
}

impl Controller for PromoTickerItemSmall {}

// ---------- UI_PromoTickerLarge (the modal promoTicker) ----------

const L_BACK: u32 = 1;
const L_BAR_CLICK: u32 = 2;
const L_PAGE: u32 = 3;
const R_POST_SHOW: u32 = 1;

/// UI_PromoTickerLarge (a UI_ModalBase child): the promotions' titles on a section bar (Collapsed in its tree), the
/// pips and the current promotion's PreDeterminedWidget in ContentPanel. The image downloader (PreConstruct @4479,
/// OnImageReady), the analytics (Record UIFlow Event), the hydra promotion's look (@1558: title, description, the
/// Purchase, Link and DeepLink buttons and the downloaded image; every promotion of the port has a widget, so it
/// never shows) and those buttons' clicks are not ported.
#[derive(Debug, Default)]
pub struct PromoTickerLarge {
    base: ModalBase,
    promotions: Vec<Promotion>,
    current: i32,
    updating: bool,
    /// WidgetMap: each PreDeterminedWidget class's widget, made once.
    widgets: Vec<(String, WidgetId)>,
    /// PreDeterminedWidgets: the widgets in ContentPanel.
    shown: Vec<WidgetId>,
    section_bar_params: Vec<Value>,
}

impl PromoTickerLarge {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PromoTickerLarge>::default()
    }

    /// Init(Promotions, ActiveIndex) @1029: the list, UpdatingAppearance on, Current Index, then
    /// LoadAllSectionBarParams (its AllSectionBarParamsReady bound to OnAllSectionBarParamsReady @1094).
    pub fn init(&mut self, cx: &mut Cx<'_>, promotions: Vec<Promotion>, index: i32) {
        self.promotions = promotions;
        self.updating = true;
        self.current = index;
        self.load_all_section_bar_params(cx);
    }

    /// LoadAllSectionBarParams @4943: per promotion, with a PreDeterminedWidget, LoadAssetClass -> OnLoaded @5511:
    /// the class's widget made (Create), into WidgetMap, and a section bar button of its Title, the index as its ID
    /// (@5845-6045); else GetSectionBarParamFromPromo (its TitleSmall). Then AllSectionBarParamsReady @5432 ->
    /// OnAllSectionBarParamsReady @5461: the bar's Populate, SetCurrentIndex(Current Index) @4836, then @4176.
    // Deviation: the classes load at once (LoadAssetClass is a later frame in the build), so the modal's first
    // frame has them.
    fn load_all_section_bar_params(&mut self, cx: &mut Cx<'_>) {
        let b = cx.view.bundle.clone();
        for (i, p) in self.promotions.clone().iter().enumerate() {
            let cls = &p.pre_determined_widget;
            let label = if cls.is_empty() {
                p.title_small.clone()
            } else {
                // a class that is not a UI_PromoParent of the bundle: the cast @5692 fails and the loop stops there
                let Some((title, _)) = promo_widget(&b, cls).filter(|_| b.widgets.contains_key(cls.as_str())) else {
                    warn_once(&format!("promo {cls}"), &format!("promo ticker: {cls} is not in the menu bundle"));
                    return;
                };
                let w = MenuView::create(cx, cls);
                match self.widgets.iter_mut().find(|(k, _)| k == cls) {
                    Some(e) => e.1 = w,
                    None => self.widgets.push((cls.clone(), w)),
                }
                title.to_string()
            };
            self.section_bar_params.push(json!({
                P_LABEL: { "CultureInvariantString": label },
                P_ICON: null,
                P_ID: i.to_string(),
                P_TOOLTIP: { "CultureInvariantString": null },
            }));
        }
        let bar = find(cx, "sectionBarPromos");
        let params = self.section_bar_params.clone();
        let current = self.current;
        cx.visit::<SectionBar, _>(bar, |s, cx| {
            s.populate(cx, Some(params));
            s.set_current_index(cx, current);
        });
        self.after_bar(cx);
    }

    /// @4176: UpdateAppearance, the pips' Init(promotion count, Current Index) (its PageChanged runs
    /// UpdateAppearance again, UpdatingAppearance being off by then), the pips SelfHitTestInvisible with more than
    /// one promotion, else Hidden (@4396), then UpdateContext(Context).
    fn after_bar(&mut self, cx: &mut Cx<'_>) {
        self.update_appearance(cx);
        let n = self.promotions.len() as i32;
        let pips = find(cx, "UI_PaginationPips");
        let current = self.current;
        cx.visit::<PaginationPips, _>(pips, |p, cx| p.init(cx, n, current));
        wd(cx, pips).vis = if n > 1 { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
    }

    /// UpdateAppearance @3004: RemoveAllPreDeterminedWidgets; the current promotion's widget from WidgetMap
    /// (HydraData Collapsed, the loading look on then off): into PreDeterminedWidgets and ContentPanel (its slot
    /// HAlign Fill @3654), Visible, UpdateContext(Context), and its PostShow("", null) when it is a modal
    /// (BPI_ModalInterface) @3980. Then UpdatingAppearance off (@4164).
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        self.remove_all_pre_determined_widgets(cx);
        let Some(p) = usize::try_from(self.current).ok().and_then(|i| self.promotions.get(i)).cloned() else {
            // GetCurrentData of an index out of range: Array_Get's default Promotion, no widget
            warn_once("promohydra", "promo ticker: a promotion without a widget (the hydra look) is not ported");
            self.updating = false;
            return;
        };
        if p.pre_determined_widget.is_empty() {
            warn_once("promohydra", "promo ticker: a promotion without a widget (the hydra look) is not ported");
            self.updating = false;
            return;
        }
        let hd = find(cx, "HydraData");
        wd(cx, hd).vis = Vis::Collapsed;
        Self::toggle_image_loading(cx, true);
        // (not in the map: LoadAssetClass -> @6331, which makes it and goes on at @3364; the map has every class
        // load_all_section_bar_params made)
        if let Some(&(_, w)) = self.widgets.iter().find(|(k, _)| *k == p.pre_determined_widget) {
            self.shown.push(w);
            Self::toggle_image_loading(cx, false);
            let panel = find(cx, "ContentPanel");
            MenuView::add_child(cx, panel, w, None);
            // Unverified: AddChildToOverlay's slot keeps UOverlaySlot's defaults (VAlign Top, no padding: the 4.26
            // source) but the HAlign @3654
            let x = wd(cx, w);
            x.h_align = HAlign::Fill;
            x.v_align = VAlign::Top;
            x.pad = Margin::default();
            x.vis = Vis::Visible;
            let ctx = self.base.context.clone();
            self.update_context(cx, &ctx);
            cx.visit_dyn(w, |c, cx| {
                if let Some(m) = c.as_modal() {
                    m.post_show(cx, "", "");
                }
            });
        }
        self.updating = false;
    }

    /// RemoveAllPreDeterminedWidgets: from the last, each Collapsed, its RequestDismiss("", false) when it is a
    /// modal, RemoveFromParent; the list cleared.
    fn remove_all_pre_determined_widgets(&mut self, cx: &mut Cx<'_>) {
        for w in std::mem::take(&mut self.shown).into_iter().rev() {
            wd(cx, w).vis = Vis::Collapsed;
            cx.visit_dyn(w, |c, cx| {
                if let Some(m) = c.as_modal() {
                    m.request_dismiss(cx, "", false);
                }
            });
            MenuView::remove_child(cx, w);
        }
    }

    /// ToggleImageLoading(Is Loading) @2757: imageLoadingIndicator SelfHitTestInvisible / Hidden, promoImage the
    /// other.
    fn toggle_image_loading(cx: &mut Cx<'_>, loading: bool) {
        let (on, off) = (Vis::SelfHitTestInvisible, Vis::Hidden);
        let i = find(cx, "imageLoadingIndicator");
        wd(cx, i).vis = if loading { on } else { off };
        let p = find(cx, "promoImage");
        wd(cx, p).vis = if loading { off } else { on };
    }

    /// UpdateContext(Context) @187: Context; BatchSetContext(GetInteractors: buttonBack's, Btn_Link's,
    /// Btn_Purchase's and Btn_DeepLink's interactors and nullTarget), the bar's UpdateContext(Context, 0), the
    /// pips' UpdateContext, and each shown widget's UpdatePromoContext (UI_PromoParent: its Promo Context, which
    /// nothing of the ported classes reads).
    fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let mut list: Vec<WidgetId> = ["buttonBack", "Btn_Link", "Btn_Purchase", "Btn_DeepLink"]
            .iter()
            .map(|n| {
                let b = find(cx, n);
                cx.view.tree().find_in(b, "UI_Interactor")
            })
            .collect();
        list.push(find(cx, "nullTarget"));
        batch_set_context(cx, &list, ctx);
        let bar = find(cx, "sectionBarPromos");
        cx.visit::<SectionBar, _>(bar, |s, cx| s.update_context(cx, ctx, Some(0)));
        let pips = find(cx, "UI_PaginationPips");
        pips_context(cx, pips, ctx);
    }
}

impl Modal for PromoTickerLarge {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @53: UpdateContext(NewGuid), then Delay 0.2 -> nullTarget.SetFocused(false) @15.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        cx.view.delay(0.2, Handler { owner: cx.w, id: R_POST_SHOW });
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @826: DismissModal(Modal ID) (the analytics' Record UIFlow Dismissal Event is not ported).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for PromoTickerLarge {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// The component bound events: buttonBack's OnClick, the bar's OnClick and the pips' PageChanged (buttonBack's
    /// OnFocus @6330 does nothing).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let back = find(cx, "buttonBack");
        cx.visit::<ButtonSecondary, _>(back, |b, _| b.on_click.add(Handler { owner: me, id: L_BACK }));
        let bar = find(cx, "sectionBarPromos");
        cx.visit::<SectionBar, _>(bar, |s, _| s.on_click.add(Handler { owner: me, id: L_BAR_CLICK }));
        let pips = find(cx, "UI_PaginationPips");
        cx.visit::<PaginationPips, _>(pips, |p, _| p.on_page_changed.add(Handler { owner: me, id: L_PAGE }));
    }

    /// Destruct @4886: RemoveAllPreDeterminedWidgets, WidgetMap cleared.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.remove_all_pre_determined_widgets(cx);
        self.widgets.clear();
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == R_POST_SHOW {
            let n = find(cx, "nullTarget");
            set_focused(cx, n, false);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // buttonBack's OnClick @2876: RemoveAllPreDeterminedWidgets, RequestDismiss("", false), WidgetMap
            // cleared, ui_menu_global_click_close
            L_BACK => {
                self.remove_all_pre_determined_widgets(cx);
                self.request_dismiss(cx, "", false);
                self.widgets.clear();
                cx.post("ui_menu_global_click_close");
            }
            // the bar's OnClick(Index) @1326 and the pips' PageChanged(Page Index) @6283: while UpdatingAppearance
            // nothing, else Current Index and UpdateAppearance
            L_BAR_CLICK => {
                let Some(&(index, _)) = arg.downcast_ref::<(i32, String)>() else { return };
                if !self.updating {
                    self.current = index;
                    self.update_appearance(cx);
                }
            }
            L_PAGE => {
                let Some(&index) = arg.downcast_ref::<i32>() else { return };
                if !self.updating {
                    self.current = index;
                    self.update_appearance(cx);
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_PromoParent's children ----------

/// The PreDeterminedWidget classes of the port's promotions (UI_PromoParent children). UI_Promo_Cromulons and
/// UI_Promo_BatmanDay implement BPI_ModalInterface: PostShow plays AnimIn (at 1.5 and 1.0, @86) and RequestDismiss
/// -> Exit -> DismissModal(Modal ID) (@157), which no PreShow of the large ticker has set: no modal of an empty ID,
/// nothing. UI_ScoobyMansionEventPromo_Shaggy is no modal: its Construct plays AnimIn at 1.0 (@10).
#[derive(Debug, Default)]
pub struct PromoParent {
    base: ModalBase,
    /// PostShow's AnimIn speed (a modal), else None.
    post_show_speed: Option<f64>,
}

impl PromoParent {
    pub fn make_cromulons(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PromoParent { post_show_speed: Some(1.5), ..PromoParent::default() })
    }

    pub fn make_batman_day(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PromoParent { post_show_speed: Some(1.0), ..PromoParent::default() })
    }

    pub fn make_shaggy(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PromoParent>::default()
    }
}

impl Modal for PromoParent {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        if let Some(speed) = self.post_show_speed {
            let me = cx.w;
            cx.view.play(me, "AnimIn", false, speed);
        }
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool) {
        self.base.request_dismiss(cx, reason, from_cancel);
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for PromoParent {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        if self.post_show_speed.is_some() { Some(self) } else { None }
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        if self.post_show_speed.is_none() {
            let me = cx.w;
            cx.view.play(me, "AnimIn", false, 1.0);
        }
    }
}
