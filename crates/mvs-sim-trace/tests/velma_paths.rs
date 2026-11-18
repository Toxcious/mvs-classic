//! Velma's police paths that no match runs (`chars/velma/police.rs`), on a fresh World of Velma against Shaggy: the call
//! (the minion, her flags, the Delay 1), and the arrest (Buff_Arrested on the targeted enemy, the car's hold on it).
//! Without a store the tests check nothing and say so.

use mvs_sim::chars::velma::police::call_police;
use mvs_sim::chars::velma::{police_w, velma};
use mvs_sim::combat::buffs::buff_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player("velma", false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (v, s) = (h.fighter(0), h.fighter(1));
    Some((h, v, s))
}

/// SpawnPoliceServer: the minion made, her flag down, the call's Delay 1 running; a second call while it runs does
/// nothing.
#[test]
fn the_call_spawns_one_police() {
    let Some((mut h, v, _)) = pair() else { return };
    call_police(&mut h.w, v);
    let m = velma(&h.w, v).police.expect("a police spawned");
    assert!(!velma(&h.w, v).allow_police_call);
    assert!(velma(&h.w, v).call.delay1 > 0);
    assert_eq!(velma(&h.w, v).call.last_spawn, Some(m));
    assert!(police_w(&mut h.w, m).target_loop);
    call_police(&mut h.w, v);
    assert_eq!(velma(&h.w, v).police, Some(m));
}

/// HandleActorCollision: the targeted enemy inside the car's body after its ArrestEnabled is arrested and held.
#[test]
fn the_targeted_enemy_inside_the_car_is_arrested() {
    let Some((mut h, v, s)) = pair() else { return };
    call_police(&mut h.w, v);
    let m = velma(&h.w, v).police.expect("a police spawned");
    let (x, y) = (h.w.fighter(m).body.x, h.w.fighter(m).body.y);
    let b = &mut h.w.fighter_mut(s).body;
    (b.x, b.prev_x, b.y, b.prev_y) = (x, x, y, y);
    let p = police_w(&mut h.w, m);
    p.targeted_enemy = Some(s);
    p.arrest_enabled = true;
    p.target_loop = false;
    h.step(&NONE);
    assert!(buff_of(&h.w, s, name!("Buff_Arrested_C")).is_some(), "the enemy is arrested");
}

/// SlowtoStunGrenade: an enemy in its Box gets Buff_SlowIntoStun with the grenade as its trap, then Buff_Ice from
/// ApplyIce after ApplyIceTime; leaving the Box ends the slow. (No match spawns the vial.)
#[test]
fn the_grenade_slows_an_enemy_into_ice() {
    use mvs_sim::chars::velma::gadgets::spawn_grenade;
    let Some((mut h, v, s)) = pair() else { return };
    let (x, y) = (h.w.fighter(s).body.x, h.w.fighter(s).body.y);
    spawn_grenade(&mut h.w, v, x, y);
    h.step(&NONE);
    assert!(buff_of(&h.w, s, name!("Buff_SlowIntoStun_C")).is_some(), "the enemy is slowed");
    for _ in 0..40 {
        h.step(&NONE);
    }
    assert!(buff_of(&h.w, s, name!("Buff_Ice_C")).is_some(), "the slow turns to ice");
    assert!(buff_of(&h.w, v, name!("Buff_SlowIntoStun_C")).is_none(), "the owner is not slowed");
    let b = &mut h.w.fighter_mut(s).body;
    (b.x, b.prev_x) = (b.x + 5000.0, b.x + 5000.0);
    h.step(&NONE);
    assert!(buff_of(&h.w, s, name!("Buff_SlowIntoStun_C")).is_none(), "leaving the box ends the slow");
}
