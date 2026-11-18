//! The reward tooltips: WBP_RichTooltip and its children by reward type (UI_Functions TooltipClassForRewardType
//! @1302), which a UI_TooltipTarget makes at run time and gives a RewardInfo_C through Set Data. The parent's Set
//! Data @9909 -> @9562 runs, in its order, WidgetSwitcher_0's index 1, IsShopItem (UnlockLocation Shop), Tooltip
//! Visibility, Tooltip Set Rarity, Tooltip Set Unlocked & Owned, SetActionResult, Tooltip Show Star Level, Tooltip
//! Set Reward Type and Tooltip Set Reward Item; each child overrides some of them and runs its own steps after.
//!
//! Offline every reward is owned and unlocked (Deviation, as the Collection's lists: the build asks the inventory),
//! so the unlock row ends Collapsed and no action label shows. Not ported: the sale ribbon and its 1 s TickEvent
//! timer (the SaleContainer stays Collapsed), the video players.

use std::any::Any;
use std::rc::Rc;

use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx};
use crate::menu::hydra::{cosmetics, picture_name};
use crate::menu::widgets::badges::{BadgeItem, badge_look, badge_text, int_text, star_tex};
use crate::menu::widgets::common::{Quadrilateral, RARITY};
use crate::menu::widgets::controls::interactor::TooltipParams;
use crate::menu::widgets::controls::tooltip::TIP_SET_DATA;
use crate::menu::widgets::controls::{find, wd};
use crate::menu::widgets::perks::{PerkTooltipData, TooltipPerkDescription, perk_data};
use crate::slate::{Vis, resolve};

