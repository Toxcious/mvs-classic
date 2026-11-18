//! Throne Room: a walker on a side elevator on its way down walks into the side of the central Stage block
//! (World.ground) (`fighter/body.rs` `walk_out_of_ground`): no trace match walks there. On the World of stage-m008 at the
//! first baseline frame where the left side has come down by a given depth, fighter 0 is set on it and walks right.
//! Without a store or the trace the tests check nothing and say so.

use mvs_sim::fighter::PlatformRef;
use mvs_sim::fighter::body;
use mvs_sim_trace::harness;

/// What the walker did each tick: (x, y, on floor, base).
type Walk = Vec<(f32, f32, bool, Option<PlatformRef>)>;

/// The left side elevator's walking box.
fn left_side(w: &mvs_sim::World) -> PlatformRef {
    let all = w.data.stage.solids.as_deref().unwrap_or(&[]);
    let solid = all.iter().position(|s| s.name == "ThroneRoomLeftElevator_7.Box").expect("the left side's Box");
    let run = w.stage_run.as_ref().expect("the stage's run");
    let bx = run.boxes.iter().position(|b| b.solid as usize == solid).expect("its walking box");
    PlatformRef::Solid(u32::try_from(bx).expect("a box index"))
}

/// Fighter 0 set standing on the left side elevator at x, at the first frame where the side's top is `down` (a range)
/// under the block's, then `ticks` steps holding right; the other fighter out of the way. Returns the walk, the
/// elevator's box, the block's left side and the World.
fn walk_right(down: std::ops::Range<f32>, x0: f32, ticks: usize) -> Option<(Walk, PlatformRef, f32, harness::Harness)> {
    let under = |w: &mvs_sim::World| mvs_sim::math::narrow(w.stage.ground.top) - body::plat(w, left_side(w)).top;
    let mut h = harness::at_baseline_where("stage-m008", 0, |w| down.contains(&under(w)))?;
    let side = left_side(&h.w);
    let (f, other) = (h.fighter(0), h.fighter(1));
    h.w.fighter_mut(other).body.x = 3000.0;
    {
        let fi = h.w.fighter_mut(f);
        fi.body.x = x0;
        fi.body.vx = 0.0;
        fi.body.vy = 0.0;
        fi.platforms.base = Some(side);
    }
    body::set_grounded(&mut h.w, f, true);
    let sy = body::stand_y_of(&h.w, f);
    h.w.fighter_mut(f).body.y = sy;
    let left = mvs_sim::math::narrow(h.w.stage.ground.left);
    let mut walk = Vec::new();
    for _ in 0..ticks {
        let mut inputs = vec![mvs_sim::InputState::default(); h.w.players.len()];
        inputs[0].x = 1.0;
        h.step(&inputs);
        let fi = h.w.fighter(f);
        walk.push((fi.body.x, fi.body.y, fi.body.on_floor, fi.platforms.base));
    }
    Some((walk, side, left, h))
}

#[test]
fn the_block_side_stops_a_walker_on_a_lowered_side() {
    // the side top about 290 under the block's (16): no step
    let Some((walk, side, left, h)) = walk_right(250.0..330.0, -1900.0, 60) else { return };
    let r = body::cap_radius(&h.w, h.fighter(0));
    for (t, &(x, _, floor, base)) in walk.iter().enumerate() {
        assert!(floor && base == Some(side), "tick {t}: still on the side ({floor}, {base:?})");
        assert!(x <= left - r + 0.2, "tick {t}: x {x} past the block's side {}", left - r);
    }
    let (x, ..) = *walk.last().expect("a walk");
    assert!((x - (left - r)).abs() < 0.2, "held at the side ({x} against {})", left - r);
}

#[test]
fn a_walker_steps_onto_the_block_from_a_side_just_down() {
    // the side top about 50 under the block's: StepUp onto it
    let Some((walk, _, _, h)) = walk_right(30.0..70.0, -1800.0, 30) else { return };
    let f = h.fighter(0);
    assert!(walk.iter().all(|w| w.2), "never off the floor");
    let &(_, y, _, base) = walk.last().expect("a walk");
    assert_eq!(base, None, "the block is his floor");
    assert_eq!(y, body::stand_y_of(&h.w, f), "standing on the block");
}
