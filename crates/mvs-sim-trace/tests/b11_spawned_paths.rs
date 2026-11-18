//! Paths of the spawned actors no trace match runs, each on a fresh World: the bot's read of Iron Giant's scrap and
//! art items (`ai::services` BotDetectEnemy), and the Meeseek box's bounce off a stage solid (`items::meeseeks`
//! `component_hit`). Without a store the tests check nothing and say so.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::ai::bt::BtBot;
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::items::spawn_item;
use mvs_sim_trace::harness::{self, Harness};
use std::sync::Arc;

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn pair(a: &str, a_bot: bool, b: &str) -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player(a, a_bot), player(b, false)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else {
        eprintln!("note: no store; nothing checked");
        return None;
    };
    h.w.bot_trees = Some(Arc::new(h.w.data.bot_trees.clone()));
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (f, g) = (h.fighter(0), h.fighter(1));
    Some((h, f, g))
}

/// The bot's blackboard Item after `n` thinks, each behind a step: whether it ever held `want`, and whether it ever
/// held anything.
fn item_seen(h: &mut Harness, bot: &mut BtBot, me: FighterId, want: ActorRef, n: usize) -> (bool, bool) {
    let (mut hit, mut any) = (false, false);
    for _ in 0..n {
        bot.think(&mut h.w, me);
        let k = bot.bb.key("Item");
        let got = bot.bb.obj(k);
        hit |= got == Some(want);
        any |= got.is_some();
        h.step(&NONE);
    }
    (hit, any)
}

/// Item_C017_Scrap of the bot itself is its Item however far it lies (no distance test, @6617-6744).
#[test]
fn the_bots_own_scrap_is_its_item_at_any_distance() {
    let Some((mut h, ig, _)) = pair("irongiant", true, "shaggy") else { return };
    let trees = h.w.bot_trees.clone().expect("the trees");
    let mut bot = BtBot::match_bot(12345, &trees, 3);
    let cls = h.w.names.get("Item_C017_Scrap_C").expect("the scrap's name");
    let (x, y) = (h.w.fighter(ig).body.x, h.w.fighter(ig).body.y);
    let far = if x > 0.0 { x - 1500.0 } else { x + 1500.0 };
    let it = spawn_item(&mut h.w, cls, ig, far, y + 50.0, 0.0, 0.0, false, None).expect("the scrap");
    let (hit, _) = item_seen(&mut h, &mut bot, ig, ActorRef::Item(it), 90);
    assert!(hit, "the bot's own scrap 1500 away never became its Item");
}

/// Item_C017_Art sets nothing (@6310), even beside the bot, where Item_Actor_Parent's branch would set Item.
#[test]
fn an_art_item_is_never_the_bots_item() {
    let Some((mut h, ig, _)) = pair("irongiant", true, "shaggy") else { return };
    let trees = h.w.bot_trees.clone().expect("the trees");
    let mut bot = BtBot::match_bot(12345, &trees, 3);
    let cls = h.w.names.get("Item_C017_Art_C").expect("the art's name");
    let (x, y) = (h.w.fighter(ig).body.x, h.w.fighter(ig).body.y);
    let it = spawn_item(&mut h.w, cls, ig, x + 100.0, y + 50.0, 0.0, 0.0, false, None).expect("the art");
    let (hit, _) = item_seen(&mut h, &mut bot, ig, ActorRef::Item(it), 90);
    assert!(!hit, "an art item became the bot's Item");
}

/// The Meeseek box's top speed over its last free ticks after it falls fast onto the stage: with CanBasketballBounce
/// its Capsule's OnComponentHit mirrors the velocity off the contact's normal (@930); without, it lands.
fn meeseek_rise(can_bounce: bool) -> Option<f32> {
    let (mut h, bugs, _) = pair("bugs", false, "shaggy")?;
    let cls = h.w.names.get("Item_Meeseek_C").expect("the box's name");
    let (x, y) = (h.w.fighter(bugs).body.x, h.w.fighter(bugs).body.y);
    let it = spawn_item(&mut h.w, cls, bugs, x, y + 600.0, 0.0, -2500.0, false, None).expect("the box");
    if let Some(i) = h.w.actors.items.get_mut(it)
        && let ItemExt::MeeseekItem(m) = &mut i.ext
    {
        m.can_bounce = can_bounce;
    }
    let mut rise: f32 = 0.0;
    for _ in 0..40 {
        h.step(&NONE);
        let Some(i) = h.w.actors.items.get(it) else { break };
        rise = rise.max(i.vy);
    }
    Some(rise)
}

#[test]
fn a_fast_meeseek_box_bounces_off_the_stage() {
    let (Some(up), Some(flat)) = (meeseek_rise(true), meeseek_rise(false)) else { return };
    assert!(up > 1000.0, "the box did not bounce: top rise {up}");
    assert!(flat < up, "the box rose as high without CanBasketballBounce: {flat} vs {up}");
}
