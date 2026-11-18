//! The main menu's Battle Pass: UI_BattlePass_Rework in the no-backend debug
//! state its PostShow takes (ShowDebugBPass @17115-17608: tier 10, XP 2, no premium, no unclaimed rewards),
//! WBP_RewardTrack and its cards, the premium button, the tokens and the rested-XP badge. Nothing here reaches a
//! backend, a purchase or a match: the buttons that would are inert and say so once per id on the console. The same
//! track with WBP_CharMastery_Card cards is the Collection's mastery track.

use std::any::Any;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use super::badges::badge_reward_info;
use super::common::{ModalBase, Quadrilateral, RARITY, rgba, text};
use super::controls::buttons::{ButtonMain, ButtonSecondary};
use super::controls::interactor::{
    InteractorState, TooltipParams, batch_set_context, no_tooltip, set_focused, set_interactable, toggle_tooltip,
};
use super::controls::tooltip::tooltip_params;
use super::controls::{bind, find, prop, prop_bool, prop_f64, quad_color, quad_wobble, set_tooltip, wd};
use super::nav::GlobalSlideOut;
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra::{cosmetics, picture_name};
use crate::menu::manager::{Manager, Modal};
use crate::menu::scroll::{ScrollDestination, scroll_into_view, scroll_offset, set_scroll_offset};
use crate::menu::stat_trackers::{badge_item, stat_tracker_count};
use crate::menu::view::MenuView;
use crate::menu::widgets::reward_tooltip::{RewardInfo, thumb_of, tooltip_class, unlock_byte};
use crate::slate::{HAlign, Margin, Mid, Stretch, VAlign, Vis, WidgetId, resolve};

fn inert(id: &str, what: &str) {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut w = W.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.insert(id.to_string()) {
        eprintln!("battlepass: {id} does nothing in the port ({what})");
    }
}

/// KismetMathLibrary InRange_IntInt.
fn in_range(v: f64, lo: f64, hi: f64, inc_lo: bool, inc_hi: bool) -> bool {
    (if inc_lo { v >= lo } else { v > lo }) && (if inc_hi { v <= hi } else { v < hi })
}

/// Math.min(Math.max(v, lo), hi).
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

/// EEasingFunc 7 (EaseInOut) with BlendExp 3: KismetMathLibrary Ease -> FMath::InterpEaseInOut.
fn ease_in_out(a: f64, b: f64, t: f64, exp: f64) -> f64 {
    let k = if t < 0.5 { 0.5 * (2.0 * t).powf(exp) } else { 1.0 - 0.5 * (2.0 * (1.0 - t)).powf(exp) };
    a + (b - a) * k
}

/// Math.round.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

fn h_align_of(i: usize) -> HAlign {
    [HAlign::Fill, HAlign::Left, HAlign::Center, HAlign::Right][i]
}

fn pad(l: f64, t: f64, r: f64, b: f64) -> Margin {
    Margin { l, t, r, b }
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

/// UMG Image GetDynamicMaterial: the brush's material made dynamic once; None for a brush that is no material.
fn dyn_mid<'a>(cx: &'a mut Cx<'_>, w: WidgetId) -> Option<&'a mut Mid> {
    let x = cx.view.tree().widget(w);
    let is_material = x.mid.is_some() || x.tex.as_deref().is_some_and(|t| cx.view.bundle.has_material(t));
    is_material.then(|| cx.view.tree_mut().dynamic_material(w))
}

fn f32x4(c: Rgba) -> [f32; 4] {
    c.map(|v| v as f32)
}

// ---------- UI_ButtonRoundedTriangle_Secondary (the track's arrows) ----------

const H_ARROW_CLICK: u32 = 1;

/// UI_ButtonRoundedTriangle_Secondary. This class is read from its bytecode directly.
#[derive(Debug)]
pub struct ButtonRoundedTriangleSecondary {
    pub on_click: Event<()>,
    pub is_enabled: bool,
}

impl Default for ButtonRoundedTriangleSecondary {
    fn default() -> ButtonRoundedTriangleSecondary {
        ButtonRoundedTriangleSecondary { on_click: Event::default(), is_enabled: true }
    }
}

impl ButtonRoundedTriangleSecondary {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonRoundedTriangleSecondary::default())
    }

    /// The button's interactor of the arrow `w`.
    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        let b = cx.view.tree().find_in(w, "UI_ButtonSecondary");
        cx.view.tree().find_in(b, "UI_Interactor")
    }

    /// SetEnabled: the interactor, IsEnabled, and the arrow and button at half opacity while disabled.
    pub fn set_enabled(&mut self, cx: &mut Cx<'_>, e: bool) {
        let i = Self::interactor(cx, cx.w);
        set_interactable(cx, i, e);
        self.is_enabled = e;
        let o = if e { 1.0 } else { 0.5 };
        let bg = find(cx, "background_1");
        wd(cx, bg).opacity = o;
        let b = find(cx, "UI_ButtonSecondary");
        wd(cx, b).opacity = o;
    }

    /// UseForSizeOnly: a hidden copy that only holds the arrow's width.
    pub fn use_for_size_only(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx, cx.w);
        set_interactable(cx, i, false);
        let arrow = find(cx, "ScaleBox_Arrow");
        MenuView::remove_child(cx, arrow);
        let b = find(cx, "UI_ButtonSecondary");
        let fc = cx.view.tree().find_in(b, "UI_FocusedCommon");
        MenuView::remove_child(cx, fc);
    }
}

impl Controller for ButtonRoundedTriangleSecondary {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let b = find(cx, "UI_ButtonSecondary");
        cx.visit::<ButtonSecondary, _>(b, |b, _| b.on_click.add(Handler { owner: me, id: H_ARROW_CLICK }));
    }

    /// PreConstruct: the arrow's render scale (FlipArrow mirrors it), its ScaleBox's user scale, the button
    /// border's OutlineThickness.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let flip = prop_bool(cx, "FlipArrow");
        let scale = prop_f64(cx, "ArrowScale", 0.5);
        let thickness = prop(cx, "Outline Thickness");
        let bx = find(cx, "ScaleBox_Arrow");
        let x = wd(cx, bx);
        x.transform.scale = [if flip { -1.0 } else { 1.0 }, 1.0];
        x.user_scale = scale;
        let b = find(cx, "UI_ButtonSecondary");
        let qb = cx.view.tree().find_in(b, "QuadBorder");
        cx.view.tree_mut().set_prop(qb, "OutlineThickness", thickness);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == H_ARROW_CLICK {
            cx.emit(self, |c| &c.on_click, &());
        }
    }
}

// ---------- WBP_GenericRT_Card (one reward of a tier) ----------

const TIER_LIGHT: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const TIER_DARK: Rgba = [0.009721, 0.011612, 0.033105, 1.0];
/// SetupEmptyRewardVisuals @9717: the empty card's halftone colours and count outline.
const EMPTY_BG: Rgba = [0.040915, 0.049707, 0.215861, 1.0];

/// A mastery level's reward as RewardInfo CreateFromAsset reads its asset (the asset BPI_AssetFromCharacterMasteryReward
/// gives): DisplayName ('' for none), the reward's Count, DetermineRewardType's ERewardType byte, Rarity (ERewardRarity
/// tail, 'None' when unset), RewardThumbnail (a texture of the bundle; None: none or not in the build), the PerkData's
/// EPerkCategory byte (0 for any other asset) and the asset's object path (the bundle's `mastery.rewards`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MasteryReward {
    pub name: String,
    pub count: f64,
    pub kind: i32,
    pub rarity: String,
    pub thumb: Option<String>,
    pub category: i32,
    pub asset: String,
    /// AssociatedCharacter's EFighter tail, UnlockLocation's tail, DetermineRewardTier (RewardInfo CreateFromAsset).
    pub fighter: Option<String>,
    pub unlock: Option<String>,
    pub tier: u32,
}

impl MasteryReward {
    fn of(v: &Value) -> MasteryReward {
        MasteryReward {
            name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            count: v.get("count").and_then(Value::as_f64).unwrap_or(0.0),
            kind: v.get("type").and_then(Value::as_f64).unwrap_or(0.0) as i32,
            rarity: v.get("rarity").and_then(Value::as_str).unwrap_or("None").to_string(),
            thumb: v.get("thumb").and_then(Value::as_str).map(str::to_string),
            category: v.get("category").and_then(Value::as_f64).unwrap_or(0.0) as i32,
            asset: v.get("asset").and_then(Value::as_str).unwrap_or_default().to_string(),
            fighter: v.get("fighter").and_then(Value::as_str).map(str::to_string),
            unlock: v.get("unlock").and_then(Value::as_str).map(str::to_string),
            tier: v.get("tier").and_then(Value::as_u64).unwrap_or(0) as u32,
        }
    }
}

/// The rewards of a character's mastery track (the level i + 1's reward at index i); none for another slug.
fn mastery_rewards(view: &MenuView, slug: &str) -> Vec<MasteryReward> {
    let Some(m) = view.bundle.menu.as_ref() else { return Vec::new() };
    m.mastery
        .get("rewards")
        .and_then(|r| r.get(slug))
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |a| a.iter().map(MasteryReward::of).collect())
}

/// RewardInfo_C as a card holds it (RewardData): the bound reward and what the card sets on it (the Tier).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RewardData {
    pub reward: MasteryReward,
    /// RewardData.Tier (WBP_CharMastery_Card GetTierSpecificData @1674).
    pub tier: i32,
}

/// SetRewardTypeParams @13698: RewardTypeIcon by the reward type (1 skins, 2 and 8 emote, 4 tracker, 5 profile icon,
/// 6 ring-out, 9 fighters, 11 announcer, 15 banner; 0, 3, 7 and 10 _Black; 12, 13, 14 and 16 none).
fn type_icon(kind: i32) -> Option<&'static str> {
    Some(match kind {
        0 | 3 | 7 | 10 => "_Black",
        1 => "ui_icons_skins",
        2 | 8 => "ui_icons_emote",
        4 => "ui_icons_tracker",
        5 => "ui_icons_profileicon",
        6 => "ui_icons_ringout",
        9 => "ui_icons_fighters",
        11 => "ui_icons_announcer",
        15 => "ui_icons_banner",
        _ => return None,
    })
}

