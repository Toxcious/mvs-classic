//! Bugs Bunny's tunnel paths that no match runs (`chars/bugs/tunnel.rs`, `actors/tunnel_carry.rs`), on a fresh World of
//! Bugs Bunny and a team mate: the dig (Buff_Tunneling) spawning its tunnel from the pawn's world tick, the jump that
//! ends it early spawning the second, and a crouching team mate teleported from one to the other (Buff_TunnelTeleport).
//! Without a store the tests check nothing and say so.
//! Not reached: the carry of a projectile or an item through the tunnel (`overlap_began`: the safe and the items are
//! other tracks' and no match shows a carried one), the Escapist perk, and the glide's slow fall on the stage's floor.

use mvs_sim::actors::WorldActorState;
use mvs_sim::chars::bugs::tunnel::{TeleportExt, tunnels_of};
use mvs_sim::combat::buffs::{BuffExt, buff_of, spawn_buff};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::registry::CharHooks;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, team: u8, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot, ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId, FighterId)> {
    let setup =
        MatchSetup { players: vec![player("bugs", 0, false), player("shaggy", 0, false)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (b, s) = (h.fighter(0), h.fighter(1));
    Some((h, b, s))
}

/// The dig spawns the first tunnel in the pawn's world tick; the jump handler (the OnJumpPressed binding, a char hook)
/// ends the dig and spawns the second, 12 s long. The first one lives 13.5 s.
fn dig(h: &mut Harness, b: FighterId) {
    let cls = h.w.lit("Buff_Tunneling_C");
    spawn_buff(&mut h.w, b, cls, 2.0, Some(0), Some(b), true, Some(b));
    for _ in 0..3 {
        h.step(&NONE);
    }
}

#[test]
fn the_dig_spawns_a_tunnel_and_the_jump_ends_it_with_a_second() {
    let Some((mut h, b, _)) = pair() else { return };
    dig(&mut h, b);
    let tunnels = tunnels_of(&h.w, b);
    assert_eq!(tunnels.len(), 1, "the dig's tunnel");
    let cls = h.w.lit("Buff_Tunneling_C");
    let buff = buff_of(&h.w, b, cls).expect("the dig is on");
    let BuffExt::Tunneling(e) = &h.w.fighter(b).buffs.buffs.iter().find(|x| x.serial == buff.serial).unwrap().ext
    else {
        panic!("the dig's extension");
    };
    assert_eq!(e.listener, Some(b), "the jump binding is made");
    // the OnJumpPressed handler
    let mut hooks = CharHooks::default();
    mvs_sim::chars::bugs::tunnel::hooks(&mut hooks);
    (hooks.jump_pressed.expect("the pawn's jump hook"))(&mut h.w, b);
    assert!(buff_of(&h.w, b, cls).is_none(), "the dig ends");
    assert_eq!(tunnels_of(&h.w, b).len(), 2, "the second tunnel");
}

#[test]
fn a_crouching_team_mate_in_a_tunnel_is_teleported_to_the_other() {
    let Some((mut h, b, s)) = pair() else { return };
    dig(&mut h, b);
    let mut hooks = CharHooks::default();
    mvs_sim::chars::bugs::tunnel::hooks(&mut hooks);
    // the jump ends the dig where he stands; the first tunnel is where the dig began
    let (bx, _) = (h.w.fighter(b).body.x, ());
    (hooks.jump_pressed.expect("the pawn's jump hook"))(&mut h.w, b);
    h.step(&NONE);
    let ids = tunnels_of(&h.w, b);
    assert_eq!(ids.len(), 2);
    let places: Vec<(mvs_sim::Real, mvs_sim::Real)> = ids
        .iter()
        .map(|&i| match &h.w.actors.actors.get(i).unwrap().state {
            WorldActorState::Tunnel(t) => (t.x, t.y),
            _ => unreachable!(),
        })
        .collect();
    // the team mate crouches on the first tunnel
    let tel = h.w.lit("Buff_TunnelTeleport_C");
    let mut teleported = None;
    for _ in 0..90 {
        let f = h.w.fighter_mut(s);
        f.body.x = places[0].0;
        f.body.prev_x = places[0].0;
        f.body.vx = 0.0;
        h.step(&[NO_INPUT, InputState { y: -1.0, down_btn: true, ..NO_INPUT }]);
        if let Some(r) = buff_of(&h.w, s, tel) {
            teleported = Some(r);
            break;
        }
    }
    let _ = bx;
    let r = teleported.expect("the crouching team mate is teleported");
    let BuffExt::TunnelTeleport(TeleportExt { dx, by, .. }) =
        &h.w.fighter(s).buffs.buffs.iter().find(|x| x.serial == r.serial).unwrap().ext
    else {
        panic!("the teleport's extension");
    };
    assert_eq!(*by, Some(b), "the instigator");
    assert_eq!(*dx, places[1].0, "the destination is the other tunnel");
}
