//! The shop's details modals: UI_ContextualUnlockModalCharacter (modal `ItemDetails`), UI_WWContextualUnlockModalBundle
//! (modal `BundleDetails`) and their UI_WWCostCalculation. Nothing can be bought: the balance is 0 in every currency, so
//! the build's own Insufficient Funds state shows, and the stand-in's PurchaseProduct does nothing
//! (shop_products.rs).

use std::any::Any;

use super::common::ModalBase;
use super::controls::buttons::{ButtonMain, ButtonSecondary};
use super::controls::interactor::{batch_set_context, set_focused, set_interactable};
use super::controls::{find, quad_color, wd};
use super::currency_shop::CurrencyShop;
use super::shop_cells::{
    PriceLabel, ProductGrid, catalogue, fighter_name, grouped, rarity_color, roster_of, set_brush, shop_brush,
    type_text,
};
use super::shop_products::{
    CURRENCY_GEMS, ShopProduct, associated_bundles, bundle_products_array, client_balance, fetch_product_from_slug,
    is_owned, purchase_product, rarity_byte,
};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::{Manager, Modal};
use crate::slate::{Vis, WidgetId, resolve};

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

const LIGHT: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const RED: Rgba = [0.904661, 0.0, 0.066626, 1.0];

/// UI_Functions CurrencyDisplayName's text of a currency (empty when unknown).
// Unverified: UI_Functions' bytecode is not in the extract; "Gleamium" (the text key the currency shop's name has) is
// taken for gems, and no other currency is priced in the port's shop.
fn currency_display_name(cx: &Cx<'_>, c: u8) -> String {
    if c == CURRENCY_GEMS { cx.view.game_text("C246C9D94D58CD1A8BB18E94B115B0DA").to_string() } else { String::new() }
}

// ---------- UI_WWCostCalculation ----------

/// UI_WWCostCalculation: the balance, the cost and what remains. Not ported: the discount ribbons (no sales), the
/// Inventory Updated binding (the balance never changes), the battle pass layout, Get Product Details' DisplayError for a
/// product not priced in the currency (its price stays 0).
#[derive(Debug, Default)]
pub struct CostCalculation {
    pub currency: u8,
    base_price: f64,
    effective_price: f64,
    current_balance: f64,
    remaining_balance: f64,
    is_bundle: bool,
}

impl CostCalculation {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<CostCalculation>::default()
    }

    /// Init(Product, Currency Type, Is Battle Pass) @3465 -> Get Product Details @3423, the balance @3254 (Remaining =
    /// Balance - EffectivePrice), UpdateAppearance @15 then @1139.
    pub fn init(&mut self, cx: &mut Cx<'_>, p: &ShopProduct, currency: u8) {
        self.currency = currency;
        let price = p.cost(currency).unwrap_or(0.0);
        self.base_price = price;
        self.effective_price = price;
        self.is_bundle = p.bundle_products.len() > 1;
        self.current_balance = client_balance(currency);
        self.remaining_balance = self.current_balance - self.effective_price;
        self.update_appearance(cx);
    }

    /// Insufficient Funds?: Remaining Balance < 0.
    pub fn insufficient_funds(&self) -> bool {
        self.remaining_balance < 0.0
    }

    fn price(cx: &mut Cx<'_>, label: &str, amount: f64, currency: u8) {
        let w = find(cx, label);
        cx.visit::<PriceLabel, _>(w, |l, cx| l.set_price(cx, amount, currency, false));
    }

    /// @15-1138, then @1139-2780.
    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let c = self.currency;
        Self::price(cx, "CurrentAmountLabel", self.current_balance, c);
        Self::price(cx, "remainingPrice", self.remaining_balance, c);
        Self::price(cx, "ValueAmountLabel", 0.0, c);
        Self::price(cx, "OriginalAmountLabel", -self.base_price, c);
        Self::price(cx, "FinalAmountLabel", -self.effective_price, c);
        self.set_appearance_for_pricing(cx);
        self.set_icon(cx);
        wd(cx, cx.w).vis = Vis::Visible;
        let short = self.insufficient_funds();
        let col = if short { RED } else { LIGHT };
        let r = find(cx, "remainingPrice");
        cx.visit::<PriceLabel, _>(r, |l, cx| l.change_color(cx, col));
        let rl = find(cx, "remainingLabel");
        wd(cx, rl).color = col;
        let status = if short {
            let name = currency_display_name(cx, c);
            cx.view.game_text("3D64D5F04BF5885F316EF5A07A927A09").replace("{currency}", &name)
        } else {
            String::new()
        };
        let s = find(cx, "statusLabel");
        let x = wd(cx, s);
        x.color = col;
        x.text = status;
    }

    /// SetAppearanceForPricing with no sale and no bundle savings: the value row Collapsed; a bundle keeps the original
    /// row as "Bundle Cost" with the final row Collapsed, any other product the final row as "Cost" with the original
    /// row Collapsed; the discount ribbons Collapsed.
    fn set_appearance_for_pricing(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "valueRow", Vis::Collapsed);
        set_vis(cx, "OriginalDiscountRibbon", Vis::Collapsed);
        set_vis(cx, "FinalDiscountRibbon", Vis::Collapsed);
        if self.is_bundle {
            let l = find(cx, "originalLabel");
            wd(cx, l).text = cx.view.game_text("909AD5DC4809DE76664B11A2ECAEBB9D").to_string();
            set_vis(cx, "finalRow", Vis::Collapsed);
        } else {
            set_vis(cx, "originalRow", Vis::Collapsed);
            let l = find(cx, "finalLabel");
            wd(cx, l).text = cx.view.game_text("AE1C112E403F9530E4EB829F2A0A93CD").to_string();
        }
    }

    /// SetIcon: Icon Factor 1.6 on the current and remaining labels (1.9 for currency 3).
    fn set_icon(&mut self, cx: &mut Cx<'_>) {
        let f = if self.currency == 3 { 1.9 } else { 1.6 };
        for n in ["CurrentAmountLabel", "remainingPrice"] {
            let w = find(cx, n);
            cx.visit::<PriceLabel, _>(w, |l, cx| l.set_icon_factor(cx, f));
        }
    }
}

