//! A knockback that starts inside WW_ShieldActor's WallbounceCollision, no trace match runs (`fighter/body.rs`
//! `collide_platforms`' start-penetrating side branch). wonderwoman-combat's frame 7867: the dummy (fighter 1) just
//! hit, its launch pending, Wonder Woman's wall standing at [917.32, 989.32] (half thickness 36). The dummy is put
//! inside the wall and its launch set to the Lab read's (a bot at the wall's near side, kb [2075, 1452], mirrored
//! here: the launch goes left into the wall's right face); the shield's 1.0 s buffer is taken as past.
//! Near side (short of the mid-plane): out of the near face, the bounce off it, x held there through the bounce's
//! pause, y as the build's (23.999 over the bounce's frame, not the trapezoid move's 24.633). Far side: out the far
//! face with the velocity kept.

use mvs_sim::actors::WorldActorClass;
use mvs_sim::actors::shield_actor::shield_mut;
use mvs_sim::fighter::body::cap_radius;
use mvs_sim::input::NO_INPUT;
use mvs_sim_trace::harness::{self, Harness};

const AT: usize = 7867;
/// The wall's right face at the frame.
const FACE: f32 = 989.3214;

/// The frame with the dummy at `x` inside the wall, its launch the Lab's, the wall's buffer over.
fn set_up(x: f32) -> Option<Harness> {
    let mut h = harness::at_baseline("wonderwoman-combat", AT)?;
    let ids: Vec<_> =
        h.w.actors
            .actors
            .iter()
            .filter(|(_, a)| !a.dead && a.class == WorldActorClass::WWShieldActor)
            .map(|(id, _)| id)
            .collect();
    assert_eq!(ids.len(), 1, "one WW_ShieldActor at the frame");
    let a = h.w.actors.actors.get_mut(ids[0]).expect("the shield");
    shield_mut(a).expect("a shield's state").start_time = -100.0;
    let f = h.fighter(1);
    let b = &mut h.w.fighter_mut(f).body;
    b.x = x;
    b.prev_x = x;
    let l = h.w.fighter_mut(f).hit.launch.as_mut().expect("the dummy's pending launch");
    l.vx = -2075.0;
    l.vy = 1452.0;
    Some(h)
}

/// Steps to the frame the launch is taken in (the frame after the hit's pause).
fn to_launch(h: &mut Harness) {
    let f = h.fighter(1);
    while h.w.fighter(f).hit.hitpause > 0.0 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    h.step(&[NO_INPUT, NO_INPUT]);
    assert!(h.w.fighter(f).hit.kb_mode, "the launch taken");
}

#[test]
fn a_knockback_from_short_of_the_mid_plane_bounces_off_the_near_face_and_holds_there() {
    let Some(mut h) = set_up(FACE - 26.0) else { return };
    let f = h.fighter(1);
    let r = cap_radius(&h.w, f);
    let y0 = h.w.fighter(f).body.y;
    to_launch(&mut h);
    let x = h.w.fighter(f);
    let held = x.body.x;
    // The port's 2D capsule MTD (the Lab read: port -250.159, build -250.156).
    assert!(held > FACE + r - 1.0 && held < FACE + r + 0.125, "out of the near face: {held}");
    assert!(x.hit.hitpause > 0.0, "the bounce's pause");
    assert_eq!(x.body.vx, 0.0, "nothing left into the face");
    // The build: 23.999 over the bounce's frame with the Lab's launch (the port's trapezoid move was 24.633).
    let rise = x.body.y - y0;
    assert!((rise - 23.999).abs() < 0.02, "the bounce frame's rise: {rise}");
    while h.w.fighter(f).hit.hitpause > 0.0 {
        h.step(&[NO_INPUT, NO_INPUT]);
        assert_eq!(h.w.fighter(f).body.x, held, "x held at the face in the bounce's pause");
    }
    // The build: 26.854 off the face on the first frame after the pause.
    h.step(&[NO_INPUT, NO_INPUT]);
    let off = h.w.fighter(f).body.x - held;
    assert!((off - 26.854).abs() < 0.01, "the bounce's first move: {off}");
}

#[test]
fn a_knockback_from_past_the_mid_plane_leaves_by_the_far_face_unbounced() {
    let Some(mut h) = set_up(FACE - 50.0) else { return };
    let f = h.fighter(1);
    let r = cap_radius(&h.w, f);
    to_launch(&mut h);
    let x = h.w.fighter(f);
    assert!(x.body.x < FACE - 72.0 - r, "out by the far face: {}", x.body.x);
    assert_eq!(x.hit.hitpause, 0.0, "no bounce");
    assert!(x.body.vx < -1900.0, "the velocity kept: {}", x.body.vx);
}