/// SetRewardTypeParams @14631-14903: MI_PerkContainer's Zoom by the perk's Category byte (0, 1, 2: 1.4; 3: 1.2); the
/// select's default (K2Node_Select_Default_3) is 0.0.
fn perk_zoom(category: i32) -> f64 {
    match category {
        0..=2 => 1.4,
        3 => 1.2,
        _ => 0.0,
    }
}

/// SetRewardTypeParams @13250: Format("x{count}") (Game.locres key).
const COUNT_TEXT: &str = "16565C5D4A5627A300CEFDA9432EEF5F";
const PERK_CONTAINER: &str = "MI_PerkContainer";

/// An int argument of Format: FText::AsNumber, grouped as the en culture groups.
fn int_text(n: f64) -> String {
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

/// Reward.SetBrushFromMaterial(MI_PerkContainer) then its dynamic material's Texture = the thumbnail (@14243-14577):
/// the instance over the container material with that texture. None: no such texture or material.
fn perk_container(view: &MenuView, thumb: &str) -> Option<(String, Mid)> {
    let b = &view.bundle;
    let tex = b.textures.get(thumb)?;
    let path = b.material_path(PERK_CONTAINER)?.to_string();
    let mut mid = Mid::of(&path);
    mid.set_texture("Texture", tex.entry());
    Some((path, mid))
}

fn pkg_of(path: &str) -> String {
    path.split('.').next().unwrap_or(path).to_lowercase()
}

const H_CARD_FOCUS: u32 = 1;
const H_CARD_UNFOCUS: u32 = 2;
const H_CARD_CLICK: u32 = 3;

/// WBP_GenericRT_Card.
#[derive(Debug, Default)]
pub struct GenericRTCard {
    /// The card widget that was focused (this card's, or the track card's that holds it).
    pub on_card_focused: Event<WidgetId>,
    pub current_tier: i32,
    pub card_tier: i32,
    tier_progress: f64,
    pub has_premium: bool,
    pub is_premium_reward: bool,
    pub is_bpass_reward: bool,
    pub is_empty_reward: bool,
    pub hovered: bool,
    pub unclaimed: bool,
    /// RewardData (SetRewardData); None: none set (a Battle Pass card of the port's debug state).
    pub reward: Option<RewardData>,
    initd: bool,
}

impl GenericRTCard {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GenericRTCard::default())
    }

    fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }

    fn bounce(cx: &Cx<'_>) -> &'static str {
        if prop_bool(cx, "BottomCheckmark") { "BounceCurrentTier" } else { "BounceCurrentTier_Top" }
    }

    fn on_initialized_impl(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        // OnFocus @15665 / OnUnfocus: the outline wobbles (not IsEOGReward), the track hears of the focus.
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_CARD_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_CARD_UNFOCUS);
        // @19095: the click toggles the reward's tooltip (Update Tooltip's params); a card with no reward data (the
        // Battle Pass with no asset) has none.
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, H_CARD_CLICK);
    }

    /// PreConstruct, the DoOnce @15-1563 (IsEOGReward and IsEmptyReward are false on every card of this screen).
    fn pre_construct_impl(&mut self, cx: &mut Cx<'_>) {
        if self.initd {
            return;
        }
        self.initd = true;
        let show_bar = prop_bool(cx, "ShowProgressBar");
        set_vis(cx, "Overlay_ProgressBar", if show_bar { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        if !prop_bool(cx, "IsEmptyReward") {
            let bg = find(cx, "HalftoneBG");
            let (a, b) = (rgba(&prop(cx, "BGColorA")), rgba(&prop(cx, "BGColorB")));
            if let Some(m) = dyn_mid(cx, bg) {
                m.set_vector("ColorA", f32x4(a));
                m.set_vector("ColorB", f32x4(b));
            }
            let progress = find(cx, "Progress");
            let fill = rgba(&prop(cx, "ProgressBarFillColor"));
            if let Some(m) = dyn_mid(cx, progress) {
                m.set_vector("FillColor", f32x4(fill));
            }
            let show_div = prop_bool(cx, "ShowDivTop");
            set_vis(cx, "DivTop", if show_div { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
            let rc = find(cx, "RewardCountText");
            wd(cx, rc).outline_color = a;
            let h = prop(cx, "CardHeight");
            let x = wd(cx, bg);
            x.brush_w = h.get("X").and_then(Value::as_f64).unwrap_or(0.0);
            x.brush_h = h.get("Y").and_then(Value::as_f64).unwrap_or(0.0);
        }
        // (Dimmer's brush has no resource and a dark TintColor: the slate multiplies it in.)
        if !prop_bool(cx, "BottomCheckmark") {
            // @1300: the claim strip goes above the reward, its quad's slant flipped
            let bottom = find(cx, "Overlay_Bottom");
            if let Some(p) = cx.view.tree().widget(bottom).parent {
                cx.view.tree_mut().insert(p, bottom, Some(0));
            }
            let q = find(cx, "QuadClaimBG");
            cx.view.tree_mut().set_prop(q, "BottomRight", json!({"X": 1, "Y": 1}));
            cx.view.tree_mut().set_prop(q, "TopLeft", json!({"X": 0, "Y": 0.15}));
            for (k, v) in [("BottomRight_X", 1.0f32), ("BottomRight_Y", 1.0), ("TopLeft_X", 0.0), ("TopLeft_Y", 0.15)] {
                let x = wd(cx, q);
                if let Some(quad) = &mut x.quad {
                    match quad.iter_mut().find(|e| e.0 == k) {
                        Some(e) => e.1 = v,
                        None => quad.push((k.to_string(), v)),
                    }
                }
                if let Some(m) = &mut x.mid {
                    m.set_scalar(k, v);
                }
            }
            wd(cx, bottom).transform.translation[1] = 1.0;
            let arrow = find(cx, "CurrentTierArrow");
            wd(cx, arrow).transform.angle = 90.0;
            // (@1601-2140: the Progress MID's DeltaTextureHorizontalTiles once its size is known; the Free card's
            // bar is collapsed and the delta is 0 on every card here)
        }
    }

    fn tier_slot(cx: &mut Cx<'_>, h_align: Option<HAlign>, pad_right: f64) {
        let t = find(cx, "Tier");
        let x = wd(cx, t);
        if let Some(h) = h_align {
            x.h_align = h;
        }
        x.pad = pad(0.0, 0.0, pad_right, 0.0);
    }

    /// SetBasicTierData(CurrentTier, CardTier, TierProgress).
    pub fn set_basic_tier_data(&mut self, cx: &mut Cx<'_>, cur: i32, card: i32, prog: f64) {
        self.current_tier = cur;
        self.card_tier = card;
        self.tier_progress = prog;
        let t = find(cx, "Tier");
        wd(cx, t).text = if card == cur { format!("TIER {cur}") } else { card.to_string() };
        let me = cx.w;
        match cur.cmp(&card) {
            std::cmp::Ordering::Less => {
                // a tier ahead
                set_vis(cx, "Overlay_Claim", Vis::Hidden);
                set_vis(cx, "Checkmark", Vis::Hidden);
                self.set_progress_bar(cx, 0.0);
                set_vis(cx, "Dimmer", Vis::Hidden);
                Self::tier_slot(cx, None, 16.0);
                let x = wd(cx, t);
                x.shadow_offset = [2.0, 2.0];
                set_vis(cx, "FilledProgress", Vis::Collapsed);
                wd(cx, t).color = TIER_LIGHT;
            }
            std::cmp::Ordering::Greater => {
                // a tier behind
                self.set_progress_bar(cx, 1.0);
                let bar = find(cx, "Overlay_ProgressBar");
                wd(cx, bar).pad.r = -4.0;
                Self::tier_slot(cx, None, 16.0);
                wd(cx, t).shadow_offset = [0.0, 0.0];
                set_vis(cx, "Dimmer", Vis::SelfHitTestInvisible);
                set_vis(cx, "CurrentTierArrow", Vis::Collapsed);
                cx.view.stop_all_animations(me);
                wd(cx, t).color = TIER_DARK;
                set_vis(cx, "FilledProgress", Vis::Collapsed);
            }
            std::cmp::Ordering::Equal => {
                // the current tier
                set_vis(cx, "Checkmark", Vis::Hidden);
                set_vis(cx, "Overlay_Claim", Vis::Hidden);
                self.set_progress_bar(cx, prog);
                set_vis(cx, "CurrentTierArrow", Vis::SelfHitTestInvisible);
                Self::tier_slot(cx, Some(HAlign::Center), 0.0);
                wd(cx, t).shadow_offset = [2.0, 2.0];
                // PlayAnimation(.., NumLoopsToPlay 0 = forever, Forward, speed 1.25) @4987
                let bounce = Self::bounce(cx);
                cx.view.play_animation(me, bounce, false, 1.25, 0, false, false);
                set_vis(cx, "FilledProgress", Vis::Collapsed);
                wd(cx, t).color = TIER_LIGHT;
            }
        }
    }

    pub fn set_progress_bar(&mut self, cx: &mut Cx<'_>, v: f64) {
        let p = find(cx, "Progress");
        if let Some(m) = dyn_mid(cx, p) {
            m.set_scalar("Percentage", v as f32);
        }
    }

    /// SetUnclaimedVisuals(Unclaimed) @3463: the first leg of its Sequence runs whatever Unclaimed is (the claim
    /// strip shows, pulses, and its quad turns transparent red), the last two only when unclaimed.
    pub fn set_unclaimed_visuals(&mut self, cx: &mut Cx<'_>, u: bool) {
        self.unclaimed = u;
        let check = find(cx, "Checkmark");
        let past = prop(cx, "Tier_Past");
        wd(cx, check).color = rgba(past.get("SpecifiedColor").unwrap_or(&Value::Null));
        set_vis(cx, "Overlay_Claim", Vis::SelfHitTestInvisible);
        // PlayAnimation(ClaimPulse, 0, loops 0 = forever, PingPong, 1) @3556
        cx.view.play_animation(cx.w, "ClaimPulse", false, 1.0, 0, true, false);
        let q = find(cx, "QuadClaimBG");
        quad_color(cx, q, [1.0, 0.122139, 0.084376, 0.0]);
        let hb = find(cx, "HorizontalBox_Claim");
        let x = wd(cx, hb);
        x.h_align = if u { HAlign::Center } else { HAlign::Right };
        x.pad = pad(0.0, 5.0, if u { 0.0 } else { 4.0 }, 5.0);
        let status = find(cx, "Status");
        wd(cx, status).text = if u { if self.has_premium { "CLAIM" } else { "UNLOCK" } } else { "" }.to_string();
        if u {
            set_vis(cx, "Dimmer", Vis::Hidden);
            wd(cx, check).vis = Vis::Collapsed;
        }
    }

    pub fn set_has_premium(&mut self, has: bool, is_premium_reward: bool) {
        self.has_premium = has;
        self.is_premium_reward = is_premium_reward;
    }

    /// GetTierSpecificData (WBP_RewardTrack_Card @1316, through WBP_BPass_Card): no Battle Pass asset, so the reward is
    /// the default struct: SetUnclaimedVisuals(false), SetBpassReward -> IsBpassReward.
    pub fn set_bpass_reward(&mut self) {
        self.is_bpass_reward = true;
    }

    fn slot_pad(cx: &mut Cx<'_>, n: &str, l: f64, t: f64, r: f64, b: f64) {
        let w = find(cx, n);
        wd(cx, w).pad = pad(l, t, r, b);
    }

    /// The catalogue's badge of a Badge reward (a StatTrackingBundleData of the same package).
    fn badge_of(cx: &Cx<'_>, r: &MasteryReward) -> Option<crate::bundle::CosmeticItem> {
        let pkg = pkg_of(&r.asset);
        cosmetics(cx.view)?.stat_tracker.iter().find(|i| pkg_of(&i.path) == pkg).cloned()
    }

    /// SetRewardData @7541.
    pub fn set_reward_data(&mut self, cx: &mut Cx<'_>, mut r: RewardData) {
        let eog = prop_bool(cx, "IsEOGReward");
        // @9292-9667: a Badge whose asset is a StatTrackingBundleData takes Count = its stat value (the tooltip reads
        // it): the stats file's tracker counts and the arcade record's Hard completions.
        // Deviation: read from the stats file here, not the counts of the last own profile read
        // (0 until the Profile screen first loads one).
        if r.reward.kind == 4
            && let Some(item) = Self::badge_of(cx, &r.reward)
        {
            let stats = &mut cx.view.session.stats;
            let trackers = stats.data().trackers.clone();
            let hard = stats.arcade().hard_completions;
            if let Some(c) = cosmetics(cx.view) {
                r.reward.count = stat_tracker_count(&item, c, &trackers, hard);
            }
        }
        self.reward = Some(r.clone());
        if r.reward.name.is_empty() {
            // an empty DisplayName: SetupEmptyRewardVisuals, the wobble's padding @7806
            self.setup_empty_reward_visuals(cx);
            Self::slot_pad(cx, "QuadWobble", -8.0, 49.0, -8.0, 0.0);
        } else {
            self.update_tooltip(cx);
            // (@7900 RewardData.IsClaimable = Unclaimed && HasPremium: the claim, not ported)
            self.set_reward_type_params(cx);
            if eog {
                Self::slot_pad(cx, "UI_Interactor", 0.0, 0.0, 0.0, 0.0);
                Self::slot_pad(cx, "QuadWobble", 0.0, 0.0, 0.0, 0.0);
            } else if prop_bool(cx, "BottomCheckmark") {
                let b = if self.unclaimed { 0.0 } else { 60.0 };
                Self::slot_pad(cx, "UI_Interactor", 10.0, 35.0, 10.0, b);
                Self::slot_pad(cx, "QuadWobble", -8.0, 30.0, -8.0, 49.0);
            } else {
                let t = if self.unclaimed { 0.0 } else { 60.0 };
                Self::slot_pad(cx, "UI_Interactor", 10.0, t, 10.0, 0.0);
                Self::slot_pad(cx, "QuadWobble", -8.0, 49.0, -8.0, 0.0);
            }
        }
    }

    /// SetupEmptyRewardVisuals @10260: the parent's (WBP_RewardTrack_Card @1018 -> @71-767: IsEmptyReward, each
    /// interactor interactable and SelfHitTestInvisible), then @9717.
    pub fn setup_empty_reward_visuals(&mut self, cx: &mut Cx<'_>) {
        self.is_empty_reward = true;
        let i = Self::interactor(cx);
        set_interactable(cx, i, true);
        wd(cx, i).vis = Vis::SelfHitTestInvisible;
        let bg = find(cx, "HalftoneBG");
        if let Some(m) = dyn_mid(cx, bg) {
            m.set_vector("ColorA", f32x4(EMPTY_BG));
            m.set_vector("ColorB", f32x4(EMPTY_BG));
        }
        let rc = find(cx, "RewardCountText");
        wd(cx, rc).outline_color = EMPTY_BG;
        set_vis(cx, "Overlay_RewardType", Vis::Hidden);
        set_vis(cx, "Reward", Vis::Hidden);
        set_vis(cx, "HBox_EmptyArrows", Vis::HitTestInvisible);
        let eog = prop_bool(cx, "IsEOGReward");
        set_vis(cx, "Overlay_Claim", if eog { Vis::Collapsed } else { Vis::Hidden });
    }

    /// SetRewardTypeParams @10275, for the types a reward of the port has (0, 4, 5, 12, 13). Not ported: the
    /// thumbnail material of types 1, 2 and 8 (@11606-12450; no such reward is bound).
    fn set_reward_type_params(&mut self, cx: &mut Cx<'_>) {
        let Some(RewardData { reward: r, .. }) = self.reward.clone() else { return };
        set_vis(cx, "HBox_EmptyArrows", Vis::Collapsed);
        let reward = find(cx, "Reward");
        wd(cx, reward).vis = Vis::SelfHitTestInvisible;
        // UI_Functions Rarity to Color: the palette's base
        let base = RARITY.iter().find(|(n, _)| *n == r.rarity).unwrap_or(&RARITY[0]).1;
        let trim = find(cx, "TypeTrim");
        wd(cx, trim).color = base;
        for q in ["Quad_IconBG", "QuadTypeSlant"] {
            let w = find(cx, q);
            quad_color(cx, w, base);
        }
        set_vis(cx, "Overlay_RewardType", if r.rarity == "None" { Vis::Collapsed } else { Vis::HitTestInvisible });
        assert!(
            !matches!(r.kind, 1 | 2 | 8),
            "WBP_GenericRT_Card: reward type {} (its thumbnail material) is not ported",
            r.kind
        );
        if r.kind == 13 {
            // @14243: MI_PerkContainer with the thumbnail and the category's Zoom, centred (@15062-15125); nothing more
            let made = r.thumb.as_deref().and_then(|t| perk_container(cx.view, t));
            let fallback = cx.view.bundle.material_path(PERK_CONTAINER).map(str::to_string);
            let x = wd(cx, reward);
            match made {
                Some((path, mut mid)) => {
                    mid.set_scalar("Zoom", perk_zoom(r.category) as f32);
                    x.tex = Some(path);
                    x.mid = Some(mid);
                }
                None => {
                    x.tex = fallback;
                    x.mid = None;
                }
            }
            let x = wd(cx, reward);
            x.v_align = VAlign::Center;
            x.h_align = HAlign::Center;
            return;
        }
        // @15313: the thumbnail; ScaleBox_0 ScaleToFit for a currency, else ScaleToFill; centred
        let tex = r.thumb.as_deref().map(|t| resolve(&cx.view.bundle, t));
        let x = wd(cx, reward);
        x.tex = tex;
        x.mid = None;
        x.v_align = VAlign::Center;
        x.h_align = HAlign::Center;
        let sb = find(cx, "ScaleBox_0");
        wd(cx, sb).stretch = if r.kind == 12 { Stretch::ScaleToFit } else { Stretch::ScaleToFill };
        // @12484-13698: the count ("x{count}") seen for a currency of more than one, the type icon
        let count = find(cx, "RewardCountText");
        let label = cx.view.game_text(COUNT_TEXT).replace("{count}", &int_text(r.count));
        let x = wd(cx, count);
        x.vis = if r.count > 1.0 && r.kind == 12 { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        x.text = label;
        let icon_tex = type_icon(r.kind).map(|n| resolve(&cx.view.bundle, n));
        let icon = find(cx, "RewardTypeIcon");
        let x = wd(cx, icon);
        x.tex = icon_tex;
        x.mid = None;
    }

    /// Update Tooltip @19458: TooltipParams with the class by type (the card's own switch @19582-20147: 1
    /// WBP_VariantTooltip, 2 WBP_TauntTooltip, 4 WBP_BadgeTooltip, 5 WBP_ProfileIconTooltip, 6 WBP_RingoutTooltip, 8
    /// WBP_EmoteTooltip, 11 WBP_AnnouncerTooltip, 12 WBP_CurrencyTooltip, 13 WBP_PerkTooltip, 14 WBP_RichTooltip, 15
    /// WBP_BannerTooltip; 0, 3, 7, 9, 10 and 16 none), Offset (30, -100) ((0, 125) for an EOG reward), Delay 0,
    /// FollowMouse, Data = RewardData, Placement 12 (1 for EOG); the interactor takes them unless IsEOGReward without
    /// ForceShowTooltip (@20879).
    fn update_tooltip(&mut self, cx: &mut Cx<'_>) {
        let Some(r) = self.reward.clone() else { return };
        let eog = prop_bool(cx, "IsEOGReward");
        let i = Self::interactor(cx);
        if !(!eog || (prop_bool(cx, "ForceShowTooltip") && !prop_bool(cx, "IsMysteryReward"))) {
            set_tooltip(cx, i, no_tooltip());
            return;
        }
        // the RewardData of the card's reward (RewardInfo CreateFromAsset of BPI_AssetFromCharacterMasteryReward's
        // asset): a badge's from the catalogue's StatTrackingBundleData (StarLevel 0, as the card's), the rest from
        // the mastery track's row, with the catalogue item's preview (ToolTipPreviewMaterial) where it lists the asset
        let rw = &r.reward;
        let info = match rw.kind {
            4 => Self::badge_of(cx, rw).map(|item| badge_reward_info(&badge_item(&item), &rw.name, rw.count, 0)),
            1 | 2 | 5 | 6 | 8 | 11 | 12 | 13 | 14 | 15 => {
                let pkg = pkg_of(&rw.asset);
                let item = cosmetics(cx.view)
                    .and_then(|c| c.lists().into_iter().flatten().find(|i| pkg_of(&i.path) == pkg).cloned());
                Some(RewardInfo {
                    reward_type: rw.kind as u8,
                    display_name: rw.name.clone(),
                    count: rw.count,
                    rarity: rw.rarity.clone(),
                    category: rw.category as u8,
                    tier: rw.tier,
                    unlock: unlock_byte(rw.unlock.as_deref()),
                    fighter: rw.fighter.clone(),
                    thumb: rw.thumb.clone(),
                    preview: item.and_then(|i| thumb_of(i.preview.as_deref())),
                    asset: picture_name(&rw.asset).unwrap_or_default(),
                    ..RewardInfo::default()
                })
            }
            _ => None,
        };
        let cls = info.as_ref().and_then(|i| tooltip_class(i.reward_type)).map(str::to_string);
        set_tooltip(
            cx,
            i,
            TooltipParams {
                tooltip: String::new(),
                cls,
                show: true,
                offset: if eog { [0.0, 125.0] } else { [30.0, -100.0] },
                delay: 0.0,
                placement: if eog { 1 } else { 12 },
                data: info.map(|i| Rc::new(i) as Rc<dyn Any>),
            },
        );
    }

    /// SetDeltaTierData -> SetProgressBarDelta: the delta bar is full on tiers behind the delta tier, empty ahead.
    pub fn set_delta_tier_data(&mut self, cx: &mut Cx<'_>, d_t: f64, d_p: f64) {
        let card = f64::from(self.card_tier);
        let v = if d_t < card {
            0.0
        } else if d_t > card {
            1.0
        } else {
            d_p
        };
        let p = find(cx, "Progress");
        if let Some(m) = dyn_mid(cx, p) {
            m.set_scalar("DeltaPercentage", v as f32);
        }
    }

    /// Update Focus Width Override @17563: WidenOnly when not a Battle Pass reward (WBP_BPass_Card plays it on both).
    pub fn update_focus_width_override(&mut self, cx: &mut Cx<'_>, on: bool) {
        if !self.is_bpass_reward {
            self.widen(cx, on);
        }
        set_vis(cx, "CurrentTierArrow", if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        let t = find(cx, "Tier");
        let x = wd(cx, t);
        x.text = if on { format!("TIER {}", self.card_tier) } else { self.card_tier.to_string() };
        x.h_align = h_align_of(if on { 2 } else { 3 });
        let rc = find(cx, "RewardCountText");
        let x = wd(cx, rc);
        x.h_align = h_align_of(if on { 3 } else { 2 });
        x.outline = 0.0;
        // @18808: forever, speed 1.25
        let me = cx.w;
        if on {
            let bounce = Self::bounce(cx);
            cx.view.play_animation(me, bounce, false, 1.25, 0, false, false);
        } else {
            cx.view.stop_all_animations(me);
        }
    }

    /// WidenOnly (Spacer_FocusWidth's Size, a MovieSceneVectorTrack, 1 -> 320 over 0.25 s), reversed to narrow.
    pub fn widen(&mut self, cx: &mut Cx<'_>, on: bool) {
        cx.view.play(cx.w, "WidenOnly", !on, 1.0);
    }

    pub fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        vec![Self::interactor(cx)]
    }

    fn handle_impl(&mut self, cx: &mut Cx<'_>, id: u32) {
        let eog = prop_bool(cx, "IsEOGReward");
        match id {
            H_CARD_FOCUS => {
                self.hovered = true;
                let q = find(cx, "QuadWobble");
                quad_wobble(cx, q, !eog || prop_bool(cx, "ForceShowTooltip"));
                let me = cx.w;
                cx.emit(self, |c| &c.on_card_focused, &me);
            }
            H_CARD_UNFOCUS => {
                self.hovered = false;
                let q = find(cx, "QuadWobble");
                quad_wobble(cx, q, false);
            }
            H_CARD_CLICK => {
                if self.reward.is_some() {
                    let i = Self::interactor(cx);
                    toggle_tooltip(cx, i, true);
                } else {
                    inert("WBP_GenericRT_Card", "the reward tooltip needs a Battle Pass asset");
                }
            }
            _ => {}
        }
    }
}

impl Controller for GenericRTCard {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.on_initialized_impl(cx);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.pre_construct_impl(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        self.handle_impl(cx, id);
    }
}

// ---------- the track's cards ----------

/// A card RewardTrack makes (WBP_RewardTrack_Card's interface as the track calls it); the context is the card's.
trait TrackCard {
    fn on_card_focused(&mut self) -> &mut Event<WidgetId>;
    fn card_tier(&self) -> i32;
    fn set_basic_tier_data(&mut self, cx: &mut Cx<'_>, cur: i32, card: i32, prog: f64);
    fn set_has_premium(&mut self, cx: &mut Cx<'_>, has: bool, is_premium: bool);
    fn set_unclaimed_visuals(&mut self, cx: &mut Cx<'_>, u: bool);
    fn get_tier_specific_data(&mut self, cx: &mut Cx<'_>, slug: &str);
    fn set_delta_tier_data(&mut self, cx: &mut Cx<'_>, d_t: f64, d_p: f64);
    fn update_focus_width_override(&mut self, cx: &mut Cx<'_>, on: bool);
    fn width_override(&self) -> f64 {
        320.0
    }
    fn card_interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId>;
    fn stop_all(&mut self, cx: &mut Cx<'_>);
}

/// `f` on the card `w` as a track card (a BPassCard or a MasteryCard); None for any other widget.
fn with_card<R>(cx: &mut Cx<'_>, w: WidgetId, f: impl FnOnce(&mut dyn TrackCard, &mut Cx<'_>) -> R) -> Option<R> {
    let mut f = Some(f);
    if let Some(r) = cx.visit::<BPassCard, _>(w, |c, cx| (f.take().expect("once"))(c, cx)) {
        return Some(r);
    }
    cx.visit::<MasteryCard, _>(w, |c, cx| (f.take().expect("once"))(c, cx))
}

// ---------- WBP_CharMastery_Card (a mastery level's reward, a WBP_GenericRT_Card) ----------

/// WBP_CharMastery_Card.
#[derive(Debug, Default)]
pub struct MasteryCard {
    g: GenericRTCard,
}

impl MasteryCard {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MasteryCard::default())
    }
}

impl TrackCard for MasteryCard {
    fn on_card_focused(&mut self) -> &mut Event<WidgetId> {
        &mut self.g.on_card_focused
    }

    fn card_tier(&self) -> i32 {
        self.g.card_tier
    }

    fn set_basic_tier_data(&mut self, cx: &mut Cx<'_>, cur: i32, card: i32, prog: f64) {
        self.g.set_basic_tier_data(cx, cur, card, prog);
    }

    /// SetHasPremium @1221: the parent's with HasPremium true.
    fn set_has_premium(&mut self, _cx: &mut Cx<'_>, _has: bool, is_premium: bool) {
        self.g.set_has_premium(true, is_premium);
    }

    fn set_unclaimed_visuals(&mut self, cx: &mut Cx<'_>, u: bool) {
        self.g.set_unclaimed_visuals(cx, u);
    }

    /// GetTierSpecificData @1242: GetMasteryRewardsForLevel(CardTier, OptionalSlug).Rewards[0] -> SetMasteryAward ->
    /// SetRewardData (the bind's reward), then RewardData.Tier = CardTier (@1674). (RewardData.AssociatedCharacter
    /// @1603: nothing of the card reads it.) Not ported: PreAsyncLoadRewardInfo's RewardGUID of an unclaimed reward
    /// (@1724).
    fn get_tier_specific_data(&mut self, cx: &mut Cx<'_>, slug: &str) {
        let tier = self.g.card_tier;
        let reward = usize::try_from(tier - 1).ok().and_then(|i| mastery_rewards(cx.view, slug).into_iter().nth(i));
        let Some(reward) = reward else {
            panic!("WBP_CharMastery_Card: no mastery reward for {slug} level {tier}");
        };
        self.g.set_reward_data(cx, RewardData { reward, tier });
    }

    fn set_delta_tier_data(&mut self, cx: &mut Cx<'_>, d_t: f64, d_p: f64) {
        self.g.set_delta_tier_data(cx, d_t, d_p);
    }

    fn update_focus_width_override(&mut self, cx: &mut Cx<'_>, on: bool) {
        self.g.update_focus_width_override(cx, on);
    }

    fn card_interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        GenericRTCard::interactors(cx)
    }

    fn stop_all(&mut self, cx: &mut Cx<'_>) {
        cx.view.stop_all_animations(cx.w);
    }
}

impl Controller for MasteryCard {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.g.on_initialized_impl(cx);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.g.pre_construct_impl(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        self.g.handle_impl(cx, id);
    }
}

// ---------- WBP_BPass_Card (a tier: the free reward over the premium one) ----------

const H_FREE_FOCUS: u32 = 1;
const H_PREMIUM_FOCUS: u32 = 2;

/// WBP_BPass_Card.
#[derive(Debug, Default)]
pub struct BPassCard {
    pub on_card_focused: Event<WidgetId>,
    card_tier: i32,
    /// SetUnclaimedVisuals: WBP_BPass_Card does not override it; its parent's (WBP_RewardTrack_Card @873) only stores
    /// Unclaimed, which nothing of this screen reads.
    pub unclaimed: bool,
}

impl BPassCard {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(BPassCard::default())
    }

    fn free(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_Free")
    }

    fn premium(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_Premium")
    }
}

impl TrackCard for BPassCard {
    fn on_card_focused(&mut self) -> &mut Event<WidgetId> {
        &mut self.on_card_focused
    }

    fn card_tier(&self) -> i32 {
        self.card_tier
    }

    fn set_basic_tier_data(&mut self, cx: &mut Cx<'_>, cur: i32, card: i32, prog: f64) {
        self.card_tier = card;
        for w in [Self::free(cx), Self::premium(cx)] {
            cx.visit::<GenericRTCard, _>(w, |c, cx| c.set_basic_tier_data(cx, cur, card, prog));
        }
    }

    fn set_has_premium(&mut self, cx: &mut Cx<'_>, has: bool, _is_premium: bool) {
        let (free, premium) = (Self::free(cx), Self::premium(cx));
        cx.visit::<GenericRTCard, _>(premium, |c, _| c.set_has_premium(has, true));
        cx.visit::<GenericRTCard, _>(free, |c, _| c.set_has_premium(true, false));
    }

    fn set_unclaimed_visuals(&mut self, _cx: &mut Cx<'_>, u: bool) {
        self.unclaimed = u;
    }

    /// GetTierSpecificData: per reward card SetUnclaimedVisuals(IsUnclaimed: the empty RewardsToClaim, false) and
    /// SetBpassReward (the asset's reward: none).
    fn get_tier_specific_data(&mut self, cx: &mut Cx<'_>, _slug: &str) {
        for w in [Self::free(cx), Self::premium(cx)] {
            cx.visit::<GenericRTCard, _>(w, |c, cx| {
                c.set_unclaimed_visuals(cx, false);
                c.set_bpass_reward();
            });
        }
    }

    fn set_delta_tier_data(&mut self, cx: &mut Cx<'_>, d_t: f64, d_p: f64) {
        for w in [Self::free(cx), Self::premium(cx)] {
            cx.visit::<GenericRTCard, _>(w, |c, cx| c.set_delta_tier_data(cx, d_t, d_p));
        }
    }

    /// Update Focus Width Override: WidenOnly on both (Spacer_FocusWidth 1 -> 320 over 0.25 s, reversed to narrow).
    fn update_focus_width_override(&mut self, cx: &mut Cx<'_>, on: bool) {
        for w in [Self::free(cx), Self::premium(cx)] {
            cx.visit::<GenericRTCard, _>(w, |c, cx| {
                c.widen(cx, on);
                let sb = find(cx, "ScaleBox_0");
                let x = wd(cx, sb);
                x.stretch = Stretch::ScaleToFitY;
                x.clip = true;
                c.update_focus_width_override(cx, on);
            });
        }
    }

    fn card_interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        [Self::free(cx), Self::premium(cx)].into_iter().map(|w| cx.view.tree().find_in(w, "UI_Interactor")).collect()
    }

    fn stop_all(&mut self, cx: &mut Cx<'_>) {
        for w in [Self::free(cx), Self::premium(cx)] {
            cx.view.stop_all_animations(w);
        }
    }
}

impl Controller for BPassCard {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let (free, premium) = (Self::free(cx), Self::premium(cx));
        cx.visit::<GenericRTCard, _>(free, |c, _| c.on_card_focused.add(Handler { owner: me, id: H_FREE_FOCUS }));
        cx.visit::<GenericRTCard, _>(premium, |c, _| c.on_card_focused.add(Handler { owner: me, id: H_PREMIUM_FOCUS }));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == H_FREE_FOCUS || id == H_PREMIUM_FOCUS {
            let me = cx.w;
            cx.emit(self, |c| &c.on_card_focused, &me);
        }
    }
}

// ---------- WBP_RewardTrack ----------

/// The RewardCards classes the port draws.
const TRACK_CARDS: [&str; 2] = ["WBP_BPass_Card", "WBP_CharMastery_Card"];

const H_LEFT: u32 = 1;
const H_RIGHT: u32 = 2;
const H_CARD_FOCUSED: u32 = 3;
const R_MEASURE: u32 = 1;
const R_LERP: u32 = 2;

/// The track of tier cards in a ScrollBox with an arrow either side: the cards are ScrollBox_Main's children (an
/// Orient_Horizontal box, menu/scroll.rs), its offset read and set through GetScrollOffset / SetScrollOffset (shown
/// from the box's next tick, interpolated while the last ScrollWidgetIntoView asked for it). Not ported: the wheel
/// and right stick over the track (OnMouseEnter binds LobbyPlayerController's MouseWheel / RightStick delegates: no
/// widget hover hook in the view), ScrollTick and the scroll timer they start, the level-up path (AddProgress,
/// HandleTierUp).
#[derive(Debug)]
pub struct RewardTrack {
    cards: Vec<Option<WidgetId>>,
    /// OptionalSlug: the character of a mastery track (UI_CollectionCharacterDetails SetRewardTrack @1987).
    pub optional_slug: String,
    target: f64,
    scrolling_left: bool,
    pub current_card_tier: i32,
    tier_progress: f64,
    unclaimed: Vec<i32>,
    delta_tier: f64,
    delta_progress: f64,
    pub focused_tier: i32,
    pub has_premium: bool,
    card_width: f64,
    focus_width: f64,
    guid: String,
    lerping: bool,
    lerp_start: f64,
    lerp_from: f64,
}

impl Default for RewardTrack {
    fn default() -> RewardTrack {
        RewardTrack {
            cards: Vec::new(),
            optional_slug: String::new(),
            target: 0.0,
            scrolling_left: false,
            current_card_tier: 0,
            tier_progress: 0.0,
            unclaimed: Vec::new(),
            delta_tier: 0.0,
            delta_progress: 0.0,
            focused_tier: 0,
            has_premium: false,
            card_width: 150.0,
            focus_width: 150.0,
            guid: String::new(),
            lerping: false,
            lerp_start: 0.0,
            lerp_from: 0.0,
        }
    }
}

impl RewardTrack {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(RewardTrack::default())
    }

    fn cards_to_show(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "RT_CardsToShow", 0.0)
    }

    fn max_tier(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "MaximumTier", 0.0)
    }

    fn left(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonLeft")
    }

    fn right(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonRight")
    }

    fn scroll_box(cx: &Cx<'_>) -> WidgetId {
        find(cx, "ScrollBox_Main")
    }

    /// SetCurrentTier(Tier, Progress, Unclaimed, OverrideFocusTier, Delta Tier, Delta Progress) @13650.
    #[allow(clippy::too_many_arguments)]
    pub fn set_current_tier(
        &mut self,
        cx: &mut Cx<'_>,
        tier: i32,
        prog: f64,
        unclaimed: &[i32],
        over: i32,
        d_t: f64,
        d_p: f64,
    ) {
        self.current_card_tier = tier + 1;
        self.tier_progress = prog;
        self.unclaimed = unclaimed.to_vec();
        self.delta_tier = d_t;
        self.delta_progress = d_p;
        self.setup_starting_cards(cx, over);
    }

    /// ScrollBox_Main.GetScrollOffset / SetScrollOffset.
    fn offset(&self, cx: &mut Cx<'_>) -> f64 {
        let sb = Self::scroll_box(cx);
        scroll_offset(cx.view.tree_mut(), sb)
    }

    fn set_offset(&self, cx: &mut Cx<'_>, v: f64) {
        let sb = Self::scroll_box(cx);
        set_scroll_offset(cx.view.tree_mut(), sb, v);
    }

    fn first_valid(&self) -> Option<usize> {
        self.cards.iter().position(Option::is_some)
    }

    fn card_tier_of(&self, cx: &mut Cx<'_>, i: usize) -> Option<i32> {
        let w = self.cards.get(i).copied().flatten()?;
        with_card(cx, w, |c, _| c.card_tier())
    }

    /// SetupStartingCards (first time @7348: no card to remove).
    fn setup_starting_cards(&mut self, cx: &mut Cx<'_>, over: i32) {
        for c in std::mem::take(&mut self.cards).into_iter().flatten() {
            MenuView::discard(cx, c);
        }
        self.scrolling_left = false;
        self.focused_tier = self.current_card_tier;
        let start = self.starting_index(cx, over);
        let n = Self::cards_to_show(cx) as i32;
        for i in start..=start + n - 1 {
            self.add_card(cx, i);
        }
        self.measure_cards(cx);
    }

    /// @30-1573: the width of a narrow card (the first, or the one after it when the first is the current tier) once
    /// it has been laid out (Delay(0) while its size is zero).
    fn measure_cards(&mut self, cx: &mut Cx<'_>) {
        let Some(f) = self.first_valid() else { return };
        let first_tier = self.card_tier_of(cx, f);
        let at = if first_tier != Some(self.current_card_tier) { f } else { f + 1 };
        let Some(card) = self.cards.get(at).copied().flatten() else { return };
        let w = cx.view.tree().arranged_size(card)[0];
        if w <= 0.0 {
            cx.view.delay(0.0, Handler { owner: cx.w, id: R_MEASURE });
            return;
        }
        self.card_width = w;
        let span = Self::cards_to_show(cx) * self.card_width;
        let sz = find(cx, "SizeBox_Rewards");
        wd(cx, sz).w_override = span;
        let split = prop_bool(cx, "UseTrackSplitter");
        let sbox = find(cx, "SizeBox_0");
        let extra = wd(cx, sbox).w_override;
        let sp = find(cx, "Spacer_2");
        let x = wd(cx, sp);
        x.brush_w = if split { span + extra + 2.0 } else { span };
        x.brush_h = 1.0;
        self.disable_buttons(cx, true);
        let g = self.guid.clone();
        self.update_context(cx, &g);
    }

    fn starting_index(&self, cx: &Cx<'_>, over: i32) -> i32 {
        let n = Self::cards_to_show(cx);
        let base = if over > 0 { over } else { self.current_card_tier };
        clamp(f64::from(base) - (n * 0.5 - 0.1).trunc() - 1.0, 0.0, Self::max_tier(cx) - n) as i32
    }

    /// AddCard(Index) @12087.
    fn add_card(&mut self, cx: &mut Cx<'_>, i: i32) {
        // Deviation: a negative index (a left scroll near tier 1) fails Array_Set and the build's later calls
        // reach None; the port makes no card for it.
        let Ok(i) = usize::try_from(i) else { return };
        // Create(RewardCards): the instance's class (WBP_BPass_Card_C on the Battle Pass, WBP_CharMastery_Card_C on
        // the Collection's character page)
        let cls = prop(cx, "RewardCards").as_str().unwrap_or_default().trim_end_matches("_C").to_string();
        if !TRACK_CARDS.contains(&cls.as_str()) || !cx.view.bundle.widgets.contains_key(&cls) {
            inert(&format!("RewardCards {cls}"), "a reward track of this card class is not ported");
            return;
        }
        let w = MenuView::create(cx, &cls);
        while self.cards.len() <= i {
            self.cards.push(None);
        }
        self.cards[i] = Some(w);
        // (a ScrollBoxSlot's defaults: HAlign_Fill, VAlign_Fill)
        let x = wd(cx, w);
        x.h_align = HAlign::Fill;
        x.v_align = VAlign::Fill;
        x.fill = 0.0;
        // InsertChildAt(ScrollBox_Main, 0) scrolling left @12273, else AddChild @13589
        let sb = Self::scroll_box(cx);
        MenuView::add_child(cx, sb, w, None);
        if self.scrolling_left {
            cx.view.tree_mut().insert(sb, w, Some(0));
        }
        let me = cx.w;
        let (cur, tier_progress, has_premium) = (self.current_card_tier, self.tier_progress, self.has_premium);
        let unclaimed = self.unclaimed.contains(&(i as i32));
        let slug = self.optional_slug.clone();
        let (d_t, d_p) = (self.delta_tier, self.delta_progress);
        let focused = i as i32 + 1 == self.focused_tier;
        let width = with_card(cx, w, |c, cx| {
            c.on_card_focused().add(Handler { owner: me, id: H_CARD_FOCUSED });
            c.set_basic_tier_data(cx, cur, i as i32 + 1, tier_progress);
            c.set_has_premium(cx, has_premium, false);
            c.set_unclaimed_visuals(cx, unclaimed);
            c.get_tier_specific_data(cx, &slug);
            c.set_delta_tier_data(cx, d_t, d_p);
            if focused {
                c.update_focus_width_override(cx, true);
                Some(c.width_override())
            } else {
                None
            }
        });
        if let Some(Some(fw)) = width {
            self.focus_width = fw;
        }
    }

    /// ScrolledToTier.
    fn scrolled_to_tier(&self) -> f64 {
        let Some(f) = self.first_valid() else { return 0.0 };
        let f = f as f64;
        let fl = (self.target / self.card_width).floor();
        f + 1.0
            + if fl >= f64::from(self.focused_tier) - f {
                fl - ((self.focus_width / self.card_width).ceil() - 1.0)
            } else {
                fl
            }
    }

    /// AttemptScroll(CardsToScroll) @10958.
    fn attempt_scroll(&mut self, cx: &mut Cx<'_>, n: f64) {
        let adj = self.adjust_for_focus(cx, n);
        self.load_cards_on_scroll(cx, adj);
        self.set_target_offset(cx, adj);
        let from = self.offset(cx);
        self.lerp_scroll(cx, from);
    }

    fn adjust_for_focus(&self, cx: &Cx<'_>, n: f64) -> f64 {
        let stt = self.scrolled_to_tier();
        let a = stt + n * if self.scrolling_left { -1.0 } else { 1.0 };
        let lo = if self.scrolling_left { a } else { stt };
        let hi = if self.scrolling_left { stt } else { a };
        if in_range(f64::from(self.focused_tier), lo, hi, true, true) && n == Self::cards_to_show(cx) {
            n - 1.0
        } else {
            n
        }
    }

    /// LoadCardsOnScroll @11110 with FindInvalidCardIndex.
    fn load_cards_on_scroll(&mut self, cx: &mut Cx<'_>, n: f64) {
        let stt = self.scrolled_to_tier();
        if self.scrolling_left {
            let Some(i) = self.first_valid() else { return };
            let Some(t) = self.card_tier_of(cx, i) else { return };
            let t = f64::from(t);
            if !(t > 1.0 && t > stt - n) {
                return;
            }
            let start = i as i32 - 1;
            let mut k = start;
            while k > start - n as i32 {
                self.add_card(cx, k);
                k -= 1;
            }
            let o = self.offset(cx);
            self.set_offset(cx, n * self.card_width + o);
            let g = self.guid.clone();
            self.update_context(cx, &g);
            return;
        }
        let Some(i) = self.cards.iter().rposition(Option::is_some) else { return };
        let Some(t) = self.card_tier_of(cx, i) else { return };
        let t = f64::from(t);
        let max = Self::max_tier(cx);
        if !(max > t && t < stt + n + Self::cards_to_show(cx)) {
            return;
        }
        let start = i as i32 + 1;
        let end = clamp(f64::from(start) + n - 1.0, 1.0, max - 1.0) as i32;
        for k in start..=end {
            self.add_card(cx, k);
        }
        let g = self.guid.clone();
        self.update_context(cx, &g);
    }

    /// Set Target Offset (the row's desired width as laid out this frame: the cards added above are measured at the
    /// next layout; Deviation: the build's ForceLayoutPrepass measures them at once).
    fn set_target_offset(&mut self, cx: &mut Cx<'_>, n: f64) {
        let left = self.scrolling_left;
        let w = self.card_width;
        let d = self.focus_width - w;
        let s_n = if left { -n } else { n };
        let f = self.first_valid().map_or(-1.0, |f| f as f64);
        let stt = self.scrolled_to_tier();
        let ft = f64::from(self.focused_tier);
        let sel0 = if in_range(ft, f + 1.0, stt, true, false) { -d } else { 0.0 };
        let sel1 = if in_range(ft, f + 1.0, s_n + stt - 1.0, true, !left) { d } else { 0.0 };
        let desired = self.row_width(cx);
        let sb = Self::scroll_box(cx);
        let view_w = cx.view.tree().arranged_size(sb)[0];
        self.target = clamp(js_round((self.target + s_n * w + sel0) / w) * w + sel1, 0.0, (desired - view_w).max(0.0));
    }

    /// The cards' widths as they will be laid out (a new card is narrow; the focused one is FW once widened).
    fn row_width(&self, cx: &mut Cx<'_>) -> f64 {
        let mut s = 0.0;
        for i in 0..self.cards.len() {
            if let Some(t) = self.card_tier_of(cx, i) {
                s += if t == self.focused_tier { self.focus_width } else { self.card_width };
            }
        }
        s
    }

    /// LerpScroll(InitialOffset): 0.5 s real time, EaseInOut exponent 3, from the offset at the call to the target; a
    /// call while one runs restarts it from there (the loop's Delay(0) is already pending).
    fn lerp_scroll(&mut self, cx: &mut Cx<'_>, from: f64) {
        self.lerp_from = from;
        self.lerp_start = cx.now();
        if self.lerping {
            return;
        }
        self.lerping = true;
        self.lerp_step(cx);
    }

    fn lerp_step(&mut self, cx: &mut Cx<'_>) {
        let a = clamp((cx.now() - self.lerp_start) / 0.5, 0.0, 1.0);
        self.set_offset(cx, ease_in_out(self.lerp_from, self.target, a, 3.0));
        if a < 1.0 {
            cx.view.delay(0.0, Handler { owner: cx.w, id: R_LERP });
            return;
        }
        self.lerping = false;
        self.set_offset(cx, self.target);
        self.disable_buttons(cx, false);
        self.unload_cards(cx);
    }

    /// DisableButtons(Direct).
    fn disable_buttons(&mut self, cx: &mut Cx<'_>, direct: bool) {
        let (left, right) = (Self::left(cx), Self::right(cx));
        let max = Self::max_tier(cx);
        if direct {
            let first = self.cards.first().copied().flatten().is_none();
            let last =
                usize::try_from(max as i64 - 1).ok().and_then(|i| self.cards.get(i)).copied().flatten().is_none();
            cx.visit::<ButtonRoundedTriangleSecondary, _>(left, |b, cx| b.set_enabled(cx, first));
            cx.visit::<ButtonRoundedTriangleSecondary, _>(right, |b, cx| b.set_enabled(cx, last));
            return;
        }
        let stt = self.scrolled_to_tier();
        let l = stt != 1.0;
        if cx.visit::<ButtonRoundedTriangleSecondary, _>(left, |b, _| b.is_enabled) != Some(l) {
            cx.visit::<ButtonRoundedTriangleSecondary, _>(left, |b, cx| b.set_enabled(cx, l));
        }
        let r = stt != max - Self::cards_to_show(cx) + 1.0;
        if cx.visit::<ButtonRoundedTriangleSecondary, _>(right, |b, _| b.is_enabled) != Some(r) {
            cx.visit::<ButtonRoundedTriangleSecondary, _>(right, |b, cx| b.set_enabled(cx, r));
        }
    }

    /// UnloadCards: only the cards in view take the focus.
    fn unload_cards(&mut self, cx: &mut Cx<'_>) {
        let stt = self.scrolled_to_tier();
        let n = Self::cards_to_show(cx);
        let lo = stt - 1.0;
        let shown = if in_range(f64::from(self.focused_tier), lo, n, true, false) {
            n - ((self.focus_width / self.card_width).trunc() - 1.0)
        } else {
            n
        };
        for (i, c) in self.cards.clone().into_iter().enumerate() {
            let Some(c) = c else { continue };
            let on = in_range(i as f64, lo, lo + shown + 1.0, true, false);
            for it in with_card(cx, c, |c, cx| c.card_interactors(cx)).unwrap_or_default() {
                set_interactable(cx, it, on);
            }
        }
    }

    /// Every interactor of the track: the arrows' and the cards'.
    pub fn interactors(&self, cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut list = vec![ButtonRoundedTriangleSecondary::interactor(cx, Self::left(cx))];
        list.push(ButtonRoundedTriangleSecondary::interactor(cx, Self::right(cx)));
        for c in self.cards.clone().into_iter().flatten() {
            list.extend(with_card(cx, c, |c, cx| c.card_interactors(cx)).unwrap_or_default());
        }
        list
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.guid = ctx.to_string();
        let list = self.interactors(cx);
        batch_set_context(cx, &list, ctx);
    }

    pub fn focus_right_arrow(&mut self, cx: &mut Cx<'_>) {
        let i = ButtonRoundedTriangleSecondary::interactor(cx, Self::right(cx));
        set_focused(cx, i, false);
    }

    /// Rework @9348-9756: the card RT_Cards[OverrideFocusTier > 0 ? it : ScrolledToTier - 1], its premium or free
    /// reward.
    pub fn focus_card(&mut self, cx: &mut Cx<'_>, over: i32, premium: bool) {
        let at = if over > 0 { f64::from(over) } else { self.scrolled_to_tier() - 1.0 };
        let Some(c) = usize::try_from(at as i64).ok().and_then(|i| self.cards.get(i)).copied().flatten() else {
            return;
        };
        let list = with_card(cx, c, |c, cx| c.card_interactors(cx)).unwrap_or_default();
        if let Some(&i) = list.get(usize::from(premium)) {
            set_focused(cx, i, false);
        }
    }

    pub fn stop_card_animations(&mut self, cx: &mut Cx<'_>) {
        for c in self.cards.clone().into_iter().flatten() {
            with_card(cx, c, |c, cx| c.stop_all(cx));
        }
    }
}

impl Controller for RewardTrack {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.card_width = prop_f64(cx, "RT_CardWidth", 150.0);
        self.focus_width = prop_f64(cx, "RT_CardWidth_FocusOverride", 150.0);
        let me = cx.w;
        let (left, right) = (Self::left(cx), Self::right(cx));
        cx.visit::<ButtonRoundedTriangleSecondary, _>(left, |b, _| b.on_click.add(Handler { owner: me, id: H_LEFT }));
        cx.visit::<ButtonRoundedTriangleSecondary, _>(right, |b, _| b.on_click.add(Handler { owner: me, id: H_RIGHT }));
    }

    /// PreConstruct -> AddArrowButtons (@13926).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let button_pad = prop_f64(cx, "ButtonPadding", 0.0);
        let top = if prop_bool(cx, "ButtonPaddingBottomOnly") { 0.0 } else { button_pad };
        for n in ["UI_ButtonLeft", "UI_ButtonRight", "SizeBox_0"] {
            let w = find(cx, n);
            let x = wd(cx, w);
            x.pad = pad(x.pad.l, top, x.pad.r, button_pad);
        }
        let split = prop_bool(cx, "UseTrackSplitter");
        set_vis(cx, "SizeBox_0", if split { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
        for n in ["IgnoreSizeOnlyLeft", "IgnoreSizeOnlyRight"] {
            let w = find(cx, n);
            cx.visit::<ButtonRoundedTriangleSecondary, _>(w, |b, cx| b.use_for_size_only(cx));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_LEFT | H_RIGHT => {
                self.scrolling_left = id == H_LEFT;
                self.attempt_scroll(cx, Self::cards_to_show(cx));
            }
            // CardFocused @15364: ScrollWidgetIntoView(card, animate, IntoView, 0), the scroll offset only
            // (TargetOffset follows the user's scrolling only through ScrolledVia Bar @15414, bound while the mouse is
            // over the track: not ported with the wheel). Unverified: SScrollBox's scroll into view and its animation
            // from the 4.26 source.
            H_CARD_FOCUSED => {
                if let Some(&card) = arg.downcast_ref::<WidgetId>() {
                    let sb = Self::scroll_box(cx);
                    scroll_into_view(cx.view.tree_mut(), sb, card, true, ScrollDestination::IntoView, 0.0);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            R_MEASURE => self.measure_cards(cx),
            R_LERP => self.lerp_step(cx),
            _ => {}
        }
    }
}

// ---------- UI_BattlePass_Tokens ----------

/// UI_BattlePass_Tokens. Construct: "x{count}" of the premium tokens in the inventory (none without a backend).
#[derive(Debug, Default)]
pub struct BattlePassTokens;

impl BattlePassTokens {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(BattlePassTokens)
    }
}

impl Controller for BattlePassTokens {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let t = find(cx, "Text_Count");
        wd(cx, t).text = "x0".to_string();
    }
}

// ---------- UI_BattlePass_PremiumUnlockButton ----------

const H_UNLOCK: u32 = 1;
const H_TOKEN: u32 = 2;

/// UI_BattlePass_PremiumUnlockButton.
#[derive(Debug, Default)]
pub struct PremiumUnlockButton;

impl PremiumUnlockButton {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PremiumUnlockButton)
    }

    fn unlock(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Btn_UnlockPremium")
    }

    fn use_token(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Btn_UseToken")
    }

    /// Update Appearance(HasPremium), with no premium tokens and the tier purchase feature off.
    pub fn update_appearance(&mut self, cx: &mut Cx<'_>, has: bool) {
        let tokens = 0;
        let (unlock, token) = (Self::unlock(cx), Self::use_token(cx));
        let i = cx.view.tree().find_in(unlock, "UI_Interactor");
        set_interactable(cx, i, !has);
        let label = if has { "PREMIUM UNLOCKED" } else { "UNLOCK PREMIUM" };
        cx.visit::<ButtonMain, _>(unlock, |b, cx| b.set_label(cx, label));
        wd(cx, token).vis = if !has && tokens > 0 { Vis::Visible } else { Vis::Collapsed };
        wd(cx, unlock).vis = if tokens > 0 || has { Vis::Collapsed } else { Vis::Visible };
        set_vis(cx, "UI_BattlePass_TierPurchaseButton", Vis::Collapsed);
    }

    pub fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        [Self::unlock(cx), Self::use_token(cx)]
            .into_iter()
            .map(|b| cx.view.tree().find_in(b, "UI_Interactor"))
            .collect()
    }
}