/// WBP_RichTooltip Tooltip Set Rarity @349-628: ERewardRarity Common "Uncommon", Rare "Rare", Epic "Epic!",
/// Legendary "Legendary!" (Game.locres); None has no text.
const T_RARITY: [(&str, &str); 4] = [
    ("Common", "0FCDEF47485E2C3D0D477988C481D8E3"),
    ("Rare", "18241CA7441AE16AAFB6EFAB499FF981"),
    ("Epic", "14692D5247150F029BF261A2AB60C421"),
    ("Legendary", "BB4BE74A4EE86D8E3CCF78A7669BEB40"),
];
/// Tooltip Set Reward Type @1365: GetEnumeratorUserFriendlyName(ERewardType), the UserDefinedEnum's display names
/// by value (Game.locres).
const T_REWARD_TYPE: [&str; 17] = [
    "A5B391114D8114CEECF5C0A3B62B1AFF",
    "F1B55AC9458680C97DE56085AF22EC71",
    "F5FFFB2C4361F4C16C63B3A89F832BA8",
    "2CD2DC5D45CF32FD3203CD9DF9FC3589",
    "2E6BCE5F49FAA5547ACA97A041F437CF",
    "3DD11FD24488BFCAB0EA6586EF99945A",
    "638AFC2842A31C0450C4D99B0F9FE02F",
    "9C4BA54546C4F1862415A5A558C2A870",
    "65A0DB4848AA30DCEF4BF59362D8E5BB",
    "17F07EF3426D0598673C94929B25E10D",
    "FA07E5F14135BF084565BBA0D76BB8FE",
    "C9E3E0F7420105438A9753A9277F5070",
    "CB7985B54D6549B52DCB8AA5DCF5595E",
    "64EBD7394B14E66DB3798E82FB46E679",
    "2016EDEE4232C3EAB61B389D900FAC08",
    "78A5AB294F8FE7A4FD993C932F8E568B",
    "BC77A2584A66BE9670574DB31E0A1B4F",
];
/// Tooltip Set Reward Type @38-299: RewardTypeIcon by ERewardType (None for 12, 13, 14 and 16).
const TYPE_ICON: [Option<&str>; 17] = [
    Some("_Black"),
    Some("ui_icons_skins"),
    Some("ui_icons_emote"),
    Some("_Black"),
    Some("ui_icons_tracker"),
    Some("ui_icons_profileicon"),
    Some("ui_icons_ringout"),
    Some("_Black"),
    Some("ui_icons_emote"),
    Some("ui_icons_fighters"),
    Some("_Black"),
    Some("ui_icons_announcer"),
    None,
    None,
    None,
    Some("ui_icons_banner"),
    None,
];
/// RewardInfo's CharMap (its class default): EFighter's tail to the fighter's name (Game.locres). A fighter not in
/// it has the empty name (Map_Find's miss).
const CHAR_MAP: [(&str, &str); 20] = [
    ("TomAndJerry", "8088A1224ED9239329945084B3E88BED"),
    ("Shaggy", "AF441BFE43EF4C5188D60CA1BCE68C67"),
    ("StevenUniverse", "A321D78349E4D64A11A31E92D99520F3"),
    ("Arya", "5586967B45993769CB31EEB75180A3ED"),
    ("Batman", "011AED8A4E960D0F4A471787475A5909"),
    ("ReinDog", "ED4C1A5949C1944230CE68AC47C97CD1"),
    ("Jake", "53B81F2040DF14FDFD6E3385397AA9CB"),
    ("Superman", "6079FFBA406F64CB082817A166CE85B1"),
    ("WonderWoman", "3ABA35AA480251E5F68931846FC3BCD5"),
    ("BugsBunny", "D8B82A5248EE8300452841BFA911943A"),
    ("HarleyQuinn", "4F0A7FB9494041CC66CA91894E51B41F"),
    ("Garnet", "C99178D345F384477004839BE6BECDFE"),
    ("Finn", "F065862640F8EEE2D08E1291D0728B72"),
    ("Velma", "CE51809D4097E60FAA4FDDBC56B4B5E8"),
    ("C015", "CC3DED2547C9E9C4BEDACAB250F4C75A"),
    ("C017", "8A87049E48E8994B52D55FA68BD02C64"),
    ("C016", "7FD99CE64BFDDA0D3794F8B3B3A66163"),
    ("C019", "AF4F02164361CF360F8603815EA2EFA7"),
    ("C020", "29A097C040D67AC3454E0FB790B1D734"),
    ("C023A", "B6494BB94D8840299C99BCAAE575E02B"),
];
/// Tooltip SetUnlockText @153: "Unlocked!".
const T_UNLOCKED: &str = "90214DDE4BC80C07AD77C9983462BBA2";
/// WBP_PerkTooltip Tooltip SetUnlockText @302: an unlocked perk's line.
const T_PERK_UNLOCKED: &str = "3AB58FE644BD9DF1D4BAB8832726D487";
/// Tooltip Set Reward Item @469: "{rewardname} x{amount}".
const T_COUNT: &str = "85B15BEB4F8E5E591D161A9E9EBF77CB";
/// WBP_VariantTooltip / WBP_TauntTooltip Tooltip Set Reward Type @659: "{Character} {RewardType}".
const T_VARIANT_TYPE: &str = "2ADF346541EE955B3A4AA1A80161D6C9";
const T_TAUNT_TYPE: &str = "50869540488060804FCF09AD1C348D39";
/// The signature perk's type line of WBP_ProfileIconTooltip and WBP_PerkTooltip (Category 3).
const T_ICON_SIGNATURE: &str = "05D5A9F74C0838B3364CA185DCFC1EA6";
const T_PERK_SIGNATURE: &str = "1ED68E1B4BA83234DC5DEDBD7B572C44";
/// WBP_EmoteTooltip Tooltip Set Reward Type @19: "Sticker".
const T_STICKER: &str = "A51AE1834B2CF69F9DEC938E06217D7E";

/// Tooltip Set Rarity's (and Set Reward Type's) colour of a character-specific reward (Category 3).
const CHARACTER_SPECIFIC: Rgba = [1.0, 0.215861, 0.0, 1.0];
/// SetUnlockText's green.
const UNLOCKED_GREEN: Rgba = [0.007499, 0.693872, 0.258183, 1.0];
/// WBP_RingoutTooltip's dark outline, its checkmark and name colours.
const RINGOUT_DARK: Rgba = [0.009721, 0.011612, 0.033105, 1.0];
const RINGOUT_ICON: Rgba = [0.008023, 0.693872, 0.258183, 1.0];
const RINGOUT_NAME: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];

/// ERewardType of a stat-tracker badge.
pub const REWARD_BADGE: u8 = 4;

