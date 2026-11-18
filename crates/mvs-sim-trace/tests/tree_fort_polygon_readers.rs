//! The polygon stage solids in the readers that met boxes alone, on the World of stage-treehouse at a baseline frame
//! (the right tree's round top, a polygon solid) and of stage-scooby (its polygon solids): a fighter's K2_TeleportTo
//! spot test (`world/spawn_spot.rs`), a hold's release check (`combat/holds.rs`), Banana Guard's walker
//! (`items/banana_guard.rs`) and the view's read of what blocks a trace (`World::blocker_shapes_read`, the seismic
//! beam's end). No match runs these on a polygon. Without a store or the trace the tests check nothing and say so.

use mvs_sim::actors::ActorId;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::fighter::body::PlatformRef;
use mvs_sim::fighter::{actor_y, cap_radius, capsule_half};
use mvs_sim::items::banana_guard::{BananaGuard, GuardMode, GuardSolid, spawn_at};
use mvs_sim::world::World;
use mvs_sim::world::spawned_platform::BlockKind;
use mvs_sim::world::stage_query::PolyBlock;
use mvs_sim_trace::harness;

const AT: usize = 1500;

/// The distance from an upright capsule's segment (centre (x, y), radius r, half height hh) to the polygon.
fn gap(q: &PolyBlock, x: f32, y: f32, r: f32, hh: f32) -> f32 {
    let s = (hh - r).max(0.0);
    q.seg_dist2(x, y - s, x, y + s).sqrt()
}

#[test]
fn a_fighter_teleported_into_a_tree_top_is_moved_out() {
    let Some(mut h) = harness::at("stage-treehouse", AT) else { return };
    let (q, mx, top) = right_top_of(&mut h.w);
    let f = h.fighter(0);
    let mut polys = Vec::new();
    mvs_sim::fighter::traces::solids::fighter_polys(&h.w, f, &mut polys);
    assert!(polys.iter().any(|p| p.solid == q.solid), "the top blocks the fighter's capsule");
    let (r, hh) = (cap_radius(&h.w, f), capsule_half(&h.w, f));
    // the capsule's bottom 10 into the top
    let y = top + hh - 10.0;
    assert!(gap(&q, mx, y, r, hh) < r - 5.0, "the spot asked overlaps the top");
    assert!(mvs_sim::world::spawn_spot::fighter_teleport_to(&mut h.w, f, mx, y), "a free spot is found");
    let (x, ay) = (h.w.fighter(f).body.x, actor_y(&h.w, f));
    assert!(x != mx || ay != y, "the fighter is moved from the spot asked");
    let d = gap(&q, x, ay, r, hh);
    assert!(d > r - 0.5, "out of the top ({d} against the radius {r})");
}

#[test]
fn a_hold_released_inside_a_tree_top_is_let_go_outside_it() {
    let Some(mut h) = harness::at("stage-treehouse", AT) else { return };
    let (q, mx, top) = right_top_of(&mut h.w);
    let (vic, by) = (h.fighter(1), h.fighter(0));
    let cls = h.w.lit("Buff_HeldByEnemy_C");
    // the holder over the top, the held fighter inside it under him
    let bh = capsule_half(&h.w, by);
    mvs_sim::fighter::moves::recall_move(&mut h.w, by, mx, top + bh + 200.0);
    mvs_sim::fighter::moves::recall_move(&mut h.w, vic, mx, top - 30.0);
    let (ax, ay) = (h.w.fighter(by).body.x, actor_y(&h.w, by));
    let g0 = gap(&q, ax, ay, 42.0, capsule_half(&h.w, by));
    assert!(
        g0 > 42.0,
        "the holder starts clear of the top ({g0} at {ax} {ay}, top {top} at {mx}, half {})",
        capsule_half(&h.w, by)
    );
    let vy0 = actor_y(&h.w, vic);
    assert!(q.inside(mx, vy0), "the held fighter starts inside the top");
    mvs_sim::combat::holds::hold_begin(&mut h.w, vic, cls, Some(by));
    assert!(h.w.fighter(vic).holds.hold.is_some(), "the hold begins");
    mvs_sim::combat::holds::hold_end(&mut h.w, vic, true);
    let (x, y) = (h.w.fighter(vic).body.x, actor_y(&h.w, vic));
    assert!(y > vy0, "the held fighter is let go up the line to the holder ({y} from {vy0})");
    assert!(!q.inside(x, y), "his centre is out of the top");
}

fn guard(w: &World, id: ActorId) -> &BananaGuard {
    match &w.actors.actors.get(id).expect("the guard").state {
        WorldActorState::BananaGuard(g) => g,
        _ => unreachable!(),
    }
}

/// A fresh World of stage-treehouse's header with Bugs (the guard's owner: its class data is his) as player 0,
/// stepped to tick 60 with no input.
fn tree_fort_with_bugs() -> Option<World> {
    let path = mvs_sim_trace::run::default_traces().join("stage-treehouse.trace");
    let Ok(t) = mvs_trace::Trace::read(&path) else {
        eprintln!("note: no stage-treehouse trace; nothing checked");
        return None;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return None;
    };
    let mut header = t.header.clone();
    header.setup["players"][0]["character"] = serde_json::Value::from("bugs");
    header.setup["players"][0]["perks"] = serde_json::Value::Array(Vec::new());
    let (mut w, _) = mvs_sim_trace::world_for(&header, &store).expect("the World");
    let inputs = vec![mvs_sim::InputState::default(); w.players.len()];
    for _ in 0..60 {
        w.step(&inputs);
    }
    Some(w)
}

