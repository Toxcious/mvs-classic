//! The shop's cards and grids: UI_WWQuadrilateral, WW_CurrencyIcon, WW_ShopPriceLabel, WW_ShopItemCell,
//! UI_WW_GridHeader, UI_WWShopProductGridGeneric and UI_WWFilterGrid. Their products come from the port's stand-in
//! (shop_products.rs).

use std::any::Any;

use serde_json::{Value, json};

use super::common::{RARITY, rgba, text};
use super::controls::bar_buttons::{P_ID, P_LABEL};
use super::controls::drop_menu::DropMenu;
use super::controls::interactor::{FocusedCommon, InteractorState, batch_set_context};
use super::controls::{bind, find, prop, prop_bool, prop_f64, quad_color, wd};
use super::shop_details::{BundleDetails, ItemDetails};
use super::shop_products::{
    GridSlot, KIND_BUNDLE, SORT_ALPHABETICAL, SORT_CHARACTER, SORT_COST, SORT_NEWEST, SORT_RARITY, ShopProduct,
    is_owned, mosaic_sizes, pack, rarity_byte, slot_size, sort_and_filter,
};
use crate::bundle::{Rgba, ShopCatalogue};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra::picture_name;
use crate::menu::manager::{Manager, NULL_GUID};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, Mid, VAlign, Vis, WidgetId, resolve};

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn f32x4(c: Rgba) -> [f32; 4] {
    c.map(|v| v as f32)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The menu bundle's shop catalogue (empty when the bundle has none).
pub fn catalogue<'a>(cx: &'a Cx<'_>) -> &'a ShopCatalogue {
    static EMPTY: std::sync::OnceLock<ShopCatalogue> = std::sync::OnceLock::new();
    cx.view.bundle.menu.as_ref().map_or_else(|| EMPTY.get_or_init(ShopCatalogue::default), |m| &m.shop)
}

/// FloatToText with grouping and no fraction (the culture's, en: "1,500").
pub fn grouped(n: f64) -> String {
    let v = n.abs().round() as u64;
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0.0 && v != 0 { format!("-{out}") } else { out }
}

/// EnumParser RewardTypeToLocalizedText's keys by ERewardType byte (empty: no text).
const TYPE_TEXT: [&str; 17] = [
    "",
    "CDB0DC30408FB5C003154999564FE987",
    "E1F8172D483D17EFD625FC9DF088A06E",
    "",
    "55FBA2A94539B328157B1EAFA0B9CB6F",
    "103158E841D601F3E7784AB8ADB0D474",
    "ED2623E04C4E3B266CF8E8830E15D726",
    "797D76C34901AF1A0FA9B28DD7879EE0",
    "D3D59FEB4E3EFAC964D94C888DEB697C",
    "98D822E3443A9B48732A70AF5A475E26",
    "ABCAD1744043655A2534F4813B18B280",
    "B6CA4EBF4B846AE461DAA6A93969871D",
    "0387AED249ACCDD9DD9D9D9B5307862D",
    "D6091ECB444C801A645C7E8FE418A40A",
    "8B6675FA452FEF7F711320BEB765F25A",
    "9C84D8444C922D877D89CAAF87A3BC08",
    "",
];

/// The reward type's text of `kind` (empty for none).
pub fn type_text(cx: &Cx<'_>, kind: u32) -> String {
    let k = TYPE_TEXT.get(kind as usize).copied().unwrap_or("");
    if k.is_empty() { String::new() } else { cx.view.game_text(k).to_string() }
}

/// The cell's rarity texts (EnumParser's) by ERewardRarity byte.
const RARITY_TEXT: [&str; 5] = [
    "A8B6C8F747C734726F068491D21322D6",
    "8E0878374598F2D5FEB2C0AAE9C28D0F",
    "B113B9E2477A57630A2F0C8E1D2CEA41",
    "0107178142E0E72CFAA4E589B5B5819C",
    "77C4C7214FAA2415768FD69245FF0F92",
];

/// Get Item Icon: the reward type's icon (None: no icon).
pub fn type_icon(kind: u32) -> Option<&'static str> {
    Some(match kind {
        1 => "ui_icons_skins",
        2 | 8 => "ui_icons_emote",
        3 => "ui_icons_promote",
        4 => "ui_icons_tracker",
        5 => "ui_icons_profileicon",
        6 => "ui_icons_ringout",
        7 => "ui_icons_musicnote",
        9 => "ui_icons_fighters",
        10 => "ui_icons_speechbubble",
        11 => "ui_icons_announcer",
        12 => "ui_icons_gold",
        13 => "ui_icons_perks",
        14 => "ui_icons_guild",
        15 => "ui_icons_banner",
        _ => return None,
    })
}

/// Rarity to Color's base colour of a rarity tail.
pub fn rarity_color(rarity: &str) -> Rgba {
    RARITY[rarity_byte(rarity) as usize].1
}

/// The character item (type 9) of an EFighter tail: its display name is the fighter's.
fn character_item<'a>(cat: &'a ShopCatalogue, fighter: &str) -> Option<&'a crate::bundle::ShopItem> {
    cat.items.iter().find(|i| i.kind == 9 && i.fighter.as_deref() == Some(fighter))
}

/// The fighter's display name of an EFighter tail.
pub fn fighter_name(cx: &Cx<'_>, fighter: Option<&str>) -> String {
    fighter.and_then(|f| character_item(catalogue(cx), f)).map(|i| i.name.clone()).unwrap_or_default()
}

/// The roster entry of a character item's slug.
pub fn roster_of<'a>(cx: &'a Cx<'_>, slug: &str) -> Option<&'a crate::bundle::RosterEntry> {
    cx.view.roster().iter().find(|r| r.slug.eq_ignore_ascii_case(slug))
}

/// A picture of the catalogue (a build object path) as a brush: a material the bundle holds as itself, a texture
/// through MI_WW_DefaultShopImage's Texture. None: the bundle has neither.
pub fn shop_brush(cx: &Cx<'_>, path: Option<&str>) -> Option<(String, Mid)> {
    let name = picture_name(path?)?;
    let b = &cx.view.bundle;
    if let Some(m) = b.material_path(&name) {
        return Some((m.to_string(), Mid::of(m)));
    }
    let tex = b.textures.get(&name)?.entry().to_string();
    let m = b.material_path("MI_WW_DefaultShopImage")?.to_string();
    let mut mid = Mid::of(&m);
    mid.set_texture("Texture", &tex);
    Some((m, mid))
}

