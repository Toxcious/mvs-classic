//! The main menu's party partner nameplate (PreMatch's PartnerNameplate_PM, UI_PM_PartnerNameplate): a screen-space
//! WidgetComponent with no Pivot in the asset (the class default 0.5, 0.5), bDrawAtDesiredSize, so the canvas root
//! sits at its desired size centred on the projected point and the overlay (anchor 0.5, 0, alignment 0.5, 0) centred
//! on it across. Needs the dev store; without one the test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::screens::main_menu::PartnerNameplate;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::text::{Fonts, GameText};

#[test]
fn the_partner_nameplate_is_centred_on_its_point() {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return;
        }
    };
    let Ok(bundle) = Bundle::load(&store, "menu") else {
        eprintln!("no menu bundle: skipped");
        return;
    };
    if !bundle.widgets.contains_key("UI_PM_PartnerNameplate") {
        eprintln!("no UI_PM_PartnerNameplate in the bundle: skipped");
        return;
    }
    let mut fonts = Fonts::new(&store, &bundle).unwrap();
    let text = GameText::load(&store).unwrap_or_default();
    let (mut prefs, mut bindings, mut sink, mut pointer) =
        (Settings::default(), Bindings::default(), NoSound, Pointer::default());
    let mut run = MenuRun {
        fonts: &mut fonts,
        prefs: &mut prefs,
        bindings: &mut bindings,
        sink: &mut sink,
        pointer: &mut pointer,
        lab: None,
        pause: None,
        tutorial: None,
    };
    let view = MenuView::new(Arc::new(bundle), Arc::new(text), [1920, 1080]);
    let mut s = MenuScreen::new(view, &mut run);
    let mut n = PartnerNameplate::new(&mut s, &mut run).expect("the nameplate");
    n.set(&mut s, &mut run, Some(("TheDarkPhoenix19", "")));
    // CameraTarget_CharSelect (1550, 450, 250) looking -X, horizontal FOV 26.731283 at 16:9: the point
    // (290, 260, 216.5) at about (1569, 647) on 1920 x 1080
    let at = [1569.0, 647.0];
    n.place(&mut s.view, Some((at, 1260.0)));
    s.frame(&mut run, &[], 1.0, &mut ElementList::default());
    let w = s.view.world.last().expect("a world widget").w;
    let overlay = s.view.tree().find_in(w, "Overlay_166");
    let p = s.view.tree().painted(overlay).expect("the overlay painted");
    let (x, y) = (p.transform[4], p.transform[5]);
    let (sw, sh) = (p.size[0] * p.transform[0], p.size[1] * p.transform[3]);
    eprintln!("overlay at ({x:.1}, {y:.1}) size ({sw:.1}, {sh:.1}), centre ({:.1}, {:.1})", x + sw / 2.0, y + sh / 2.0);
    assert!((x + sw / 2.0 - at[0]).abs() <= 1.0, "the plate's centre is on the point across");
    assert!((y + sh / 2.0 - at[1]).abs() <= 1.0, "the plate's centre is on the point down");
}
