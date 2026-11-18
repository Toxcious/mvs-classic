//! Finn's shop buffs, perks and BMO (`chars/finn/shop.rs`, `perks.rs`, `bmo.rs`), the paths no trace match runs: the
//! shop's Boots and ArmorBuns buffs, the fire sale, BMO and the gem boost blast. The expected values are read off the
//! oracle's source, not an oracle probe: the matches never buy at the shop. The purchase itself (the
//! wallet's FinnTreasureBuff) waits on coins.rs's `wallet_stacks`.

use mvs_sim::InputState;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::finn::finn_of;
use mvs_sim::combat::buffs::{buff_of, buff_signal, end_buff, spawn_buff};
use mvs_sim::fighter::FighterId;
use mvs_sim::name;
use mvs_sim::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_sim_trace::harness::{self, Harness};

/// An early frame of finn-finn: Finn is fighter 0, the other Finn fighter 1.
const AT: usize = 5;

fn tick(h: &mut Harness, n: usize) {
    let idle = vec![InputState::default(); h.w.players.len()];
    for _ in 0..n {
        h.step(&idle);
    }
}

fn stacks(h: &Harness, f: FighterId, cls: mvs_sim::names::Name) -> Option<f64> {
    buff_of(&h.w, f, cls).and_then(|b| mvs_sim::registry::props::buff(&h.w, b)).map(|b| b.stacks).map(f64::from)
}

#[test]
fn damage_to_activate_puts_the_shop_on_sale_for_good() {
    let Some(mut h) = harness::at("finn-finn", AT) else { return };
    let f = h.fighter(0);
    let sale = name!("Buff_Finn_GoingOutOfBusiness_C");
    spawn_buff(&mut h.w, f, sale, 0.0, Some(1), Some(f), true, Some(f));
    assert!(!mvs_sim::chars::finn::perks::is_sale_active(&h.w, f));
    // under DamageToActivate (100): nothing
    h.w.fighter_mut(f).stats.damage = 50.0;
    buff_signal(&mut h.w, f, BuffSignal::HitByEnemy, BuffSignalArg::default());
    assert!(!mvs_sim::chars::finn::perks::is_sale_active(&h.w, f));
    // at it: Activated and the sale on, and a later ring-out is not looked at
    h.w.fighter_mut(f).stats.damage = 120.0;
    buff_signal(&mut h.w, f, BuffSignal::HitByEnemy, BuffSignalArg::default());
    assert!(mvs_sim::chars::finn::perks::is_sale_active(&h.w, f));
    tick(&mut h, 700); // past Duration (10 s): an activated sale does not end
    assert!(mvs_sim::chars::finn::perks::is_sale_active(&h.w, f));
}

#[test]
fn an_ally_ring_out_puts_the_shop_on_sale_for_duration() {
    let Some(mut h) = harness::at("finn-finn", AT) else { return };
    let (f, other) = (h.fighter(0), h.fighter(1));
    let sale = name!("Buff_Finn_GoingOutOfBusiness_C");
    spawn_buff(&mut h.w, f, sale, 0.0, Some(1), Some(f), true, Some(f));
    // the other fighter on Finn's team, knocked out
    let team = h.w.fighter(f).team.team;
    h.w.fighter_mut(other).team.team = team;
    h.w.fighter_mut(other).life.dead = 1.0;
    let arg = BuffSignalArg { victim: Some(other), ..BuffSignalArg::default() };
    buff_signal(&mut h.w, f, BuffSignal::PlayerKnockedOut, arg);
    tick(&mut h, 3); // Delay(0.0) resumes
    assert!(mvs_sim::chars::finn::perks::is_sale_active(&h.w, f));
    tick(&mut h, 620); // Delay(Duration 10) -> SetSale(false)
    assert!(!mvs_sim::chars::finn::perks::is_sale_active(&h.w, f));
}

#[test]
fn boots_stack_into_a_faster_speed_mod() {
    let Some(mut h) = harness::at("finn-finn", AT) else { return };
    let f = h.fighter(0);
    let boots = name!("Finn_Shop_Boots_Buff_C");
    spawn_buff(&mut h.w, f, boots, 0.0, Some(1), Some(f), true, Some(f));
    assert_eq!(stacks(&h, f, boots), Some(1.0));
    // a second purchase adds a stack: SpeedMod 1.15 (n == 2), 1.15 + 0.03 (n - 2) above
    spawn_buff(&mut h.w, f, boots, 0.0, Some(1), Some(f), true, Some(f));
    let b = buff_of(&h.w, f, boots).expect("the Boots buff");
    let d = mvs_sim::combat::buffs::inst_def_of(&h.w, b).expect("its values");
    assert_eq!(d.get("SpeedMod").and_then(|v| v.as_f64()), Some(f64::from(1.15_f32)));
    spawn_buff(&mut h.w, f, boots, 0.0, Some(1), Some(f), true, Some(f));
    let b = buff_of(&h.w, f, boots).expect("the Boots buff");
    let d = mvs_sim::combat::buffs::inst_def_of(&h.w, b).expect("its values");
    assert_eq!(d.get("SpeedMod").and_then(|v| v.as_f64()), Some(f64::from(1.15_f32 + 0.03)));
    tick(&mut h, 8); // the Delay(0.1) pop text and VO resume without a panic
}

#[test]
fn armor_buns_block_projectiles_while_charging() {
    let Some(mut h) = harness::at("finn-finn", AT) else { return };
    let f = h.fighter(0);
    let buns = name!("Finn_Shop_ArmorBuns_Buff_C");
    let block = name!("Buff_ProjectileBlock_C");
    spawn_buff(&mut h.w, f, buns, 0.0, Some(3), Some(f), true, Some(f));
    assert_eq!(stacks(&h, f, buns), Some(3.0));
    assert_eq!(stacks(&h, f, block), None);
    let n = mvs_sim::events::NotifyRef { montage: buns, index: 0 };
    let started = h.w.fighter(f).hooks.char.charge_started.expect("Finn's chargeStarted");
    let ended = h.w.fighter(f).hooks.char.charge_end.expect("Finn's chargeEnd");
    started(&mut h.w, f, n);
    tick(&mut h, 8); // Delay(StartupDelay 0.1): the owner's Buff_ProjectileBlock of the Buns' stacks
    assert_eq!(stacks(&h, f, block), Some(3.0));
    ended(&mut h.w, f, n);
    assert_eq!(stacks(&h, f, block), None, "StopProjectileArmor takes the stacks it applied back");
    assert_eq!(stacks(&h, f, buns), Some(3.0));
}

#[test]
fn bmo_follows_finn_and_ends_with_its_buff() {
    let Some(mut h) = harness::at("finn-finn", AT) else { return };
    let f = h.fighter(0);
    let chop = name!("Bmo_Chop_Buff_C");
    spawn_buff(&mut h.w, f, chop, 0.0, Some(1), Some(f), true, Some(f));
    let bmo = finn_of(&h.w, f).and_then(|s| s.bmo).expect("the BMO is Finn's");
    let ActorRef::Actor(id) = bmo else { panic!("a world actor") };
    tick(&mut h, 10); // past Delay(0.1): bound to the buff, kept
    assert!(!h.w.actors.actors.get(id).expect("listed").dead);
    let bb = buff_of(&h.w, f, chop).expect("the buff");
    end_buff(&mut h.w, f, bb);
    tick(&mut h, 1);
    assert!(h.w.actors.actors.get(id).is_none_or(|a| a.dead), "DieWithBuff");
    assert_eq!(finn_of(&h.w, f).and_then(|s| s.bmo), None);
}