/// An Image's brush from `shop_brush`; None clears it.
pub fn set_brush(cx: &mut Cx<'_>, w: WidgetId, b: Option<(String, Mid)>) {
    let x = wd(cx, w);
    match b {
        Some((path, mid)) => {
            x.tex = Some(path);
            x.mid = Some(mid);
        }
        None => {
            x.tex = None;
            x.mid = None;
        }
    }
}

// ---------- UI_WWQuadrilateral ----------

/// UI_WWQuadrilateral: the WW shop's quad (an Image with MaterialOverride, M_WWQuadrilateral's parameters).
// Unverified: the class's bytecode is not in the extract (only its exports: SetParameters, UpdateColorSolid,
// UpdateColorGradient, UpdateSharpness, the wobble). Modelled on WBP_Quadrilateral: the Image gets a dynamic
// instance of MaterialOverride with the corners, EdgeFalloff = Sharpness, Dot Offset / Dot Range, ColorA, ColorB and
// HalftoneColor = ColorHalftone (M_WWQuadrilateral's parameter names); the wobble is not modelled.
#[derive(Debug, Default)]
pub struct WWQuad;

impl WWQuad {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(WWQuad)
    }

    fn image(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Image")
    }

    fn set_vectors(cx: &mut Cx<'_>, a: Rgba, b: Rgba, h: Rgba) {
        let i = Self::image(cx);
        if let Some(m) = &mut wd(cx, i).mid {
            m.set_vector("ColorA", f32x4(a));
            m.set_vector("ColorB", f32x4(b));
            m.set_vector("HalftoneColor", f32x4(h));
        }
    }

    /// UpdateColorSolid(Color, Halftone).
    pub fn update_color_solid(&mut self, cx: &mut Cx<'_>, c: Rgba, h: Rgba) {
        Self::set_vectors(cx, c, c, h);
    }

    /// UpdateColorGradient(ColorA, ColorB, Halftone).
    pub fn update_color_gradient(&mut self, cx: &mut Cx<'_>, a: Rgba, b: Rgba, h: Rgba) {
        Self::set_vectors(cx, a, b, h);
    }
}

impl Controller for WWQuad {
    /// SetParameters (Unverified, see the type).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        let mat = p.get("MaterialOverride").and_then(Value::as_str).unwrap_or("MI_Quad_ShopCell");
        let Some(path) = cx.view.bundle.material_path(mat).map(str::to_string) else { return };
        let mut mid = Mid::of(&path);
        for k in ["TopLeft", "TopRight", "BottomLeft", "BottomRight"] {
            let c = p.get(k).cloned().unwrap_or(Value::Null);
            let n = |a: &str, d: f64| c.get(a).and_then(Value::as_f64).unwrap_or(d) as f32;
            let (dx, dy) = match k {
                "TopRight" => (1.0, 0.0),
                "BottomLeft" => (0.0, 1.0),
                "BottomRight" => (1.0, 1.0),
                _ => (0.0, 0.0),
            };
            mid.set_scalar(&format!("{k}_X"), n("X", dx));
            mid.set_scalar(&format!("{k}_Y"), n("Y", dy));
        }
        let num = |k: &str| p.get(k).and_then(Value::as_f64);
        mid.set_scalar("EdgeFalloff", num("Sharpness").unwrap_or(0.01) as f32);
        for k in ["Dot Offset", "Dot Range"] {
            if let Some(v) = num(k) {
                mid.set_scalar(k, v as f32);
            }
        }
        let col = |k: &str| p.get(k).map(rgba).unwrap_or([1.0; 4]);
        mid.set_vector("ColorA", f32x4(col("ColorA")));
        mid.set_vector("ColorB", f32x4(col("ColorB")));
        mid.set_vector("HalftoneColor", f32x4(col("ColorHalftone")));
        let i = Self::image(cx);
        let x = wd(cx, i);
        x.tex = Some(path);
        x.mid = Some(mid);
    }
}

/// UpdateColorSolid of a UI_WWQuadrilateral child.
fn ww_solid(cx: &mut Cx<'_>, q: WidgetId, c: Rgba, h: Rgba) {
    cx.visit::<WWQuad, _>(q, |q, cx| q.update_color_solid(cx, c, h));
}

/// UpdateColorGradient of a UI_WWQuadrilateral child.
fn ww_gradient(cx: &mut Cx<'_>, q: WidgetId, a: Rgba, b: Rgba, h: Rgba) {
    cx.visit::<WWQuad, _>(q, |q, cx| q.update_color_gradient(cx, a, b, h));
}

// ---------- WW_CurrencyIcon ----------

/// ECurrencyType's byte of a property's value (`ECurrencyType::gems`).
fn currency_prop(v: &Value) -> u8 {
    let s = v.as_str().unwrap_or_default();
    super::shop_products::currency_byte(s.rsplit("::").next().unwrap_or(s)).unwrap_or(0)
}

/// WW_CurrencyIcon.
#[derive(Debug, Default)]
pub struct CurrencyIcon;

impl CurrencyIcon {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CurrencyIcon)
    }

    /// SetCurrencyType @38: CurrencyIconSwitcher's active index = the byte.
    pub fn set_currency_type(&mut self, cx: &mut Cx<'_>, c: u8) {
        let s = find(cx, "CurrencyIconSwitcher");
        wd(cx, s).active = i32::from(c);
    }
}

impl Controller for CurrencyIcon {
    /// PreConstruct @152: SetCurrencyType(CurrencyType).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let c = currency_prop(&prop(cx, "CurrencyType"));
        self.set_currency_type(cx, c);
    }
}

// ---------- WW_ShopPriceLabel ----------

const PRICE_SHADOW: Rgba = [0.026241, 0.026241, 0.026241, 1.0];

/// WW_ShopPriceLabel.
#[derive(Debug, Default)]
pub struct PriceLabel {
    amount: f64,
    currency: u8,
    strike: bool,
    font_size: f64,
    icon_factor: f64,
    color: Rgba,
    drop_shadow: bool,
    font_type: bool,
}

