//! UI_CollectionChooser on the menu bundle: a pick moves the tiles' checkmark (EquipperItem SelectedIndicator) at
//! once, in the list whose tile was clicked. Needs the dev store; without one the test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::screens::lab_selectors::{EquipperItem, GridItemList};
use mvs_ui::menu::widgets::collection_chooser::CollectionChooser;
use mvs_ui::menu::widgets::controls::grid_item::GridItemState;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::{Vis, WidgetId};
use mvs_ui::text::{Fonts, GameText};

struct Env {
    bundle: Arc<Bundle>,
    text: Arc<GameText>,
    fonts: Fonts,
    prefs: Settings,
    bindings: Bindings,
    sink: NoSound,
    pointer: Pointer,
}

fn env() -> Option<Env> {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return None;
        }
    };
    let bundle = match Bundle::load(&store, "menu") {
        Ok(b) if b.widgets.contains_key("UI_CollectionChooser") => b,
        _ => {
            eprintln!("no menu bundle with UI_CollectionChooser: skipped");
            return None;
        }
    };
    let fonts = Fonts::new(&store, &bundle).unwrap();
    let text = GameText::load(&store).unwrap_or_default();
    Some(Env {
        bundle: Arc::new(bundle),
        text: Arc::new(text),
        fonts,
        prefs: Settings::default(),
        bindings: Bindings::default(),
        sink: NoSound,
        pointer: Pointer::default(),
    })
}

impl Env {
    fn run(&mut self) -> MenuRun<'_> {
        MenuRun {
            fonts: &mut self.fonts,
            prefs: &mut self.prefs,
            bindings: &mut self.bindings,
            sink: &mut self.sink,
            pointer: &mut self.pointer,
            lab: None,
            pause: None,
            tutorial: None,
        }
    }
}

/// One second of frames.
fn settle(s: &mut MenuScreen, e: &mut Env, t: &mut f64) {
    for _ in 0..60 {
        *t += 1.0 / 60.0;
        s.frame(&mut e.run(), &[], *t, &mut ElementList::default());
    }
}

/// The tile's SelectedIndicator shown.
fn checked(s: &MenuScreen, tile: WidgetId) -> bool {
    let i = s.view.tree().find_in(tile, "SelectedIndicator");
    s.view.tree().widget(i).vis == Vis::HitTestInvisible
}

#[test]
fn a_pick_moves_the_checkmark_at_once() {
    let Some(mut e) = env() else { return };
    for kind in ["announcer", "banner", "ringout"] {
        let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1920, 1080]);
        let mut s = MenuScreen::new(view, &mut e.run());
        let k = kind.to_string();
        let ch = s.with_cx(&mut e.run(), |cx| {
            let w = MenuView::create(cx, "UI_CollectionChooser");
            MenuView::add_to_viewport(cx, w, 0);
            cx.visit::<CollectionChooser, _>(w, |c, cx| {
                c.init(cx, None, None, 0.0, &k);
                c.show(cx);
            });
            w
        });
        let mut t = 1.0;
        settle(&mut s, &mut e, &mut t);
        let list = s.view.tree().find_in(ch, "primaryList");
        let tiles = s.view.controller::<GridItemList>(list).expect("the list").items();
        if tiles.len() < 2 {
            eprintln!("{kind}: fewer than two tiles: skipped");
            continue;
        }
        // two picks: the first onto an unchecked tile, the second away from it
        for _ in 0..2 {
            let before: Vec<bool> = tiles.iter().map(|&w| checked(&s, w)).collect();
            let Some(pick) = tiles.iter().rposition(|&w| !checked(&s, w)) else {
                panic!("{kind}: every tile checked: {before:?}");
            };
            let tile = tiles[pick];
            s.with_cx(&mut e.run(), |cx| {
                cx.visit::<EquipperItem, _>(tile, |t, cx| GridItemState::select(t, cx, 0))
                    .expect("the tile's controller");
            });
            settle(&mut s, &mut e, &mut t);
            let after: Vec<bool> = tiles.iter().map(|&w| checked(&s, w)).collect();
            let want: Vec<bool> = (0..tiles.len()).map(|i| i == pick).collect();
            assert_eq!(after, want, "{kind}: the checkmarks after picking tile {pick} (before: {before:?})");
        }
    }
}