impl Controller for PremiumUnlockButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // TryUnlockPremium opens the purchase: inert
        let me = cx.w;
        let (unlock, token) = (Self::unlock(cx), Self::use_token(cx));
        cx.visit::<ButtonMain, _>(unlock, |b, _| b.on_click.add(Handler { owner: me, id: H_UNLOCK }));
        cx.visit::<ButtonMain, _>(token, |b, _| b.on_click.add(Handler { owner: me, id: H_TOKEN }));
    }

    fn handle(&mut self, _cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_UNLOCK => inert("Btn_UnlockPremium", "buying the premium pass needs the backend"),
            H_TOKEN => inert("Btn_UseToken", "a premium token needs the backend"),
            _ => {}
        }
    }
}

// ---------- UI_BXP_Boost (the rested-XP badge, at render opacity 0 on this screen) ----------

const H_BOOST_FOCUS: u32 = 1;
const H_BOOST_UNFOCUS: u32 = 2;

/// UI_BXP_Boost.
#[derive(Debug, Default)]
pub struct BXPBoost;

/// String(n) of a number.
fn js_number(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }
}

impl BXPBoost {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(BXPBoost)
    }

    pub fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }

    pub fn update_multiplier(&mut self, cx: &mut Cx<'_>, m: f64) {
        let t = find(cx, "txtb_Multiplier");
        wd(cx, t).text = js_number(m);
    }

    /// HandleDefaultAppearance: the medal's brush becomes M_RestedIcon_Scroll.
    pub fn handle_default_appearance(&mut self, cx: &mut Cx<'_>) {
        let tex = resolve(&cx.view.bundle, "M_RestedIcon_Scroll");
        let i = find(cx, "i_Medal");
        let x = wd(cx, i);
        x.tex = Some(tex);
        x.mid = None;
    }
}