/// The right tree's top of a World: the polygon, its middle x and its top there.
fn right_top_of(w: &mut World) -> (PolyBlock, f32, f32) {
    let s = w.blocker_shapes(BlockKind::Trace);
    let q = s.polys.into_iter().find(|q| q.bounds().left > 0.0).expect("the right tree's top");
    let g = q.bounds();
    let mx = mvs_sim::math::narrow((g.left + g.right) / 2.0);
    let top = q.top_at(mx);
    (q, mx, top)
}

/// The guard's CMC alone for a tick: the World stepped with no input.
fn step(w: &mut World) {
    let inputs = vec![mvs_sim::InputState::default(); w.players.len()];
    w.step(&inputs);
}

#[test]
fn a_banana_guard_lands_and_walks_on_a_tree_top() {
    let Some(mut w) = tree_fort_with_bugs() else { return };
    let (q, mx, top) = right_top_of(&mut w);
    let f = w.players[0];
    let cls = w.lit("ItemHelper_BananaGuard_C");
    let id = spawn_at(&mut w, f, cls, mx, top + 600.0);
    let g = guard(&w, id);
    assert_eq!(
        g.mode,
        GuardMode::Fall,
        "spawned in the air ({} {} half {} base {:?}, top {top})",
        g.x,
        g.y,
        g.half,
        g.base
    );
    let on_top = GuardSolid::Part(PlatformRef::Poly(q.solid));
    let mut landed = None;
    let mut walked = 0.0f32;
    for n in 0..150 {
        let x0 = guard(&w, id).x;
        step(&mut w);
        let Some(a) = w.actors.actors.get(id) else { break };
        if a.dead {
            break;
        }
        let g = guard(&w, id);
        let d = gap(&q, g.x, g.y, g.radius, g.half);
        assert!(d > g.radius - 0.5, "not into the top at {n} (gap {d})");
        if landed.is_none() && g.mode == GuardMode::Walk {
            landed = Some(n);
            assert_eq!(g.base, Some(on_top), "it lands on the top");
            let floor = g.y - g.half;
            assert!((floor - top).abs() < 5.0, "it stands on the top ({floor} against {top})");
        }
        if landed.is_some() && g.mode == GuardMode::Walk && g.base == Some(on_top) {
            walked += (g.x - x0).abs();
            // resting on it: its round bottom near the outline, not floating over it
            let d = gap(&q, g.x, g.y, g.radius, g.half);
            assert!(d < g.radius + 5.0, "on the top at {n} (gap {d})");
        }
    }
    assert!(landed.is_some(), "the guard lands on the top");
    assert!(walked > 10.0, "it runs along the top ({walked})");
}

#[test]
fn a_banana_guard_thrown_up_into_a_tree_top_stops_under_it() {
    let Some(mut w) = tree_fort_with_bugs() else { return };
    let (q, mx, _) = right_top_of(&mut w);
    let bottom = mvs_sim::math::narrow(q.bounds().bottom);
    let f = w.players[0];
    let cls = w.lit("ItemHelper_BananaGuard_C");
    let id = spawn_at(&mut w, f, cls, mx, bottom - 300.0);
    match &mut w.actors.actors.get_mut(id).expect("the guard").state {
        WorldActorState::BananaGuard(g) => {
            g.mode = GuardMode::Fall;
            g.base = None;
            g.vel_y = 3000.0;
        }
        _ => unreachable!(),
    }
    let mut stopped = false;
    for n in 0..10 {
        step(&mut w);
        let g = guard(&w, id);
        assert!(gap(&q, g.x, g.y, g.radius, g.half) > g.radius - 0.5, "the capsule stays out of the top at {n}");
        stopped |= g.vel_y <= 0.0;
    }
    assert!(stopped, "its rise is stopped");
}

#[test]
fn the_views_trace_read_meets_the_polygons() {
    for name in ["stage-treehouse", "stage-scooby"] {
        let Some(mut h) = harness::at(name, AT) else { return };
        let read = h.w.blocker_shapes_read(BlockKind::Trace);
        let made = h.w.blocker_shapes(BlockKind::Trace);
        assert_eq!(read.boxes, made.boxes, "{name}: the same boxes");
        let solids = |s: &mvs_sim::world::stage_query::StageShapes| s.polys.iter().map(|q| q.solid).collect::<Vec<_>>();
        assert_eq!(solids(&read), solids(&made), "{name}: the same polygons");
        assert!(!read.polys.is_empty(), "{name}: a polygon blocks a trace");
    }
    let Some(mut h) = harness::at("stage-treehouse", AT) else { return };
    let (_, mx, top) = right_top_of(&mut h.w);
    let mut s = h.w.blocker_shapes_read(BlockKind::Trace);
    s.boxes.insert(0, h.w.stage.ground);
    let hit = mvs_sim::chars::creature::seismic::trace_down(s.view(), mx, top + 200.0).expect("a hit");
    assert!((hit - top).abs() < 1.0, "the beam's end is on the top ({hit} against {top})");
}
