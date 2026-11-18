//! The Battle Pass modal on the menu bundle: opened by id, its track of tier cards, the arrows, the focus. Needs the dev
//! store; without one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::manager::Manager;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
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
        Ok(b) if b.widgets.contains_key("UI_BattlePass_Rework") => b,
        _ => {
            eprintln!("no menu bundle with UI_BattlePass_Rework: skipped");
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

    fn screen(&mut self) -> MenuScreen {
        let view = MenuView::new(self.bundle.clone(), self.text.clone(), [1920, 1080]);
        MenuScreen::new(view, &mut self.run())
    }
}

fn open(e: &mut Env) -> (MenuScreen, f64) {
    let mut s = e.screen();
    let w = s.with_cx(&mut e.run(), |cx| Manager::show_modal(cx, "UI_BattlePass_Rework", "battlepass", "", ""));
    assert!(w.is_some(), "the modal opens");
    let mut t = 1.0;
    for _ in 0..180 {
        t += 1.0 / 60.0;
        s.frame(&mut e.run(), &[], t, &mut ElementList::default());
    }
    (s, t)
}

#[test]
fn the_modal_opens_and_draws_its_track() {
    let Some(mut e) = env() else { return };
    let (mut s, t) = open(&mut e);
    let mut list = ElementList::default();
    s.frame(&mut e.run(), &[], t + 1.0 / 60.0, &mut list);
    // the header, the missions text, ten tier cards of two rewards each and the buttons
    assert!(list.elements.len() > 200, "{} elements", list.elements.len());
}

/// The modal drawn to a PNG for a look (`BP_SHOT=<file.png> cargo test --test menu_battle_pass -- --ignored`).
#[test]
#[ignore]
fn draw_the_battle_pass() {
    use mvs_render::gpu::{Gpu, Subresource, TexDesc, TexKind, TexViews};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
    let Some(mut e) = env() else { return };
    let out = std::env::var("BP_SHOT").unwrap_or_else(|_| "battle_pass.png".into());
    let (mut s, mut t) = open(&mut e);
    let mut list = ElementList::default();
    t += 1.0 / 60.0;
    s.frame(&mut e.run(), &[], t, &mut list);
    let gpu = Gpu::new(None, false).unwrap();
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
    let assets = mvs_render::Assets::open(store, &gpu).unwrap();
    let [w, h] = [1920u32, 1080];
    let black = [0u8, 0, 0, 0xff].repeat((w * h) as usize);
    let target = gpu
        .texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [w, h, 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            },
            &[Subresource { data: &black, row_pitch: w * 4, slice_pitch: 0 }],
        )
        .unwrap();
    let mut ui = mvs_ui::element::Ui::new(&gpu, &assets).unwrap();
    ui.time = Some(t);
    ui.draw(&gpu, &assets, &mut e.fonts, &list, &target).unwrap();
    let px = gpu.read_texture(&target).unwrap();
    mvs_render::image::write_png(std::path::Path::new(&out), w, h, &px).unwrap();
    eprintln!("{} elements, {out}", list.elements.len());
}
