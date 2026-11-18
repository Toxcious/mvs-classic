//! Bugs Bunny's safe, the paths no trace match runs (`chars/bugs/safe.rs`): a safe dropped on the stage floor lands and
//! comes to rest on its body and its hitbox goes off.

use mvs_sim::actors::ProjectileId;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::chars::bugs::safe::BugsBunnySafe;
use mvs_sim::input::NO_INPUT;
use mvs_sim::name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness;

const NONE: [mvs_sim::input::InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn state(
    h: &harness::Harness,
    id: ProjectileId,
) -> Option<(mvs_sim::Real, mvs_sim::Real, bool, bool, bool, mvs_sim::Real)> {
    let p = h.w.actors.projectiles.get(id)?;
    let ProjectileExt::BugsBunnySafe(s): &ProjectileExt = &p.ext else { panic!("a safe: {:?}", p.ext) };
    let s: &BugsBunnySafe = s;
    Some((p.x, p.y, s.landed, s.sim, s.box_on, s.hp))
}

/// Bugs spawns the safe high above the floor, straight down at 5000: it lands, simulates, turns its hitbox off and
/// stays at rest on the floor.
#[test]
fn the_safe_lands_and_rests() {
    let setup = MatchSetup { players: vec![player("bugs", false), player("shaggy", true)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..13 {
        h.step(&NONE);
    }
    let bugs = h.fighter(0);
    let id = comp_spawn_projectile(
        &mut h.w,
        bugs,
        name!("BugsBunnySafe_C"),
        0.0,
        1500.0,
        [Some(0.0), Some(0.0), Some(-1.0)],
        5000.0,
        5000.0,
        None,
    )
    .expect("the safe");
    let (_, _, landed, sim, on, hp) = state(&h, id).expect("alive");
    assert!(!landed && !sim && on);
    assert_eq!(hp, 14.0);
    for _ in 0..90 {
        h.step(&NONE);
    }
    let (_, y, landed, sim, on, _) = state(&h, id).expect("alive");
    assert!(landed && sim, "landed {landed}, simulating {sim}");
    assert!(!on, "the hitbox is off at rest");
    for _ in 0..30 {
        h.step(&NONE);
    }
    let (_, y2, ..) = state(&h, id).expect("alive");
    assert_eq!(y, y2, "at rest");
    assert!(y < 1500.0);
}
