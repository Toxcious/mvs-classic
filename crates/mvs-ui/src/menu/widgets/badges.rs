//! The stat-tracker badges: WBP_StatBadge (the pre-match plates' badge), with what
//! it and WBP_BadgeTooltip (reward_tooltip.rs) read of a badge.
//!
//! A badge's data is the cosmetics catalogue's item (mvs-import kinds/cosmetics.rs `CosmeticItem`), which the menus
//! do not load yet: `BadgeItem` holds what these widgets read of it, filled by the screen that has the catalogue.
//! Its count comes with the call.

use crate::menu::controller::{Controller, Cx};
use crate::menu::hydra::picture_name;
use crate::menu::widgets::controls::interactor::TooltipParams;
use crate::menu::widgets::controls::tooltip::TooltipTarget;
use crate::menu::widgets::controls::{find, wd};
use crate::menu::widgets::reward_tooltip::{REWARD_BADGE, RewardInfo, reward_tooltip_params};
use crate::slate::{Vis, resolve};

/// UI_BadgeFunctions UI_BadgeInitItemData's Description "Badge" (Game.locres).
const T_BADGE: &str = "9AFDB84143045DD7DD15FA94BC4BDF7B";
/// The star overlays by StarLevel (WBP_StatBadge @372-527, WBP_RichTooltip Tooltip Show Star Level @146-301).
const STAR_TEX: [&str; 6] = ["", "UI_Stat_Tier1", "UI_Stat_Tier2", "UI_Stat_Tier3", "UI_Stat_Tier4", "UI_Stat_Tier5"];

/// One of BadgeLevelThreshholds: Threshhold, StarLevel, IconOverride, BadgeOverride.
pub use crate::bundle::BadgeTier;

/// What the badge widgets read of a StatTrackingBundleData (the catalogue's `CosmeticItem` fields of the same
/// names): DisplayName, ERewardRarity's tail, StarLevelThreshholds, StarThreshholdsReversed, the ShowCountMethod
/// byte, the RewardThumbnail and TextureRef pictures (build paths), BadgeLevelThreshholds [Threshhold, StarLevel,
/// IconOverride, BadgeOverride].
#[derive(Clone, Debug, Default)]
pub struct BadgeItem {
    pub name: String,
    pub rarity: String,
    pub stars: Option<Vec<f64>>,
    pub rev: bool,
    pub show: Option<u32>,
    pub badge: Option<String>,
    pub tex: Option<String>,
    pub tiers: Option<Vec<BadgeTier>>,
}

/// UI_BadgeFunctions UI_BadgeGetStarsAndIcons(bundle, count): the star level and the two pictures.
#[derive(Clone, Debug, PartialEq)]
pub struct BadgeLook {
    pub star: u32,
    pub badge: Option<String>,
    pub icon: Option<String>,
}

pub fn badge_look(item: &BadgeItem, count: f64) -> BadgeLook {
    let tiers = item.tiers.as_deref().unwrap_or_default();
    if tiers.is_empty() {
        // @149-586: the first threshold at or above the count, 5 when none; @1471 the reversed 6 - star
        let t = item.stars.as_deref().unwrap_or_default();
        let mut star = t.iter().position(|&x| x >= count).map_or(5, |i| i as u32);
        if item.rev {
            star = 6 - star;
        }
        return BadgeLook {
            star,
            badge: item.badge.as_deref().and_then(picture_name),
            icon: item.tex.as_deref().and_then(picture_name),
        };
    }
    // @763-1213: the first tier at or above the count, the last when none; @1862 / @2085 the overrides, else
    // TextureRef (for the badge too, not RewardThumbnail); @2219 the tier's StarLevel
    let k = tiers.iter().position(|t| t.0 >= count).unwrap_or(tiers.len() - 1);
    let (_, star, icon, badge) = &tiers[k];
    BadgeLook {
        star: *star as u32,
        badge: badge.as_deref().or(item.tex.as_deref()).and_then(picture_name),
        icon: icon.as_deref().or(item.tex.as_deref()).and_then(picture_name),
    }
}