impl PriceLabel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<PriceLabel>::default()
    }

    /// Set Price(amount, currency, strikethrough): SetCurrencyType, SetStrikethrough, Display Amount. (Animate is
    /// never asked for by the port's callers.)
    pub fn set_price(&mut self, cx: &mut Cx<'_>, amount: f64, currency: u8, strike: bool) {
        self.amount = amount;
        self.currency = currency;
        self.strike = strike;
        let icon = find(cx, "WW_CurrencyIcon");
        cx.visit::<CurrencyIcon, _>(icon, |i, cx| i.set_currency_type(cx, currency));
        self.set_strikethrough(cx, strike);
        let t = find(cx, "ItemPrice");
        wd(cx, t).text = grouped(amount);
    }

    /// SetStrikethrough: StrikethroughContainer Visible or Collapsed; the price at half alpha when struck.
    fn set_strikethrough(&mut self, cx: &mut Cx<'_>, on: bool) {
        set_vis(cx, "StrikethroughContainer", if on { Vis::Visible } else { Vis::Collapsed });
        let mut c = self.color;
        if on {
            c[3] = 0.5;
        }
        let t = find(cx, "ItemPrice");
        wd(cx, t).color = c;
    }

    /// Change Color: Set Font with the colour and no shadow.
    pub fn change_color(&mut self, cx: &mut Cx<'_>, c: Rgba) {
        self.color = c;
        self.drop_shadow = false;
        self.update_display(cx);
    }

    /// The Icon Factor (UI_WWCostCalculation SetIcon), then Update Display.
    pub fn set_icon_factor(&mut self, cx: &mut Cx<'_>, f: f64) {
        self.icon_factor = f;
        self.update_display(cx);
    }

    /// Update Display -> Set Font(size, type, colour, shadow): the shadow, the typeface (Norms-ExtraBoldItalic with
    /// Font Type, else NormsCond-ExtraBoldItalic) at the size, the colour; the currency icon's box (size x 1.15 - 6)
    /// x Icon Factor wide, 1.125 times that high.
    fn update_display(&mut self, cx: &mut Cx<'_>) {
        let t = find(cx, "ItemPrice");
        let size = self.font_size;
        let mut shadow = PRICE_SHADOW;
        shadow[3] = if self.drop_shadow { 1.0 } else { 0.0 };
        let face = if self.font_type { "Norms-ExtraBoldItalic" } else { "NormsCond-ExtraBoldItalic" };
        let mut c = self.color;
        if self.strike {
            c[3] = 0.5;
        }
        let x = wd(cx, t);
        x.shadow_color = shadow;
        x.typeface = Some(face.to_string());
        x.font_size = size;
        x.color = c;
        let w = (size * 1.15 - 6.0) * self.icon_factor;
        let b = find(cx, "CurrencyIconContainer");
        let x = wd(cx, b);
        x.w_override = w;
        x.h_override = w * 1.125;
    }
}

impl Controller for PriceLabel {
    /// PreConstruct: the instance's fields, then Update Display and the price.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        self.font_size = p.get("Font Size").and_then(Value::as_f64).unwrap_or(40.0);
        self.icon_factor = p.get("Icon Factor").and_then(Value::as_f64).unwrap_or(1.0);
        self.color = p.get("Color").map(rgba).unwrap_or([1.0; 4]);
        self.drop_shadow = p.get("Drop Shadow").and_then(Value::as_bool).unwrap_or(true);
        self.font_type = p.get("Font Type").and_then(Value::as_bool).unwrap_or(false);
        let amount = p.get("DisplayAmount").and_then(Value::as_f64).unwrap_or(0.0);
        let strike = p.get("Strikethrough").and_then(Value::as_bool).unwrap_or(false);
        let currency = currency_prop(p.get("Currency Type").unwrap_or(&Value::Null));
        self.update_display(cx);
        self.set_price(cx, amount, currency, strike);
    }
}

// ---------- WW_ShopItemCell ----------

const CELL_FOCUS: u32 = 1;
const CELL_UNFOCUS: u32 = 2;
const CELL_CLICK: u32 = 3;

/// The cell's description formats.
const FMT_BUNDLE: &str = "60EE73D9491890BE10904EA3638CADD0";
const FMT_FIGHTER_TYPE: &str = "692D7CC54D9EF9857EDC47869BF7927A";
const FMT_RARITY_TYPE: &str = "4089E8BA41DD73692EB1E89AD445AC36";
const FMT_TYPE: &str = "86C95C034DE7DD1BDF3691A787D0907A";
/// "{class} | {type} | {universe}" (WW_ShopItemCellBase @2867).
const FMT_CHARACTER_DETAILS: &str = "1CC8954B4600F7653E9180AEE48D2B1B";

/// WW_ShopItemCell (base WW_ShopItemCellBase), as the stand-in's products fill it.
/// Not ported: the sale and expiry ribbons (Unverified: no sale data; the Bundle, Discount and Timespan ribbons are
/// left Collapsed), the banner hover's render-target path (@12321-14720), the image load's async spinner (the
/// picture is set at once: Set Loading Status(true) follows Receive Product), the cell's navigation (the grid's
/// Memorize Cell Locations), TickShopCells, Apply Display Preferences' Get Item Color scheme.
#[derive(Debug)]
pub struct ShopItemCell {
    pub product: Option<ShopProduct>,
    /// OnCellOpenedModal.
    pub on_opened_modal: Event<()>,
    kind: u32,
    owned: bool,
    show_prices: bool,
    focused: bool,
    focused_color: Rgba,
    unfocused_color: Rgba,
    rarity_color: Rgba,
    actual_color: Rgba,
    start_color: Rgba,
    dest_color: Rgba,
    focused_linear: f64,
    start_focused: f64,
    dest_focused: f64,
    anim_time: f64,
    animating: bool,
}

impl Default for ShopItemCell {
    fn default() -> ShopItemCell {
        let d = [0.066667, 0.101961, 0.152941, 1.0];
        ShopItemCell {
            product: None,
            on_opened_modal: Event::default(),
            kind: 0,
            owned: false,
            show_prices: true,
            focused: false,
            focused_color: [0.0; 4],
            unfocused_color: d,
            rarity_color: [1.0, 0.29159, 0.928835, 1.0],
            actual_color: d,
            start_color: d,
            dest_color: d,
            focused_linear: 0.0,
            start_focused: 0.0,
            dest_focused: 0.0,
            anim_time: 0.0,
            animating: false,
        }
    }
}

