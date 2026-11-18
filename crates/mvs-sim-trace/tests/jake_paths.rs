//! Jake's perk paths no trace match runs (`chars/jake/perks.rs`): SafetyNet's AddAirSuperiorityServer gives an ally
//! Buff_AirSuperiority_C once per cooldown, and the buff's looping RefreshAirMoves timer re-arms itself. Against the
//! the oracle's code read line by line (no oracle probe of this path).

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::jake::perks::{AirExt, SafetyNetExt, add_air_superiority_server};
use mvs_sim::combat::buffs::{BuffExt, buff, buff_of};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::InputState;
use mvs_sim::name;
use mvs_sim_trace::harness::{self, Harness};

/// An early frame of jake-jake (the 2v2 with Jake's perks).
const AT: usize = 5;

fn tick(h: &mut Harness, n: usize) {
    let idle = vec![InputState::default(); h.w.players.len()];
    for _ in 0..n {
        h.step(&idle);
    }
}

fn air_at(h: &Harness, f: FighterId) -> Option<mvs_sim::Real> {
    let b = buff_of(&h.w, f, name!("Buff_AirSuperiority_C"))?;
    match &buff(&h.w, b)?.ext {
        BuffExt::AirSuperiority(AirExt { at }) => Some(*at),
        _ => None,
    }
}

#[test]
fn safety_net_gives_an_ally_air_superiority_once_per_cooldown() {
    let Some(mut h) = harness::at("jake-jake", AT) else { return };
    let n = h.w.players.len();
    let owner = (0..n).map(|i| h.fighter(i)).find(|f| buff_of(&h.w, *f, name!("Buff_Jake_SafetyNet_C")).is_some());
    let Some(owner) = owner else {
        eprintln!("note: no SafetyNet owner");
        return;
    };
    let team = h.w.fighter(owner).team.team;
    let ally = (0..n).map(|i| h.fighter(i)).find(|f| *f != owner && h.w.fighter(*f).team.team == team);
    let Some(ally) = ally else {
        eprintln!("note: no ally");
        return;
    };
    let b = buff_of(&h.w, owner, name!("Buff_Jake_SafetyNet_C")).expect("the perk");
    let _ = ActorRef::Fighter(owner);
    add_air_superiority_server(&mut h.w, owner, b, ally);
    let first = air_at(&h, ally).expect("the ally has the buff and its timer");
    let stamps = |h: &Harness| match &buff(&h.w, b).expect("buff").ext {
        BuffExt::JakeSafetyNet(SafetyNetExt { allies }) => allies.iter().map(|a| a.t).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let t0 = stamps(&h);
    assert_eq!(t0.len(), 1);
    // inside the cooldown (14 s): no second stamp, no second spawn
    tick(&mut h, 30);
    add_air_superiority_server(&mut h.w, owner, b, ally);
    assert_eq!(stamps(&h), t0);
    // the looping timer re-arms after it fires
    tick(&mut h, 200);
    let later = air_at(&h, ally).expect("the buff lives its 8 s");
    assert!(later > first);
}