impl Controller for CostCalculation {}

// ---------- the details modals' shared parts ----------

const H_DISMISSER: u32 = 1;
const H_CONFIRM: u32 = 2;
const H_HARD_CURRENCY: u32 = 3;
const H_RELATED: u32 = 4;
const R_CONFIRM_READY: u32 = 1;
const R_EXITED: u32 = 2;
const R_COST_READY: u32 = 3;

/// Exit(false): OnClose at 1.25x, then OnExitAnimFinished -> DismissModal(Modal ID).
fn exit(cx: &mut Cx<'_>, exiting: &mut bool) {
    if *exiting {
        return;
    }
    *exiting = true;
    let me = cx.w;
    let class = cx.view.tree().user_class(me).unwrap_or_default().to_string();
    let len = cx.view.anim_length(&class, "OnClose") / 1.25;
    cx.view.play(me, "OnClose", false, 1.25);
    cx.view.delay(len, Handler { owner: me, id: R_EXITED });
}

/// Btn_PurchaseHardCurrency: ShowModal(UI_CurrencyShop, "currencyShop"), SetSlideOutPreviousContext(Context, true).
fn open_currency_shop(cx: &mut Cx<'_>, ctx: &str) {
    if let Some(m) = Manager::show_modal(cx, "UI_CurrencyShop", "currencyShop", "", "") {
        let ctx = ctx.to_string();
        cx.visit::<CurrencyShop, _>(m, |s, cx| s.set_slide_out_previous_context(cx, &ctx, true));
    }
}

fn bind_main(cx: &mut Cx<'_>, name: &str, id: u32) {
    let me = cx.w;
    let b = find(cx, name);
    cx.visit::<ButtonMain, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
}

fn bind_secondary(cx: &mut Cx<'_>, name: &str, id: u32) {
    let me = cx.w;
    let b = find(cx, name);
    cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(Handler { owner: me, id }));
}

// ---------- UI_ContextualUnlockModalCharacter, modal `ItemDetails` ----------

/// ERewardRarity's texts of the modal's Rarity label (@25084-25295).
const MODAL_RARITY: [&str; 5] = [
    "",
    "94DD1F874406FD5FF22032A9C0CAC215",
    "AC9179804DEAC0A9AB936B891906B17E",
    "0C8EC20F4D15D8463B310FB7165D9F3A",
    "D8A5CA3A41EB0C8556866EB9BB821CFA",
];

/// RewardTypeIcon's textures by ERewardType byte (@25548-25852).
fn modal_type_icon(kind: u32) -> Option<&'static str> {
    Some(match kind {
        0 | 3 | 10 => "_Black",
        1 => "ui_icons_skins",
        2 | 8 => "ui_icons_emote",
        4 => "ui_icons_tracker",
        5 => "ui_icons_profileicon",
        6 => "ui_icons_ringout",
        7 => "ui_icons_musicnote",
        9 => "ui_icons_fighters",
        11 => "ui_icons_announcer",
        12 => "ui_icons_gold",
        13 => "ui_icons_perks",
        14 => "ui_icons_guild",
        15 => "ui_icons_banner",
        _ => return None,
    })
}

/// SizeBoxOverrides by ERewardType byte (the default's key is byte 0), and FinalModalScale (1.3 for 1 and 9).
fn modal_size(kind: u32) -> ([f64; 2], f64) {
    let size = match kind {
        9 | 1 | 2 => [1340.0, 640.0],
        6 | 15 => [1300.0, 700.0],
        _ => [1200.0, 600.0],
    };
    (size, if matches!(kind, 1 | 9) { 1.3 } else { 1.0 })
}

