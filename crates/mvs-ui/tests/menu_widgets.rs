//! The menu widget types on the menu bundle: a ScrollBox's content, offset and bar, a GridPanel and a WrapBox's
//! cells. Needs the dev store; without one each test says so and passes.

use std::sync::Arc;

use mvs_ui::Bundle;
use mvs_ui::menu::scroll::{ScrollBars, scroll_by, scroll_list, scroll_offset};
use mvs_ui::menu::view::{Layer, ViewBars};
use mvs_ui::slate::{Geometry, Tree, WidgetId, instantiate, layout_widget};
use mvs_ui::text::Fonts;

fn env() -> Option<(Arc<Bundle>, Fonts)> {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return None;
        }
    };
    let bundle = Bundle::load(&store, "menu").ok()?;
    let fonts = Fonts::new(&store, &bundle).ok()?;
    Some((Arc::new(bundle), fonts))
}

/// A canvas of 400 x 300 holding a ScrollBox of ten 100-high size boxes in a VerticalBox's place (the box's own
/// slots).
fn scroll_tree(b: &Bundle) -> (Tree, WidgetId, WidgetId) {
    let mut t = instantiate(b, "UI_ContextMenu", "x");
    let canvas = t.create_plain(b, "CanvasPanel", "canvas");
    let sb = t.create_plain(b, "ScrollBox", "sb");
    t.add(canvas, sb);
    {
        let w = t.widget_mut(sb);
        w.anchors = [0.0; 4];
        w.offsets.l = 0.0;
        w.offsets.t = 0.0;
        w.offsets.r = 400.0;
        w.offsets.b = 300.0;
    }
    for i in 0..10 {
        let k = t.create_plain(b, "SizeBox", &format!("k{i}"));
        t.widget_mut(k).h_override = 100.0;
        t.add(sb, k);
    }
    (t, canvas, sb)
}

#[test]
fn a_scroll_box_scrolls_by_its_content_less_its_view() {
    let Some((b, mut fonts)) = env() else { return };
    let (mut t, canvas, sb) = scroll_tree(&b);
    layout_widget(&mut t, &mut fonts, canvas, Geometry { pos: [0.0, 0.0], size: [400.0, 300.0] }, 1.0);
    let _ = canvas;
    assert_eq!(t.widget(sb).scroll_max, 700.0, "1000 of content in a view of 300");
    assert!(scroll_by(&mut t, sb, 50.0, false));
    assert_eq!(scroll_offset(&mut t, sb), 50.0);
    scroll_by(&mut t, sb, 5000.0, false);
    assert_eq!(scroll_offset(&mut t, sb), 700.0, "clamped to the end");
    scroll_list(&mut t, sb, 10.0);
    assert_eq!(scroll_offset(&mut t, sb), 630.0, "ScrollList moves 7 per value");
}

#[test]
fn a_scroll_bar_takes_its_slot_from_the_content_when_it_scrolls() {
    let Some((b, mut fonts)) = env() else { return };
    let (mut t, canvas, sb) = scroll_tree(&b);
    let mut bars = ScrollBars::new(b.clone());
    bars.collect(&t, &[Layer { w: canvas, z: 0 }]);
    layout_widget(&mut t, &mut fonts, canvas, Geometry { pos: [0.0, 0.0], size: [400.0, 300.0] }, 1.0);
    bars.tick(&mut t, 0.016, None);
    layout_widget(&mut t, &mut fonts, canvas, Geometry { pos: [0.0, 0.0], size: [400.0, 300.0] }, 1.0);
    let k = t.widget(sb).children[0];
    let [_, _, w, _] = t.arranged(k);
    assert!(w < 400.0, "the bar's slot is out of the children's width: {w}");
    let _ = sb;
}
