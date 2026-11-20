//! The Lab menu on the menu bundle with a fake Lab session: the pause menu made, its config applied to the four
//! panels, the pickers and the Move List opened. Needs the dev store; without one each test says so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::screens::lab::{LabPanel, LabPauseMenu, LabPlayerPanel};
use mvs_ui::menu::screens::lab_selectors::GridItemList;
use mvs_ui::menu::screens::move_list::toggle_movelist;
use mvs_ui::menu::training_config::{LabConfig, LabPlayerConfig, LabSession, LabToggles};
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::text::{Fonts, GameText};

struct Fake {
    config: LabConfig,
    toggles: LabToggles,
    sent: Vec<LabConfig>,
    broadcast: Option<LabConfig>,
    clock: f64,
}

impl LabSession for Fake {
    fn config(&self) -> &LabConfig {
        &self.config
    }
    fn me(&self) -> &str {
        "me"
    }
    fn toggles(&mut self) -> &mut LabToggles {
        &mut self.toggles
    }
    fn request_config_update(&mut self, config: LabConfig) {
        self.sent.push(config.clone());
        self.config = config.clone();
        self.broadcast = Some(config);
    }
    fn request_authority_config(&mut self) {
        self.broadcast = Some(self.config.clone());
    }
    fn request_kick_player(&mut self, _a: &str) {}
    fn pause(&mut self, _on: bool) {}
    fn take_config(&mut self) -> Option<LabConfig> {
        self.broadcast.take()
    }
    fn is_matchmaking(&self) -> bool {
        false
    }
    fn back_to_matchmaking(&mut self) {}
}

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
        Ok(b) => b,
        Err(e) => {
            eprintln!("no menu bundle ({e}): skipped");
            return None;
        }
    };
    if !bundle.widgets.contains_key("UI_LAB_PauseMenu") {
        eprintln!("no UI_LAB_PauseMenu in the bundle: skipped");
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
    fn run<'a>(&'a mut self, lab: &'a mut Fake) -> MenuRun<'a> {
        MenuRun {
            fonts: &mut self.fonts,
            prefs: &mut self.prefs,
            bindings: &mut self.bindings,
            sink: &mut self.sink,
            pointer: &mut self.pointer,
            lab: Some(lab),
            pause: None,
            tutorial: None,
        }
    }
}

// The frame without its paint: the retained outline quad draw is not ported (slate.rs Ty::Quad), and the menus' quads
// with outlines are in every picker.
fn tick(s: &mut MenuScreen, e: &mut Env, lab: &mut Fake, now: f64) {
    while lab.clock < now {
        lab.clock = (lab.clock + 0.03).min(now);
        s.view.set_now(lab.clock);
        s.with_cx(&mut e.run(lab), MenuView::tick);
    }
}

fn player(id: &str, name: &str, character: Option<&str>) -> LabPlayerConfig {
    LabPlayerConfig {
        account_id: id.into(),
        player_name: name.into(),
        character: character.map(str::to_string),
        skin: String::new(),
        starting_damage: 0.0,
        bot_difficulty: 0,
        team_index: 0,
        perks: vec![None; 4],
    }
}

fn fake() -> Fake {
    fake_as("shaggy")
}

fn fake_as(character: &str) -> Fake {
    let mut players = vec![player("me", "Me", Some(character))];
    players.extend((0..3).map(|_| LabPlayerConfig::default()));
    Fake {
        config: LabConfig { players, leader_account_id: "me".into(), ..LabConfig::default() },
        toggles: LabToggles::default(),
        sent: Vec::new(),
        broadcast: None,
        clock: 0.0,
    }
}

