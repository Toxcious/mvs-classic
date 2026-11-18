//! The Settings panel on the menu bundle: made, its Graphics page built, a pick written into the settings, a
//! volume row writing its volume, the Controls page it opens on and the Interface and Classic pages writing their
//! rows. Needs the dev store; without one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::{Controller, MenuRun};
use mvs_ui::menu::input::{MenuInput, Pointer};
use mvs_ui::menu::widgets::controls::buttons::ToggleSwitch;
use mvs_ui::menu::widgets::controls::input_binding::InputBindingSet;
use mvs_ui::menu::widgets::preferences_graphics::PreferencesGraphics;
use mvs_ui::menu::widgets::preferences_interface::PreferencesInterface;
use mvs_ui::menu::widgets::section_bar::SectionBar;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::Vis;
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
    let bundle = Bundle::load(&store, "menu").ok()?;
    if !bundle.widgets.contains_key("UI_PreferencesPanelRework") {
        eprintln!("no UI_PreferencesPanelRework in the bundle: skipped");
        return None;
    }
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

    fn screen(&mut self) -> MenuScreen {
        let view = MenuView::new(self.bundle.clone(), self.text.clone(), [1600, 900]);
        MenuScreen::new(view, &mut self.run())
    }
}

#[test]
fn the_panel_builds_its_graphics_page_and_writes_the_picks() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let g = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PreferencesGraphicsRework");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    assert!(s.view.is_constructed(g));
    // the rows the port cannot honour are collapsed, the window's rows too (no displays given)
    let vs = s.view.tree().find_in(g, "UI_ToggleVerticalSync");
    assert_eq!(s.view.tree().widget(vs).vis, Vis::Collapsed);
    let wm = s.view.tree().find_in(g, "DropdownWindowMode");
    assert_eq!(s.view.tree().widget(wm).vis, Vis::Collapsed);
    // the frame rate row is added under the display row
    assert!(
        s.view.tree().widget(s.view.tree().widget(wm).parent.unwrap()).children.iter().any(|&c| s
            .view
            .tree()
            .widget(c)
            .name
            == "DropdownFrameRateLimit")
    );
    // an anti-aliasing and a resolution scale pick
    s.with_cx(&mut e.run(), |cx| {
        cx.visit::<PreferencesGraphics, _>(g, |p, cx| {
            p.handle(cx, 5, &(1_i32, "1".to_string()));
            p.handle(cx, 4, &(0_i32, "50".to_string()));
            p.handle(cx, 6, &(1_i32, "1".to_string()));
        });
    });
    assert_eq!(e.prefs.graphics.anti_aliasing_quality, 1);
    assert_eq!(e.prefs.graphics.texture_quality, 1);
    assert!(e.prefs.graphics.resolution_quality < 100.0 && e.prefs.graphics.resolution_quality >= 10.0);
}

#[test]
fn a_volume_row_writes_its_volume() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let panel = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PreferencesPanelRework");
        MenuView::add_to_viewport(cx, w, 0);
        let bar = cx.view.tree().find_in(w, "mainMenuSectionBar");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.button_clicked(cx, "audio", true));
        w
    });
    let row = s.view.tree().find_in(panel, "UI_MusicVolume");
    s.with_cx(&mut e.run(), |cx| {
        cx.visit::<mvs_ui::menu::widgets::controls::buttons::PlusMinusHorizontal, _>(row, |r, cx| {
            r.set_percentage(cx, 0.5)
        });
    });
    assert!((e.prefs.audio.music - 0.5).abs() < 0.02, "{}", e.prefs.audio.music);
    assert_eq!(e.prefs.audio.master, 1.0);
}

/// ToggleSwitch's click (its interactor's OnClick).
fn click_toggle(cx: &mut mvs_ui::menu::controller::Cx<'_>, w: mvs_ui::slate::WidgetId) {
    cx.visit::<ToggleSwitch, _>(w, |t, cx| t.handle(cx, 3, &()));
}

#[test]
fn the_panel_opens_on_the_controls_page_and_its_rows_write_the_input_settings() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let panel = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PreferencesPanelRework");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    // CONTROLS > Controller: the binding set has its rows, in the controller's mode
    let page = s.view.tree().find_in(panel, "UI_PreferencesControlsRework");
    assert!(s.view.is_constructed(page));
    let set = s.view.tree().find_in(page, "Bindings");
    let (rows, keyboard) = s.with_cx(&mut e.run(), |cx| {
        cx.visit::<InputBindingSet, _>(set, |b, _| (b.rows.len(), b.keyboard_mode)).unwrap()
    });
    assert!(rows > 10, "{rows} binding rows");
    assert!(!keyboard);
    // a switch writes player 0's preference
    let toggles = s.view.tree().find_in(page, "Toggles");
    let row = s.view.tree().find_in(toggles, "UI_ToggleSwapSideNeutralGround");
    s.with_cx(&mut e.run(), |cx| click_toggle(cx, row));
    assert!(e.prefs.input.players[0].swapped_attack_style_ground);
    assert!(!e.prefs.input.players[1].swapped_attack_style_ground);
}

