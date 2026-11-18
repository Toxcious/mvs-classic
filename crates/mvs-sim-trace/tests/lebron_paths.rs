//! LeBron paths no trace match runs: C016_NoBall_Attack_Normal_Forward_Montage's 0.138-0.389 s window, whose row of
//! no button (CurrentAttackHitCondition) is C016_NoBall_BallReturn_Montage, with the attack's hit and nothing pressed:
//! the window's end starts it, and so does the montage's stop by its OffGroundStopAnim inside the window.

use mvs_sim::fighter::set_grounded;
use mvs_sim::input::NO_INPUT;
use mvs_sim_trace::harness;

/// lebron-shaggy frame 1295: LeBron (fighter 0) on the ground at 0.2167 s of the forward attack, inside the window.
const MATCH: &str = "lebron-shaggy";
const FRAME: usize = 1295;

fn montage_of(h: &harness::Harness, f: mvs_sim::fighter::FighterId) -> String {
    h.w.fighter(f).montages.montage.as_ref().map_or_else(String::new, |m| h.w.names.str(m.montage.name).to_owned())
}

/// The World at FRAME with the attack's hit and no stored press; the montages of the next `n` steps with no input.
fn run(lift: bool, n: usize) -> Option<Vec<String>> {
    let mut h = harness::at_baseline(MATCH, FRAME)?;
    let f = h.fighter(0);
    assert_eq!(montage_of(&h, f), "C016_NoBall_Attack_Normal_Forward_Montage");
    h.w.fighter_mut(f).montages.montage_hit = true;
    h.w.fighter_mut(f).branching.queued = None;
    if lift {
        // off the floor, above it: Falling from the next CMC on, so the montage's OffGroundStopAnim stops it
        h.w.fighter_mut(f).body.y += 300.0;
        set_grounded(&mut h.w, f, false);
    }
    let mut seen = Vec::new();
    for _ in 0..n {
        h.step(&[NO_INPUT, NO_INPUT]);
        seen.push(montage_of(&h, f));
    }
    Some(seen)
}

#[test]
fn the_window_end_starts_the_ball_return() {
    let Some(seen) = run(false, 14) else { return };
    assert!(seen.iter().any(|m| m == "C016_NoBall_BallReturn_Montage"), "the ball return starts: {seen:?}");
}

#[test]
fn a_stop_off_the_ground_in_the_window_starts_the_ball_return() {
    let Some(seen) = run(true, 6) else { return };
    assert!(seen.iter().any(|m| m == "C016_NoBall_BallReturn_Montage"), "the ball return starts: {seen:?}");
}

// ---- Buff_C016_DistanceShot's explosion (no trace match has a ball's hit with the perk at its distance)

/// lebron-combat frame 220: LeBron's shot ball in flight, carrying Buff_C016_DistanceShot.
const SHOT_MATCH: &str = "lebron-combat";
const SHOT_FRAME: usize = 220;

/// The World at SHOT_FRAME with Shaggy (fighter 1) at the ball and LeBron 3000 away, then the ball's
/// 'HitEnemyPlayerFirst' with Shaggy; `invulnerable` makes Shaggy invulnerable first.
fn explode(invulnerable: bool) -> Option<(harness::Harness, mvs_sim::Real, i32)> {
    use mvs_sim::actors::ActorRef;
    use mvs_sim::registry::signals::{ActorBuffArg, ActorBuffSignal};
    let mut h = harness::at_baseline(SHOT_MATCH, SHOT_FRAME)?;
    let (lebron, vic) = (h.fighter(0), h.fighter(1));
    let (ball, bx, by) = {
        let (id, p) = h.w.actors.projectiles.iter().next().expect("the shot ball");
        (id, p.x, p.y)
    };
    let cls = h.w.names.str(h.w.actors.projectiles.get(ball).unwrap().cls).to_owned();
    assert_eq!(cls, "BasketBallProjectile_Shot_C");
    h.w.fighter_mut(vic).body.x = bx;
    h.w.fighter_mut(vic).body.y = by;
    h.w.fighter_mut(lebron).body.x = bx + 3000.0;
    if invulnerable {
        h.w.fighter_mut(vic).invuln.invuln_count = 1;
    }
    let before = (h.w.fighter(vic).stats.damage, h.w.fighter(vic).stats.attacks_dodged);
    h.w.events.clear();
    h.w.fighter_mut(vic).events.clear();
    let arg = ActorBuffArg { by: vic, kx: 0.0, ky: 0.0 };
    mvs_sim::combat::elem::actor_buff_signal(
        &mut h.w,
        ActorRef::Projectile(ball),
        ActorBuffSignal::HitEnemyPlayerFirst,
        Some(&arg),
    );
    assert!(h.w.actors.projectiles.get(ball).is_none_or(|p| p.dead), "the explosion destroys the ball");
    Some((h, before.0, before.1))
}

#[test]
fn the_distance_shot_explosion_hits_the_victim() {
    let Some((h, damage, _)) = explode(false) else { return };
    let vic = h.fighter(1);
    let id = h.w.fighter(vic).id;
    let hit = h.w.events.iter().any(|e| matches!(e, mvs_sim::events::Event::Hit { victim, .. } if *victim == id));
    assert!(hit, "a hit event on the victim");
    assert!(h.w.fighter(vic).stats.damage > damage, "the victim takes damage");
    assert!(h.w.fighter(vic).hit.hitstun > 0.0 || h.w.fighter(vic).hit.hitpause > 0.0, "the victim is launched");
}

#[test]
fn the_distance_shot_explosion_over_an_invulnerable_victim_is_a_dodge() {
    let Some((h, damage, dodged)) = explode(true) else { return };
    let vic = h.fighter(1);
    let id = h.w.fighter(vic).id;
    assert!(!h.w.events.iter().any(|e| matches!(e, mvs_sim::events::Event::Hit { .. })), "no hit");
    assert_eq!(h.w.fighter(vic).stats.damage, damage);
    assert_eq!(h.w.fighter(vic).stats.attacks_dodged, dodged + 1, "the dodge counts");
    assert!(h.w.fighter(vic).evade.dodged_successfully);
    let fx =
        h.w.fighter(vic)
            .events
            .iter()
            .any(|e| matches!(e, mvs_sim::events::Event::EvadeFx { fighter } if *fighter == id));
    assert!(fx, "the dodge burst");
}
