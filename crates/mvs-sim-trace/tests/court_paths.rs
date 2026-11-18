//! The Court's paths no match runs (`stages/the_court/`), on the World of stage-m010 at a baseline frame: a ball in a
//! hoop (the score, the explosion on the other team), the ball's respawn and a wall's break and respawn.
//! Without a store or the trace the tests check nothing and say so.

use mvs_sim::actors::ActorRef;
use mvs_sim::stages::the_court::{own, slot_of, wall::wall};
use mvs_sim_trace::harness::{self, Harness};

fn court() -> Option<Harness> {
    harness::at("stage-m010", 100)
}

fn step(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step_recorded();
    }
}

/// The centre of a solid of the stage data by its component name, in sim axes (x = -UE Y, y = UE Z).
fn centre(h: &Harness, comp: &str, owner_x: f64) -> (f64, f64) {
    let suffix = format!(".{comp}");
    let s =
        h.w.data
            .stage
            .solids
            .iter()
            .flatten()
            .filter(|s| s.name.ends_with(&suffix))
            .min_by(|a, b| (-a.center[1] - owner_x).abs().total_cmp(&(-b.center[1] - owner_x).abs()))
            .expect("the hoop's solid");
    (-s.center[1], s.center[2])
}

#[test]
fn a_free_ball_in_a_hoop_scores_and_explodes() {
    let Some(mut h) = court() else { return };
    let hoop_x = 2700.0;
    let (bx, by) = centre(&h, "HoopHitCollision", hoop_x);
    let (bx, by) = (mvs_sim::math::narrow(bx), mvs_sim::math::narrow(by));
    let ball = h.w.lit("Item_Basketball_C");
    let it = mvs_sim::world::items::spawn_stage_item(&mut h.w, ball, bx, by).expect("the ball");
    let seq = h.w.actors.items.get(it).expect("the ball").seq;
    let team_hit = h.fighter(1);
    {
        let b = &mut h.w.fighter_mut(team_hit).body;
        b.x = bx;
        b.prev_x = bx;
    }
    let before = h.w.fighter(team_hit).stats.damage;
    step(&mut h, 3);
    let by_hoop = own(&h.w).score.by_hoop.clone();
    assert_eq!(by_hoop.iter().sum::<i32>(), 3, "a ball no one threw is scored from x 0: three points ({by_hoop:?})");
    assert!(h.w.actors.items.iter().all(|(_, i)| i.seq != seq || i.dead), "the scoring ball is destroyed");
    assert!(own(&h.w).score.scored_at >= 0);
    // the explosion hits the other team (no holder: team 0), the fighter standing in it
    let after = h.w.fighter(team_hit).stats.damage;
    assert!(after > before, "the hoop's explosion hit him ({before} -> {after})");
    let _ = ActorRef::Stage(0);
}

#[test]
fn a_wall_breaks_and_comes_back() {
    let Some(mut h) = court() else { return };
    let slot = slot_of(&h.w, 2).expect("a wall");
    let any = h.w.registry.stage_actors.get("m010_wall_C").and_then(|k| k.any_damage).expect("its damage hook");
    let start = wall(&h.w, slot).uy;
    any(&mut h.w, slot, 31.0, ActorRef::Stage(slot));
    assert!(wall(&h.w, slot).hp <= 0.0 && !wall(&h.w, slot).on && !wall(&h.w, slot).edges);
    assert!(wall(&h.w, slot).broken_at >= 0);
    step(&mut h, 200);
    assert!(!wall(&h.w, slot).on, "five seconds not yet over");
    assert!(wall(&h.w, slot).dissolve > 0.0, "the dissolve runs");
    step(&mut h, 120);
    let m = wall(&h.w, slot);
    assert!(m.on && m.hp == m.max_hp && m.respawn_at >= 0, "the wall is back");
    step(&mut h, 80);
    let m = wall(&h.w, slot);
    assert!(!m.move_on && m.uy == start && m.glow, "and has moved to its place");
}

#[test]
fn a_destroyed_ball_is_spawned_again_after_five_seconds() {
    let Some(mut h) = harness::at("stage-m010", 1600) else { return };
    let first = own(&h.w).script.ball.item;
    assert!(first >= 0, "the chain's first ball is out");
    let id = h.w.actors.items.iter().find(|(_, i)| i.seq == first).map(|(id, _)| id).expect("the ball");
    mvs_sim::world::items::destroy_item(&mut h.w, id);
    assert_eq!(own(&h.w).script.ball.item, -1);
    step(&mut h, 290);
    assert_eq!(own(&h.w).script.ball.item, -1, "not yet");
    step(&mut h, 20);
    let next = own(&h.w).script.ball.item;
    assert!(next > first, "the next ball ({first} -> {next})");
}
