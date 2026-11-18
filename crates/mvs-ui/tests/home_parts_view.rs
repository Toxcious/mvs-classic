//! The main menu's side parts and its match options in a menu view: the glossary, quests, promo ticker, social panel
//! and match options made, shown and painted, frames run over them. Needs the dev store; without one each test says
//! so and passes.

use std::sync::Arc;

use mvs_settings::Settings;
use mvs_ui::Bundle;
use mvs_ui::bindings::Bindings;
use mvs_ui::element::ElementList;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::widgets::controls::pips::PaginationPips;
use mvs_ui::menu::widgets::modals::show_modal_generic;
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
        Ok(b) => b,
        Err(e) => {
            eprintln!("no menu bundle ({e}): skipped");
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
        let view = MenuView::new(self.bundle.clone(), self.text.clone(), [1600, 900]);
        MenuScreen::new(view, &mut self.run())
    }
}

/// Frames from `t0` for `secs` at 60 Hz; the element count of the last.
fn frames(e: &mut Env, s: &mut MenuScreen, t0: f64, secs: f64) -> usize {
    let n = (secs * 60.0) as usize;
    let mut last = 0;
    for f in 0..=n {
        let mut out = ElementList::default();
        s.frame(&mut e.run(), &[], t0 + f as f64 / 60.0, &mut out);
        last = out.elements.len();
    }
    last
}

/// A class made and added to the viewport.
fn shown(e: &mut Env, s: &mut MenuScreen, cls: &str) -> WidgetId {
    s.with_cx(&mut e.run(), |cx| {
        let w = MenuView::create(cx, cls);
        MenuView::add_to_viewport(cx, w, 0);
        w
    })
}

#[test]
fn the_glossary_fills_its_lists_and_shows_the_modifiers() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "UI_GlossaryTooltipContainer", "glossary", ""));
    let m = m.expect("the glossary is shown");
    assert!(frames(&mut e, &mut s, 1.0, 0.5) > 0);
    let t = s.view.tree();
    for (table, list) in [
        ("DT_GameplayGlossary", "GameplayVertBox"),
        ("DT_ModifierGlossary", "ModifiersVertBox"),
        ("DT_EconomyGlossary", "EconomyVertBox"),
    ] {
        let rows = s.view.glossary(table).len();
        assert!(rows > 0, "{table} has rows");
        let bx = t.find_in(m, list);
        assert_eq!(t.widget(bx).children.len(), rows, "{list} holds a row per entry");
        let second = t.widget(bx).children[1];
        let backing = t.find_in(second, "Backing");
        assert_eq!(t.widget(backing).vis, Vis::SelfHitTestInvisible, "row 1's backing shows");
    }
    let sw = t.find_in(m, "WidgetSwitcher_0");
    let active = t.widget(sw).children[t.widget(sw).active as usize];
    assert_eq!(t.widget(active).name, "ModifiersVertBox");
}

#[test]
fn the_quest_tabs_collapse_their_tabs_and_drop_the_designer_rows() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let w = shown(&mut e, &mut s, "UI_QuestTabs");
    assert!(frames(&mut e, &mut s, 1.0, 0.2) > 0);
    let t = s.view.tree();
    let daily = t.find_in(w, "UI_QuestsContainer_Daily");
    assert_eq!(t.widget(t.find_in(daily, "MissionsTitle")).vis, Vis::Collapsed);
    assert!(t.find_opt(daily, "Container").is_none_or(|c| t.widget(c).parent.is_none()), "the designer rows went");
}

#[test]
fn the_promo_ticker_shows_the_local_promotions() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let w = shown(&mut e, &mut s, "UI_PromoTickerSmall");
    assert!(frames(&mut e, &mut s, 1.0, 0.2) > 0);
    let n = mvs_ui::menu::widgets::promo::local_promotions(&e.bundle).len();
    assert!(n > 0, "the bundle has promotions");
    let t = s.view.tree();
    let sc = t.find_in(w, "scrollContainer");
    assert_eq!(t.widget(sc).children.len(), n);
    let first = t.widget(sc).children[0];
    let label = t.find_in(first, "titleLabel");
    assert!(!t.widget(label).text.is_empty(), "the class defaults' title");
    assert_eq!(t.widget(t.find_in(first, "thumbnail")).vis, Vis::SelfHitTestInvisible);
}

