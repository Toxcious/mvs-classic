//! UBackgroundBlur::SetBlurStrength. The pass's numbers and the
//! draw are in menu/blur.rs, the strength rules in slate.rs.

use crate::menu::controller::Cx;
use crate::slate::WidgetId;

/// SetBlurStrength on an instance: the value it has from then on, over its tree's.
pub fn set_blur_strength(cx: &mut Cx<'_>, w: WidgetId, strength: f64) {
    cx.view.tree_mut().widget_mut(w).blur_set = Some(strength);
}
