//! The perk page on the menu bundle: a character's lists, equipping a perk into the current slot, the slot limits,
//! clearing, the remembered loadout written on Ready, and the lock state. Needs the dev store; without one each test
//! says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::widgets::controls::grid_item::{GridItem, GridItemState};
use mvs_ui::menu::widgets::perks::{PerkItem, PerkLockIndicator, PerksEquipper, PerksSublist, open_perk_page};
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
        Ok(b) if b.widgets.contains_key("UI_Perks_Equipper") => b,
        _ => {
            eprintln!("no menu bundle with UI_Perks_Equipper: skipped");
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

    /// A character whose four lists all have perks.
    fn character(&self) -> Option<String> {
        let m = self.bundle.menu.as_ref()?;
        m.fighters
            .iter()
            .find(|(_, f)| {
                f.perks.as_ref().is_some_and(|p| p.lists.len() == 4 && p.lists.iter().all(|l| !l.is_empty()))
            })
            .map(|(k, _)| k.clone())
    }
}

/// A page on the viewport, opened for `c` with the remembered loadout, its delays run.
struct Page {
    s: MenuScreen,
    eq: WidgetId,
    t: f64,
}

impl Page {
    fn open(e: &mut Env, c: &str) -> Page {
        let mut s = e.screen();
        let c = c.to_string();
        let eq = s.with_cx(&mut e.run(), |cx| {
            let w = MenuView::create(cx, "UI_Perks_Equipper");
            MenuView::add_to_viewport(cx, w, 0);
            open_perk_page(cx, w, &c, true, None);
            w
        });
        let mut p = Page { s, eq, t: 1.0 };
        p.settle(e);
        p
    }

    /// One second of frames: the flights land, the delays run.
    fn settle(&mut self, e: &mut Env) {
        for _ in 0..60 {
            self.t += 1.0 / 60.0;
            self.s.frame(&mut e.run(), &[], self.t, &mut ElementList::default());
        }
    }

    fn equipper(&mut self) -> &mut PerksEquipper {
        self.s.view.controller::<PerksEquipper>(self.eq).expect("the equipper")
    }

    fn equipped(&mut self) -> Vec<Option<String>> {
        self.equipper().equipped()
    }

    /// The tiles of the list `name` and their perks.
    fn tiles(&mut self, name: &str) -> Vec<WidgetId> {
        let l = self.s.view.tree().find_in(self.eq, name);
        self.s.view.controller::<PerksSublist>(l).expect("the sublist").items()
    }

    fn perk_of(&mut self, tile: WidgetId) -> String {
        let it = self.s.view.controller::<PerkItem>(tile).expect("the tile");
        it.data.as_ref().expect("a perk").id.clone()
    }

    /// The tile clicked by player 0 (UI_GridItemBase Select), then the frames.
    fn click(&mut self, e: &mut Env, tile: WidgetId) {
        self.s.with_cx(&mut e.run(), |cx| {
            cx.visit::<PerkItem, _>(tile, |t, cx| GridItemState::select(t, cx, 0)).expect("the tile's controller");
        });
        self.settle(e);
    }