#[test]
fn the_social_panel_hides_itself_offline() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let w = shown(&mut e, &mut s, "UI_SocialPanel");
    frames(&mut e, &mut s, 1.0, 0.2);
    assert_eq!(s.view.tree().widget(w).vis, Vis::Hidden);
}

#[test]
fn a_social_player_row_is_made_and_painted() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    shown(&mut e, &mut s, "UI_SocialPlayer");
    assert!(frames(&mut e, &mut s, 1.0, 0.1) > 0);
}

#[test]
fn the_invite_modal_says_nobody_is_online() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "UI_SocialInviteModal", "socialInvite", ""));
    let m = m.expect("the invite modal is shown");
    assert!(frames(&mut e, &mut s, 1.0, 0.3) > 0);
    let t = s.view.tree();
    assert_eq!(t.widget(t.find_in(m, "NoFriendsOnline")).vis, Vis::SelfHitTestInvisible);
    assert_eq!(t.widget(t.find_in(m, "scrollContainer")).vis, Vis::Hidden);
}

#[test]
fn match_options_open_on_the_normal_row() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "WBP_MatchOptions", "matchoptions", ""));
    let m = m.expect("match options are shown");
    assert!(frames(&mut e, &mut s, 1.0, 1.0) > 0);
    let t = s.view.tree();
    for (n, v) in
        [("2v2", true), ("FFA", true), ("1v1", true), ("Casual", true), ("CoopVsAI", false), ("TheLab", false)]
    {
        let w = t.find_in(m, &format!("WBP_GameModeCard_{n}"));
        assert_eq!(t.widget(w).shown(), v, "{n}");
    }
    let row = t.find_in(m, "modeCardsContainer");
    assert_eq!(t.widget(row).transform.translation[0], 160.0);
    let c = t.find_in(m, "Button_Confirm");
    assert_eq!(t.widget(t.find_in(c, "label")).text, "Choose One or More Modes");
    assert_eq!(t.widget(t.find_in(m, "SelectedLabel")).vis, Vis::Collapsed);
    // Normal's XSmall cards at the port's width (the 2v2 card's ManualFocus came before the style's size)
    let card = t.find_in(m, "WBP_GameModeCard_2v2");
    assert_eq!(t.widget(t.find_in(card, "SizeBox_1")).h_override, 800.0);
    assert_eq!(t.widget(t.find_in(card, "SizeBox_1")).w_override, 550.0);
}

#[test]
fn match_options_from_arcade_hide_the_type_bar() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "WBP_MatchOptions", "matchoptions", "Arcade"));
    let m = m.expect("match options are shown");
    assert!(frames(&mut e, &mut s, 1.0, 1.0) > 0);
    let t = s.view.tree();
    assert_eq!(t.widget(t.find_in(m, "UI_SectionBar_Type")).vis, Vis::Hidden);
    assert!(t.widget(t.find_in(m, "WBP_GameModeCard_ArcadeNormal")).shown());
    assert!(!t.widget(t.find_in(m, "WBP_GameModeCard_2v2")).shown());
}

#[test]
fn the_advanced_tutorials_fill_their_grid() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let m = s.with_cx(&mut e.run(), |cx| {
        show_modal_generic(cx, true, "WBP_SubOptionModal", "SubOption", "advancedtutorial|")
    });
    let m = m.expect("the sub option modal is shown");
    assert!(frames(&mut e, &mut s, 1.0, 0.5) > 0);
    let t = s.view.tree();
    let panel = t.find_in(t.find_in(m, "UI_GridList"), "gridPanelBase");
    let tiles = t.widget(panel).children.clone();
    assert!(!tiles.is_empty(), "a tile per tutorial");
    let first = tiles[0];
    assert!(!t.widget(t.find_in(first, "TutorialType")).text.is_empty());
    let att = t.find_in(first, "UI_Attention");
    assert_eq!(t.widget(t.find_in(att, "MainContainer")).vis, Vis::Collapsed, "no badge (the user's choice)");
}

