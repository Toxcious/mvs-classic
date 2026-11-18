//! Finn's side special and gem paths that no match runs (`chars/finn/aimer.rs`, `gem.rs`), on a fresh World of Finn
//! against Shaggy: BPI_Finn_Dash_Aim's vectors, the aimer's lock on a fighter in front of him, and its pick of a gem
//! (the teleport, the gem destroyed). Without a store the tests check nothing and say so.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::finn::aimer::dash_aim;
use mvs_sim::chars::finn::gem::spawn_gem;
use mvs_sim::chars::finn::state::finn_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::actor_y;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player("finn", false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (f, s) = (h.fighter(0), h.fighter(1));
    Some((h, f, s))
}

fn place(h: &mut Harness, f: FighterId, x: f32, y: f32) {
    let b = &mut h.w.fighter_mut(f).body;
    b.x = x;
    b.prev_x = x;
    b.y = y;
    b.prev_y = y;
}

/// Finn at the origin facing right, Shaggy `dx` along and `dy` up from him.
fn lined(dx: f32, dy: f32) -> Option<(Harness, FighterId, FighterId)> {
    let (mut h, f, s) = pair()?;
    let (fx, fy) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    h.w.fighter_mut(f).body.facing_dir = 1.0;
    place(&mut h, s, fx + dx, fy + dy);
    Some((h, f, s))
}

/// BPI_Finn_Dash_Aim: a fighter within 0.3 of the facing's dot is aimed along the ground (Z halved), above that at
/// 45 degrees, either way by its side; no target and a dead one give none.
#[test]
fn dash_aim_reads_the_target_fighters_direction() {
    let Some((mut h, f, s)) = lined(300.0, 0.0) else { return };
    let (x, z) = dash_aim(&mut h.w, f, Some(ActorRef::Fighter(s)), false).expect("a target in front");
    assert_eq!((x.to_bits(), z.to_bits()), (1.0_f32.to_bits(), (-0.0_f32).to_bits()), "level target: ({x}, {z})");

    let Some((mut h, f, s)) = lined(200.0, 200.0) else { return };
    let (x, z) = dash_aim(&mut h.w, f, Some(ActorRef::Fighter(s)), false).expect("a target in front");
    let r = std::f32::consts::FRAC_1_SQRT_2;
    // x is one float32 step above 1 / sqrt(2), z is half of 1 / sqrt(2)
    assert_eq!((x.to_bits(), z), (r.to_bits() + 1, r * 0.5), "target above in front");

    let Some((mut h, f, s)) = lined(20.0, 300.0) else { return };
    let (x, z) = dash_aim(&mut h.w, f, Some(ActorRef::Fighter(s)), false).expect("a target above");
    assert_eq!((x, z), (r, r), "target overhead");

    let Some((mut h, f, s)) = lined(-20.0, -300.0) else { return };
    let (x, z) = dash_aim(&mut h.w, f, Some(ActorRef::Fighter(s)), false).expect("a target below");
    assert_eq!((x, z), (r, -r), "target below");

    assert!(dash_aim(&mut h.w, f, None, false).is_none());
}

fn start_dash(h: &mut Harness, f: FighterId) {
    let m = h.w.lit("Finn_Ground_F_Special_01_Montage");
    start_montage(&mut h.w, f, m, &PROXY_NONE, None);
}

/// The aimer spawned by the montage locks Shaggy 250 in front of Finn (DashTarget), then ends with the notify.
#[test]
fn the_aimer_locks_a_fighter_in_front_and_ends() {
    let Some((mut h, f, s)) = lined(250.0, 0.0) else { return };
    start_dash(&mut h, f);
    let mut seen = false;
    let mut locked = false;
    for _ in 0..40 {
        h.step(&NONE);
        let fs = finn_of(&h.w, f).expect("a Finn");
        seen |= fs.aimer.is_some();
        locked |= fs.dash.target == Some(ActorRef::Fighter(s));
    }
    assert!(seen, "the montage's spawner notify made the aimer");
    assert!(locked, "the aimer took Shaggy as DashTarget");
    for _ in 0..60 {
        h.step(&NONE);
    }
    assert!(finn_of(&h.w, f).expect("a Finn").aimer.is_none(), "the aimer ended with its notify");
}

/// A gem of his own team in the arena: the aimer picks it, Finn is put at its place and the gem destroyed 0.075 s on.
#[test]
fn the_aimer_teleports_to_a_gem_and_destroys_it() {
    let Some((mut h, f, _s)) = lined(900.0, 0.0) else { return };
    let (fx, fy) = (h.w.fighter(f).body.x, actor_y(&h.w, f));
    let Some(g) = spawn_gem(&mut h.w, f, fx + 400.0, fy, (0.0, 0.0), 0.0, 0.0, false) else { return };
    start_dash(&mut h, f);
    let mut moved = false;
    for _ in 0..40 {
        h.step(&NONE);
        moved |= (h.w.fighter(f).body.x - fx).abs() > 100.0;
    }
    assert!(moved, "Finn was put at the gem's place");
    assert!(h.w.actors.items.get(g).is_none_or(|i| i.dead), "the gem was destroyed after the teleport");
}
