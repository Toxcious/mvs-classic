//! UI_ParallaxItems (the Blueprint): a list of its owner's widgets, each with a ParallaxMultiplier,
//! handed to the cursor, whose OffsetView moves them with the pointer (cursor.rs). An item whose Widget is None in
//! the instance (the home screen's two, five of the pre-match's) moves nothing: the cursor drops it as invalid.

use serde_json::Value;

use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::cursor::{Parallaxer, with_cursor};
use crate::slate::WidgetId;

/// The Locate Timer's period (K2_SetTimerDelegate(0.05, looping) @58).
const LOCATE_PERIOD: f64 = 0.05;
const LOCATE: u32 = 1;

/// UI_ParallaxItemStruct as the instance holds it: the owner's widget (None: unset or not in the owner's tree) and
/// the multiplier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParallaxItem {
    pub widget: Option<WidgetId>,
    pub multiplier: i32,
}

/// UI_ParallaxItems.
#[derive(Debug, Default)]
pub struct ParallaxItems {
    /// Parallax Items.
    pub items: Vec<ParallaxItem>,
    locating: bool,
}

/// A struct field by its member name (the stored name may carry the `_<n>_<GUID>` suffix).
fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v.as_object()?
        .iter()
        .find(|(k, _)| *k == name || k.strip_prefix(name).is_some_and(|r| r.starts_with('_')))
        .map(|(_, x)| x)
}

impl ParallaxItems {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<ParallaxItems>::default()
    }

    /// The user widget whose tree holds `w` (the owner the items' widgets are named in).
    fn owner_of(cx: &Cx<'_>, w: WidgetId) -> Option<WidgetId> {
        let t = cx.view.tree();
        let mut p = t.widget(w).parent;
        while let Some(x) = p {
            if t.user_class(x).is_some() {
                return Some(x);
            }
            p = t.widget(x).parent;
        }
        None
    }

    /// The instance's Parallax Items, each widget looked up by name in the owner's tree.
    fn read_items(cx: &Cx<'_>) -> Vec<ParallaxItem> {
        let owner = Self::owner_of(cx, cx.w);
        let Some(Value::Array(list)) = cx.view.tree().props(cx.w).get("Parallax Items") else { return Vec::new() };
        list.iter()
            .map(|it| {
                let name = field(it, "Widget").and_then(Value::as_str).filter(|n| !n.is_empty() && *n != "None");
                let widget = owner.zip(name).and_then(|(o, n)| cx.view.tree().find_opt(o, n));
                let multiplier = field(it, "ParallaxMultiplier").and_then(Value::as_f64).unwrap_or(0.0) as i32;
                ParallaxItem { widget, multiplier }
            })
            .collect()
    }

    /// The cursor's entry of `item` (None: no widget, which the cursor would drop at once).
    pub fn parallaxer(&self, cx: &Cx<'_>, item: ParallaxItem) -> Option<Parallaxer> {
        Some(Parallaxer { widget: item.widget?, multiplier: item.multiplier, owner: cx.w })
    }
}

impl Controller for ParallaxItems {
    /// OnInitialized @143 -> @35: the Locate Timer, every 0.05 s until FindUICursor(0) finds the cursor (@269), then
    /// the cursor's AddParallaxItems(Parallax Items) and the timer cleared (@148-244).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.items = Self::read_items(cx);
        self.locating = true;
        cx.view.delay(LOCATE_PERIOD, Handler { owner: cx.w, id: LOCATE });
    }

    /// Destruct @568: Parallax Items emptied, the timer cleared. The cursor's entries stay until it finds them gone.
    fn destruct(&mut self, _cx: &mut Cx<'_>) {
        self.items.clear();
        self.locating = false;
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id != LOCATE || !self.locating {
            return;
        }
        if cx.view.cursor.is_none() {
            cx.view.delay(LOCATE_PERIOD, Handler { owner: cx.w, id: LOCATE });
            return;
        }
        self.locating = false;
        let list: Vec<Parallaxer> = self.items.iter().filter_map(|&i| self.parallaxer(cx, i)).collect();
        with_cursor(cx, move |c| {
            for p in list {
                c.add_parallax_item(p);
            }
        });
    }
}