/// The character selector opened from the card focuses the card's fighter's tile first (Wonder Woman's, not the
/// grid's first).
#[test]
fn the_character_selector_focuses_the_current_fighter() {
    let Some(mut e) = env() else { return };
    let mut lab = fake_as("wonderwoman");
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    tick(&mut s, &mut e, &mut lab, 3.0);
    s.with_cx(&mut e.run(&mut lab), |cx| {
        let panel = {
            let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
            cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0")
        };
        let card = cx.view.tree().find_in(panel, "UI_LAB_PlayerPanel_Character");
        let b = cx.view.tree().find_in(card, "buttonChangeCharacter");
        // (on a pad: with the cursor, SetFocused moves the context only)
        cx.view.manager.cursor_mode[0] = false;
        mvs_ui::menu::widgets::controls::interactor::set_focused(cx, b, false);
        cx.view.manager.interactor_mut(b).unwrap().is_focused = true;
        mvs_ui::menu::widgets::controls::interactor::submit(cx, b, 0);
    });
    tick(&mut s, &mut e, &mut lab, 4.0);
    let m = s.view.manager.get_modal("labCharacterSelector").expect("the selector");
    let focused: Vec<String> = s.with_cx(&mut e.run(&mut lab), |cx| {
        let grid = cx.view.tree().find_in(m, "CharacterGrid");
        cx.visit::<GridItemList, _>(grid, |g, cx| {
            g.items()
                .into_iter()
                .filter(|&t| {
                    let i = cx.view.tree().find_in(t, "UI_InteractorP0");
                    cx.view.manager.interactor(i).is_some_and(|st| st.is_focused)
                })
                .filter_map(|t| g.record(t)?.0?.roster()?.fighter.clone())
                .collect()
        })
        .unwrap()
    });
    assert_eq!(focused, ["wonderwoman"]);
    s.with_cx(&mut e.run(&mut lab), |cx| cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx)));
    s.remove(&mut e.run(&mut lab));
}

