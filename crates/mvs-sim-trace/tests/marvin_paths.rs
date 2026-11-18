//! Marvin's bubble no trace match runs (`chars/marvin/bubble.rs`): a bubble of Marvin's over a projectile of Shaggy's
//! (the other team): the capture (a reflect, the scale, Buff_C018BubbledProjectile with its carry), the held actor
//! riding the bubble, and the release when the bubble ends (the buff ended, the collision back).
//! Unverified: no oracle probe of the same World was run; the asserts are the oracle's rules, not its printed values.

use mvs_sim::actors::{ActorRef, ProjectileExt};
use mvs_sim::chars::marvin::bubble::BubbledExt;
use mvs_sim::combat::elem::ElemExt;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::{comp_spawn_projectile, destroy_projectile};
use mvs_sim_trace::harness;

fn player(c: &str) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() }
}

#[test]
fn a_bubble_holds_an_enemy_projectile_and_lets_it_go() {
    let setup = MatchSetup { players: vec![player("marvin"), player("shaggy")], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let (m, s) = (h.fighter(0), h.fighter(1));
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let sandwich = h.w.lit("ShaggySandwichProjectile_C");
    let bubble = h.w.lit("C018BubbleProj_C");
    let (x, y) = (h.w.fighter(s).body.x, h.w.fighter(s).body.y + 700.0);
    let held = comp_spawn_projectile(&mut h.w, s, sandwich, x, y, [Some(0.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
        .expect("Shaggy's projectile spawns");
    let b = comp_spawn_projectile(&mut h.w, m, bubble, x, y, [Some(0.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
        .expect("the bubble spawns");
    let bubbled = |h: &harness::Harness| -> Option<BubbledExt> {
        let p = h.w.actors.projectiles.get(held)?;
        p.elem.as_ref()?.buffs.iter().find_map(|b| match b.ext {
            ElemExt::C018Bubbled(e) => Some(e),
            _ => None,
        })
    };
    let mut caught = false;
    for _ in 0..20 {
        h.step(&[NO_INPUT, NO_INPUT]);
        if h.w
            .actors
            .projectiles
            .get(b)
            .is_some_and(|p| matches!(&p.ext, ProjectileExt::BubbleProj(e) if e.captured.is_some()))
        {
            caught = true;
            break;
        }
    }
    let state =
        h.w.actors
            .projectiles
            .iter()
            .map(|(_, p)| (h.w.names.str(p.cls).to_owned(), p.x, p.y, p.dead, p.hit_var))
            .collect::<Vec<_>>();
    assert!(caught, "the bubble caught the projectile: {state:?}");
    let ProjectileExt::BubbleProj(e) = &h.w.actors.projectiles.get(b).unwrap().ext else { panic!("a bubble") };
    assert_eq!(e.captured, Some(ActorRef::Projectile(held)));
    assert!(e.has_projectile);
    let seq = h.w.actors.projectiles.get(b).unwrap().seq;
    let ext = bubbled(&h).expect("the Bubbled buff");
    assert_eq!(ext.bubble, Some(seq));
    assert!(ext.activated || ext.wait, "{ext:?}");
    h.step(&[NO_INPUT, NO_INPUT]);
    h.step(&[NO_INPUT, NO_INPUT]);
    let (bx, by) = {
        let p = h.w.actors.projectiles.get(b).unwrap();
        (p.x, p.y)
    };
    let p = h.w.actors.projectiles.get(held).expect("the held projectile");
    if bubbled(&h).is_some_and(|e| e.activated) {
        assert_eq!((p.x, p.y, p.no_collision), (bx, by, true), "it rides the bubble");
    }
    destroy_projectile(&mut h.w, b, false);
    let p = h.w.actors.projectiles.get(held).expect("the held projectile");
    assert!(bubbled(&h).is_none(), "the buff ended with the bubble");
    assert!(!p.no_collision, "the collision is back");
}
