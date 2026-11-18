//! Garnet's shockwave and throw paths that no match runs (`chars/garnet/shockwave.rs`, `hold.rs`), on a fresh World of
//! Garnet against Shaggy: her ground D attack's shockwaves meeting an invulnerable enemy (the dodge record, not a hit),
//! and her throw of the fighter she holds. Without a store the tests check nothing and say so.
//! Not reached: the shockwave leaving the arena (the oracle's shockwave code): it needs the box's exit on the
//! timeline's own tick, which the stage under test (Map_TrainingRoomSmall) has no blast box for.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::chars::garnet::hold::throw_fighters_body;
use mvs_sim::combat::buffs::{buff_of, spawn_buff};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player("garnet", false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (g, s) = (h.fighter(0), h.fighter(1));
    Some((h, g, s))
}

fn shockwaves(h: &Harness) -> usize {
    h.w.actors.actors.iter().filter(|(_, a)| h.w.names.str(a.cls).starts_with("GarnetShockwaveHitBox")).count()
}

/// Her ground D montage spawns the shockwaves; Shaggy stands in their way. Invulnerable he is not hit (the dodge is
/// recorded and no damage dealt); otherwise the shockwave hits him.
fn run(invulnerable: bool) -> Option<(Harness, FighterId, bool)> {
    let (mut h, g, s) = pair()?;
    let (gx, _) = (h.w.fighter(g).body.x, ());
    for (f, x) in [(g, gx), (s, gx - 300.0)] {
        let b = &mut h.w.fighter_mut(f).body;
        b.x = x;
        b.prev_x = x;
    }
    if invulnerable {
        h.w.fighter_mut(s).invuln.invuln_count = 1;
    }
    let m = h.w.lit("Garnet_Attack_Ground_D_Montage");
    start_montage(&mut h.w, g, m, &PROXY_NONE, None);
    let mut seen = false;
    for _ in 0..120 {
        h.step(&NONE);
        seen |= shockwaves(&h) > 0;
    }
    Some((h, s, seen))
}

#[test]
fn a_shockwave_meets_an_invulnerable_enemy_without_a_hit() {
    let Some((h, s, seen)) = run(true) else { return };
    assert!(seen, "the ground D attack spawned its shockwaves");
    assert_eq!(h.w.fighter(s).stats.damage, 0.0, "no hit on an invulnerable fighter");
}

#[test]
fn a_shockwave_hits_a_vulnerable_enemy() {
    let Some((h, s, seen)) = run(false) else { return };
    assert!(seen);
    assert!(h.w.fighter(s).stats.damage > 0.0);
}

#[test]
fn a_throw_ends_the_hold_and_queues_its_hit() {
    let Some((mut h, g, s)) = pair() else { return };
    let cls = h.w.lit("Buff_HeldByEnemyGarnet_C");
    spawn_buff(&mut h.w, s, cls, 0.0, Some(0), Some(g), true, Some(g));
    assert!(buff_of(&h.w, s, cls).is_some());
    if let Some(hd) = h.w.fighter_mut(s).holds.hold.as_mut() {
        hd.attached = true;
    }
    let hit = mvs_data::value::Props::new();
    throw_fighters_body(&mut h.w, g, &hit, false);
    assert!(buff_of(&h.w, s, cls).is_none(), "the throw destroys the held buff");
    let q = &h.w.fighter(g).holds.hold_hits;
    assert_eq!(q.len(), 1);
    assert!(q[0].p.contains_key("KnockBackDirectionVector"));
    assert_eq!(q[0].p.get("IgnoreFacing"), Some(&mvs_data::value::Value::Bool(true)));
}