#[test]
fn the_ranked_help_and_server_prompt_are_made_and_painted() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let m = s
        .with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "UI_RankedExplanationModal", "rankedExplanation", ""));
    let m = m.expect("the ranked help is shown");
    assert!(frames(&mut e, &mut s, 1.0, 0.5) > 0);
    let t = s.view.tree();
    assert_eq!(t.widget(t.find_in(m, "tiersContainer")).children.len(), 1);
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "UI_ServerSettingsPrompt", "serverSettings", ""));
    let m = m.expect("the server prompt is shown");
    assert!(frames(&mut e, &mut s, 2.0, 0.5) > 0);
    let t = s.view.tree();
    assert_eq!(t.widget(t.find_in(m, "XBoxCrossplayWarning")).vis, Vis::Collapsed);
}

/// The ranked help's bullets: `<img id="BulletDiamond"/>` is its decorator's image (RichTextImageDecorator's
/// RichTextImages row: UI_perkItemBG), not the tag's text.
#[test]
fn the_ranked_help_draws_its_bullet_images() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    s.view.set_parts(
        Box::new(mvs_ui::menu::scroll::ScrollBars::new(e.bundle.clone())),
        Box::new(mvs_ui::menu::rich_text::RichTexts::new(e.bundle.clone())),
    );
    let m = s
        .with_cx(&mut e.run(), |cx| show_modal_generic(cx, true, "UI_RankedExplanationModal", "rankedExplanation", ""));
    let m = m.expect("the ranked help is shown");
    frames(&mut e, &mut s, 1.0, 0.5);
    let t = s.view.tree();
    let (mut images, mut tags) = (0, 0);
    let mut stack = vec![m];
    while let Some(w) = stack.pop() {
        let x = t.widget(w);
        images += usize::from(x.tex.as_deref() == Some("UI_perkItemBG") && x.brush_w > 0.0);
        tags += usize::from(x.name == "run" && x.text.contains("<img"));
        stack.extend(x.children.iter().copied());
    }
    assert!(images >= 3, "a bullet image per line ({images})");
    assert_eq!(tags, 0, "no run draws a tag's text");
}

/// The promo modal shows the clicked promotion's widget in ContentPanel, and a page change swaps it.
#[test]
fn the_large_promo_ticker_shows_the_promotion_of_its_page() {
    use mvs_ui::menu::widgets::promo::{PromoTickerLarge, local_promotions};
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let list = local_promotions(&e.bundle);
    assert!(list.len() > 1, "the local promotions");
    let m = s.with_cx(&mut e.run(), |cx| show_modal_generic(cx, false, "UI_PromoTickerLarge", "promoTicker", ""));
    let m = m.expect("the large ticker is shown");
    let l2 = list.clone();
    s.with_cx(&mut e.run(), |cx| cx.visit::<PromoTickerLarge, _>(m, |l, cx| l.init(cx, l2, 1)));
    frames(&mut e, &mut s, 1.0, 0.5);
    let shown = |s: &MenuScreen| {
        let t = s.view.tree();
        let panel = t.find_in(m, "ContentPanel");
        let c: Vec<_> = t.widget(panel).children.iter().filter(|&&c| t.user_class(c).is_some()).copied().collect();
        assert_eq!(c.len(), 1, "one promotion's widget (HydraData beside it is a plain widget)");
        t.user_class(c[0]).unwrap_or_default().to_string()
    };
    assert_eq!(shown(&s), list[1].pre_determined_widget);
    let t = s.view.tree();
    assert_eq!(t.widget(t.find_in(m, "UI_PaginationPips")).vis, Vis::SelfHitTestInvisible);
    s.with_cx(&mut e.run(), |cx| {
        let p = cx.view.tree().find_in(m, "UI_PaginationPips");
        cx.visit::<PaginationPips, _>(p, |p, cx| p.increment_page(cx, 1));
    });
    frames(&mut e, &mut s, 1.6, 0.2);
    assert_eq!(shown(&s), list[2 % list.len()].pre_determined_widget);
}

