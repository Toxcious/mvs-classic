//! The hit paths on a team mate no trace match runs: the World's projectile ally test with a class's On Hit Buffs
//! rows (`combat::phases` PendingHit::ProjectileAlly) and the electric chain's hit on a DamageDisabled fighter
//! (`combat::electric`). On fresh Worlds; without a store they check nothing.

use mvs_sim::combat::BuffExt;
use mvs_sim::combat::buffs::{buff_mut, buff_of, spawn_buff};
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() }
}

fn fresh(players: Vec<MatchPlayer>) -> Option<Harness> {
    let n = players.len();
    let setup = MatchSetup { players, ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&vec![NO_INPUT; n]);
    }
    Some(h)
}

/// Velma's book over her team mate: pfgDoDamage's entry rows (Buff_Educated and Buff_DecrementEvadeCount: Self) go to
/// Velma, its ally rows (the same two: Hit and Ally) to the team mate, from the World's ally test.
#[test]
fn a_projectile_over_a_team_mate_makes_its_entry_and_ally_rows() {
    let Some(mut h) = fresh(vec![player("velma", 0), player("shaggy", 0), player("shaggy", 1)]) else { return };
    let (v, ally) = (h.fighter(0), h.fighter(1));
    let book = h.w.lit("VelmaBookProjectile_C");
    let educated = h.w.lit("Buff_Educated_C");
    assert!(buff_of(&h.w, ally, educated).is_none() && buff_of(&h.w, v, educated).is_none());
    let (x, y) = (h.w.fighter(ally).body.x, h.w.fighter(ally).body.y + 100.0);
    comp_spawn_projectile(&mut h.w, v, book, x, y, [Some(0.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
        .expect("Velma's book spawns");
    let none: Vec<InputState> = vec![NO_INPUT; 3];
    let mut n = 0;
    while buff_of(&h.w, ally, educated).is_none() && n < 10 {
        h.step(&none);
        n += 1;
    }
    assert!(buff_of(&h.w, ally, educated).is_some(), "the ally row reached the team mate");
    assert!(buff_of(&h.w, v, educated).is_some(), "the entry row reached Velma");
}

/// The chain on a DamageDisabled enemy of its Instigator: the DamageManager's HitByAlly (a held plasma ball at its
/// attached stage ends on it), no damage, and no HitBoxHandler HitByAlly (Bugs's charm, which ends on that one, stays:
/// the Instigator is of another team).
#[test]
fn the_chain_on_a_damage_disabled_enemy_sends_the_damage_managers_hit_by_ally() {
    let Some(mut h) = fresh(vec![player("irongiant", 0), player("bugs", 1)]) else { return };
    let (g, b) = (h.fighter(0), h.fighter(1));
    let held = h.w.lit("Buff_HeldByPlasmaBall_C");
    let charm = h.w.lit("Buff_Charm_C");
    let electric = h.w.lit("Buff_Damage_Electric_C");
    let none: Vec<InputState> = vec![NO_INPUT; 2];
    let put = |h: &mut Harness| {
        let r = spawn_buff(&mut h.w, b, held, 5.0, None, Some(g), true, Some(g)).expect("the hold spawns");
        if let Some(x) = buff_mut(&mut h.w, r)
            && let BuffExt::HeldByPlasmaBall(e) = &mut x.ext
        {
            e.stage = 2;
        }
        spawn_buff(&mut h.w, b, charm, 5.0, None, Some(g), true, Some(g)).expect("the charm spawns");
        h.w.fighter_mut(b).invuln.damage_disabled = true;
    };
    // Without the chain both stay a frame.
    put(&mut h);
    h.step(&none);
    assert!(buff_of(&h.w, b, held).is_some() && buff_of(&h.w, b, charm).is_some(), "both stay without a hit");
    let before = h.w.fighter(b).stats.damage;
    spawn_buff(&mut h.w, b, electric, 5.0, Some(2), Some(g), true, Some(b)).expect("the chain spawns");
    h.step(&none); // the BeginPlay's Trigger runs in the frame's timer pass
    assert!(buff_of(&h.w, b, held).is_none(), "the DamageManager's HitByAlly ended the hold");
    assert!(buff_of(&h.w, b, charm).is_some(), "no HitBoxHandler HitByAlly: the charm stays");
    assert_eq!(h.w.fighter(b).stats.damage, before, "a refused hit deals nothing");
}