/// The skin page opened for a Lab slot ticks the slot's skin, not the profile's saved one; with no slot skin it ticks
/// the profile's (the default here).
#[test]
fn the_skin_page_ticks_the_lab_slots_skin() {
    use mvs_ui::menu::manager::Manager;
    use mvs_ui::menu::screens::lab_selectors::skin_selector_data;
    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let Some(f) = s.view.fighters().get("shaggy").cloned() else { return };
    let Some(other) = f.skins.iter().find(|k| !k.is_default) else { return };
    let ticked = |s: &mut MenuScreen, e: &mut Env, lab: &mut Fake, extra: &str, t: f64| -> Vec<String> {
        let m = s.with_cx(&mut e.run(lab), |cx| {
            Manager::show_modal(cx, "UI_LAB_SkinSelector", "labSkinSelector", "lab", extra).expect("the page")
        });
        tick(s, e, lab, t);
        tick(s, e, lab, t + 0.1);
        let names = s.with_cx(&mut e.run(lab), |cx| {
            let grid = cx.view.tree().find_in(m, "SkinGrid");
            cx.visit::<GridItemList, _>(grid, |g, _| {
                g.items()
                    .into_iter()
                    .filter_map(|w| g.record(w)?.0)
                    .filter_map(|d| d.skin().filter(|k| k.selected).map(|k| k.skin.slug.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap()
        });
        s.with_cx(&mut e.run(lab), |cx| Manager::dismiss_modal(cx, "labSkinSelector"));
        tick(s, e, lab, t + 1.0);
        names
    };
    let slot = ticked(&mut s, &mut e, &mut lab, &skin_selector_data("shaggy", Some(&other.name)), 1.0);
    assert_eq!(slot, std::slice::from_ref(&other.slug));
    let saved = ticked(&mut s, &mut e, &mut lab, "shaggy", 3.0);
    let default = f.skins.iter().find(|k| k.is_default).map(|k| k.slug.clone());
    assert_eq!(saved.first(), default.as_ref());
    s.remove(&mut e.run(&mut lab));
}

#[test]
fn the_pause_menu_takes_the_labs_config_and_opens_its_pickers() {
    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.0 + i as f64 * 0.02);
    }
    // the config came back through take_config: panel 0 holds the local player
    let (occupied, leader) = s.with_cx(&mut e.run(&mut lab), |cx| {
        let panel = {
            let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
            cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0")
        };
        cx.visit::<LabPlayerPanel, _>(panel, |p, _| (p.is_occupied, p.is_leader)).unwrap()
    });
    assert!(occupied && leader);

    tick(&mut s, &mut e, &mut lab, 3.0);
    // an empty slot's bot button makes a bot and sends the config
    s.with_cx(&mut e.run(&mut lab), |cx| {
        let panel = {
            let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
            cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel1")
        };
        let btn = cx.view.tree().find_in(panel, "btn_InviteBot");
        let i = cx.view.tree().find_in(btn, "UI_Interactor");
        mvs_ui::menu::widgets::controls::interactor::set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        mvs_ui::menu::widgets::controls::interactor::submit(cx, i, 0);
    });
    tick(&mut s, &mut e, &mut lab, 1.3);

    assert!(lab.sent.last().is_some_and(|c| c.players[1].account_id == "Bot1"), "the bot slot was sent");

    tick(&mut s, &mut e, &mut lab, 4.0);
    // the Move List opens for the local player's fighter and closes again
    s.with_cx(&mut e.run(&mut lab), toggle_movelist);
    tick(&mut s, &mut e, &mut lab, 1.4);
    assert!(s.view.manager.get_modal("training_movelist").is_some());
    s.with_cx(&mut e.run(&mut lab), toggle_movelist);
    assert!(s.view.manager.get_modal("training_movelist").is_none());

    tick(&mut s, &mut e, &mut lab, 5.0);
    // the character selector opens from the card and fills its grid
    s.with_cx(&mut e.run(&mut lab), |cx| {
        let panel = {
            let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
            cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0")
        };
        let card = cx.view.tree().find_in(panel, "UI_LAB_PlayerPanel_Character");
        let b = cx.view.tree().find_in(card, "buttonChangeCharacter");
        mvs_ui::menu::widgets::controls::interactor::set_focused(cx, b, false);
        cx.view.manager.interactor_mut(b).unwrap().is_focused = true;
        mvs_ui::menu::widgets::controls::interactor::submit(cx, b, 0);
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.5 + i as f64 * 0.02);
    }
    assert!(s.view.manager.get_modal("labCharacterSelector").is_some());

    s.with_cx(&mut e.run(&mut lab), |cx| {
        cx.visit::<LabPanel, _>(cx.view.tree().find_in(menu, "UI_LAB_Panel"), |_, _| ());
        cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx));
    });
    s.remove(&mut e.run(&mut lab));
}

#[test]
fn the_edit_button_opens_the_perk_page_and_its_save_goes_to_the_config() {
    use mvs_ui::menu::widgets::controls::buttons::ButtonSecondary;
    use mvs_ui::menu::widgets::controls::interactor::{set_focused, submit};
    use mvs_ui::menu::widgets::perks::PerksEquipper;
    use mvs_ui::slate::Vis;

    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.0 + i as f64 * 0.02);
    }
    let vis = |s: &MenuScreen| {
        let t = s.view.tree();
        let lp = t.find_in(menu, "UI_LAB_Panel");
        t.widget(t.find_in(lp, "PerksContainer")).vis
    };
    // UI_LAB_PlayerPanel_Perks.buttonEdit of panel 0
    s.with_cx(&mut e.run(&mut lab), |cx| {
        let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
        let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0");
        let row = cx.view.tree().find_in(panel, "UI_LAB_PlayerPanel_Perks");
        let b = cx.view.tree().find_in(row, "buttonEdit");
        let i = ButtonSecondary::interactor(cx, b);
        set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        submit(cx, i, 0);
    });
    tick(&mut s, &mut e, &mut lab, 2.0);
    assert_eq!(vis(&s), Vis::SelfHitTestInvisible, "the page's container is shown");
    let eq = {
        let t = s.view.tree();
        let lp = t.find_in(menu, "UI_LAB_Panel");
        t.find_in(lp, "UI_Perks_Equipper")
    };
    assert_eq!(s.view.controller::<PerksEquipper>(eq).unwrap().character.as_deref(), Some("shaggy"));
    // Ready: the page's save reaches the config, the container hides after 0.1 s
    let sent = lab.sent.len();
    s.with_cx(&mut e.run(&mut lab), |cx| {
        cx.visit::<PerksEquipper, _>(eq, |p, cx| p.ready(cx, true));
    });
    assert!(lab.sent.len() > sent, "the config was sent");
    tick(&mut s, &mut e, &mut lab, 2.5);
    assert_eq!(vis(&s), Vis::Hidden);
    s.with_cx(&mut e.run(&mut lab), |cx| cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx)));
    s.remove(&mut e.run(&mut lab));
}

