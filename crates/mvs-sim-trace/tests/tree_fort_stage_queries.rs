//! The stage queries of the spawned actors against a polygon stage solid that no match runs (`world/stage_query.rs`):
//! on the World of stage-treehouse at a baseline frame, the right tree's round top (a polygon solid) blocks a line of
//! sight, stops a down trace and pushes a spawn out of it.
//! Without a store or the trace the tests check nothing and say so.

use mvs_sim::world::spawned_platform::BlockKind;
use mvs_sim::world::stage_query::{PolyBlock, static_shapes};
use mvs_sim_trace::harness;

/// The right tree's top: the polygon, its middle x, its top there and its bottom.
fn right_top(h: &mut harness::Harness) -> (PolyBlock, f32, f32, f32) {
    let s = h.w.blocker_shapes(BlockKind::Trace);
    let q = s.polys.into_iter().find(|q| q.bounds().left > 0.0).expect("the right tree's top");
    let g = q.bounds();
    let mx = mvs_sim::math::narrow((g.left + g.right) / 2.0);
    let top = q.top_at(mx);
    (q, mx, top, mvs_sim::math::narrow(g.bottom))
}

#[test]
fn a_tree_top_blocks_the_line_of_sight() {
    let Some(mut h) = harness::at("stage-treehouse", 1500) else { return };
    let (_, mx, top, bottom) = right_top(&mut h);
    let mid = (top + bottom) / 2.0;
    assert!(mvs_sim::combat::phases::los_blocked(&mut h.w, mx, top + 100.0, mx, mid), "into the top");
    assert!(!mvs_sim::combat::phases::los_blocked(&mut h.w, mx, top + 100.0, mx, top + 50.0), "over it");
}

#[test]
fn a_down_trace_stops_on_a_tree_top() {
    let Some(mut h) = harness::at("stage-treehouse", 1500) else { return };
    let (_, mx, top, _) = right_top(&mut h);
    let mut s = h.w.blocker_shapes(BlockKind::Trace);
    s.boxes.insert(0, h.w.stage.ground);
    let hit = mvs_sim::chars::creature::seismic::trace_down(s.view(), mx, top + 200.0).expect("a hit");
    assert!((hit - top).abs() < 1.0, "the column stops on the top ({hit} against {top})");
}

#[test]
fn a_spawn_inside_a_tree_top_is_pushed_out() {
    let Some(mut h) = harness::at("stage-treehouse", 1500) else { return };
    let (q, mx, top, _) = right_top(&mut h);
    let why = mvs_sim::names::Name::NONE;
    let s = static_shapes(&mut h.w, mvs_sim::consts::SOLID_NAV, why);
    let (x, y) = (mx, top - 20.0);
    assert!(q.inside(x, y), "the spawn starts inside the top");
    let at = mvs_sim::world::spawn_spot::find_teleport_spot(s.view(), x, y, 35.0);
    assert!(at.x != x || at.y != y, "the spot is moved");
    let (cx, cy) = q.closest(at.x, at.y);
    let d = ((cx - at.x) * (cx - at.x) + (cy - at.y) * (cy - at.y)).sqrt();
    assert!(!q.inside(at.x, at.y) && d > 34.0, "out of the top by the radius ({d})");
}