    fn with_equipper<R>(
        &mut self,
        e: &mut Env,
        f: impl FnOnce(&mut PerksEquipper, &mut mvs_ui::menu::Cx<'_>) -> R,
    ) -> R {
        let eq = self.eq;
        let r = self.s.with_cx(&mut e.run(), |cx| cx.visit::<PerksEquipper, _>(eq, f).expect("the equipper"));
        self.settle(e);
        r
    }
}

#[test]
fn the_page_fills_its_lists_from_the_character() {
    let Some(mut e) = env() else { return };
    let Some(c) = e.character() else {
        eprintln!("no character with four perk lists: skipped");
        return;
    };
    let lists = e.bundle.menu.as_ref().unwrap().fighters[&c].perks.clone().unwrap().lists;
    let mut p = Page::open(&mut e, &c);
    assert_eq!(p.s.view.tree().widget(p.eq).vis, Vis::SelfHitTestInvisible);
    // listSignature is EPerkCategory CharacterSpecific (3), listOffense 0
    let sig = p.tiles("listSignature");
    assert_eq!(sig.len(), lists[3].len());
    let off = p.tiles("listOffense");
    assert_eq!(off.len(), lists[0].len());
    assert_eq!(p.perk_of(off[0]), lists[0][0]);
    // nothing equipped: the first slot to fill is the signature slot
    assert_eq!(p.equipped(), vec![None; 4]);
    assert_eq!(p.equipper().current_slot, 0);
}

#[test]
fn a_perk_clicked_goes_to_the_current_slot_once() {
    let Some(mut e) = env() else { return };
    let Some(c) = e.character() else { return };
    let mut p = Page::open(&mut e, &c);
    // the signature slot first, then the next slot
    let sig = p.tiles("listSignature")[0];
    let sig_id = p.perk_of(sig);
    p.click(&mut e, sig);
    assert_eq!(p.equipped()[0].as_deref(), Some(sig_id.as_str()));
    assert_eq!(p.equipper().current_slot, 1);
    let off = p.tiles("listOffense");
    let a = p.perk_of(off[0]);
    p.click(&mut e, off[0]);
    assert_eq!(p.equipped()[1].as_deref(), Some(a.as_str()));
    assert_eq!(p.equipper().current_slot, 2);
    // the tile shows it equipped
    assert!(p.s.view.controller::<PerkItem>(off[0]).unwrap().selected());
    // the same perk again: not equipped twice, the slot stays
    p.click(&mut e, off[0]);
    let eq = p.equipped();
    assert_eq!(eq.iter().flatten().filter(|x| **x == a).count(), 1);
    assert_eq!(eq[2], None);
    assert_eq!(p.equipper().current_slot, 2);
}

#[test]
fn the_slots_wrap_and_never_hold_more_than_four() {
    let Some(mut e) = env() else { return };
    let Some(c) = e.character() else { return };
    let mut p = Page::open(&mut e, &c);
    // from slot 1, every general perk in turn
    p.with_equipper(&mut e, |q, cx| q.set_current_slot(cx, 1, false));
    let mut general = Vec::new();
    for l in ["listOffense", "listDefense", "listUtility"] {
        general.extend(p.tiles(l));
    }
    if general.len() < 4 {
        eprintln!("fewer than four general perks: skipped");
        return;
    }
    for &t in general.iter().take(3) {
        p.click(&mut e, t);
    }
    let eq = p.equipped();
    assert_eq!(eq.len(), 4);
    // slots 1, 2, 3 filled, then the current slot wraps to 0: only the signature list is seen there, so no
    // general perk can be picked into it
    assert_eq!(eq[0], None, "{eq:?}");
    let want: Vec<String> = general.iter().take(3).map(|&t| p.perk_of(t)).collect();
    assert_eq!(eq[1..].iter().flatten().cloned().collect::<Vec<_>>(), want);
    assert_eq!(p.equipper().current_slot, 0);
    let vis = |p: &mut Page, n: &str| {
        let w = p.s.view.tree().find_in(p.eq, n);
        p.s.view.tree().widget(w).vis
    };
    assert_eq!(vis(&mut p, "listSignature"), Vis::SelfHitTestInvisible);
    for l in ["listOffense", "listDefense", "listUtility"] {
        assert_eq!(vis(&mut p, l), Vis::Collapsed, "{l}");
    }
}

#[test]
fn clear_empties_the_loadout_and_ready_remembers_it() {
    let Some(mut e) = env() else { return };
    let Some(c) = e.character() else { return };
    let mut p = Page::open(&mut e, &c);
    let sig = p.tiles("listSignature")[0];
    let sig_id = p.perk_of(sig);
    p.click(&mut e, sig);
    let off = p.tiles("listOffense")[0];
    let off_id = p.perk_of(off);
    p.click(&mut e, off);
    p.with_equipper(&mut e, |q, cx| q.ready(cx, false));
    assert_eq!(p.s.view.tree().widget(p.eq).vis, Vis::Collapsed, "Ready exits");
    assert_eq!(e.prefs.perks.get(&c).cloned(), Some(vec![Some(sig_id.clone()), Some(off_id), None, None]));
    // a page opened again reads it back
    let mut p = Page::open(&mut e, &c);
    assert_eq!(p.equipped()[0].as_deref(), Some(sig_id.as_str()));
    p.with_equipper(&mut e, |q, cx| q.clear(cx));
    assert_eq!(p.equipped(), vec![None; 4]);
    assert!(!p.s.view.controller::<PerkItem>(sig).is_some_and(|i| i.selected()));
    // auto equip fills the recommended perks
    p.with_equipper(&mut e, |q, cx| q.auto_equip(cx));
    let rec = e.bundle.menu.as_ref().unwrap().fighters[&c].perks.clone().unwrap().recommended;
    let got: Vec<String> = p.equipped().into_iter().flatten().collect();
    for r in &rec {
        assert!(got.contains(r), "{r} of {rec:?} in {got:?}");
    }
}

#[test]
fn no_perk_or_slot_is_locked_offline() {
    let Some(mut e) = env() else { return };
    let Some(c) = e.character() else { return };
    let mut p = Page::open(&mut e, &c);
    for l in ["listSignature", "listOffense", "listDefense", "listUtility"] {
        for t in p.tiles(l) {
            let it = p.s.view.controller::<PerkItem>(t).unwrap();
            assert!(!it.locked());
            let lock = p.s.view.tree().find_in(t, "LockedIndicator");
            assert_eq!(p.s.view.tree().widget(lock).vis, Vis::Hidden);
        }
    }
    // the loadout's lock indicators: every instance is Disabled, so Hidden with a character set
    let lo = p.s.view.tree().find_in(p.eq, "UI_Perks_Loadout");
    for n in ["perkLockSignature", "perkLockCommon1", "perkLockCommon2", "perkLockCommon3"] {
        let w = p.s.view.tree().find_in(lo, n);
        assert!(p.s.view.controller::<PerkLockIndicator>(w).is_some());
        assert_eq!(p.s.view.tree().widget(w).vis, Vis::Hidden, "{n}");
    }
}

/// The page drawn to a PNG for a look (`PERK_SHOT=<file.png> cargo test --test menu_perks -- --ignored`): opened,
/// a signature and an offense perk equipped, one second on.
#[test]
#[ignore]
fn draw_the_perk_page() {
    use mvs_render::gpu::{Gpu, Subresource, TexDesc, TexKind, TexViews};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
    let Some(mut e) = env() else { return };
    let Some(c) = e.character() else { return };
    let out = std::env::var("PERK_SHOT").unwrap_or_else(|_| "perk_page.png".into());
    let mut p = Page::open(&mut e, &c);
    let sig = p.tiles("listSignature")[0];
    p.click(&mut e, sig);
    let off = p.tiles("listOffense")[0];
    p.click(&mut e, off);
    let mut list = ElementList::default();
    p.t += 1.0 / 60.0;
    p.s.frame(&mut e.run(), &[], p.t, &mut list);
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
    ui.time = Some(p.t);
    ui.draw(&gpu, &assets, &mut e.fonts, &list, &target).unwrap();
    let px = gpu.read_texture(&target).unwrap();
    mvs_render::image::write_png(std::path::Path::new(&out), w, h, &px).unwrap();
    eprintln!("{} elements, {out}", list.elements.len());
}