#[test]
fn the_promo_ticker_scrolls_to_the_page_its_timer_turns() {
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    s.view.set_parts(
        Box::new(mvs_ui::menu::scroll::ScrollBars::new(e.bundle.clone())),
        Box::new(mvs_ui::menu::rich_text::RichTexts::new(e.bundle.clone())),
    );
    let w = shown(&mut e, &mut s, "UI_PromoTickerSmall");
    frames(&mut e, &mut s, 0.0, 0.2);
    s.with_cx(&mut e.run(), |cx| {
        let p = cx.view.tree().find_in(w, "UI_PaginationPips");
        cx.visit::<PaginationPips, _>(p, |p, cx| p.increment_page(cx, 1));
    });
    // the animated scroll (FInterpTo at 15) is there within a second
    frames(&mut e, &mut s, 0.3, 1.0);
    let t = s.view.tree();
    let sc = t.find_in(w, "scrollContainer");
    assert_eq!(t.widget(sc).scroll, 500.0, "the second item fills the box");
}

/// The sidebar's party member slot (UI_PartyMemberSlot, Party Member Index 1) is collapsed alone at Construct and
/// shows within Update Party Info's 2 s looping timer once a member arrives: the timer runs while it is collapsed.
#[test]
fn the_party_member_slot_shows_a_member_who_joins_later() {
    use mvs_net::service::api::{PartyMember, PartyView};
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    let w = shown(&mut e, &mut s, "WBP_PlayerDataNew");
    frames(&mut e, &mut s, 1.0, 0.5);
    let slot = s.view.tree().find_in(w, "UI_PartyMemberProfileIcon");
    assert_eq!(s.view.tree().widget(slot).vis, Vis::Collapsed, "alone at Construct");
    let member = |id: &str| PartyMember { id: id.into(), name: id.into(), icon: String::new() };
    s.view.session.online.party =
        Some(PartyView { leader: "me".into(), members: vec![member("me"), member("pal")], me: "me".into() });
    frames(&mut e, &mut s, 1.6, 2.1);
    assert_eq!(s.view.tree().widget(slot).vis, Vis::SelfHitTestInvisible, "the member shows");
}

/// The online list that answers the panel's own ask arrives after the quick invite's click; its populate
/// keeps Quick Invite Mode, so the online rows keep their "+" (style 2).
#[test]
fn the_quick_invite_keeps_its_rows_plus_when_the_online_list_arrives() {
    use mvs_ui::menu::online::OnlinePlayer;
    use mvs_ui::menu::widgets::social::{SocialList, SocialPanel};
    let Some(mut e) = env() else { return };
    let mut s = e.screen();
    s.view.session.online.connected = true;
    let w = shown(&mut e, &mut s, "UI_SocialPanel");
    frames(&mut e, &mut s, 1.0, 0.2);
    s.with_cx(&mut e.run(), |cx| cx.visit::<SocialPanel, _>(w, |p, cx| p.set_mode(cx, 1, true, true)));
    frames(&mut e, &mut s, 1.3, 0.1);
    s.view.session.online.listed =
        vec![OnlinePlayer { id: "pal".into(), name: "pal".into(), present: true, ..Default::default() }];
    frames(&mut e, &mut s, 1.5, 0.2);
    let l = s.view.tree().find_in(w, "socialListOnline");
    let style = s.with_cx(&mut e.run(), |cx| cx.visit::<SocialList, _>(l, |l, _| (l.style, l.players.len())));
    assert_eq!(style, Some((2, 1)), "the online row is listed with the quick invite's style");
}
