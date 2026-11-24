//! A map's tag on its tile: the build's Experimental tag (UI_PM_Experimental), and (the port's own) the same tag
//! reading Custom on a shipped layout's map.

use serde_json::json;

use crate::bundle::MapData;
use crate::menu::controller::Cx;
use crate::menu::custom_text::CUSTOM_TAG;
use crate::menu::widgets::controls::wd;
use crate::slate::{Vis, WidgetId};

/// The tag's text widget.
const LABEL: &str = "ExperimentalLabel";
/// (the port's) The tag's own words, kept on it while it reads Custom (a tile shown again with another map).
const BUILD_TEXT: &str = "(port) Build Text";

/// Which tag a map's tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapTag {
    None,
    Experimental,
    Custom,
}

impl MapTag {
    pub fn of(m: &MapData) -> MapTag {
        if m.custom {
            MapTag::Custom
        } else if m.experimental {
            MapTag::Experimental
        } else {
            MapTag::None
        }
    }
}

/// The tag widget `tag` showing `t`: `shown` or `hidden`, and its words the build's or Custom.
pub fn show(cx: &mut Cx<'_>, tag: WidgetId, t: MapTag, shown: Vis, hidden: Vis) {
    let label = cx.view.tree().find_in(tag, LABEL);
    let kept = cx.view.tree().props(tag).get(BUILD_TEXT).and_then(|v| v.as_str()).map(str::to_owned);
    match t {
        MapTag::Custom => {
            if kept.is_none() {
                let own = cx.view.tree().widget(label).text.clone();
                cx.view.tree_mut().set_prop(tag, BUILD_TEXT, json!(own));
            }
            wd(cx, label).text = CUSTOM_TAG.to_owned();
        }
        _ => {
            if let Some(own) = kept {
                wd(cx, label).text = own;
            }
        }
    }
    wd(cx, tag).vis = if t == MapTag::None { hidden } else { shown };
}