/// Set a material's corner, zoom and offset scalars (Reset Material Shape, Character Material Shape).
fn material_shape(mid: &mut crate::slate::Mid, zoom: f32, y_offset: f32) {
    for (k, v) in [
        ("Zoom", zoom),
        ("XOffset", 0.0),
        ("YOffset", y_offset),
        ("RotationAngle", 0.0),
        ("TopLeft_X", 0.0),
        ("TopLeft_Y", 0.0),
        ("TopRight_X", 1.0),
        ("TopRight_Y", 0.0),
        ("BottomLeft_X", 0.0),
        ("BottomLeft_Y", 1.0),
        ("BottomRight_X", 1.0),
        ("BottomRight_Y", 1.0),
    ] {
        mid.set_scalar(k, v);
    }
}

/// UI_ContextualUnlockModalCharacter as the shop's ItemDetails. With no price on a single item (the build's prices are
/// Hydra's) the cost state shows no unlock button: each price container shows only when its price exists.
/// Not ported: the 3D character capture, the banner's render target and its material offsets (the banner shows the
/// cell's picture: Deviation), the announcer's VO, Try In Lab (Collapsed in the tree; WWShopVersion 2 and ShowTryInLab
/// are not modelled), the sale and expiry timers, the purchase flow past the stand-in's PurchaseProduct, the
/// optional currency, the owned-character warning, the rewards celebration.
#[derive(Debug, Default)]
pub struct ItemDetails {
    base: ModalBase,
    product: Option<ShopProduct>,
    state: String,
    display_name: String,
    is_loading: bool,
    related_bundle: Option<ShopProduct>,
    exiting: bool,
    quick_exit_blocked: bool,
}