#[test]
fn the_interface_page_writes_its_rows() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let panel = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PreferencesPanelRework");
        MenuView::add_to_viewport(cx, w, 0);
        let bar = cx.view.tree().find_in(w, "mainMenuSectionBar");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.button_clicked(cx, "interface", true));
        w
    });
    let canvas = s.view.tree().find_in(panel, "InterfaceCanvasv2");
    let page = *s.view.tree().widget(canvas).children.last().expect("the Interface page");
    assert!(s.view.is_constructed(page));
    // the port's own rows are on the Classic page, not here
    for n in ["UI_ToggleChatInMatch", "UI_ToggleShowInOnlineList", "UI_ToggleInvites"] {
        assert!(s.view.tree().find_opt(page, n).is_none(), "{n} on the Interface page");
    }
    s.with_cx(&mut e.run(), |cx| {
        // the cooldowns on the scoreboard (the position row's pick), Knockback Influence on
        cx.visit::<PreferencesInterface, _>(page, |p, cx| {
            p.handle(cx, 2, &(0_i32, "scoreboard".to_string()));
            p.handle(cx, 26, &true);
        });
    });
    let u = &e.prefs.ui;
    assert!(!u.world_show_cooldowns && u.hud_show_cooldowns);
    assert!(u.world_show_di);
    assert_eq!(mvs_ui::menu::prefs::preset_id(u), "custom");
}

#[test]
fn the_classic_section_shows_the_ports_own_rows_and_they_write_their_settings() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let panel = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PreferencesPanelRework");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    // CLASSIC is the bar's last section
    let bar = s.view.tree().find_in(panel, "mainMenuSectionBar");
    let last = s.with_cx(&mut e.run(), |cx| {
        cx.visit::<SectionBar, _>(bar, |b, cx| {
            b.button_clicked(cx, "classic", true);
            (b.button_by_id("classic"), b.current_selected_id())
        })
    });
    let (button, current) = last.unwrap();
    assert!(button.is_some());
    assert_eq!(current, "classic");
    // its page is the switcher's active one
    let sw = s.view.tree().find_in(panel, "WidgetSwitcher");
    let page = s.view.tree().find_in(panel, "PanelClassic");
    let active = s.view.tree().widget(sw).active as usize;
    assert_eq!(s.view.tree().widget(sw).children.get(active), Some(&page));
    let rows: Vec<_> = ["UI_ToggleChatInMatch", "UI_ToggleShowInOnlineList", "UI_ToggleInvites"]
        .iter()
        .map(|n| s.view.tree().find_in(page, n))
        .collect();
    for &r in &rows {
        assert!(s.view.is_constructed(r));
    }
    let u = &e.prefs.ui;
    assert!(u.chat_in_match && u.show_in_online_list && u.invites);
    s.with_cx(&mut e.run(), |cx| {
        for &r in &rows {
            click_toggle(cx, r);
        }
    });
    let u = &e.prefs.ui;
    assert!(!u.chat_in_match && !u.show_in_online_list && !u.invites);
    s.with_cx(&mut e.run(), |cx| click_toggle(cx, rows[1]));
    let u = &e.prefs.ui;
    assert!(!u.chat_in_match && u.show_in_online_list && !u.invites);
}

#[test]
fn the_wheel_scrolls_the_interface_page_built_after_the_panel_was_shown() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    s.view.set_parts(
        Box::new(mvs_ui::menu::scroll::ScrollBars::new(e.bundle.clone())),
        Box::new(mvs_ui::menu::rich_text::RichTexts::new(e.bundle.clone())),
    );
    let panel = s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, "UI_PreferencesPanelRework");
        MenuView::add_to_viewport(cx, w, 0);
        w
    });
    let mut t = 0.0;
    let mut frames = |e: &mut Env, s: &mut MenuScreen, input: &[MenuInput], n: usize| {
        for k in 0..n {
            t += 1.0 / 60.0;
            s.frame(&mut e.run(), if k == 0 { input } else { &[] }, t, &mut ElementList::default());
        }
    };
    frames(&mut e, &mut s, &[], 10);
    // the Interface page is built now, into the layer already shown
    s.with_cx(&mut e.run(), |cx| {
        let bar = cx.view.tree().find_in(panel, "mainMenuSectionBar");
        cx.visit::<SectionBar, _>(bar, |b, cx| b.button_clicked(cx, "interface", true));
    });
    frames(&mut e, &mut s, &[], 10);
    let page = s.view.tree().find_in(panel, "UI_PreferencesInterfaceRework");
    let sb = s.view.tree().find_in(page, "interfaceScrollBox");
    assert!(s.view.tree().widget(sb).scroll_max > 0.0);
    // the pointer over a row of the page (1600 x 900), one notch down
    let input = [MenuInput::MouseMove { x: 833.0, y: 667.0 }, MenuInput::Wheel { notches: -1.0 }];
    frames(&mut e, &mut s, &input, 30);
    assert!(s.view.tree().widget(sb).scroll > 0.0, "the page scrolled");
}