/// UE Ease(0, 1, a, EaseInOut, 2).
fn ease_in_out(a: f64) -> f64 {
    if a < 0.5 { 0.5 * (2.0 * a).powi(2) } else { 1.0 - 0.5 * (2.0 * (1.0 - a)).powi(2) }
}

impl ShopItemCell {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<ShopItemCell>::default()
    }

    fn color_prop(cx: &Cx<'_>, k: &str, d: Rgba) -> Rgba {
        cx.view.tree().props(cx.w).get(k).map(rgba).unwrap_or(d)
    }

    /// Recieve Product (Base @724-3312) then SetItem (@784): SetOwned, SetItemLayout, SetItemPricing, the prices'
    /// visibility; Set Item Colors; the picture and Set Loading Status(true).
    pub fn receive_product(&mut self, cx: &mut Cx<'_>, p: ShopProduct, show_prices: bool) {
        self.kind = if p.is_bundle() { KIND_BUNDLE } else { p.kind };
        self.owned = is_owned(&p);
        self.show_prices = show_prices;
        self.set_owned(cx);
        self.set_item_layout(cx, &p);
        self.set_item_pricing(cx, &p);
        set_vis(cx, "Price", if self.show_prices { Vis::Visible } else { Vis::Collapsed });
        for n in ["BundleRibbon", "DiscountRibbon", "TimespanRibbon"] {
            set_vis(cx, n, Vis::Collapsed);
        }
        self.rarity_color = rarity_color(&p.rarity);
        self.set_item_colors(cx);
        let img = find(cx, "ItemMainImage");
        let brush = shop_brush(cx, p.image.as_deref());
        wd(cx, img).vis = if brush.is_some() { Vis::Visible } else { Vis::Hidden };
        set_brush(cx, img, brush);
        // Set Image Loading Status(true) @9378: the picture is set at once
        set_vis(cx, "HeroImageLoader", Vis::Collapsed);
        set_vis(cx, "HeroImageScaleBox", Vis::Visible);
        self.apply_feather_colour(cx);
        self.set_loading_status(cx, true);
        self.product = Some(p);
    }

    /// Set Loading Status @568: LoadedContent Visible / Collapsed, PendingLoad the other way.
    pub fn set_loading_status(&mut self, cx: &mut Cx<'_>, loaded: bool) {
        set_vis(cx, "LoadedContent", if loaded { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, "PendingLoad", if loaded { Vis::Collapsed } else { Vis::Visible });
    }

    /// SetOwned @9521: OwnedOverlay, RarityStripe, Price and PurchasedText by Owned (never in the port's shop).
    fn set_owned(&mut self, cx: &mut Cx<'_>) {
        let o = self.owned;
        if o {
            self.show_prices = false;
        }
        set_vis(cx, "OwnedOverlay", if o { Vis::Visible } else { Vis::Hidden });
        set_vis(cx, "RarityStripe", if o { Vis::Collapsed } else { Vis::HitTestInvisible });
        set_vis(cx, "Price", if o { Vis::Collapsed } else { Vis::Visible });
        set_vis(cx, "PurchasedText", if o { Vis::Visible } else { Vis::Collapsed });
    }

    /// SetItemLayout @4445: the title, the type 9 details, the description, the icon.
    fn set_item_layout(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let kind = self.kind;
        let t = find(cx, "MainTitle");
        wd(cx, t).text = p.name.clone();
        let character = kind == 9;
        set_vis(cx, "CharacterDetails", if character && !self.owned { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, "ItemDescription", if character { Vis::Collapsed } else { Vis::Visible });
        let ty = type_text(cx, kind);
        if character {
            let d = self.character_details(cx, p);
            let w = find(cx, "CharacterDetails");
            wd(cx, w).text = d;
        }
        let desc = if kind == KIND_BUNDLE {
            cx.view.game_text(FMT_BUNDLE).replace("{bundletext}", &p.sub_text)
        } else if matches!(kind, 1 | 2) {
            let f = fighter_name(cx, p.fighter.as_deref());
            cx.view.game_text(FMT_FIGHTER_TYPE).replace("{fighter}", &f).replace("{type}", &ty)
        } else if rarity_byte(&p.rarity) >= 1 {
            let r = cx.view.game_text(RARITY_TEXT[rarity_byte(&p.rarity) as usize]).to_string();
            cx.view.game_text(FMT_RARITY_TYPE).replace("{rarity}", &r).replace("{type}", &ty)
        } else {
            cx.view.game_text(FMT_TYPE).replace("{type}", &ty)
        };
        let w = find(cx, "ItemDescription");
        wd(cx, w).text = desc;
        // Get Item Icon: a character's class icon, else the type's
        let class_icon = if character {
            roster_of(cx, &p.slug).and_then(|r| cx.view.class_info(&r.class)).map(|c| c.icon.clone())
        } else {
            None
        };
        let icon = class_icon.or_else(|| type_icon(kind).map(str::to_string));
        let i = find(cx, "Icon");
        let tex = icon.as_deref().map(|n| resolve(&cx.view.bundle, n));
        let x = wd(cx, i);
        x.vis = if tex.is_some() { Vis::Visible } else { Vis::Hidden };
        x.tex = tex;
    }

    /// Character Details (Base @2047-2973): "{class} | {type} | {universe}" of the character's CharacterData.
    // Unverified: CharacterTypeDisplayName's texts are not in the extract; the roster's EFighterType tail stands for
    // {type}.
    fn character_details(&self, cx: &Cx<'_>, p: &ShopProduct) -> String {
        let Some(r) = roster_of(cx, &p.slug) else { return String::new() };
        let class = cx.view.class_info(&r.class).map(|c| c.display_name.clone()).unwrap_or_default();
        cx.view
            .game_text(FMT_CHARACTER_DETAILS)
            .replace("{class}", &class)
            .replace("{type}", &r.kind)
            .replace("{universe}", &r.property)
    }

    /// SetItemPricing: the three labels Collapsed, then each displayed price (CurrencyCosts in order) Set Price and
    /// Visible.
    fn set_item_pricing(&mut self, cx: &mut Cx<'_>, p: &ShopProduct) {
        let labels = ["PrimaryPrice", "SecondaryPrice", "TertiaryPrice"];
        for n in labels {
            set_vis(cx, n, Vis::Collapsed);
        }
        for (i, &(c, amount)) in p.costs.iter().take(3).enumerate() {
            let w = find(cx, labels[i]);
            wd(cx, w).vis = Vis::Visible;
            cx.visit::<PriceLabel, _>(w, |l, cx| l.set_price(cx, amount, c, false));
        }
    }

    /// Set Item Colors @14860: the rarity stripe and description colour, FocusedColor / UnfocusedColour by type,
    /// Apply Background Color(UnfocusedColour), Apply Numerical Animations(0).
    fn set_item_colors(&mut self, cx: &mut Cx<'_>) {
        let rc = self.rarity_color;
        let stripe = find(cx, "RarityStripe");
        quad_color(cx, stripe, rc);
        let k = self.kind;
        set_vis(cx, "RarityStripe", if matches!(k, 9 | 16 | 12) { Vis::Collapsed } else { Vis::HitTestInvisible });
        let d = find(cx, "ItemDescription");
        wd(cx, d).color = rc;
        let bg = Self::color_prop(cx, "BackgroundColor", [0.003922, 0.007843, 0.019608, 1.0]);
        let def = Self::color_prop(cx, "DefaultColor", [0.066667, 0.101961, 0.152941, 1.0]);
        let plain = matches!(k, 6 | 5 | 15);
        self.unfocused_color = if plain { bg } else { def };
        self.focused_color = if self.owned {
            def
        } else if plain {
            bg
        } else {
            rc
        };
        self.apply_background_color(cx, self.unfocused_color);
        self.apply_numerical_animations(cx, 0.0);
    }

    /// Apply Background Color @11468: topColorQuad solid, the stripe quads, coloredGradient from the colour to it at
    /// alpha 0.
    // Unverified: UI_Functions.ModifiedColor(c, 0, 0, true, 0) is taken as the colour with alpha 0.
    fn apply_background_color(&mut self, cx: &mut Cx<'_>, c: Rgba) {
        self.actual_color = c;
        let top = find(cx, "topColorQuad");
        ww_solid(cx, top, c, [0.0; 4]);
        for n in ["StripeQuad1", "StripeQuad2"] {
            let q = find(cx, n);
            quad_color(cx, q, c);
        }
        let mut faded = c;
        faded[3] = 0.0;
        let g = find(cx, "coloredGradient");
        ww_gradient(cx, g, c, faded, faded);
    }

    /// Apply Numerical Animations @15930: the icon's opacity, the owned overlay's, the picture's scale (types 2, 5,
    /// 8, 9, 11) or lift (1, 4, 15, 16).
    fn apply_numerical_animations(&mut self, cx: &mut Cx<'_>, t: f64) {
        self.focused_linear = t;
        let smooth = ease_in_out(t);
        let i = find(cx, "Icon");
        wd(cx, i).opacity = lerp(0.5, 1.0, t);
        let o = find(cx, "OwnedOverlay");
        wd(cx, o).opacity = lerp(1.0, 0.0, smooth);
        let hero = find(cx, "HeroImageScaleBox");
        match self.kind {
            2 | 5 | 8 | 9 | 11 => {
                let s = lerp(1.0, 1.06, smooth);
                wd(cx, hero).transform.scale = [s, s];
            }
            1 | 4 | 15 | 16 => wd(cx, hero).transform.translation = [0.0, lerp(0.0, -10.0, smooth)],
            _ => {}
        }
    }

    /// Apply Feather Colour(Feathering, colour) @17054: Feather HitTestInvisible (Hidden without Feathering), the
    /// four feathers' gradients from the colour to it at alpha 0 (Outline Color focused, Shop Background Color not).
    // Unverified: the right / down feathers' gradient order is taken as the mirror of the left / up ones (@17290 is
    // read for FeatherLeft only).
    fn apply_feather_colour(&mut self, cx: &mut Cx<'_>) {
        let on = cx.view.tree().props(cx.w).get("Feathering").and_then(Value::as_bool).unwrap_or(true);
        set_vis(cx, "Feather", if on { Vis::HitTestInvisible } else { Vis::Hidden });
        if !on {
            return;
        }
        let c = if self.focused {
            Self::color_prop(cx, "Outline Color", [0.896269, 0.973445, 0.871367, 1.0])
        } else {
            Self::color_prop(cx, "Shop Background Color", [0.009134, 0.01033, 0.029557, 1.0])
        };
        let mut z = c;
        z[3] = 0.0;
        for (n, a, b) in [("FeatherLeft", c, z), ("FeatherRight", z, c), ("FeatherUp", c, z), ("FeatherDown", z, c)] {
            let q = find(cx, n);
            ww_gradient(cx, q, a, b, [0.0; 4]);
        }
    }

    /// OnChangeFocusInternal @11837: Launch Focus Transition to the focused or unfocused colour, the feathers.
    fn change_focus(&mut self, cx: &mut Cx<'_>, focus: bool) {
        if self.focused == focus {
            return;
        }
        self.focused = focus;
        self.dest_color = if focus { self.focused_color } else { self.unfocused_color };
        self.start_color = self.actual_color;
        self.dest_focused = if focus { 1.0 } else { 0.0 };
        self.start_focused = self.focused_linear;
        self.anim_time = 0.0;
        self.animating = true;
        self.apply_feather_colour(cx);
    }

    /// SetDoubleHeight @16893: the lower interactor and its focus frame Visible.
    pub fn set_double_height(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "UI_Interactor_Lower", Vis::Visible);
        set_vis(cx, "UI_FocusedCommon_Lower", Vis::Visible);
    }

    /// Update Context @687.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let (a, b) = (find(cx, "UI_Interactor"), find(cx, "UI_Interactor_Lower"));
        batch_set_context(cx, &[a, b], ctx);
    }

    /// The cell's interactor.
    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    /// ShopCellInteract (Base @9085 -> @3972-5521): an owned product does nothing; a bundle opens BundleDetails, any
    /// other product ItemDetails, each with Init_WithProductSlug.
    fn interact(&mut self, cx: &mut Cx<'_>) {
        let Some(p) = self.product.clone() else { return };
        if self.owned {
            return;
        }
        if p.is_bundle() {
            if let Some(m) = Manager::show_modal(cx, "UI_WWContextualUnlockModalBundle", "BundleDetails", "", "") {
                cx.visit::<BundleDetails, _>(m, |d, cx| d.init_with_product_slug(cx, &p.slug));
            }
        } else if let Some(m) = Manager::show_modal(cx, "UI_ContextualUnlockModalCharacter", "ItemDetails", "", "") {
            cx.visit::<ItemDetails, _>(m, |d, cx| d.init_with_product_slug(cx, &p.slug, ""));
        }
        cx.emit(self, |c| &c.on_opened_modal, &());
    }
}