/// RewardInfo_C as CreateFromAsset @335 fills it: DisplayName, Count, StarLevel, Rarity (ERewardRarity's tail),
/// Category, Tier (DetermineRewardTier), the EUnlockLocation byte (None 0, Shop 1, AccountLevel 2,
/// CharacterMastery 3, Battlepass 4), AssociatedCharacter (EFighter's tail), Thumbnail (RewardThumbnail) and
/// TooltipPreview (ToolTipPreviewMaterial) as bundle picture names, the asset's name (Custom Tooltip Items' key, a
/// PerkData's id), and a badge's data (GetBadgeText and its pictures by count).
#[derive(Clone, Debug, Default)]
pub struct RewardInfo {
    pub reward_type: u8,
    pub display_name: String,
    pub count: f64,
    pub star_level: u32,
    pub rarity: String,
    pub category: u8,
    pub tier: u32,
    pub unlock: u8,
    pub fighter: Option<String>,
    pub thumb: Option<String>,
    pub preview: Option<String>,
    pub asset: String,
    pub badge: Option<BadgeItem>,
}

/// EUnlockLocation's byte by its tail.
pub fn unlock_byte(tail: Option<&str>) -> u8 {
    match tail {
        Some("Shop") => 1,
        Some("AccountLevel") => 2,
        Some("CharacterMastery") => 3,
        Some("Battlepass") => 4,
        _ => 0,
    }
}

/// A build path's bundle picture name.
pub fn thumb_of(path: Option<&str>) -> Option<String> {
    path.and_then(picture_name)
}

/// UI_Functions TooltipClassForRewardType @1302: the tooltip class of a reward type (None: none).
pub fn tooltip_class(t: u8) -> Option<&'static str> {
    Some(match t {
        0 | 14 => "WBP_RichTooltip",
        1 => "WBP_VariantTooltip",
        2 => "WBP_TauntTooltip",
        4 => "WBP_BadgeTooltip",
        5 => "WBP_ProfileIconTooltip",
        6 => "WBP_RingoutTooltip",
        8 => "WBP_EmoteTooltip",
        11 => "WBP_AnnouncerTooltip",
        12 => "WBP_CurrencyTooltip",
        13 => "WBP_PerkTooltip",
        15 => "WBP_BannerTooltip",
        _ => return None,
    })
}

/// The tooltip params of a reward: its type's class, Placement 12 MatchBottomLeft, Delay 0, Data the RewardInfo
/// (UI_EquipperItem_V2 GetDefaultTooltipParams, WBP_StatBadge @846-1212).
pub fn reward_tooltip_params(info: &RewardInfo, tooltip: String, show: bool, x: f64, y: f64) -> TooltipParams {
    TooltipParams {
        tooltip,
        cls: tooltip_class(info.reward_type).map(str::to_string),
        show,
        offset: [x, y],
        delay: 0.0,
        placement: 12,
        data: Some(Rc::new(info.clone()) as Rc<dyn Any>),
    }
}

/// The class a controller drives.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Rich,
    Badge,
    Variant,
    Taunt,
    ProfileIcon,
    Ringout,
    Emote,
    Announcer,
    Currency,
    Perk,
    Banner,
}

/// One reward tooltip. OnInitialized @1665 -> @118 plays AnimIn; Show, Hide and Set Text are empty events.
#[derive(Debug)]
pub struct RewardTooltip {
    kind: Kind,
}

fn set_vis(cx: &mut Cx<'_>, n: &str, v: Vis) {
    let w = find(cx, n);
    wd(cx, w).vis = v;
}

/// Widget Visibility(Collapsed, Not Hit-Testable (Self Only), Hidden) in that order.
fn widget_visibility(cx: &mut Cx<'_>, collapsed: &[&str], self_hti: &[&str], hidden: &[&str]) {
    for n in collapsed {
        set_vis(cx, n, Vis::Collapsed);
    }
    for n in self_hti {
        set_vis(cx, n, Vis::SelfHitTestInvisible);
    }
    for n in hidden {
        set_vis(cx, n, Vis::Hidden);
    }
}

/// Widget Set Render: the render transform (no shear, no angle) and the opacity.
fn set_render(cx: &mut Cx<'_>, n: &str, at: [f64; 2], scale: f64, opacity: f64) {
    let w = find(cx, n);
    let x = wd(cx, w);
    x.transform.translation = at;
    x.transform.scale = [scale, scale];
    x.transform.shear = [0.0, 0.0];
    x.transform.angle = 0.0;
    x.opacity = opacity;
}

