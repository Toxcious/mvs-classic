//! Shared paths that no match runs until the characters' ports land: `actors::item_bounce` (the
//! BouncingItem flight of Stripe's dropped bomb and Finn's gem) and `actors::actor_montage` (the montages of the
//! actors that play one: the Meeseeks, Neptr).

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::ItemExt;
use mvs_sim::actors::ProjectileExt;
use mvs_sim::actors::actor_montage::{
    ActorAnim, has_anim, mesh_actor, mesh_tick, montage_current_section, montage_jump_to_section, montage_play,
    montage_set_play_rate,
};
use mvs_sim::actors::item::ItemBox;
use mvs_sim::actors::item_bounce::{Bounce, bounce_of};
use mvs_sim::chars::stripe::bomb::DroppedBomb;
use mvs_sim::consts::DT;
use mvs_sim::math::narrow;
use mvs_sim::world::World;
use mvs_sim::world::spawned_platform::BlockKind;
use mvs_sim_trace::harness;
use mvs_trace::Value;

fn num(rest: &Value, k: &str) -> mvs_sim::Real {
    match rest {
        Value::Obj(o) => match o.get(k) {
            Some(Value::Num(n)) => mvs_sim::math::narrow(*n),
            v => panic!("rest.{k}: {v:?}"),
        },
        _ => panic!("no rest"),
    }
}

/// The float32 ulps between two values of one sign (any for a sign change but zeros).
fn ulps(a: mvs_sim::Real, b: mvs_sim::Real) -> u32 {
    if a == b {
        return 0;
    }
    if a.is_sign_negative() != b.is_sign_negative() {
        return u32::MAX;
    }
    a.to_bits().abs_diff(b.to_bits())
}

fn flag(rest: &Value, k: &str) -> bool {
    match rest {
        Value::Obj(o) => matches!(o.get(k), Some(Value::Bool(true))),
        _ => panic!("no rest"),
    }
}

/// Each frame of the first bomb's life in stripe-stripe (1069..1259; it lands and slides from 1133): the item set to
/// the frame the oracle wrote, one `Item::step` (so `Item::fly` hands it to `item_bounce::fly`), then its place, speed
/// and bounce state against the oracle's next frame: the flags bit for bit, the bounce floats within 2 float32 ulps and
/// the place and speed within 2 ulps of 1024 (one float32 step of the oracle's double frame narrowed; the oracle chain
/// is double, and a speed near 0 after the gravity's sums keeps the error of its terms, about 800).
#[test]
fn a_dropped_bomb_flies_as_the_ts() {
    let first = 1069;
    let Some(mut h) = harness::at("stripe-stripe", first) else { return };
    let id = h.w.actors.items.nth(0).expect("the bomb is loaded");
    let mut g = Vec::new();
    h.w.blockers_into(BlockKind::Item, &mut g);
    let boxes: Vec<ItemBox> =
        g.into_iter().map(|g| ItemBox { g, is_static: false, kin: None, solid: None, poly: None }).collect();
    let ground = h.w.stage.ground;
    let data = std::sync::Arc::clone(&h.w.data);
    let (mut slid, mut landed) = (false, false);
    for f in first..1258 {
        let rec = h.recorded(f);
        let a = &rec.world.actors[0];
        assert_eq!(a.class, "C023B_DroppedBomb_C");
        let b = Bounce {
            prev_hit_t: num(&a.rest, "prevHitT"),
            prev_nx: num(&a.rest, "prevNx"),
            prev_ny: num(&a.rest, "prevNy"),
            sliding: flag(&a.rest, "sliding"),
            start_pen: flag(&a.rest, "startPen"),
            bounciness: num(&a.rest, "bounciness"),
            friction: num(&a.rest, "friction"),
        };
        let it = h.w.actors.items.get_mut(id).expect("the bomb");
        it.ext = ItemExt::DroppedBomb(DroppedBomb { bounce: b, ..Default::default() });
        (it.x, it.y) = (narrow(a.x.expect("x")), narrow(a.y.expect("y")));
        (it.vx, it.vy) = (narrow(a.vx.expect("vx")), narrow(a.vy.expect("vy")));
        it.flying = flag(&a.rest, "flying");
        it.dead = false;
        let d = it.def(&data);
        let mut contacts = Vec::new();
        let mut q = Vec::new();
        let st = it.step(id, d, &ground, &boxes, &mut contacts, None, &mut q);
        assert!(!st.blocked, "frame {f}: a bomb with no hit box is never blocked");
        let next = h.recorded(f + 1);
        let n = &next.world.actors[0];
        let it = h.w.actors.items.get(id).expect("the bomb");
        let b = bounce_of(it).expect("a bouncing item");
        let got = (it.x, it.y, it.vx, it.vy, b.prev_hit_t, b.prev_nx, b.prev_ny, b.sliding, b.start_pen);
        let want = (
            narrow(n.x.expect("x")),
            narrow(n.y.expect("y")),
            narrow(n.vx.expect("vx")),
            narrow(n.vy.expect("vy")),
            num(&n.rest, "prevHitT"),
            num(&n.rest, "prevNx"),
            num(&n.rest, "prevNy"),
            flag(&n.rest, "sliding"),
            flag(&n.rest, "startPen"),
        );
        let (g, w) = (
            [got.0, got.1, got.2, got.3, got.4, got.5, got.6],
            [want.0, want.1, want.2, want.3, want.4, want.5, want.6],
        );
        let near = g.iter().zip(&w).enumerate().all(|(k, (a, b))| {
            if k < 4 { (a - b).abs() <= 2.0 * 1024.0 * mvs_sim::Real::EPSILON } else { ulps(*a, *b) <= 2 }
        });
        assert!(
            near && (got.7, got.8) == (want.7, want.8),
            "frame {} (x, y, vx, vy, prevHitT, prevNx, prevNy, sliding, startPen): {got:?} | {want:?}",
            f + 1
        );
        slid |= b.sliding;
        landed |= b.prev_ny == 1.0;
    }
    assert!(landed && slid, "the life holds a bounce off the floor and a slide");
}