impl Controller for ShopItemCell {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        for (iw, fc) in [("UI_Interactor", "UI_FocusedCommon"), ("UI_Interactor_Lower", "UI_FocusedCommon_Lower")] {
            let i = find(cx, iw);
            bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, CELL_FOCUS);
            bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, CELL_UNFOCUS);
            bind(cx, i, |s: &mut InteractorState| &mut s.on_click, CELL_CLICK);
            let f = find(cx, fc);
            cx.visit::<FocusedCommon, _>(f, |f, cx| f.set_interactor(cx, i));
        }
        set_vis(cx, "PurchasedText", Vis::Collapsed);
    }

    /// Tick @11296 -> CustomTickEvent: the focus transition over Duration.
    fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if !self.animating {
            return;
        }
        let dur = prop_f64(cx, "Duration", 0.1);
        self.anim_time += dt;
        let a = (self.anim_time / dur).clamp(0.0, 1.0);
        self.apply_numerical_animations(cx, lerp(self.start_focused, self.dest_focused, a));
        let c = std::array::from_fn(|i| lerp(self.start_color[i], self.dest_color[i], a));
        self.apply_background_color(cx, c);
        if self.anim_time >= dur {
            self.animating = false;
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            CELL_FOCUS => self.change_focus(cx, true),
            CELL_UNFOCUS => self.change_focus(cx, false),
            CELL_CLICK => self.interact(cx),
            _ => {}
        }
    }
}

