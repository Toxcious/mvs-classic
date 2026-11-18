//! The banner element classes with a graph beyond the UCS,
//! entered by class name. A class the entry names with no row warns once, no default.

use mvs_vfx::RandomSource;

use super::BannerElement;
use crate::fx::warn_once;

/// A class's BeginPlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BannerClass {
    /// WaterfallStyle / SparkleFalls_Legendary: CreateDynamicMaterialInstance(0, the parent Material) on `waterfall`
    /// (@10), then its OffsetU = RandomFloat (@83-111).
    Waterfall(&'static str),
    /// WorkInProgress: PS_Electrical_Wire.SetWorldScale3D(1, 1, 1) (@10); PS_Electrical_Wire1 keeps its 0.8.
    WorkInProgress,
}

fn class_of(name: &str) -> Option<BannerClass> {
    Some(match name {
        "BP_Banner_WaterfallStyle_C" => BannerClass::Waterfall("M_Waterfall_BannerStyle"),
        "BP_Banner_SparkleFalls_Legendary_C" => BannerClass::Waterfall("M_SparkleFalls_Banner"),
        "BP_Banner_WorkInProgress_C" => BannerClass::WorkInProgress,
        _ => return None,
    })
}

/// The BeginPlay graphs of the classes the entry's `graphs` names, nearest first.
pub(super) fn begin_play(el: &mut BannerElement, rng: &mut dyn RandomSource) {
    for cls in el.data.graphs.clone() {
        match class_of(&cls) {
            // The component's slot already holds that Material (its data key is the Material's own), so the instance is
            // a MID of it; a key that is not that Material warns. Initialize Banner Element's own
            // CreateDynamicMaterialInstance on the slot (old path @846, both classes OldInitialize) returns this MID as
            // it is: the one instance carries OffsetU and then StencilValue.
            Some(BannerClass::Waterfall(material)) => {
                let want = format!("/Game/Panda_Main/PreMatch/Banners/Waterfall/{material}.{material}");
                let key = el.material_of("waterfall");
                let path = key.as_deref().and_then(mvs_material::draw::parse_draw_ref).map(|r| r.material);
                if path != Some(want.as_str()) {
                    warn_once(format!("banners: {}'s waterfall slot is {key:?}, not {material}", el.data.slug));
                }
                el.set_scalar("waterfall", "OffsetU", rng.next_f64());
            }
            Some(BannerClass::WorkInProgress) => el.set_world_scale("PS_Electrical_Wire", [1.0, 1.0, 1.0]),
            None => warn_once(format!("banners: class {cls} ({}) has a graph and no class entry", el.data.slug)),
        }
    }
}