/// The starting damage's plus and minus change the local player's damage in the config, and a second press
/// moves it again.
#[test]
fn the_starting_damage_buttons_change_the_config() {
    use mvs_ui::menu::screens::lab::PlusMinusVertical;
    use mvs_ui::menu::widgets::controls::buttons::ButtonSecondary;
    use mvs_ui::menu::widgets::controls::interactor::{set_focused, submit};

    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.0 + i as f64 * 0.02);
    }
    let press = |s: &mut MenuScreen, e: &mut Env, lab: &mut Fake, name: &str| {
        s.with_cx(&mut e.run(lab), |cx| {
            let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
            let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0");
            let dmg = cx.view.tree().find_in(panel, "settingStartingDamage");
            let b = cx.view.tree().find_in(dmg, name);
            let i = ButtonSecondary::interactor(cx, b);
            set_focused(cx, i, false);
            cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
            submit(cx, i, 0);
        });
    };
    let shown = |s: &mut MenuScreen| {
        let t = s.view.tree();
        let lp = t.find_in(menu, "UI_LAB_Panel");
        let dmg = t.find_in(t.find_in(lp, "UI_LAB_PlayerPanel0"), "settingStartingDamage");
        s.view.controller::<PlusMinusVertical>(dmg).unwrap().value
    };
    tick(&mut s, &mut e, &mut lab, 3.0);
    press(&mut s, &mut e, &mut lab, "ButtonPlus");
    tick(&mut s, &mut e, &mut lab, 3.5);
    let first = lab.config.players[0].starting_damage;
    assert!(first > 0.0, "the first plus moved the damage (shown {})", shown(&mut s));
    press(&mut s, &mut e, &mut lab, "ButtonPlus");
    tick(&mut s, &mut e, &mut lab, 4.0);
    assert!(lab.config.players[0].starting_damage > first, "the second plus moved it again");
    press(&mut s, &mut e, &mut lab, "ButtonMinus");
    tick(&mut s, &mut e, &mut lab, 4.5);
    assert_eq!(lab.config.players[0].starting_damage, first, "minus took one step back");
    assert_eq!(shown(&mut s), first);
    s.with_cx(&mut e.run(&mut lab), |cx| cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx)));
    s.remove(&mut e.run(&mut lab));
}

/// A pick in one of a bot panel's behaviour menus as a click takes it: the row's main interactor opens its context
/// menu, the menu's button with the id is submitted.
fn pick_behavior(s: &mut MenuScreen, e: &mut Env, lab: &mut Fake, menu: mvs_ui::slate::WidgetId, row: &str, id: &str) {
    use mvs_ui::menu::widgets::controls::interactor::{set_focused, submit};
    s.with_cx(&mut e.run(lab), |cx| {
        let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
        let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel1");
        let m = cx.view.tree().find_in(panel, row);
        let i = cx.view.tree().find_in(m, "mainInteractor");
        set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        submit(cx, i, 0);
    });
    let now = lab.clock + 0.3;
    tick(s, e, lab, now);
    s.with_cx(&mut e.run(lab), |cx| {
        // the context menu's button: the newest interactor with the id
        let t = cx.view.tree();
        let b = t
            .ids()
            .filter(|&w| cx.view.manager.interactor(w).is_some_and(|s| s.id == id))
            .max_by_key(|&w| t.born(w))
            .unwrap_or_else(|| panic!("no button {id} in {row}'s context menu"));
        set_focused(cx, b, false);
        cx.view.manager.interactor_mut(b).unwrap().is_focused = true;
        submit(cx, b, 0);
    });
    let now = lab.clock + 0.6;
    tick(s, e, lab, now);
}