// ---------- UI_WW_Toggle ----------

/// UI_WW_Toggle (the filter grid's toggles): PreConstruct @227 and Construct @1315. Not ported: the click's SetOn and
/// ToggleChangedEvent (the filter grid's toggles filter nothing in the port), the interactable look (@685-1286,
/// @1909-2260), the tooltip.
#[derive(Debug, Default)]
pub struct WWToggle;

impl WWToggle {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(WWToggle)
    }
}

impl Controller for WWToggle {
    /// PreConstruct @227: HoverBacking, the dividers, the label shown, the height, RemovePadding.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        set_vis(
            cx,
            "HoverBacking",
            if prop_bool(cx, "HIdeHoverBacking") { Vis::Hidden } else { Vis::SelfHitTestInvisible },
        );
        set_vis(cx, "DivTop", if prop_bool(cx, "HideTopDiv") { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        set_vis(cx, "DivBottom", if prop_bool(cx, "HideBottomDiv") { Vis::Hidden } else { Vis::SelfHitTestInvisible });
        let short = prop_bool(cx, "UseShortWrapLabel");
        let sw = find(cx, "WidgetSwitcher_0");
        wd(cx, sw).active = i32::from(short);
        let h = prop_f64(cx, "In Height Override", 120.0);
        let sb = find(cx, "SizeBox_1");
        wd(cx, sb).h_override = h;
        if prop_bool(cx, "RemovePadding") {
            let hb = find(cx, "HorizontalBox_1");
            wd(cx, hb).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 0.0 };
        }
    }

    /// Construct @1315: the focus frame's interactor, both labels LabelText.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        let f = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(f, |f, cx| f.set_interactor(cx, i));
        let t = text(Some(&prop(cx, "LabelText")));
        for n in ["Label", "ShortWrapLabel"] {
            let w = find(cx, n);
            wd(cx, w).text = t.clone();
        }
    }
}

// ---------- UI_WW_GridHeader ----------

/// UI_WW_GridHeader.
#[derive(Debug, Default)]
pub struct GridHeader;

impl GridHeader {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GridHeader)
    }

    /// Set Title(Display Rule, Header Text): HorizontalRule Visible or Collapsed, the text.
    pub fn set_title(&mut self, cx: &mut Cx<'_>, rule: bool, t: &str) {
        set_vis(cx, "HorizontalRule", if rule { Vis::Visible } else { Vis::Collapsed });
        let w = find(cx, "GridHeaderText");
        wd(cx, w).text = t.to_string();
    }
}

impl Controller for GridHeader {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let rule = prop_bool(cx, "Display Rule");
        let t = text(Some(&prop(cx, "Header Text")));
        self.set_title(cx, rule, &t);
    }
}

// ---------- UI_WWShopProductGridGeneric ----------

/// UI_WWShopProductGridGeneric.
/// Not ported: the cells' navigation (Memorize Cell Locations, Get Next Cell, Internal Update Cell Navigation:
/// the manager's own spatial navigation moves between cells), OwnedProductsText (DisplayOwnedProductsText is not
/// called), TickShopCells.
// Deviation: the build creates the cells over frames (Internal_AddCells, CellsFinishedCreation); the port makes them
// at once.
#[derive(Debug, Default)]
pub struct ProductGrid {
    pub cells: Vec<WidgetId>,
    empties: Vec<WidgetId>,
    context: String,
    pub max_cols: i32,
    pub min_cols: i32,
}