impl Controller for BXPBoost {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // OnFocus @694: PlayAnimation(HoverBounce, 0, loops 0 = forever, PingPong, 1)
        let i = Self::interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_BOOST_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_BOOST_UNFOCUS);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let m = prop_f64(cx, "Multiplier", 2.0);
        let t = find(cx, "txtb_Multiplier");
        wd(cx, t).text = js_number(m);
    }

    /// Construct: the Tooltip text as the interactor's tip at (0, -50), placement 7.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let tip = text(cx.view.tree().props(cx.w).get("Tooltip"));
        if !tip.is_empty() {
            let i = Self::interactor(cx);
            set_tooltip(cx, i, tooltip_params(&tip, true, 0.0, -50.0, 7));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_BOOST_FOCUS => cx.view.play_animation(cx.w, "HoverBounce", false, 1.0, 0, true, false),
            H_BOOST_UNFOCUS => cx.view.stop_all_animations(cx.w),
            _ => {}
        }
    }
}

// ---------- UI_BattlePass_Rework (the modal `battlepass`) ----------

/// The debug Battle Pass ShowDebugBPass loads when there is no backend: tier 10, 2 XP, no premium.
const DEBUG_TIER: i32 = 10;

const H_MODAL_DISMISSED: u32 = 1;
const H_CLAIM: u32 = 2;
const R_CLAIM_OFF: u32 = 1;
const R_FADE_IN: u32 = 2;
const R_ANIM_IN: u32 = 3;
const R_DISMISS: u32 = 4;

