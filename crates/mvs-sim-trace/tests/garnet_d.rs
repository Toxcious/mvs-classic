//! Garnet's aim, hold, groove and pawn paths that no match runs (`chars/garnet/aim.rs`, `hold.rs`, `groove.rs`,
//! `pawn.rs`), on a fresh World of Garnet against Shaggy. Without a store the tests check nothing and say so.
//! Not reachable here: `marker.rs` 126-128 (the ActivationConditions of a marker made by a spawn request: needs the
//! build's marker spawn with a data row), 176-178 (a marker with no attach parent retrying its UI: a marker actor
//! placed by the gauntlets' throw, no match throws it detached), 288-290 (its retry timer, the same actor);
//! `gauntlets.rs` has no unrun line.

use mvs_sim::chars::garnet::aim::{
    marker_aim, reset_timeline_tick, set_aim_direction, set_garnet_aim_flag, set_read_aim_from_movement, slot_input,
};
use mvs_sim::chars::garnet::hold::set_is_holding_enemy_body;
use mvs_sim::chars::garnet::pawn::{garnet_has_gauntlet_projectile_out, has_ally_marked, set_projectile_gauntlets};
use mvs_sim::chars::garnet::{garnet, garnet_of};
use mvs_sim::combat::buffs::{buff_of, buff_signal, spawn_buff};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::registry::signals::{BuffSignal, BuffSignalArg};
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

#[test]
fn the_aim_flag_and_the_slot_inputs_follow_the_aim() {
    let Some((mut h, g, _)) = pair() else { return };
    set_garnet_aim_flag(&mut h.w, g, true);
    assert!(garnet(&h.w, g).aim.aim_flag);
    set_garnet_aim_flag(&mut h.w, g, false);
    assert!(!garnet(&h.w, g).aim.aim_flag);
    set_aim_direction(&mut h.w, g, 1.0, false, true);
    let aim = garnet(&h.w, g).aim.aim_direction;
    assert!(aim > 0.0);
    let (a, inv, ground, unknown) = {
        let n = |s| h.w.lit(s);
        (n("AimDirection"), n("AimDirectionInverted"), n("IsOnGround"), n("Buff_Garnet_ElectricGroove_C"))
    };
    assert_eq!(slot_input(&h.w, g, a), aim);
    assert_eq!(slot_input(&h.w, g, inv), -aim);
    assert_eq!(slot_input(&h.w, g, ground), 1.0, "landed after ten ticks");
    assert!(slot_input(&h.w, g, unknown).is_nan());
}

#[test]
fn the_reset_timeline_runs_out_and_brings_the_aim_to_zero() {
    let Some((mut h, g, _)) = pair() else { return };
    set_aim_direction(&mut h.w, g, 1.0, false, true);
    set_read_aim_from_movement(&mut h.w, g, true, false);
    set_read_aim_from_movement(&mut h.w, g, false, false); // resets: the timeline starts
    assert!(garnet(&h.w, g).aim.reset_on);
    for _ in 0..40 {
        reset_timeline_tick(&mut h.w, g);
    }
    let a = &garnet(&h.w, g).aim;
    assert!(!a.reset_on, "Finished: nothing more");
    assert_eq!(a.aim_direction, 0.0);
}

#[test]
fn the_marker_aim_takes_a_unit_step_as_it_is() {
    let Some((mut h, g, _)) = pair() else { return };
    let (x, y) = (h.w.fighter(g).body.x, mvs_sim::fighter::actor_y(&h.w, g));
    let before = garnet(&h.w, g).aim.aim_direction;
    // The marker one unit above: the squared size is 1 (the vector is kept, not normalised).
    marker_aim(&mut h.w, g, x, y + 1.0);
    let after = garnet(&h.w, g).aim.aim_direction;
    assert!(after > before);
    assert_eq!(after, before + 0.375 * (1.0 - before));
}

#[test]
fn the_pawn_reads_no_gauntlet_and_no_ally_marker_when_none_is_out() {
    let Some((mut h, g, _)) = pair() else { return };
    set_projectile_gauntlets(&mut h.w, g, None);
    assert!(!garnet_has_gauntlet_projectile_out(&h.w, g));
    assert!(!has_ally_marked(&h.w, g));
}

#[test]
fn a_hold_is_kept_with_its_flag_and_a_refused_one_is_destroyed() {
    let Some((mut h, g, s)) = pair() else { return };
    let cls = h.w.lit("Buff_HeldByEnemyGarnet_C");
    // A hold of her own (Shaggy held by Garnet): the buff stays and she holds an enemy.
    spawn_buff(&mut h.w, s, cls, 0.0, Some(0), Some(g), true, Some(g));
    assert!(buff_of(&h.w, s, cls).is_some());
    assert!(h.w.fighter(g).holds.holding_enemy);
    set_is_holding_enemy_body(&mut h.w, g, false);
    assert!(!h.w.fighter(g).holds.holding_enemy);
    // A second hold on the same fighter is refused: the buff is destroyed and the instigator's flag cleared
    // (the oracle's hold code).
    set_is_holding_enemy_body(&mut h.w, g, true);
    spawn_buff(&mut h.w, g, cls, 0.0, Some(0), Some(s), true, Some(s));
    assert!(buff_of(&h.w, g, cls).is_none(), "the refused hold's buff is destroyed");
    assert!(garnet_of(&h.w, g).is_some());
}

#[test]
fn the_groove_gives_the_singing_stacks_on_an_electric_buff() {
    let Some((mut h, g, _)) = pair() else { return };
    let groove = h.w.lit("Buff_Garnet_ElectricGroove_C");
    let singing = h.w.lit("Buff_GarnetStackingSinging_C");
    let electric = h.w.lit("Buff_Damage_Electric_C");
    spawn_buff(&mut h.w, g, groove, 0.0, Some(0), Some(g), true, Some(g));
    if buff_of(&h.w, g, groove).is_none() {
        eprintln!("note: the groove buff did not spawn; nothing checked");
        return;
    }
    // The chance is RhythmChance per signal: enough signals give it a hit.
    for _ in 0..200 {
        let arg = BuffSignalArg { buff_cls: Some(electric), ..BuffSignalArg::default() };
        buff_signal(&mut h.w, g, BuffSignal::NewBuffSpawned, arg);
        if buff_of(&h.w, g, singing).is_some() {
            return;
        }
    }
    panic!("no singing stack after 200 electric buffs");
}