impl ProductGrid {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<ProductGrid>::default()
    }

    /// ClearGrid: GridPanel's children go.
    pub fn clear_grid(&mut self, cx: &mut Cx<'_>) {
        for c in std::mem::take(&mut self.cells).into_iter().chain(std::mem::take(&mut self.empties)) {
            MenuView::discard(cx, c);
        }
    }

    /// SetLoadingStatus(IsLoaded) @9435.
    pub fn set_loading_status(&mut self, cx: &mut Cx<'_>, loaded: bool) {
        set_vis(cx, "LoadedContent", if loaded { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, "PendingLoad", if loaded { Vis::Collapsed } else { Vis::Visible });
    }

    /// Set Title(Display Header, Rule, Header Text) (PreConstruct): HorizontalBox_0 HitTestInvisible or Collapsed,
    /// the Header's Set Title.
    fn set_title(&mut self, cx: &mut Cx<'_>, show: bool, rule: bool, t: &str) {
        set_vis(cx, "HorizontalBox_0", if show { Vis::HitTestInvisible } else { Vis::Collapsed });
        let h = find(cx, "Header");
        let t = t.to_string();
        cx.visit::<GridHeader, _>(h, |g, cx| g.set_title(cx, rule, &t));
    }

    /// Set Products(products, Containing Tab Name) @8924-9402 -> Internal_SetProducts: the grid cleared, the content
    /// or the "No items to display." text, the packer (the bundles tab its mosaic and Fill Row Completely), the
    /// grid's size (at least Min Columns x Min Rows squares of CellPixelSize), a WW_ShopItemCell per product in its
    /// squares, UI_WW_EmptyWidget in each square left.
    pub fn set_products(&mut self, cx: &mut Cx<'_>, products: Vec<ShopProduct>, tab: &str, show_prices: bool) {
        self.clear_grid(cx);
        let has = !products.is_empty();
        set_vis(cx, "PresentContent", if has { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, "NoContent", if has { Vis::Collapsed } else { Vis::Visible });
        let fill_row = tab == "bundles";
        let custom = fill_row.then(|| mosaic_sizes(products.len()));
        let max_cols = self.max_cols.max(1);
        let sizes: Vec<(i32, i32)> = products
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let kind = if p.is_bundle() { KIND_BUNDLE } else { p.kind };
                slot_size(kind, custom.as_ref().and_then(|c| c.get(i).copied()), max_cols)
            })
            .collect();
        let packed = pack(&sizes, max_cols, fill_row);
        let min_rows = prop_f64(cx, "Min Rows", 1.0) as i32;
        let width = packed.cols.max(self.min_cols);
        let height = packed.rows.max(min_rows);
        let cell_px = prop(cx, "CellPixelSize");
        let px = |k: &str, d: f64| cell_px.get(k).and_then(Value::as_f64).unwrap_or(d);
        let gutter = prop_f64(cx, "Item Gutter", 15.0);
        let size = find(cx, "GridPanelSize");
        let x = wd(cx, size);
        x.w_override = f64::from(width) * px("X", 58.0);
        x.h_override = f64::from(height) * px("Y", 350.0);
        let grid = find(cx, "GridPanel");
        let g = wd(cx, grid);
        g.col_fill = vec![1.0; width as usize];
        g.row_fill = vec![1.0; height as usize];
        let half = gutter / 2.0;
        g.pad = Margin { l: -half, t: -half, r: -half, b: -half };
        let mut taken = vec![false; (width * height) as usize];
        let ctx = self.context.clone();
        for GridSlot { index, col, row, w, h } in packed.slots {
            let cell = MenuView::create(cx, "WW_ShopItemCell");
            let x = wd(cx, cell);
            x.col = col;
            x.row = row;
            x.col_span = w;
            x.row_span = h;
            x.h_align = HAlign::Fill;
            x.v_align = VAlign::Fill;
            x.pad = Margin { l: half, t: half, r: half, b: half };
            MenuView::add_child(cx, grid, cell, None);
            let p = products[index].clone();
            cx.visit::<ShopItemCell, _>(cell, |c, cx| {
                c.receive_product(cx, p, show_prices);
                if h > 1 {
                    c.set_double_height(cx);
                }
                c.update_context(cx, &ctx);
            });
            for c in col..(col + w).min(width) {
                for r in row..(row + h).min(height) {
                    taken[(r * width + c) as usize] = true;
                }
            }
            self.cells.push(cell);
        }
        // Internal Fill Empty Space
        // Unverified: every square of the Width x Height grid that no cell takes gets an empty widget.
        for r in 0..height {
            for c in 0..width {
                if taken[(r * width + c) as usize] {
                    continue;
                }
                let e = MenuView::create(cx, "UI_WW_EmptyWidget");
                let x = wd(cx, e);
                x.col = c;
                x.row = r;
                x.col_span = 1;
                x.row_span = 1;
                MenuView::add_child(cx, grid, e, None);
                self.empties.push(e);
            }
        }
        self.set_loading_status(cx, true);
    }

    /// Update Context: the cells' interactors.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        for &c in &self.cells.clone() {
            cx.visit::<ShopItemCell, _>(c, |s, cx| s.update_context(cx, ctx));
        }
    }

    /// The cells' interactors.
    pub fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        self.cells.iter().map(|&c| ShopItemCell::interactor(cx, c)).collect()
    }
}

impl Controller for ProductGrid {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.context = NULL_GUID.to_string();
        self.max_cols = prop_f64(cx, "Max Columns", 30.0) as i32;
        self.min_cols = prop_f64(cx, "Min Columns", 30.0) as i32;
    }

    /// PreConstruct: Set Title.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let show = prop_bool(cx, "Display Header");
        let rule = prop_bool(cx, "Display Header Rule");
        let t = text(Some(&prop(cx, "Header Text")));
        self.set_title(cx, show, rule, &t);
    }
}

// ---------- UI_WWFilterGrid ----------

const F_SORT: u32 = 1;
const F_UNIVERSE: u32 = 2;

/// GetSortOrderLabel's keys by ESortingOrder byte (the ones the grid offers), and "Sort by {0}".
const SORT_LABEL: [(u8, &str); 5] = [
    (SORT_NEWEST, "51B2A6CB4A28ABF17DA4D1B60BC93E31"),
    (SORT_ALPHABETICAL, "F7E5F68248925D53AC8D9FAE110E3923"),
    (SORT_CHARACTER, "8B6FF6164A7DE6AEE473F089B2DC61E3"),
    (SORT_RARITY, "2936D7A249C6968C7ADD29942A01A574"),
    (SORT_COST, "F815ABDD4A2A08E1F54C77AFA97E392F"),
];
const SORT_BY: &str = "29BFF24748C244C59CB2F49C070D4EE7";
const ALL_UNIVERSES: &str = "94895B414F6A8E9CA33C96907B0C4C63";

/// ESortingOrder's name of the byte (the drop menu's ID).
fn sort_name(b: u8) -> &'static str {
    match b {
        SORT_ALPHABETICAL => "Alphabetical",
        SORT_CHARACTER => "Character",
        SORT_NEWEST => "Newest",
        SORT_RARITY => "Rarity",
        _ => "Cost",
    }
}

