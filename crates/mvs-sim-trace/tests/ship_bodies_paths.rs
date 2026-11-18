//! Harley's bomb against Morty's ship (`chars/harley/bomb.rs` fly, `chars/morty/ship.rs` ship_statics), which no
//! match runs: on a fresh World of Harley and Morty, Harley's HarleyQuinn_Bomb_ThrowU_Montage throws a bomb, Morty's
//! up special (C019_Attack_Special_U_Montage) brings the ship; the bomb is set falling just above a live piece, and its
//! step must bounce it off the piece as off a kinematic box rising at the ship's speed vk (the notify's 1000 u/s):
//! vz' = vk + 0.65 (vk - vz*) (vz*: after the step's gravity and damping), the sideways speed x 0.7147 (the build's
//! PhysX DLL with a kinematic box). Rick's PolymorphGrenade (the welded body of `actors/smoke_bomb.rs`
//! grenade_parts) meets every piece the same way. Without a store it checks nothing.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ShipId};
use mvs_sim::chars::harley::bomb::Bomb;
use mvs_sim::chars::rick::grenade::PolymorphGrenade;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn pair(a: &str) -> Option<Harness> {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player(a, 0), player("morty", 1)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..20 {
        h.step(&NONE);
    }
    Some(h)
}

fn bomb(w: &World) -> Option<ActorId> {
    w.actors.actors.iter().find(|(_, a)| matches!(a.state, WorldActorState::HarleyBomb(_)) && !a.dead).map(|(id, _)| id)
}

fn bomb_mut(w: &mut World, id: ActorId) -> &mut Bomb {
    match &mut w.actors.actors.get_mut(id).expect("a live bomb").state {
        WorldActorState::HarleyBomb(b) => b,
        _ => unreachable!(),
    }
}

/// The World with a bomb in free flight and a live ship, else `None` (no store).
fn bomb_and_ship() -> Option<(Harness, ActorId, ShipId)> {
    let mut h = pair("harley")?;
    let (harley, morty) = (h.fighter(0), h.fighter(1));
    let m = h.w.lit("HarleyQuinn_Bomb_ThrowU_Montage");
    start_montage(&mut h.w, harley, m, &PROXY_NONE, None);
    let mut id = None;
    for _ in 0..30 {
        h.step(&NONE);
        id = bomb(&h.w);
        if id.is_some() {
            break;
        }
    }
    let id = id.expect("Harley's throw spawns a HarleyBomb");
    let m = h.w.lit("C019_Attack_Special_U_Montage");
    start_montage(&mut h.w, morty, m, &PROXY_NONE, None);
    let mut sh = None;
    for _ in 0..40 {
        h.step(&NONE);
        sh = h.w.actors.ships.iter().find(|(_, s)| !s.dead && s.age > 1).map(|(id, _)| id);
        if sh.is_some() {
            break;
        }
    }
    let sh = sh.expect("Morty's up special spawns C019_Ship");
    assert!(h.w.actors.actors.get(id).is_some_and(|a| !a.dead), "the bomb lives on");
    Some((h, id, sh))
}

/// The bomb set at `gap` over the top of the ship's piece farthest from both fighters (where it is now: the place
/// the next step starts from), with velocity (vx, vy).
fn set_over_piece(h: &mut Harness, id: ActorId, sh: ShipId, gap: f32, vx: f32, vy: f32) -> (f32, f32) {
    let fx: Vec<f32> = h.w.players.iter().map(|&f| h.w.fighter(f).body.x).collect();
    let s = h.w.actors.ships.get(sh).expect("a live ship");
    let pc = s
        .pieces
        .iter()
        .filter(|pc| pc.on && pc.nav)
        .max_by(|a, b| {
            let d = |dx: f32| fx.iter().map(|x| (s.x + dx - x).abs()).fold(f32::INFINITY, f32::min);
            d(a.dx).total_cmp(&d(b.dx))
        })
        .expect("a piece with its collision on");
    let (px, top) = (s.x + pc.dx, s.y + pc.dy + pc.hh);
    let b = bomb_mut(&mut h.w, id);
    assert_eq!(b.state, 0, "the bomb is in free flight");
    assert!(!b.no_collision);
    let y = top + b.radius + gap;
    (b.x, b.y, b.vx, b.vy, b.pvx, b.pvy, b.spin) = (px, y, vx, vy, 0.0, 0.0, 0.0);
    // the -50 timer off for the step under test
    b.grav_at = 1e9;
    (px, top)
}

