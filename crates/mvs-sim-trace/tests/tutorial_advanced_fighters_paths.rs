//! Paths of the advanced tutorials' fighters family on the oracle's tut-armor match, stepped with its recorded
//! inputs: the Armor Break minigame's enemy takes Buff_ArmorPack_C for 9999999 s (a timer count in the hundreds of
//! millions), a duration the demo's 3 s pack does not reach.

use mvs_sim_trace::harness;

/// The Armor Break minigame begins at tick 270; 0.2 s later its enemy is given the armour pack.
#[test]
fn armor_break_enemy_gets_the_armour_pack() {
    let Some(mut h) = harness::at("tut-armor", 0) else { return };
    for _ in 0..300 {
        h.step_recorded();
    }
    let w = &h.w;
    let armoured = w
        .pawns
        .as_ref()
        .expect("the tutorial's pawns")
        .iter()
        .any(|&f| w.fighter(f).buffs.buffs.iter().any(|b| w.names.str(b.cls) == "Buff_ArmorPack_C"));
    assert!(armoured, "an armoured pawn once the Armor Break minigame has run for 30 ticks");
}