/// Widget Set Brush Load Asset with no scalar offsets: the picture as the image's brush; nothing for a reward
/// without one (the cast fails, the brush stays).
fn set_brush(cx: &mut Cx<'_>, n: &str, pic: Option<&str>) {
    let Some(p) = pic else { return };
    let p = resolve(&cx.view.bundle, p);
    cx.view.load_texture(&p);
    let w = find(cx, n);
    let x = wd(cx, w);
    x.tex = Some(p);
    x.mid = None;
}

fn rarity_base(r: &RewardInfo) -> Rgba {
    RARITY.iter().find(|(n, _)| *n == r.rarity).map_or(RARITY[0].1, |(_, c)| *c)
}

fn type_name(cx: &Cx<'_>, t: u8) -> String {
    T_REWARD_TYPE.get(t as usize).map(|k| cx.view.game_text(k).to_string()).unwrap_or_default()
}

impl RewardTooltip {
    fn of(kind: Kind) -> Box<dyn Controller> {
        Box::new(RewardTooltip { kind })
    }

    pub fn make_rich(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Rich)
    }

    pub fn make_badge(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Badge)
    }

    pub fn make_variant(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Variant)
    }

    pub fn make_taunt(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Taunt)
    }

    pub fn make_profile_icon(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::ProfileIcon)
    }

    pub fn make_ringout(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Ringout)
    }

    pub fn make_emote(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Emote)
    }

    pub fn make_announcer(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Announcer)
    }

    pub fn make_currency(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Currency)
    }

    pub fn make_perk(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Perk)
    }

    pub fn make_banner(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Self::of(Kind::Banner)
    }

    /// Set Data: the parent's @9562 in its order, then the child's own steps.
    pub fn set_data(&mut self, cx: &mut Cx<'_>, r: &RewardInfo) {
        let sw = find(cx, "WidgetSwitcher_0");
        wd(cx, sw).active = 1;
        let is_shop = r.unlock == 1;
        self.visibility(cx, r);
        // Tooltip Set Rarity(Rarity, Category == 3)
        let base = if r.category == 3 { CHARACTER_SPECIFIC } else { rarity_base(r) };
        for q in ["QuadBG_Trim", "Quad_IconBG"] {
            let w = find(cx, q);
            cx.visit::<Quadrilateral, _>(w, |q, cx| q.update_color(cx, base));
        }
        let rarity = T_RARITY
            .iter()
            .find(|(n, _)| *n == r.rarity)
            .map(|(_, k)| cx.view.game_text(k).to_string())
            .unwrap_or_default();
        let rt = find(cx, "Rarity_Text");
        wd(cx, rt).text = rarity;
        // Tooltip Set Unlocked & Owned: unlocked, the checkmark @4681; owned, so no action label @4852; Event Fetch
        // Product @1151 runs SetUnlockText at once for a reward that is not a shop item.
        // Unverified: a shop item's FetchProductFromInventorySlug read as never answering offline, so its
        // SetUnlockText does not run.
        let icon = find(cx, "icon");
        let t = resolve(&cx.view.bundle, "ui_icons_checkmark");
        wd(cx, icon).tex = Some(t);
        if !is_shop {
            self.set_unlock_text(cx);
        }
        // Unverified: SetActionResult with no action for an owned reward read as the label Collapsed
        set_vis(cx, "UI_InputLabel", Vis::Collapsed);
        // Tooltip Show Star Level(StarLevel): 0 Hidden; else seen with the tier picture
        let star = find(cx, "StarLevel");
        if r.star_level == 0 {
            wd(cx, star).vis = Vis::Hidden;
        } else {
            let t = star_tex(cx, r.star_level);
            let w = wd(cx, star);
            w.vis = Vis::SelfHitTestInvisible;
            w.tex = Some(t);
        }
        self.set_reward_type(cx, r, base);
        self.set_reward_item(cx, r);
        match self.kind {
            Kind::Currency => Self::custom_tooltip_item(cx, r),
            Kind::Perk => Self::set_perk_data(cx, r),
            Kind::Ringout => Self::ringout_colors(cx, is_shop),
            _ => {}
        }
    }

    /// Tooltip Visibility: the parent's (SmallRewardOverlay Collapsed), then the child's.
    fn visibility(&self, cx: &mut Cx<'_>, r: &RewardInfo) {
        const HBOX: &str = "HBox_UnlockLocation";
        widget_visibility(cx, &["SmallRewardOverlay"], &[], &[]);
        let t0 = r.tier == 0;
        // the HBox by the Tier test: Collapsed when it holds, seen when not
        let (on, off): (&[&str], &[&str]) = if t0 { (&[HBOX], &[]) } else { (&[], &[HBOX]) };
        let pair =
            |a: &[&'static str], b: &[&'static str]| -> Vec<&'static str> { a.iter().chain(b).copied().collect() };
        match self.kind {
            Kind::Rich => {}
            Kind::Variant | Kind::Taunt | Kind::Badge => {
                widget_visibility(cx, &[], &["RewardFrameBreak"], &[]);
                widget_visibility(cx, &pair(&["Quad_RewardBacking"], on), &pair(&["Overlay_RewardType"], off), &[]);
            }
            Kind::Emote => {
                widget_visibility(cx, &["RewardSmall"], &["Sticker_Preview", "SmallRewardOverlay"], &[]);
                widget_visibility(cx, &pair(&["Quad_RewardBacking"], on), &pair(&["Overlay_RewardType"], off), &[]);
            }
            Kind::Banner => {
                // Deviation: Banner_Preview's brush is a scene render target of the banner's template actor
                // (Widget Set Brush Load Asset @1223-3021), which the port does not draw: it stays Collapsed
                widget_visibility(cx, &["RewardFrameBreak"], &[], &[]);
                widget_visibility(cx, &pair(&["Quad_RewardBacking"], on), &pair(&["Overlay_RewardType"], off), &[]);
            }
            Kind::ProfileIcon | Kind::Currency | Kind::Perk => {
                widget_visibility(cx, &[], &["SmallRewardOverlay"], &[]);
                // WBP_PerkTooltip tests Tier 0 and UnlockLocation Shop or None
                let c = if self.kind == Kind::Perk { t0 && r.unlock <= 1 } else { t0 };
                let (on, off): (&[&str], &[&str]) = if c { (&[HBOX], &[]) } else { (&[], &[HBOX]) };
                widget_visibility(cx, on, &pair(&["Quad_RewardBacking"], off), &["Overlay_RewardType"]);
            }
            Kind::Announcer => {
                widget_visibility(cx, &[], &["SmallRewardOverlay"], &[]);
                widget_visibility(cx, &pair(on, &["Quad_RewardBacking"]), &pair(off, &["Overlay_RewardType"]), &[]);
            }
            Kind::Ringout => {
                widget_visibility(cx, &pair(&["Quad_RewardBacking"], on), &pair(&["Overlay_RewardType"], off), &[]);
            }
        }
    }

    /// Tooltip SetUnlockText, unlocked (@238): "Unlocked!" in green, the unlock row Collapsed, the input label
    /// Collapsed (not claimable); WBP_PerkTooltip's then sets its own line @302.
    fn set_unlock_text(&self, cx: &mut Cx<'_>) {
        let t = cx.view.game_text(T_UNLOCKED).to_string();
        let req = find(cx, "RewardUnlockReq");
        {
            let w = wd(cx, req);
            w.text = t;
            w.color = UNLOCKED_GREEN;
        }
        set_vis(cx, "HBox_UnlockLocation", Vis::Collapsed);
        set_vis(cx, "UI_InputLabel", Vis::Collapsed);
        if self.kind == Kind::Perk {
            let t = cx.view.game_text(T_PERK_UNLOCKED).to_string();
            wd(cx, req).text = t;
        }
    }

    /// Tooltip Set Reward Type: the parent's icon and type line, then the child's.
    fn set_reward_type(&self, cx: &mut Cx<'_>, r: &RewardInfo, base: Rgba) {
        let icon = TYPE_ICON.get(r.reward_type as usize).copied().flatten();
        let ti = find(cx, "RewardTypeIcon");
        let t = icon.map(|n| resolve(&cx.view.bundle, n));
        {
            let w = wd(cx, ti);
            if t.is_some() {
                w.tex = t;
                w.mid = None;
            }
            w.vis = if icon.is_some() { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        }
        let name = type_name(cx, r.reward_type);
        let ty = find(cx, "RewardType");
        {
            let w = wd(cx, ty);
            w.text = name.clone();
            w.color = base;
            w.outline_color = WHITE;
        }
        let text = match self.kind {
            Kind::Variant | Kind::Taunt => {
                let key = if self.kind == Kind::Variant { T_VARIANT_TYPE } else { T_TAUNT_TYPE };
                let ch = r
                    .fighter
                    .as_deref()
                    .and_then(|f| CHAR_MAP.iter().find(|(n, _)| *n == f))
                    .map(|(_, k)| cx.view.game_text(k).to_string())
                    .unwrap_or_default();
                Some(cx.view.game_text(key).replace("{Character}", &ch).replace("{RewardType}", &name))
            }
            Kind::ProfileIcon if r.category == 3 => Some(cx.view.game_text(T_ICON_SIGNATURE).to_string()),
            Kind::Perk if r.category == 3 => Some(cx.view.game_text(T_PERK_SIGNATURE).to_string()),
            Kind::Emote => Some(cx.view.game_text(T_STICKER).to_string()),
            Kind::Ringout => {
                wd(cx, ty).outline_color = RINGOUT_DARK;
                None
            }
            _ => None,
        };
        if let Some(t) = text {
            wd(cx, ty).text = t;
        }
    }

    /// Tooltip Set Reward Item(DisplayName, Count): the name ("{rewardname} x{amount}" above one), then the
    /// child's picture.
    fn set_reward_item(&self, cx: &mut Cx<'_>, r: &RewardInfo) {
        let name = if self.kind == Kind::Badge {
            // WBP_BadgeTooltip's: GetBadgeText
            badge_text(r.badge.as_ref().unwrap_or(&BadgeItem::default()), &r.display_name, r.count)
        } else if r.count as i64 > 1 {
            cx.view.game_text(T_COUNT).replace("{rewardname}", &r.display_name).replace("{amount}", &int_text(r.count))
        } else {
            r.display_name.clone()
        };
        let n = find(cx, "RewardName");
        wd(cx, n).text = name;
        let cat3 = r.category == 3;
        // the spacer and RewardSmall's transform; the children but the ring-out's then set its brush
        let small_place = |cx: &mut Cx<'_>| {
            let s = find(cx, "BigThumbSpacer");
            wd(cx, s).brush_w = if cat3 { 300.0 } else { 270.0 };
            wd(cx, s).brush_h = 1.0;
            let (at, scale) = if cat3 { ([-10.0, -25.0], 1.0) } else { ([-25.0, -25.0], 0.8) };
            set_render(cx, "RewardSmall", at, scale, 1.0);
        };
        let small = |cx: &mut Cx<'_>| {
            small_place(cx);
            set_brush(cx, "RewardSmall", r.thumb.as_deref());
        };
        match self.kind {
            Kind::Rich => {}
            Kind::Badge => {
                // RewardFrameBreak at (-4, -10) scale 0.9, its brush the badge picture of the real count
                set_render(cx, "RewardFrameBreak", [-4.0, -10.0], 0.9, 1.0);
                let pic = r.badge.as_ref().and_then(|b| badge_look(b, r.count).badge);
                set_brush(cx, "RewardFrameBreak", pic.as_deref());
            }
            Kind::Variant | Kind::Taunt => {
                set_render(cx, "RewardFrameBreak", [-8.0, 5.0], 1.0, 1.0);
                set_brush(cx, "RewardFrameBreak", r.thumb.as_deref());
            }
            Kind::ProfileIcon | Kind::Currency | Kind::Perk | Kind::Announcer => small(cx),
            Kind::Emote => {
                if r.preview.is_some() {
                    set_brush(cx, "Sticker_Preview", r.preview.as_deref());
                } else {
                    // SetBrushFromSoftMaterial(null) @28: the Image with no resource
                    let w = find(cx, "Sticker_Preview");
                    let x = wd(cx, w);
                    x.tex = None;
                    x.mid = None;
                }
            }
            Kind::Ringout => {
                let w = find(cx, "RewardName");
                {
                    let x = wd(cx, w);
                    x.color = RINGOUT_NAME;
                    x.outline_color = RINGOUT_DARK;
                }
                // Deviation: QuadBG's MI_QuadWithTexture_RingoutAnimated and RO_Flipbook's scene render target of
                // the ring-out's preview are not drawn (the material is not in the bundle, the preview actor not
                // ported): QuadBG keeps its material, RO_Flipbook stays Collapsed. @110-1249 sets the spacer and
                // RewardSmall's transform, never its brush.
                small_place(cx);
            }
            Kind::Banner => {}
        }
    }

    /// WBP_CurrencyTooltip's Tooltip Custom Tooltip Item: the reward's line from Custom Tooltip Items.
    fn custom_tooltip_item(cx: &mut Cx<'_>, r: &RewardInfo) {
        let line = {
            let props = cx.view.tree().props(cx.w);
            props.get("Custom Tooltip Items").and_then(|m| m.as_array()).and_then(|m| {
                m.iter().find(|e| e.get("Key").and_then(|k| k.as_str()) == Some(r.asset.as_str())).map(|e| {
                    let v = e.get("Value");
                    let key = v.and_then(|v| v.get("Key")).and_then(|k| k.as_str()).unwrap_or_default().to_string();
                    (key, crate::menu::widgets::common::text(v))
                })
            })
        };
        let Some((key, fallback)) = line else { return };
        let t = if key.is_empty() { fallback } else { cx.view.game_text(&key).to_string() };
        let c = find(cx, "CustomTooltip");
        {
            let w = wd(cx, c);
            w.text = t;
            w.vis = Vis::SelfHitTestInvisible;
        }
        set_vis(cx, "Quad_RewardBacking", Vis::Collapsed);
    }

    /// WBP_PerkTooltip Tooltip Set Perk Data: a PerkData with a slug shows UI_TooltipPerkDescription with its
    /// data (Get Tooltip Data for Perk: player 0, not equipped, no cost) and drops the backing.
    fn set_perk_data(cx: &mut Cx<'_>, r: &RewardInfo) {
        let Some(perk) = perk_data(cx.view, Some(&r.asset)) else { return };
        if perk.info.slug.is_empty() {
            return;
        }
        let sw = find(cx, "WidgetSwitcher_0");
        let d = find(cx, "UI_TooltipPerkDescription");
        let at = cx.view.tree().widget(sw).children.iter().position(|&c| c == d);
        if let Some(i) = at {
            wd(cx, sw).active = i as i32;
        }
        // Set Perk Data @262: Get Tooltip Data for Perk of GetCharacterDataByFighterEnum(AssociatedCharacter): the
        // port's character of that EFighter (None: not a ported one)
        let character = r
            .fighter
            .as_deref()
            .and_then(|f| cosmetics(cx.view)?.characters.values().find(|c| c.fighter == f).map(|c| c.key.clone()));
        let data = PerkTooltipData {
            perk,
            character,
            player_index: 0,
            equipped: false,
            ally_equipped: false,
            cost: 0.0,
            discount: 0.0,
        };
        cx.visit::<TooltipPerkDescription, _>(d, |t, cx| t.set_perk_tooltip_data(cx, Some(&data)));
        set_vis(cx, "Quad_RewardBacking", Vis::Collapsed);
    }

    /// WBP_RingoutTooltip's ubergraph after Set Data @171-597: the unlock line green (unlocked), the checkmark
    /// green for a reward that is not a shop item. The input label's key colour (@29-103) is not ported: the label
    /// is Collapsed.
    fn ringout_colors(cx: &mut Cx<'_>, is_shop: bool) {
        let req = find(cx, "RewardUnlockReq");
        wd(cx, req).color = UNLOCKED_GREEN;
        if !is_shop {
            let icon = find(cx, "icon");
            wd(cx, icon).color = RINGOUT_ICON;
        }
    }
}

impl Controller for RewardTooltip {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "AnimIn", false, 1.0);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        if id != TIP_SET_DATA {
            return;
        }
        let Some(d) = arg.downcast_ref::<Rc<dyn Any>>() else { return };
        if let Some(r) = d.downcast_ref::<RewardInfo>() {
            let r = r.clone();
            self.set_data(cx, &r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_classes_by_reward_type() {
        assert_eq!(tooltip_class(1), Some("WBP_VariantTooltip"));
        assert_eq!(tooltip_class(13), Some("WBP_PerkTooltip"));
        assert_eq!(tooltip_class(14), Some("WBP_RichTooltip"));
        for t in [3, 7, 9, 10, 16] {
            assert_eq!(tooltip_class(t), None);
        }
    }

    #[test]
    fn unlock_bytes() {
        assert_eq!(unlock_byte(Some("Shop")), 1);
        assert_eq!(unlock_byte(Some("CharacterMastery")), 3);
        assert_eq!(unlock_byte(None), 0);
    }
}