#[test]
fn a_bots_behaviour_pick_sends_its_ebotbehavior_and_keeps_it_a_bot() {
    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.0 + i as f64 * 0.02);
    }
    s.with_cx(&mut e.run(&mut lab), |cx| {
        let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
        let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel1");
        let btn = cx.view.tree().find_in(panel, "btn_InviteBot");
        let i = cx.view.tree().find_in(btn, "UI_Interactor");
        mvs_ui::menu::widgets::controls::interactor::set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        mvs_ui::menu::widgets::controls::interactor::submit(cx, i, 0);
    });
    tick(&mut s, &mut e, &mut lab, 2.0);
    assert_eq!(lab.config.players[1].bot_difficulty, 1, "a new bot is recovery");

    // (menu, id, the EBotBehavior sent): UI_LAB_PlayerPanel GetBotDifficulty
    let picks = [
        ("menuBehavior", "fight", 2),
        ("menuBehaviorFight", "hard", 5),
        ("menuBehaviorFight", "medium", 4),
        ("menuBehaviorFight", "veryeasy", 2),
        ("menuBehaviorFight", "easy", 3),
        // (the port's own Expert entry, after Hard)
        ("menuBehaviorFight", "expert", 29),
        ("menuBehavior", "evade", 11),
        ("menuBehaviorEvade", "evadeBack", 9),
        ("menuBehaviorEvade", "evadeNeutral", 8),
        ("menuBehaviorEvade", "evadeForward", 10),
        ("menuBehavior", "normalAttack", 12),
        ("menuBehaviorDirection", "up", 14),
        ("menuBehaviorDirection", "forward", 15),
        ("menuBehavior", "specialAttack", 16),
        ("menuBehaviorDirection", "down", 17),
        ("menuBehavior", "evade_di", 24),
        ("menuBehaviorDIDirection", "back", 25),
        ("menuBehaviorDIDirection", "down", 27),
        ("menuBehaviorDIDirection", "up", 26),
        // (the menu has no stun_di entry)
        ("menuBehavior", "idle", 6),
        ("menuBehavior", "jumping", 7),
        ("menuBehavior", "patrolling", 28),
        ("menuBehavior", "recovery", 1),
    ];
    for (row, id, want) in picks {
        let sent = lab.sent.len();
        pick_behavior(&mut s, &mut e, &mut lab, menu, row, id);
        assert!(lab.sent.len() > sent, "{row} {id}: no config sent");
        assert_eq!(lab.config.players[1].bot_difficulty, want, "{row} {id}");
        // the broadcast came back through take_config: the panel is still a bot
        let is_bot = s.with_cx(&mut e.run(&mut lab), |cx| {
            let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
            let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel1");
            cx.visit::<LabPlayerPanel, _>(panel, |p, _| p.is_bot).unwrap()
        });
        assert!(is_bot, "{row} {id}: the panel turned human");
    }
    s.with_cx(&mut e.run(&mut lab), |cx| cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx)));
    s.remove(&mut e.run(&mut lab));
}

#[test]
fn the_random_fighter_portrait_material_is_in_the_bundle() {
    let Some(e) = env() else { return };
    let key = "MI_Quad_RandomChar_Lab";
    let found = e.bundle.materials.contains_key(key) || e.bundle.materials.values().any(|p| p.ends_with(key));
    assert!(found, "{key} is not among the bundle's materials (a short name that slate's resolve leaves as it is)");
}