impl ItemDetails {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<ItemDetails>::default()
    }

    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let mut v = vec![find(cx, "buttonDismisser")];
        for n in [
            "buttonUnlockHard",
            "buttonUnlockSoft",
            "Btn_Close",
            "buttonUnlockOptional",
            "Btn_ConfirmPurchase",
            "Btn_PurchaseHardCurrency",
            "buttonRelatedBundleA",
            "buttonRelatedBundleB",
        ] {
            let w = find(cx, n);
            v.push(interactor_of(cx, w));
        }
        v
    }

    /// ToggleLoading @7845: LoadingIndicator Visible / Hidden, PurchaseDialog Hidden / SelfHitTestInvisible.
    fn toggle_loading(&mut self, cx: &mut Cx<'_>, on: bool) {
        if on == self.is_loading {
            return;
        }
        self.is_loading = on;
        set_vis(cx, "LoadingIndicator", if on { Vis::Visible } else { Vis::Hidden });
        set_vis(cx, "PurchaseDialog", if on { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        if on {
            cx.view.play(cx.w, "Loading", false, 1.0);
        }
    }

    /// UpdateDisplayName @27878: the first name given stays.
    fn update_display_name(&mut self, cx: &mut Cx<'_>, name: &str) {
        if !self.display_name.is_empty() || name.is_empty() {
            return;
        }
        self.display_name = name.to_string();
        let t = find(cx, "titleLabel");
        wd(cx, t).text = name.to_string();
    }

    /// Init_WithProductSlug(ProductSlug, DisplayNameOverride) @30149: the name, ToggleLoading(true), the stand-in's
    /// FetchProductFromProductSlug (answered at once), then @4787 ValidateProduct.
    pub fn init_with_product_slug(&mut self, cx: &mut Cx<'_>, slug: &str, name: &str) {
        self.update_display_name(cx, name);
        self.toggle_loading(cx, true);
        let p = fetch_product_from_slug(catalogue(cx), slug);
        self.validate_product(cx, p);
    }

    /// ValidateProduct @8093: no product, an empty slug or an owned one dismisses the modal; else GetAssociatedBundles
    /// (the first with an asset is RelatedBundle), ToggleLoading(false), SetState("cost").
    fn validate_product(&mut self, cx: &mut Cx<'_>, p: Option<ShopProduct>) {
        let Some(p) = p.filter(|p| !p.slug.is_empty() && !is_owned(p)) else {
            self.toggle_loading(cx, false);
            exit(cx, &mut self.exiting);
            return;
        };
        self.related_bundle = associated_bundles(catalogue(cx), &p.slug).into_iter().next();
        self.product = Some(p);
        self.toggle_loading(cx, false);
        self.set_state(cx, "cost");
    }

    /// SetState(State) @10330.
    fn set_state(&mut self, cx: &mut Cx<'_>, state: &str) {
        let Some(p) = self.product.clone() else { return };
        self.state = state.to_string();
        self.update_appearance(cx, &p);
        let soft = p.cost(0);
        let hard = p.cost(CURRENCY_GEMS);
        if p.costs.len() == 1 {
            if soft.is_some() {
                self.state = "confirmSoft".into();
            } else if hard.is_some() {
                self.state = "confirmHard".into();
            }
        }
        let cost = self.state == "cost";
        let sw = find(cx, "stateSwitcher");
        wd(cx, sw).active = if cost { 0 } else { 1 };
        if cost {
            self.state_cost(cx, &p, soft, hard);
        } else {
            self.state_confirm(cx, &p);
        }
    }

    /// The cost state @10983-12547: each unlock container shows only with its price; "or" only with both.
    fn state_cost(&mut self, cx: &mut Cx<'_>, p: &ShopProduct, soft: Option<f64>, hard: Option<f64>) {
        set_vis(cx, "unlockHardContainer", if hard.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        if let Some(h) = hard {
            let b = find(cx, "buttonUnlockHard");
            let t = grouped(h);
            cx.visit::<ButtonSecondary, _>(b, |b, cx| b.set_label(cx, &t));
            set_vis(cx, "HardDiscountBox", Vis::Hidden);
        }
        set_vis(cx, "unlockSoftContainer", if soft.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        if let Some(s) = soft {
            let b = find(cx, "buttonUnlockSoft");
            let t = grouped(s);
            cx.visit::<ButtonSecondary, _>(b, |b, cx| b.set_label(cx, &t));
            set_vis(cx, "SoftDiscountBox", Vis::Hidden);
        }
        let both = soft.is_some() && hard.is_some();
        set_vis(cx, "orContainer", if both { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        let calc = find(cx, "UI_WWCostCalculation");
        let short = cx
            .visit::<CostCalculation, _>(calc, |c, cx| {
                c.init(cx, p, 0);
                c.insufficient_funds()
            })
            .unwrap_or(true);
        let b = find(cx, "buttonUnlockSoft");
        let i = interactor_of(cx, b);
        set_interactable(cx, i, !short);
        self.set_related_bundle(cx, p);
        self.quick_exit_blocked = true;
        // The cost state's Delay(0.75) @12492 resumes at @724 (the confirm states' at @15).
        cx.view.delay(0.75, Handler { owner: cx.w, id: R_COST_READY });
    }

    /// The confirm states @12547-13374 (confirmSoft, confirmHard; confirmOptional is not reached).
    fn state_confirm(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let hard = self.state == "confirmHard";
        let currency = if hard { CURRENCY_GEMS } else { 0 };
        let calc = find(cx, "UI_WWCostCalculation");
        let short = cx
            .visit::<CostCalculation, _>(calc, |c, cx| {
                c.init(cx, p, currency);
                c.insufficient_funds()
            })
            .unwrap_or(true);
        let v = if hard && short { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        set_vis(cx, "Btn_PurchaseHardCurrency", v);
        self.set_related_bundle(cx, p);
        let b = find(cx, "Btn_ConfirmPurchase");
        let i = interactor_of(cx, b);
        set_interactable(cx, i, false);
        self.quick_exit_blocked = true;
        cx.view.delay(0.75, Handler { owner: cx.w, id: R_CONFIRM_READY });
    }

    /// Set Related Bundle Internal @35166: both "View Bundle" containers Visible when the item is in a bundle not all
    /// owned (and the cvar wwshop.enabled is above 1), else Collapsed; their label names the bundle ("Appears in
    /// \"{0}\"" past 24 characters).
    // Unverified: wwshop.enabled is taken as 2 (the build's value is a server or ini setting).
    fn set_related_bundle(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let shown = self.related_bundle.is_some();
        let v = if shown { Vis::Visible } else { Vis::Collapsed };
        set_vis(cx, "RelatedBundleContainerA", v);
        set_vis(cx, "RelatedBundleContainerB", v);
        let Some(b) = &self.related_bundle else { return };
        let name = b.name.clone();
        let len = name.chars().count();
        let key = if len > 24 { "5758B21F42DEB194440C8BBF6EC3963C" } else { "360152804121FFDF16F82F9713BE4AE4" };
        let t = cx.view.game_text(key).replace("{0}", &name);
        for n in ["ViewBundleLabelA", "ViewBundleLabelB"] {
            let w = find(cx, n);
            wd(cx, w).text = t.clone();
        }
        if p.costs.len() != 1 && len > 32 {
            set_vis(cx, "RelatedBundleContainerB", Vis::Collapsed);
        }
    }

    /// UpdateAppearance @14287: ItemPurchase, SizeForType, the glow, the art by type, the name, the description, the
    /// rarity, the type icon, the confirm label.
    fn update_appearance(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let kind = p.kind;
        let sw = find(cx, "PurchaseVisualsWidgetSwitcher");
        wd(cx, sw).active = 0;
        // SizeForType
        let (size, scale) = modal_size(kind);
        let sb = find(cx, "ModalSizeBox");
        let x = wd(cx, sb);
        x.w_override = size[0];
        x.h_override = size[1];
        let fs = find(cx, "FinalModalScale");
        wd(cx, fs).user_scale = scale;
        let glow = match kind {
            9 | 1 => Some([-90.0, 120.0]),
            2 => Some([-135.0, 120.0]),
            8 => Some([-200.0, 0.0]),
            _ => None,
        };
        let g = find(cx, "Glow");
        wd(cx, g).transform.translation = glow.unwrap_or([0.0, 0.0]);
        let gv = match kind {
            1 | 2 | 8 | 9 => Vis::SelfHitTestInvisible,
            16 => Vis::Visible,
            _ => Vis::Collapsed,
        };
        set_vis(cx, "GlowBacking", gv);
        self.set_art(cx, p);
        // the sale time @34409: no sale, so DoesExpire is false and TimeContainer Collapsed
        set_vis(cx, "TimeContainer", Vis::Collapsed);
        self.update_display_name(cx, &p.name.clone());
        // the description @24907
        let ty = type_text(cx, kind);
        let desc = match kind {
            9 => self.character_details(cx, p),
            11 => String::new(),
            1 => {
                let f = fighter_name(cx, p.fighter.as_deref());
                cx.view
                    .game_text("26464DBA4C4F0B02CAED82A873284767")
                    .replace("{character}", &f)
                    .replace("{rewardtype}", &ty)
            }
            _ => ty.clone(),
        };
        let d = find(cx, "descriptionLabel");
        wd(cx, d).text = desc;
        let r = rarity_byte(&p.rarity) as usize;
        let rt = cx.view.game_text(MODAL_RARITY[r]).to_string();
        let rarity = find(cx, "Rarity");
        wd(cx, rarity).text = if MODAL_RARITY[r].is_empty() { String::new() } else { rt };
        let icon = find(cx, "RewardTypeIcon");
        let tex = modal_type_icon(kind).map(|n| resolve(&cx.view.bundle, n));
        wd(cx, icon).tex = tex;
        for n in ["Overlay_Rarity", "RarityBacking", "descriptionLabel"] {
            set_vis(cx, n, Vis::SelfHitTestInvisible);
        }
        let rc = rarity_color(&p.rarity);
        for n in ["Quad_IconBG", "RarityBacking"] {
            let q = find(cx, n);
            quad_color(cx, q, rc);
        }
        // the confirm label @28351: "Unlock {title}" with the type's text
        let label = cx.view.game_text("610BB05146381DDDE38738955255C954").replace("{title}", &ty);
        let b = find(cx, "Btn_ConfirmPurchase");
        cx.visit::<ButtonMain, _>(b, |b, cx| b.set_label(cx, &label));
    }

    /// The type 9 description @23268-23852: "{class} | {type} | {property}".
    // Unverified: CharacterTypeDisplayName's texts are not in the extract; the roster's type stands for {type}.
    fn character_details(&self, cx: &Cx<'_>, p: &ShopProduct) -> String {
        let Some(r) = roster_of(cx, &p.slug) else { return String::new() };
        let class = cx.view.class_info(&r.class).map(|c| c.display_name.clone()).unwrap_or_default();
        cx.view
            .game_text("6AF7D05E477456A18829048309CFE3FE")
            .replace("{class}", &class)
            .replace("{type}", &r.kind)
            .replace("{property}", &r.property)
    }

    /// The art by reward type @15727-27873.
    fn set_art(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let item = catalogue(cx).items.iter().find(|i| i.slug == p.slug).cloned();
        let Some(item) = item else { return };
        let rc = rarity_color(&p.rarity);
        let backing = find(cx, "Backing");
        let rs = find(cx, "RewardTypeSwitcher");
        let pick = |cx: &mut Cx<'_>, name: &str| {
            let at = ["CharacterArt", "Ringouts", "Stickers", "default"].iter().position(|n| *n == name).unwrap_or(3);
            wd(cx, rs).active = at as i32;
            find(cx, name)
        };
        match item.kind {
            // @19851: the skin's (a character's: its first skin's) ShopModalOverrideMaterial, else its
            // VariantIconMaterial through Character Material Shape
            1 | 9 => {
                // Unverified: a character's CharacterData BackgroundColor is not imported; its rarity colour stands in
                quad_color(cx, backing, rc);
                let skin = if item.kind == 9 {
                    let f = item.fighter.clone();
                    catalogue(cx)
                        .items
                        .iter()
                        .find(|i| i.kind == 1 && i.fighter == f && i.modal_material.is_some())
                        .cloned()
                } else {
                    Some(item.clone())
                };
                let modal = skin.as_ref().and_then(|s| s.modal_material.clone());
                let w = pick(cx, "CharacterArt");
                let b = if let Some(b) = shop_brush(cx, modal.as_deref()) {
                    Some(b)
                } else {
                    let icon = skin.as_ref().and_then(|s| s.variant_icon.clone()).or(item.variant_icon.clone());
                    shop_brush(cx, icon.as_deref()).map(|(t, mut m)| {
                        material_shape(&mut m, 0.8, 0.1);
                        (t, m)
                    })
                };
                let b = b.or_else(|| shop_brush(cx, item.image.as_deref()));
                set_brush(cx, w, b);
                cx.view.play(cx.w, "AnimIn_Character", false, 1.0);
            }
            // @18171: a taunt's RewardThumbnailMaterial through Reset Material Shape
            2 => {
                quad_color(cx, backing, rc);
                let w = pick(cx, "CharacterArt");
                let b = shop_brush(cx, item.thumbnail_material.as_deref()).map(|(t, mut m)| {
                    material_shape(&mut m, 1.0, 0.0);
                    (t, m)
                });
                set_brush(cx, w, b.or_else(|| shop_brush(cx, item.image.as_deref())));
                cx.view.play(cx.w, "AnimIn_Character", false, 1.0);
            }
            // @17127: a ring-out's ToolTipPreviewMaterial
            6 => {
                quad_color(cx, backing, [0.009721, 0.011612, 0.033105, 1.0]);
                let w = pick(cx, "Ringouts");
                let b =
                    shop_brush(cx, item.tooltip_preview.as_deref()).or_else(|| shop_brush(cx, item.image.as_deref()));
                set_brush(cx, w, b);
            }
            // @16800: an emote's ToolTipPreviewMaterial
            8 => {
                let w = pick(cx, "Stickers");
                let b =
                    shop_brush(cx, item.tooltip_preview.as_deref()).or_else(|| shop_brush(cx, item.image.as_deref()));
                set_brush(cx, w, b);
            }
            // @27120: a banner
            // Deviation: the banner's render target and its material's offsets are not ported; the cell's picture
            // stands in.
            15 => {
                let w = pick(cx, "Ringouts");
                let b = shop_brush(cx, item.image.as_deref());
                set_brush(cx, w, b);
            }
            // @21877: the Thumbnail Material, else the Thumbnail
            _ => {
                let w = pick(cx, "default");
                let b = shop_brush(cx, item.thumbnail_material.as_deref())
                    .or_else(|| shop_brush(cx, item.thumbnail.as_deref()))
                    .or_else(|| shop_brush(cx, item.image.as_deref()));
                set_brush(cx, w, b);
            }
        }
    }

    /// The related bundle's "View Bundle" @36929: BundleDetails with Init_WithProductSlug.
    fn open_related(&mut self, cx: &mut Cx<'_>) {
        let Some(b) = self.related_bundle.clone() else { return };
        if let Some(m) = Manager::show_modal(cx, "UI_WWContextualUnlockModalBundle", "BundleDetails", "", "") {
            cx.visit::<BundleDetails, _>(m, |d, cx| d.init_with_product_slug(cx, &b.slug));
        }
    }

    /// Btn_ConfirmPurchase @32136 -> @30679: the stand-in's PurchaseProduct, which does nothing.
    // Deviation: no ToggleLoading, fallback timer or DisplayError around it (nothing can be bought).
    fn confirm(&mut self, _cx: &mut Cx<'_>) {
        if let (Some(p), false) = (&self.product, self.is_loading) {
            let _ = purchase_product(p, if self.state == "confirmHard" { CURRENCY_GEMS } else { 0 });
        }
    }
}

impl Modal for ItemDetails {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @7145: UpdateContext(NewGuid), buttonDismisser focused.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let g = cx.view.manager.new_guid();
        self.base.context = g.clone();
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, &g);
        let d = find(cx, "buttonDismisser");
        set_focused(cx, d, false);
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @7262: nothing while loading; a confirm state goes back to cost; else Exit(false).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        if self.is_loading {
            return;
        }
        let one_price = self.product.as_ref().is_some_and(|p| p.costs.len() == 1);
        if self.state != "cost" && !one_price {
            self.set_state(cx, "cost");
        } else {
            exit(cx, &mut self.exiting);
        }
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for ItemDetails {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// OnInitialized @13615: the buttons' clicks, the unlock buttons' currency icons, OnLoad.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let d = find(cx, "buttonDismisser");
        super::controls::bind(
            cx,
            d,
            |s: &mut super::controls::interactor::InteractorState| &mut s.on_click,
            H_DISMISSER,
        );
        bind_main(cx, "Btn_ConfirmPurchase", H_CONFIRM);
        bind_main(cx, "Btn_PurchaseHardCurrency", H_HARD_CURRENCY);
        bind_secondary(cx, "buttonRelatedBundleA", H_RELATED);
        bind_secondary(cx, "buttonRelatedBundleB", H_RELATED);
        cx.view.play(cx.w, "OnLoad", false, 1.0);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            // @13402: the dismisser exits unless the quick exit is blocked
            H_DISMISSER if !self.quick_exit_blocked => exit(cx, &mut self.exiting),
            H_CONFIRM => self.confirm(cx),
            H_HARD_CURRENCY => {
                let ctx = self.base.context.clone();
                open_currency_shop(cx, &ctx);
            }
            H_RELATED => self.open_related(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            // @15: QuickExitBlocked false, Btn_ConfirmPurchase interactable when the funds suffice
            R_CONFIRM_READY => {
                self.quick_exit_blocked = false;
                let short = {
                    let calc = find(cx, "UI_WWCostCalculation");
                    cx.visit::<CostCalculation, _>(calc, |c, _| c.insufficient_funds()).unwrap_or(true)
                };
                let b = find(cx, "Btn_ConfirmPurchase");
                let i = interactor_of(cx, b);
                set_interactable(cx, i, !short);
            }
            // @724: QuickExitBlocked false. Not ported: the focus on buttonUnlockSoft / buttonUnlockHard after it.
            R_COST_READY => self.quick_exit_blocked = false,
            R_EXITED => {
                let id = self.base.modal_id.clone();
                Manager::dismiss_modal(cx, &id);
            }
            _ => {}
        }
    }
}

// ---------- UI_WWContextualUnlockModalBundle, modal `BundleDetails` ----------

/// UI_WWContextualUnlockModalBundle as the shop's BundleDetails: the bundle's items in the inner grid (not clickable),
/// its gem cost with the balance, "Buy More Gleamium".
/// Not ported: SetDescriptionText (DescriptionHorizontalBox stays Collapsed), the cells' SetPadding(0, 0, 10, 10), the
/// sale timer, the battle pass layout, the purchase flow past the stand-in's PurchaseProduct, the rewards celebration.
#[derive(Debug, Default)]
pub struct BundleDetails {
    base: ModalBase,
    product: Option<ShopProduct>,
    state: String,
    display_name: String,
    is_loading: bool,
    exiting: bool,
}

impl BundleDetails {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<BundleDetails>::default()
    }

    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        let mut v = vec![find(cx, "buttonDismisser")];
        for n in ["Btn_ConfirmPurchase", "Btn_PurchaseHardCurrency"] {
            let w = find(cx, n);
            v.push(interactor_of(cx, w));
        }
        v
    }

    /// ToggleLoading @3399.
    fn toggle_loading(&mut self, cx: &mut Cx<'_>, on: bool) {
        if on == self.is_loading {
            return;
        }
        self.is_loading = on;
        set_vis(cx, "LoadingIndicator", if on { Vis::Visible } else { Vis::Hidden });
        set_vis(cx, "PurchaseDialog", if on { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        if on {
            cx.view.play(cx.w, "Loading", false, 1.0);
        }
    }

    /// Init_WithProductSlug @7892: ToggleLoading(true), the stand-in's FetchProductFromProductSlug (answered at once),
    /// ValidateProduct @4123: no product, an empty slug, an owned bundle or one with no items dismisses the modal;
    /// else ToggleLoading(false) and UpdateAppearance.
    pub fn init_with_product_slug(&mut self, cx: &mut Cx<'_>, slug: &str) {
        self.toggle_loading(cx, true);
        let p = fetch_product_from_slug(catalogue(cx), slug);
        let Some(p) = p.filter(|p| !p.slug.is_empty() && !is_owned(p) && !p.bundle_products.is_empty()) else {
            self.toggle_loading(cx, false);
            exit(cx, &mut self.exiting);
            return;
        };
        self.toggle_loading(cx, false);
        self.update_appearance(cx, p);
    }

    /// UpdateAppearance @12165 (WWShopVersion 2): the name, Populate Items, ItemPurchase, then SetState.
    fn update_appearance(&mut self, cx: &mut Cx<'_>, p: ShopProduct) {
        if self.display_name.is_empty() {
            self.display_name = p.name.clone();
            let t = find(cx, "titleLabel");
            wd(cx, t).text = p.name.clone();
        }
        self.populate_items(cx, &p);
        // CheckSaleTime @11871: no sale, so DoesExpire is false and TimeContainer Collapsed (@11990)
        set_vis(cx, "TimeContainer", Vis::Collapsed);
        wd(cx, cx.w).vis = Vis::SelfHitTestInvisible;
        let sw = find(cx, "PurchaseVisualsWidgetSwitcher");
        wd(cx, sw).active = 0;
        self.product = Some(p);
        self.set_state(cx);
    }

    /// Populate Items @8820: the inner grid's Set Products of the bundle's items, its spinner Collapsed, each cell
    /// HitTestInvisible with ShowPrices false.
    fn populate_items(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let items = bundle_products_array(catalogue(cx), p);
        let g = find(cx, "UI_WWShopProductGridGeneric");
        let cells = cx
            .visit::<ProductGrid, _>(g, |grid, cx| {
                grid.set_products(cx, items, "", false);
                grid.cells.clone()
            })
            .unwrap_or_default();
        let pl = cx.view.tree().find_in(g, "PendingLoad");
        wd(cx, pl).vis = Vis::Collapsed;
        for c in cells {
            wd(cx, c).vis = Vis::HitTestInvisible;
        }
    }

    /// SetState @8580: confirmSoft with a gold price, else confirmHard with a gem price; confirmHard @8242: the cost
    /// calculation in gems, Btn_PurchaseHardCurrency SelfHitTestInvisible when short (else Hidden), Btn_ConfirmPurchase
    /// not interactable until Delay(0.75) @15.
    fn set_state(&mut self, cx: &mut Cx<'_>) {
        let Some(p) = self.product.clone() else { return };
        if p.cost(0).is_some() {
            self.state = "confirmSoft".into();
        } else if p.cost(CURRENCY_GEMS).is_some() {
            self.state = "confirmHard".into();
        }
        if self.state != "confirmHard" {
            return;
        }
        let calc = find(cx, "UI_WWCostCalculation");
        let short = cx
            .visit::<CostCalculation, _>(calc, |c, cx| {
                c.init(cx, &p, CURRENCY_GEMS);
                c.insufficient_funds()
            })
            .unwrap_or(true);
        set_vis(cx, "Btn_PurchaseHardCurrency", if short { Vis::SelfHitTestInvisible } else { Vis::Hidden });
        let b = find(cx, "Btn_ConfirmPurchase");
        let i = interactor_of(cx, b);
        set_interactable(cx, i, false);
        cx.view.delay(0.75, Handler { owner: cx.w, id: R_CONFIRM_READY });
    }

    /// Btn_ConfirmPurchase @9569: the stand-in's PurchaseProduct, which does nothing.
    // Deviation: no ToggleLoading, fallback timer or DisplayError around it (nothing can be bought).
    fn confirm(&mut self, _cx: &mut Cx<'_>) {
        if let (Some(p), false) = (&self.product, self.is_loading) {
            let _ = purchase_product(p, CURRENCY_GEMS);
        }
    }
}

impl Modal for BundleDetails {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @3223: UpdateContext(NewGuid) (with the scroll box registered), buttonDismisser focused.
    fn post_show(&mut self, cx: &mut Cx<'_>, _instructions: &str, _extra_data: &str) {
        let g = cx.view.manager.new_guid();
        self.base.context = g.clone();
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, &g);
        let sb = find(cx, "ScrollBox");
        cx.view.manager.register_scrollbox(sb);
        let d = find(cx, "buttonDismisser");
        set_focused(cx, d, false);
    }

    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        let sb = find(cx, "ScrollBox");
        cx.view.manager.unregister_scrollbox(sb);
    }

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @3340: nothing while loading, else Exit(false).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        if !self.is_loading {
            exit(cx, &mut self.exiting);
        }
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for BundleDetails {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// OnInitialized @6387: the buttons' clicks, OnLoad.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let d = find(cx, "buttonDismisser");
        super::controls::bind(
            cx,
            d,
            |s: &mut super::controls::interactor::InteractorState| &mut s.on_click,
            H_DISMISSER,
        );
        bind_main(cx, "Btn_ConfirmPurchase", H_CONFIRM);
        bind_main(cx, "Btn_PurchaseHardCurrency", H_HARD_CURRENCY);
        cx.view.play(cx.w, "OnLoad", false, 1.0);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            // @6274
            H_DISMISSER if !self.is_loading => exit(cx, &mut self.exiting),
            H_CONFIRM => self.confirm(cx),
            H_HARD_CURRENCY => {
                let ctx = self.base.context.clone();
                open_currency_shop(cx, &ctx);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            // @15: Btn_ConfirmPurchase interactable, then the focus on it (funds suffice) or on Btn_PurchaseHardCurrency
            // Unverified: @15 sets it interactable whatever the funds (ItemDetails' @100 takes !Insufficient); kept as
            // read, the click reaches only the stand-in's no-op PurchaseProduct.
            R_CONFIRM_READY => {
                let b = find(cx, "Btn_ConfirmPurchase");
                let i = interactor_of(cx, b);
                set_interactable(cx, i, true);
            }
            R_EXITED => {
                let id = self.base.modal_id.clone();
                Manager::dismiss_modal(cx, &id);
            }
            _ => {}
        }
    }
}