#[test]
fn a_falling_bomb_bounces_off_a_rising_ship_piece() {
    let Some((mut h, id, sh)) = bomb_and_ship() else { return };
    for vz in [-500.0f32, -1025.0, -2325.0] {
        set_over_piece(&mut h, id, sh, 0.3, 598.0, vz);
        let vk = h.w.actors.ships.get(sh).expect("a live ship").vy;
        assert_eq!(vk, 1000.0, "the notify's InitialSpeed");
        h.step(&NONE);
        let b = bomb_mut(&mut h.w, id);
        let dt = mvs_sim::consts::DELTA_SECONDS;
        let damp = 1.0 - dt * 0.2;
        let vz_star = (vz + dt * -980.0) * damp;
        let want = vk + 0.65 * (vk - vz_star);
        assert!((b.vy - want).abs() < 0.05, "vz {vz}: after {} want {want}", b.vy);
        let vt = 598.0 * damp * 0.7147;
        assert!((b.vx - vt).abs() < 0.5, "vz {vz}: sideways {} want {vt}", b.vx);
    }
}

#[test]
fn a_rising_bomb_passes_a_ship_piece() {
    let Some((mut h, id, sh)) = bomb_and_ship() else { return };
    // inside the piece's top, rising: the Platform channel is Ignore, the piece does not push it
    set_over_piece(&mut h, id, sh, -10.0, 0.0, 100.0);
    h.step(&NONE);
    let b = bomb_mut(&mut h.w, id);
    let dt = mvs_sim::consts::DELTA_SECONDS;
    let want = (100.0 + dt * -980.0) * (1.0 - dt * 0.2);
    assert!((b.vy - want).abs() < 1e-3, "after {} want {want}", b.vy);
}

fn grenade(w: &World) -> Option<ActorId> {
    w.actors
        .actors
        .iter()
        .find(|(_, a)| matches!(a.state, WorldActorState::PolymorphGrenade(_)) && !a.dead)
        .map(|(id, _)| id)
}

fn grenade_mut(w: &mut World, id: ActorId) -> &mut PolymorphGrenade {
    match &mut w.actors.actors.get_mut(id).expect("a live grenade").state {
        WorldActorState::PolymorphGrenade(g) => g,
        _ => unreachable!(),
    }
}

#[test]
fn a_falling_polymorph_grenade_is_thrown_up_by_each_ship_piece() {
    let Some(mut h) = pair("rick") else { return };
    let (rick, morty) = (h.fighter(0), h.fighter(1));
    let m = h.w.lit("C020_Attack_PolymorphGun_Montage");
    start_montage(&mut h.w, rick, m, &PROXY_NONE, None);
    let mut id = None;
    for _ in 0..40 {
        h.step(&NONE);
        id = grenade(&h.w);
        if id.is_some() {
            break;
        }
    }
    let id = id.expect("Rick's polymorph gun spawns a PolymorphGrenade");
    let m = h.w.lit("C019_Attack_Special_U_Montage");
    start_montage(&mut h.w, morty, m, &PROXY_NONE, None);
    let mut sh = None;
    for _ in 0..40 {
        h.step(&NONE);
        sh =
            h.w.actors
                .ships
                .iter()
                .find(|(_, s)| !s.dead && s.age > 1 && s.pieces.iter().all(|pc| pc.on))
                .map(|(id, _)| id);
        if sh.is_some() {
            break;
        }
    }
    let sh = sh.expect("Morty's up special spawns C019_Ship, its six pieces on (clear of the stage)");
    // the fighters moved off the ship (the grenade's overlap with one is not under test)
    let (sx, vk) = h.w.actors.ships.get(sh).map(|s| (s.x, s.vy)).expect("a live ship");
    for f in [rick, morty] {
        let b = &mut h.w.fighter_mut(f).body;
        b.x = sx - 1200.0;
        b.prev_x = b.x;
    }
    let s = h.w.actors.ships.get(sh).expect("a live ship");
    let pieces: Vec<(String, f32, f32)> = s
        .pieces
        .iter()
        .filter(|pc| pc.on)
        .map(|pc| (h.w.names.str(pc.name).to_owned(), pc.dx, pc.dy + pc.hh))
        .collect();
    assert_eq!(pieces.len(), 6, "the six pieces on");
    for (name, dx, top) in pieces {
        assert!(h.w.actors.actors.get(id).is_some_and(|a| !a.dead), "the grenade lives on");
        let s = h.w.actors.ships.get(sh).expect("a live ship");
        let (px, py) = (s.x + dx, s.y + top);
        let g = grenade_mut(&mut h.w, id);
        let y = py + g.radius + 0.3;
        (g.x, g.y, g.vx, g.vy, g.pvx, g.pvy, g.spin, g.angle) = (px, y, 0.0, -500.0, 0.0, 0.0, 0.0, 0.0);
        (g.wa, g.wb, g.wc) = (0.0, 0.0, 0.0);
        h.step(&NONE);
        let g = grenade_mut(&mut h.w, id);
        assert!(g.vy > vk, "{name}: the grenade leaves the rising piece faster than it ({} against {vk})", g.vy);
    }
}