#[test]
fn the_row_shows_its_perks_and_a_tile_opens_the_perk_page() {
    use mvs_ui::menu::widgets::controls::interactor::{set_focused, submit};
    use mvs_ui::menu::widgets::perks::{PerkItem, PerksLoadout};
    use mvs_ui::slate::Vis;

    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let id = view
        .fighters()
        .get("shaggy")
        .and_then(|f| f.perks.as_ref())
        .and_then(|p| p.lists.iter().flatten().next().cloned());
    let Some(id) = id else { return };
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.0 + i as f64 * 0.02);
    }
    // the panel's SetPerks (the page's save, a config's perks) reaches the row's tiles
    let saved = id.clone();
    let shown = s.with_cx(&mut e.run(&mut lab), |cx| {
        let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
        let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0");
        cx.visit::<LabPlayerPanel, _>(panel, |p, cx| p.set_perks(cx, &[None, Some(saved), None, None]));
        let row = cx.view.tree().find_in(panel, "UI_LAB_PlayerPanel_Perks");
        let lo = cx.view.tree().find_in(row, "UI_Perks_Loadout");
        cx.view.controller::<PerksLoadout>(lo).unwrap().equipped_perks(false)
    });
    assert!(shown.iter().flatten().any(|p| p.id == id), "the row's loadout holds the saved perk");
    // a tile pressed opens the page
    s.with_cx(&mut e.run(&mut lab), |cx| {
        let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
        let panel = cx.view.tree().find_in(lp, "UI_LAB_PlayerPanel0");
        let row = cx.view.tree().find_in(panel, "UI_LAB_PlayerPanel_Perks");
        let lo = cx.view.tree().find_in(row, "UI_Perks_Loadout");
        let item = PerksLoadout::items_of(cx, lo)[1];
        let i = PerkItem::interactor(cx, item);
        set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        submit(cx, i, 0);
    });
    tick(&mut s, &mut e, &mut lab, 2.0);
    let t = s.view.tree();
    let lp = t.find_in(menu, "UI_LAB_Panel");
    assert_eq!(t.widget(t.find_in(lp, "PerksContainer")).vis, Vis::SelfHitTestInvisible, "the page opened");
    s.with_cx(&mut e.run(&mut lab), |cx| cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx)));
    s.remove(&mut e.run(&mut lab));
}

/// The context menu rows up now: (row widget, its label text).
fn context_rows(s: &MenuScreen) -> Vec<(mvs_ui::slate::WidgetId, String)> {
    let t = s.view.tree();
    t.ids()
        .filter(|&w| t.user_class(w) == Some("UI_ContextButton") && t.widget(w).parent.is_some())
        .map(|w| (w, t.widget(t.find_in(w, "label")).text.clone()))
        .collect()
}

