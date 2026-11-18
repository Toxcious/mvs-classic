//! WBP_TextTag: a text on a three-part backing.

use crate::menu::controller::{Controller, Cx};
use crate::menu::widgets::common::{rgba, text};
use crate::menu::widgets::controls::{find, prop, wd};
use crate::slate::Vis;

#[derive(Debug, Default)]
pub struct TextTag;

impl TextTag {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(TextTag)
    }
}

impl Controller for TextTag {
    /// PreConstruct: SetText(In Text), the text's colour and size, the shadow unless HideShadow.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let t = find(cx, "TagText");
        let s = text(Some(&prop(cx, "In Text")));
        wd(cx, t).text = s;
        let c = prop(cx, "TextColor");
        if c.is_object() {
            let c = rgba(c.get("SpecifiedColor").unwrap_or(&c));
            wd(cx, t).color = c;
        }
        if let Some(n) = prop(cx, "FontSize").as_f64().filter(|n| *n != 0.0) {
            wd(cx, t).font_size = n;
        }
        let hide = prop(cx, "HideShadow").as_bool().unwrap_or(false);
        let sh = find(cx, "Shadow");
        wd(cx, sh).vis = if hide { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
    }

    /// Construct: the backing in Color, its shadow copy in ShadowColor.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let color = prop(cx, "Color");
        let shadow = prop(cx, "ShadowColor");
        for s in ["Left", "Middle", "Right"] {
            if color.is_object() {
                let w = find(cx, &format!("TagBacking_{s}"));
                wd(cx, w).color = rgba(&color);
            }
            if shadow.is_object() {
                let w = find(cx, &format!("TagBacking_{s}_1"));
                wd(cx, w).color = rgba(&shadow);
            }
        }
    }
}