/// UI_BattlePass_Rework.
#[derive(Debug, Default)]
pub struct BattlePassRework {
    base: ModalBase,
    focus_on_load: bool,
    override_focus_tier: i32,
    tier_set: bool,
    ctx_once: bool,
    quests_once: bool,
    anim_once: bool,
}

impl BattlePassRework {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(BattlePassRework::default())
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }

    fn track(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_RewardTrack")
    }

    fn claim(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_ButtonMain_Claim")
    }

    fn premium(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_BattlePass_PremiumUnlockButton")
    }

    fn boost(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_BXP_Boost")
    }

    /// UpdateOtherWidgets: the cached Battle Pass (none: tier 0 + 1 clamped to its 0 tiers) and the quests (none).
    fn update_other_widgets(&mut self, cx: &mut Cx<'_>) {
        let t = find(cx, "Txt_Tier");
        wd(cx, t).text = "To Tier 0 Rewards".to_string();
        let p = Self::premium(cx);
        cx.visit::<PremiumUnlockButton, _>(p, |b, cx| b.update_appearance(cx, false));
        set_vis(cx, "Missions", Vis::Collapsed);
        set_vis(cx, "StarterMissionText", Vis::SelfHitTestInvisible);
    }

    /// ForceSetRewardsButton(UnclaimedRewards): the label, the badge's count (its MainContainer stays hidden), and
    /// after Delay(0) the button takes clicks only with rewards loaded (never here).
    fn force_set_rewards_button(&mut self, cx: &mut Cx<'_>, n: i32) {
        let label = if n > 0 { format!("CLAIM ALL REWARDS ({n})") } else { "NO REWARDS TO CLAIM".to_string() };
        let c = Self::claim(cx);
        cx.visit::<ButtonMain, _>(c, |b, cx| b.set_label(cx, &label));
        cx.view.delay(0.0, Handler { owner: cx.w, id: R_CLAIM_OFF });
    }

    /// LoadPlayerBatlePassInfoResult @11051 with the debug data.
    fn load_player_battle_pass_info_result(&mut self, cx: &mut Cx<'_>) {
        // UpdateTierXPValues
        set_vis(cx, "Overlay_RewardsComplete", Vis::Collapsed);
        set_vis(cx, "HorizontalBox_TierInfo", Vis::SelfHitTestInvisible);
        set_vis(cx, "RestedXPContainer", Vis::SelfHitTestInvisible);
        set_vis(cx, "Hbox_TotalBP", Vis::Collapsed);
        // Deviation: the XP to the next tier comes from the Battle Pass asset, absent here; the capture's 8
        let (xp, total) = (find(cx, "Txt_XPRemaining"), find(cx, "Txt_Total"));
        wd(cx, xp).text = "8".to_string();
        wd(cx, total).text = "0".to_string();
        // LoadUnclaimedRewards -> ForceSetRewardsButton(0) -> LoadedTrack -> SlideoutLoaded
        self.force_set_rewards_button(cx, 0);
        self.update_other_widgets(cx);
        let track = Self::track(cx);
        let over = self.override_focus_tier;
        cx.visit::<RewardTrack, _>(track, |t, _| t.has_premium = false);
        if !self.tier_set {
            self.tier_set = true;
            // GetTierProgress returns 1.0 when IsValidIndex fails (@399: no asset); the unclaimed list [0] is @10828
            cx.visit::<RewardTrack, _>(track, |t, cx| t.set_current_tier(cx, DEBUG_TIER, 1.0, &[0], over, 0.0, 0.0));
        }
        let ctx = self.base.context.clone();
        self.update_context(cx, &ctx);
        self.override_focus_tier = 0;
        let boost = Self::boost(cx);
        cx.visit::<BXPBoost, _>(boost, |b, cx| {
            b.update_multiplier(cx, 0.0);
            b.handle_default_appearance(cx);
        });
        Manager::set_waiting(cx, false, "", false, 0.0);
        if !self.quests_once {
            self.quests_once = true;
            for n in ["UI_QuestsContainer_Daily", "UI_QuestsContainer_Seasonal"] {
                let q = find(cx, n);
                wd(cx, q).opacity = 0.0;
            }
        }
        cx.view.delay(0.2, Handler { owner: cx.w, id: R_FADE_IN });
    }

    fn all_interactors(&self, cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut list = vec![cx.view.tree().find_in(Self::claim(cx), "UI_Interactor")];
        let linked = find(cx, "WBP_BPass_Linked");
        if let Some(b) = cx.view.tree().find_opt(linked, "UI_ButtonSecondary") {
            list.push(ButtonSecondary::interactor(cx, b));
        }
        list.push(BXPBoost::interactor_of(cx, Self::boost(cx)));
        let track = Self::track(cx);
        if let Some(l) = cx.visit::<RewardTrack, _>(track, |t, cx| t.interactors(cx)) {
            list.extend(l);
        }
        list.extend(PremiumUnlockButton::interactors_of(cx, Self::premium(cx)));
        list
    }

    /// UpdateContext @16760.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let list = self.all_interactors(cx);
        batch_set_context(cx, &list, ctx);
        let track = Self::track(cx);
        cx.visit::<RewardTrack, _>(track, |t, cx| t.update_context(cx, ctx));
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.update_context(cx, ctx));
        if !self.ctx_once {
            self.ctx_once = true;
            // DoOnce @9308: FocusOnLoad -> the card (@9334-9793), else the right arrow (@9805)
            if self.focus_on_load {
                let over = self.override_focus_tier;
                let premium = prop_bool(cx, "HighlightPremium");
                cx.visit::<RewardTrack, _>(track, |t, cx| t.focus_card(cx, over, premium));
                self.focus_on_load = false;
            } else {
                cx.visit::<RewardTrack, _>(track, |t, cx| t.focus_right_arrow(cx));
            }
        }
    }

    fn unbind(cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_dismissed.remove(Handler { owner: me, id: H_MODAL_DISMISSED });
    }
}

