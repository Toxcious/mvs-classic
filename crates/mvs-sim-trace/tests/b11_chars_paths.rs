//! Character paths no trace match runs: Bugs' rocket ride started on the ground, Superman's dash montages, Morty's
//! marker burst over a team mate.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn fresh(a: &str, b: &str) -> Option<Harness> {
    let setup = MatchSetup { players: vec![player(a, false), player(b, true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    Some(h)
}

/// Bugs' air rocket ride started by hand on a grounded Bugs (the oracle probe's NaN place at its third tick): at his
/// landed place and at x 0, 700 up with the floor flag kept, his place stays a number through the ride.
#[test]
fn rocket_ride_from_the_ground_keeps_a_place() {
    for (x, y) in [(None, None), (Some(0.0), Some(700.0))] {
        let Some(mut h) = fresh("bugs", "shaggy") else { return };
        let f = h.fighter(0);
        let b = &mut h.w.fighter_mut(f).body;
        if let (Some(x), Some(y)) = (x, y) {
            (b.x, b.prev_x, b.y, b.prev_y) = (x, x, y, y);
        }
        assert!(b.on_floor, "Bugs starts on the floor");
        let m = h.w.lit("BugsBunny_Attack_Air_RocketRideF_Montage");
        start_montage(&mut h.w, f, m, &PROXY_NONE, None);
        for t in 0..60 {
            h.step(&NONE);
            let b = &h.w.fighter(f).body;
            assert!(!b.x.is_nan() && !b.y.is_nan(), "a NaN place at tick {t}");
        }
    }
}

/// The montage of Superman's dash montage slot, if any.
fn dash_montage(h: &Harness, f: mvs_sim::fighter::FighterId) -> Option<String> {
    h.w.fighter(f).montages.dash_montage.as_ref().map(|m| h.w.names.str(m.montage.name).to_owned())
}

/// Superman's dash and dash turn on the ground start his DashAnim / DashTurnAnim (Pawn_SuperMan: Superman_Dash_Montage,
/// Superman_Dash_Turn_Montage) through HandleDashAnim.
#[test]
fn superman_dash_and_dash_turn_start_their_montages() {
    let Some(mut h) = fresh("superman", "shaggy") else { return };
    let f = h.fighter(0);
    let dir = h.w.fighter(f).body.facing_dir;
    let stick = |x: mvs_sim::Real| [InputState { x, ..NO_INPUT }, NO_INPUT];
    h.step(&stick(dir));
    assert_eq!(dash_montage(&h, f).as_deref(), Some("Superman_Dash_Montage"), "the dash");
    for _ in 0..90 {
        h.step(&NONE);
    }
    h.step(&stick(-dir));
    assert_eq!(dash_montage(&h, f).as_deref(), Some("Superman_Dash_Turn_Montage"), "the dash turn");
}

/// Morty's down special marker ends its recall with the burst over a team mate beside him: the ally rows and
/// NotifyAllyOfHit (a Marker dealer), no damage.
#[test]
fn the_marker_burst_passes_over_a_team_mate() {
    let team = |c: &str, t: u8| MatchPlayer { character: Some(c.to_owned()), team: Some(t), ..MatchPlayer::default() };
    let setup =
        MatchSetup { players: vec![team("morty", 0), team("shaggy", 0), team("shaggy", 1)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let none = [NO_INPUT; 3];
    for _ in 0..10 {
        h.step(&none);
    }
    let (m, mate) = (h.fighter(0), h.fighter(1));
    let down = h.w.lit("C019_Special_Down_Montage");
    start_montage(&mut h.w, m, down, &PROXY_NONE, None);
    let mut seen = false;
    for _ in 0..900 {
        let (x, y) = (h.w.fighter(m).body.x, h.w.fighter(m).body.y);
        let b = &mut h.w.fighter_mut(mate).body;
        (b.x, b.prev_x, b.y, b.prev_y) = (x, x, y, y);
        h.step(&none);
        let live = !h.w.actors.markers.is_empty();
        let burst = h.w.events.iter().any(|e| matches!(e, mvs_sim::events::Event::MarkerBurst { .. }));
        if seen && !live {
            assert!(burst, "the marker ended with its burst");
            assert_eq!(h.w.fighter(mate).stats.damage, 0.0, "the team mate takes no damage");
            return;
        }
        seen |= live;
    }
    panic!("the marker never came and went (seen {seen})");
}

/// Harley's air down glove falling straight onto the floor: PS_HarleyNADAIR_Ricochet's roll is DegAtan2 of the
/// reflected direction, straight up: 0 (the oracle's pitch of the arriving velocity gave 180).
#[test]
fn the_glove_ricochet_off_a_floor_has_roll_zero() {
    let Some(mut h) = fresh("harley", "shaggy") else { return };
    let f = h.fighter(0);
    let cls = h.w.lit("HarleyDownAirProjectile_C");
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    let id = mvs_sim::world::spawning::comp_spawn_projectile(
        &mut h.w,
        f,
        cls,
        x,
        y + 120.0,
        [Some(0.0), Some(0.0), Some(-1.0)],
        1200.0,
        1200.0,
        None,
    )
    .expect("the glove spawns");
    let ricochet = h.w.lit("PS_HarleyNADAIR_Ricochet");
    h.w.events.clear();
    for _ in 0..16 {
        h.step(&NONE);
        let rot = h.w.events.iter().find_map(|e| match e {
            mvs_sim::events::Event::ProjectileDeath { vfx: Some(v), rot, .. } if *v == ricochet => Some(*rot),
            _ => None,
        });
        if let Some(rot) = rot {
            assert_eq!(rot, [0.0, 0.0, 0.0], "pitch 0, yaw 0, roll 0");
            return;
        }
        if h.w.actors.projectiles.get(id).is_none_or(|p| p.dead) {
            break;
        }
    }
    panic!("the glove never bounced");
}