/// Experimental Settings under Move List opens the mutators' page; its list is a checklist: each click ticks one
/// more into the Lab's config, the list stays up, the ticked rows have their mark and the row its count.
#[test]
fn the_labs_experimental_settings_tick_mutators_into_the_config() {
    use mvs_protocol::lobby_rules::MutatorSet;
    use mvs_ui::menu::custom_text;
    use mvs_ui::menu::screens::lab::LAB_EXPERIMENTAL_MODAL;
    use mvs_ui::menu::widgets::controls::buttons::ButtonSecondary;
    use mvs_ui::menu::widgets::controls::interactor::{set_focused, submit};

    let Some(mut e) = env() else { return };
    let mut lab = fake();
    let view = MenuView::new(e.bundle.clone(), e.text.clone(), [1600, 900]);
    let mut s = MenuScreen::new(view, &mut e.run(&mut lab));
    let menu = s.with_cx(&mut e.run(&mut lab), |cx| {
        let w = MenuView::create(cx, "UI_LAB_PauseMenu");
        MenuView::add_to_viewport(cx, w, 0);
        cx.visit::<LabPauseMenu, _>(w, |m, cx| m.shown(cx));
        w
    });
    for i in 0..4 {
        tick(&mut s, &mut e, &mut lab, 1.0 + i as f64 * 0.02);
    }
    let press = |s: &mut MenuScreen, e: &mut Env, lab: &mut Fake, i: mvs_ui::slate::WidgetId| {
        s.with_cx(&mut e.run(lab), |cx| {
            set_focused(cx, i, false);
            cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
            submit(cx, i, 0);
        });
    };
    let b = s.with_cx(&mut e.run(&mut lab), |cx| {
        let lp = cx.view.tree().find_in(menu, "UI_LAB_Panel");
        let b = cx.view.tree().find_in(lp, "Btn_Experimental");
        let label = cx.view.tree().find_in(b, "label");
        assert_eq!(cx.view.tree().widget(label).text, custom_text::EXPERIMENTAL_BUTTON);
        ButtonSecondary::interactor(cx, b)
    });
    tick(&mut s, &mut e, &mut lab, 3.0);
    press(&mut s, &mut e, &mut lab, b);
    tick(&mut s, &mut e, &mut lab, 4.0);
    let m = s.view.manager.get_modal(LAB_EXPERIMENTAL_MODAL).expect("the page is up");
    let row = s.view.tree().find_in(m, "Option_Mutators");
    let open = s.view.tree().find_in(row, "mainInteractor");
    press(&mut s, &mut e, &mut lab, open);
    tick(&mut s, &mut e, &mut lab, 5.0);
    let rows = context_rows(&s);
    assert_eq!(rows.len(), custom_text::MUTATORS.len(), "the list has Off and every mutator");
    // the list fits on the screen, and still shows its first 8 rows with a scroll (painted frames: the cap needs
    // the rows laid out)
    {
        let mut out = mvs_ui::element::ElementList::default();
        for k in 0..30 {
            s.frame(&mut e.run(&mut lab), &[], 5.0 + f64::from(k) / 60.0, &mut out);
        }
        let t = s.view.tree();
        let list = t
            .ids()
            .find(|&w| t.user_class(w) == Some("UI_ContextMenu") && t.widget(w).parent.is_some())
            .expect("the list is up");
        let first = t.widget(t.find_in(list, "Container")).children[0];
        let row_h = t.arranged_size(first)[1];
        let h = t.widget(t.find_in(list, "SizeBox")).h_override;
        assert!(row_h > 0.0 && (h - row_h * 8.0).abs() < 0.01, "the list is 8 rows of {row_h} high, not {h}");
    }
    for (k, n) in [(2, 1u32), (6, 2)] {
        let (w, _) = rows.iter().find(|(_, l)| l.ends_with(custom_text::MUTATORS[k])).cloned().expect("the row");
        let i = s.view.tree().find_in(w, "UI_Interactor");
        press(&mut s, &mut e, &mut lab, i);
        tick(&mut s, &mut e, &mut lab, 5.0 + f64::from(n));
        assert_eq!(lab.config.mutators.len(), n as usize, "click {n} ticks one more");
    }
    assert_eq!(lab.config.mutators, MutatorSet::default().with(2, true).with(6, true));
    let rows = context_rows(&s);
    assert_eq!(rows.len(), custom_text::MUTATORS.len(), "the list stays up");
    let ticked: Vec<&str> = rows.iter().filter(|(_, l)| l.starts_with('\u{2022}')).map(|(_, l)| l.as_str()).collect();
    assert_eq!(ticked.len(), 2, "the two ticked rows have their mark: {ticked:?}");
    let label = s.view.tree().widget(s.view.tree().find_in(row, "WrapSelectionLabel")).text.clone();
    assert_eq!(label, custom_text::mutator_count(2));
    // Off clears the list
    let (w, _) = rows.iter().find(|(_, l)| l.ends_with(custom_text::MUTATORS[0])).cloned().expect("Off");
    let i = s.view.tree().find_in(w, "UI_Interactor");
    press(&mut s, &mut e, &mut lab, i);
    tick(&mut s, &mut e, &mut lab, 8.0);
    assert!(lab.config.mutators.is_empty());
    s.with_cx(&mut e.run(&mut lab), |cx| cx.visit::<LabPauseMenu, _>(menu, |m, cx| m.hidden(cx)));
    s.remove(&mut e.run(&mut lab));
}