/// FText::AsNumber of an integer, grouped as the en culture groups (1,234,567).
pub fn int_text(n: f64) -> String {
    let n = n.trunc() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// WBP_RichTooltip GetBadgeText's Placement texts by count (@873-5215; the switch @5326 has 0 to 64, its default
/// the empty text). "17h place" is the build's own text. Unverified: the build's texts are design-time strings
/// with no Game.locres keys of their own.
fn placement(n: f64) -> String {
    let n = n as i64;
    if !(1..=64).contains(&n) {
        return String::new();
    }
    if n == 17 {
        return "17h place".into();
    }
    let (r, rr) = (n % 10, n % 100);
    let suffix = if (11..=13).contains(&rr) {
        "th"
    } else {
        match r {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{n}{suffix} place")
}

/// GetBadgeText(DisplayText, Count) by ShowCountMethod: 0 "{rewardname}: {amount}" @742, 1 Placement @6867, 2 Hide
/// (DisplayText alone) @6998.
pub fn badge_text(item: &BadgeItem, display: &str, count: f64) -> String {
    match item.show.unwrap_or(0) {
        2 => display.to_string(),
        1 => format!("{display}: {}", placement(count)),
        _ => format!("{display}: {}", int_text(count)),
    }
}

/// UI_BadgeInitItemData (UI_BadgeFunctions @577-746): the item data of a badge of `count`.
#[derive(Clone, Debug)]
pub struct BadgeItemData {
    pub item: BadgeItem,
    pub display_name: String,
    pub description: String,
    pub icon: Option<String>,
    pub star_level: u32,
    pub count: f64,
    /// RewardInfo_C as CreateFromAsset fills it for a StatTrackingBundleData (WBP_StatBadge SetupRewardInfo @501):
    /// DisplayName, Count, StarLevel, Rarity; RewardType 4 "Badge", Category 0, Tier 0.
    pub reward_info: RewardInfo,
}

pub fn badge_item_data(cx: &Cx<'_>, item: &BadgeItem, count: f64) -> BadgeItemData {
    let look = badge_look(item, count);
    BadgeItemData {
        item: item.clone(),
        display_name: item.name.clone(),
        description: cx.view.game_text(T_BADGE).to_string(),
        icon: look.icon,
        star_level: look.star,
        count,
        reward_info: badge_reward_info(item, &item.name, count, look.star),
    }
}

/// The tooltip params a badge's tooltip target gets: WBP_StatBadge @846-1212 (offset 0) and UI_EquipperItem_V2
/// GetDefaultTooltipParams (offset by its tile). Placement 12 MatchBottomLeft, Delay 0.
pub fn badge_tooltip_params(d: &BadgeItemData, show: bool, x: f64, y: f64) -> TooltipParams {
    reward_tooltip_params(&d.reward_info, d.description.clone(), show, x, y)
}

/// A badge's RewardInfo: `name` and `count` as given, its star level and rarity.
pub fn badge_reward_info(item: &BadgeItem, name: &str, count: f64, star: u32) -> RewardInfo {
    RewardInfo {
        reward_type: REWARD_BADGE,
        display_name: name.to_string(),
        count,
        star_level: star,
        rarity: item.rarity.clone(),
        badge: Some(item.clone()),
        ..RewardInfo::default()
    }
}

/// A picture's bundle name as the entry the batches draw.
fn pic(cx: &Cx<'_>, name: &str) -> String {
    resolve(&cx.view.bundle, name)
}

pub fn star_tex(cx: &Cx<'_>, level: u32) -> String {
    pic(cx, STAR_TEX.get(level as usize).copied().unwrap_or_default())
}

// ---------- WBP_StatBadge ----------

/// The pre-match plates' badge.
#[derive(Debug, Default)]
pub struct StatBadge;

impl StatBadge {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(StatBadge)
    }

    /// SetEquipper(EquipperItemData, StatTracker) @10: BadgeImage the count-0 picture (@10 passes 0, whatever the
    /// count), loaded @157, set @795; BadgeStarLevel Collapsed for StarLevel 0 @329 (nothing shows it again), else
    /// the tier picture @527; the tooltip target's params @846-1212.
    pub fn set_equipper(&mut self, cx: &mut Cx<'_>, d: &BadgeItemData) {
        let star = find(cx, "BadgeStarLevel");
        if d.star_level == 0 {
            wd(cx, star).vis = Vis::Collapsed;
        } else {
            let t = star_tex(cx, d.star_level);
            wd(cx, star).tex = Some(t);
        }
        if let Some(p) = badge_look(&d.item, 0.0).badge {
            let pic = pic(cx, &p);
            cx.view.load_texture(&pic);
            let img = find(cx, "BadgeImage");
            wd(cx, img).tex = Some(pic);
        }
        let t = find(cx, "UI_TooltipTarget");
        let p = badge_tooltip_params(d, true, 0.0, 0.0);
        cx.visit::<TooltipTarget, _>(t, |tt, _| tt.params = p);
    }
}

impl Controller for StatBadge {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_level_is_the_first_threshold_at_or_above_the_count() {
        let item = BadgeItem { stars: Some(vec![10.0, 50.0, 100.0]), ..BadgeItem::default() };
        assert_eq!(badge_look(&item, 0.0).star, 0);
        assert_eq!(badge_look(&item, 30.0).star, 1);
        assert_eq!(badge_look(&item, 1000.0).star, 5);
        let rev = BadgeItem { rev: true, ..item };
        assert_eq!(badge_look(&rev, 30.0).star, 5);
    }

    #[test]
    fn a_tier_overrides_the_pictures() {
        let item = BadgeItem {
            tex: Some("/Game/UI/T_Base.T_Base".into()),
            tiers: Some(vec![(5.0, 1.0, None, Some("/Game/UI/T_B1.T_B1".into())), (20.0, 3.0, None, None)]),
            ..BadgeItem::default()
        };
        let l = badge_look(&item, 3.0);
        assert_eq!((l.star, l.badge.as_deref(), l.icon.as_deref()), (1, Some("T_B1"), Some("T_Base")));
        assert_eq!(badge_look(&item, 99.0).star, 3);
    }

    #[test]
    fn badge_text_by_show_count_method() {
        let normal = BadgeItem::default();
        assert_eq!(badge_text(&normal, "Wins", 1234567.0), "Wins: 1,234,567");
        let place = BadgeItem { show: Some(1), ..BadgeItem::default() };
        assert_eq!(badge_text(&place, "EVO", 2.0), "EVO: 2nd place");
        assert_eq!(badge_text(&place, "EVO", 12.0), "EVO: 12th place");
        assert_eq!(badge_text(&place, "EVO", 17.0), "EVO: 17h place");
        assert_eq!(badge_text(&place, "EVO", 0.0), "EVO: ");
        let hide = BadgeItem { show: Some(2), ..BadgeItem::default() };
        assert_eq!(badge_text(&hide, "X", 5.0), "X");
    }
}