impl BXPBoost {
    fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }
}

impl PremiumUnlockButton {
    fn interactors_of(cx: &Cx<'_>, w: WidgetId) -> Vec<WidgetId> {
        ["Btn_UnlockPremium", "Btn_UseToken"]
            .into_iter()
            .map(|n| {
                let b = cx.view.tree().find_in(w, n);
                cx.view.tree().find_in(b, "UI_Interactor")
            })
            .collect()
    }
}

impl Modal for BattlePassRework {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @18029, then ShowDebugBPass @17115.
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        let id = self.base.modal_id.clone();
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.open_slide_out(cx, &id));
        let me = cx.w;
        wd(cx, me).opacity = 0.0;
        Manager::set_waiting(cx, true, "Loading BattlePass", false, 0.0);
        let n = instructions.trim().parse::<f64>().unwrap_or(0.0);
        self.override_focus_tier = if n.is_finite() { n.trunc() as i32 } else { 0 };
        self.focus_on_load = !instructions.is_empty();
        Manager::set_waiting(cx, false, "", false, 0.0);
        self.load_player_battle_pass_info_result(cx);
        // (the Delay(10) @17647 -> @2930 fills the collapsed quest containers with debug quests: not ported)
    }

    /// PreDismiss @24088: StopAllAnimations on every reward card and on self.
    fn pre_dismiss(&mut self, cx: &mut Cx<'_>) {
        let track = Self::track(cx);
        cx.visit::<RewardTrack, _>(track, |t, cx| t.stop_card_animations(cx));
        cx.view.stop_all_animations(cx.w);
    }

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @17886: Delay(0) -> CloseSlideOut (the analytics are not ported).
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        cx.view.delay(0.0, Handler { owner: cx.w, id: R_DISMISS });
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for BattlePassRework {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.base.context = cx.view.manager.new_guid();
        let me = cx.w;
        cx.view.manager.modal_dismissed.add(Handler { owner: me, id: H_MODAL_DISMISSED });
        // ClaimAllRewards needs the backend
        let c = Self::claim(cx);
        cx.visit::<ButtonMain, _>(c, |b, _| b.on_click.add(Handler { owner: me, id: H_CLAIM }));
    }

    /// PreConstruct @21232.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.update_other_widgets(cx);
        self.force_set_rewards_button(cx, 55);
        let so = Self::slide_out(cx);
        let clip = cx.view.tree().find_in(so, "Overlay_ClipBounds");
        let o = &mut wd(cx, clip).offsets;
        o.r = 2250.0;
        o.b = 0.0;
        // WBP_MenuHeader.OverrideFillColor -> UpdateColor of its three fill quads
        let header = find(cx, "WBP_MenuHeader");
        let c = [0.009721, 0.011612, 0.033105, 1.0];
        for n in ["FillQuadBottom", "FillQuadLeft", "FillQuadRight"] {
            if let Some(q) = cx.view.tree().find_opt(header, n) {
                cx.visit::<Quadrilateral, _>(q, |q, cx| q.update_color(cx, c));
            }
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_MODAL_DISMISSED => {
                if arg.downcast_ref::<String>() == Some(&self.base.modal_id) {
                    Self::unbind(cx);
                }
            }
            H_CLAIM => inert("UI_ButtonMain_Claim", "claiming rewards needs the backend"),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            R_CLAIM_OFF => {
                let c = Self::claim(cx);
                let i = cx.view.tree().find_in(c, "UI_Interactor");
                set_interactable(cx, i, false);
            }
            R_FADE_IN => {
                let me = cx.w;
                wd(cx, me).opacity = 1.0;
                cx.view.delay(0.1, Handler { owner: me, id: R_ANIM_IN });
            }
            R_ANIM_IN => {
                if self.anim_once {
                    return;
                }
                self.anim_once = true;
                cx.view.play(cx.w, "AnimIn", false, 1.0);
                let ctx = self.base.context.clone();
                self.update_context(cx, &ctx);
            }
            R_DISMISS => {
                let id = self.base.modal_id.clone();
                let so = Self::slide_out(cx);
                cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.close_slide_out(cx, &id));
                Self::unbind(cx);
            }
            _ => {}
        }
    }
}

/// (the port's own) the pictures a character's mastery track draws: the rewards' thumbnails and type icons,
/// MI_PerkContainer, and the perk icons its instances sample. The bundle's names and those sampled
/// (`MenuView::hold_pictures`).
pub fn mastery_pictures(view: &MenuView, slug: &str) -> (Vec<String>, Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    let mut sampled: Vec<String> = Vec::new();
    let add = |list: &mut Vec<String>, n: &str| {
        if !list.iter().any(|x| x == n) {
            list.push(n.to_string());
        }
    };
    for r in mastery_rewards(view, slug) {
        if r.kind == 13 {
            add(&mut names, PERK_CONTAINER);
            if let Some(t) = &r.thumb {
                add(&mut sampled, t);
            }
            continue;
        }
        if let Some(t) = &r.thumb {
            add(&mut names, t);
        }
        if let Some(icon) = type_icon(r.kind) {
            add(&mut names, icon);
        }
    }
    (names, sampled)
}