/// UI_WWFilterGrid: the tab's title and icon, the universe and sort menus; the toggles show as the tree has them.
/// Not ported: the On Sale, Show Purchased and Fighter Owned toggles' filtering and Clear Filters (no sale or
/// ownership in the port).
#[derive(Debug, Default)]
pub struct FilterGrid {
    /// The products before the filter, to filter again on a change.
    pub products: Vec<ShopProduct>,
    /// Filters changed: the page sets its products again.
    pub on_changed: Event<()>,
    pub sort: u8,
    pub universe: String,
}

impl FilterGrid {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(FilterGrid { universe: "All".into(), ..FilterGrid::default() })
    }

    /// SetTabInfo: FiltersTitle (upper case) and FiltersIcon of the tab, OwnedCharactersSwitch by Has Character,
    /// the sort menu.
    // Unverified: the sort IDs are ESortingOrder's names; Character only on a tab with Has Character, Rarity only
    // with Sort by Rarity (Filter-HasSortByRarity).
    pub fn set_tab_info(
        &mut self,
        cx: &mut Cx<'_>,
        label: &str,
        icon: &str,
        has_character: bool,
        by_rarity: bool,
        sort: u8,
    ) {
        let t = find(cx, "FiltersTitle");
        wd(cx, t).text = label.to_uppercase();
        let i = find(cx, "FiltersIcon");
        let tex = resolve(&cx.view.bundle, icon);
        wd(cx, i).tex = Some(tex);
        set_vis(cx, "OwnedCharactersSwitch", if has_character { Vis::Visible } else { Vis::Collapsed });
        let by = cx.view.game_text(SORT_BY).to_string();
        let items: Vec<Value> = SORT_LABEL
            .iter()
            .filter(|(b, _)| (*b != SORT_CHARACTER || has_character) && (*b != SORT_RARITY || by_rarity))
            .map(|(b, k)| {
                let l = by.replace("{0}", cx.view.game_text(k));
                json!({ P_LABEL: { "CultureInvariantString": l }, P_ID: sort_name(*b) })
            })
            .collect();
        self.sort = if SORT_LABEL.iter().any(|(b, _)| *b == sort) { sort } else { SORT_ALPHABETICAL };
        let m = find(cx, "SortDropMenu");
        cx.view.tree_mut().set_prop(m, "Items", Value::Array(items));
        let id = sort_name(self.sort);
        cx.visit::<DropMenu, _>(m, |d, cx| {
            d.initialize(cx);
            d.set_active_by_id(cx, id);
        });
    }

    /// The universe menu of the products (FilterProductsByUniverse, NQ3): "All Universes" first, then each universe
    /// with products, sorted; VerticalBox_Universe shows with more than two entries.
    // Unverified: a universe's label is the roster's Property of a fighter of it, else its enum name.
    fn set_universes(&mut self, cx: &mut Cx<'_>) {
        let mut names: Vec<String> = self.products.iter().filter_map(|p| p.universe.clone()).collect();
        names.sort();
        names.dedup();
        let cat = catalogue(cx);
        let label = |u: &str| {
            cat.items
                .iter()
                .filter(|i| i.kind == 9 && i.universe.as_deref() == Some(u))
                .find_map(|i| roster_of(cx, &i.slug).map(|r| r.property.clone()))
                .unwrap_or_else(|| u.to_string())
        };
        let mut items =
            vec![json!({ P_LABEL: { "CultureInvariantString": cx.view.game_text(ALL_UNIVERSES) }, P_ID: "All" })];
        for u in &names {
            items.push(json!({ P_LABEL: { "CultureInvariantString": label(u) }, P_ID: u }));
        }
        set_vis(cx, "VerticalBox_Universe", if items.len() > 2 { Vis::Visible } else { Vis::Collapsed });
        let m = find(cx, "UniverseDropMenu");
        cx.view.tree_mut().set_prop(m, "Items", Value::Array(items));
        let id = self.universe.clone();
        cx.visit::<DropMenu, _>(m, |d, cx| {
            d.initialize(cx);
            d.set_active_by_id(cx, &id);
        });
    }

    /// FilterSortAndReturnProducts: the products kept, the universe menu made, the sorted and filtered list.
    pub fn filter_sort_and_return(&mut self, cx: &mut Cx<'_>, products: Vec<ShopProduct>) -> Vec<ShopProduct> {
        self.products = products;
        self.set_universes(cx);
        sort_and_filter(&self.products, self.sort, &self.universe)
    }

    /// The products again with the current filters.
    pub fn current(&self) -> Vec<ShopProduct> {
        sort_and_filter(&self.products, self.sort, &self.universe)
    }

    /// UpdateContext: the menus', toggles' and Clear Filters' interactors.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        for n in ["SortDropMenu", "UniverseDropMenu"] {
            let m = find(cx, n);
            cx.visit::<DropMenu, _>(m, |_, cx| DropMenu::update_context(cx, ctx, None));
        }
        let mut own = Vec::new();
        for n in ["OnSaleToggleSwitch", "PurchasedToggleSwitch", "OwnedCharactersSwitch", "ClearFiltersButton"] {
            let w = find(cx, n);
            let i = cx.view.tree().find_in(w, "UI_Interactor");
            own.push(i);
        }
        batch_set_context(cx, &own, ctx);
    }
}

impl Controller for FilterGrid {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for (n, id) in [("SortDropMenu", F_SORT), ("UniverseDropMenu", F_UNIVERSE)] {
            let m = find(cx, n);
            cx.visit::<DropMenu, _>(m, |d, _| d.on_click.add(Handler { owner: me, id }));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let Some((_, sid)) = arg.downcast_ref::<(i32, String)>() else { return };
        match id {
            F_SORT => {
                self.sort =
                    SORT_LABEL.iter().map(|(b, _)| *b).find(|b| sort_name(*b) == sid).unwrap_or(SORT_ALPHABETICAL);
            }
            F_UNIVERSE => self.universe = sid.clone(),
            _ => return,
        }
        cx.emit(self, |c| &c.on_changed, &());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_are_grouped() {
        assert_eq!(grouped(700.0), "700");
        assert_eq!(grouped(1500.0), "1,500");
        assert_eq!(grouped(1234567.0), "1,234,567");
        assert_eq!(grouped(-2200.0), "-2,200");
        assert_eq!(grouped(0.0), "0");
    }

    #[test]
    fn the_focus_ease() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(0.5), 0.5);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert!(ease_in_out(0.25) < 0.25);
    }
}