/// The warnings since the last call (the montage events of a class with no mesh hook land there, once per text).
fn warned(w: &World) -> Vec<String> {
    let mut v = w.warned.borrow_mut();
    let out = v.iter().map(|s| s.to_string()).collect();
    v.clear();
    out
}

fn has(ws: &[String], text: &str) -> bool {
    ws.iter().any(|s| s.contains(text))
}

/// Rick's Meeseeks of rick-rick (frame 2440, its projectile; no trace plays a montage on it) given the mesh its port
/// gives it: Montage_Play, the current section, a jump, the play rate, the ticks to the end and the proxy's events
/// (the class's mesh hook takes them: no warning), a play over a bound instance (OnInterrupted) and a missing montage.
/// The expected values are the montages' data (LookAtMe: 1.6 s, one section; GolfSwing: Default then Swing at
/// 2.410236 s) and the actor montage rules.
#[test]
fn a_meeseeks_plays_its_montages() {
    let Some(mut h) = harness::at("rick-rick", 2440) else { return };
    let w = &mut h.w;
    let id = w.actors.projectiles.nth(0).expect("the Meeseeks");
    let a = ActorRef::Projectile(id);
    let mesh = w.lit("Meeseeks_SkelMesh");
    // Its port's own state is loaded from the trace: the Meeseeks is a MeeseeksSummon with its mesh.
    assert!(matches!(w.actors.projectiles.get(id).expect("the Meeseeks").ext, ProjectileExt::MeeseeksSummon(_)));
    assert!(has_anim(w, a));
    let look = w.lit("Meeseeks_Attack_LookAtMe_Montage");
    let golf = w.lit("Meeseeks_Attack_GolfSwing_Montage");
    warned(w);

    // LookAtMe with a proxy: its section, then one tick at rate 1 and one at rate 2.
    assert!(montage_play(w, a, look, 1.0, 0.0, true));
    assert_eq!(w.names.str(montage_current_section(w, a)), "Default");
    assert_eq!(mesh_actor(w, a).expect("a mesh actor").montage, Some(look));
    mesh_tick(w, a);
    let p1 = anim(w, a).m.as_ref().expect("playing").pos;
    assert_eq!(p1, f64::from(DT), "one tick at rate 1");
    montage_set_play_rate(w, a, 2.0);
    assert_eq!(anim(w, a).m.as_ref().expect("playing").play_rate, 2.0);
    mesh_tick(w, a);
    let p2 = anim(w, a).m.as_ref().expect("playing").pos;
    // two steps of 1/60 in float32 added to the first: the position is a double holding float32 sums
    assert_eq!(p2, 0.05000000447034836, "one tick at rate 2: {p1} -> {p2}");
    // To its end (1.6 s, no blend-out time): the proxy's OnBlendOut, then OnCompleted, the instance gone.
    let mut ticks = 2;
    while anim(w, a).m.is_some() {
        mesh_tick(w, a);
        ticks += 1;
        assert!(ticks < 100, "LookAtMe ends");
    }
    assert!((47..=50).contains(&ticks), "1.6 s at rate 2 after one tick at rate 1: {ticks} ticks");
    assert!(anim(w, a).ended, "OnMontageEnded broadcast");
    assert_eq!(montage_current_section(w, a), mvs_sim::names::Name::NONE, "nothing plays");
    assert!(!montage_jump_to_section(w, a, w.lit("Swing")), "nothing to jump in");

    // GolfSwing without a proxy: a jump to its second section, a section it lacks.
    assert!(montage_play(w, a, golf, 1.0, 0.0, false));
    assert!(!anim(w, a).ended && !anim(w, a).proxy, "a new play resets the proxy's state");
    assert_eq!(w.names.str(montage_current_section(w, a)), "Default");
    assert!(montage_jump_to_section(w, a, w.lit("Swing")));
    let pos = anim(w, a).m.as_ref().expect("playing").pos;
    assert_eq!(pos, 2.410236, "at Swing");
    assert_eq!(w.names.str(montage_current_section(w, a)), "Swing");
    assert!(!montage_jump_to_section(w, a, w.lit("Charging")));
    let ws = warned(w);
    assert!(has(&ws, "has no section Charging"), "{ws:?}");

    // A play over an unbound instance sends nothing; over a bound one at full weight, OnInterrupted at once.
    let uid = anim(w, a).m.as_ref().expect("playing").montage.uid;
    assert!(montage_play(w, a, look, 1.0, 0.0, true));
    assert_ne!(anim(w, a).m.as_ref().expect("playing").montage.uid, uid, "a new instance");
    assert!(warned(w).iter().all(|s| !s.contains("interrupted")));
    mesh_tick(w, a);
    assert!(montage_play(w, a, golf, 1.0, 0.0, true));

    // A montage the class's data lacks: warned, OnInterrupted for a proxy, nothing plays.
    assert!(!montage_play(w, a, mesh, 1.0, 0.0, true));
    let ws = warned(w);
    assert!(has(&ws, "actor montage missing: Meeseeks_SkelMesh"), "{ws:?}");
    assert!(anim(w, a).m.is_none());
    assert!(w.scratch.actor_limbo.is_empty() && w.scratch.actor_running.is_empty(), "no instance left held");
}

fn anim(w: &World, a: ActorRef) -> &ActorAnim {
    mvs_sim::actors::actor_montage::anim_of(w, a).expect("the anim")
}
