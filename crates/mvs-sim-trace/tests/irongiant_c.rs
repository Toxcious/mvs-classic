//! Paths of Iron Giant's actors no trace match runs (`chars::iron_giant::actors`): the bomb's explosion, the thrown
//! scrap and what it leaves, the bolts of the eye laser. Each starts from a trace's World and steps it with no input.

use mvs_data::value::{Props, Value};
use mvs_sim::InputState;
use mvs_sim::actors::projectile::{MakeOpts, ProjectileExt, make_projectile};
use mvs_sim::actors::{ActorRef, ProjectileClass};
use mvs_sim::chars::iron_giant::actors::{eye_laser, scrap_condition};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::moves::V2;
use mvs_sim::world::spawning::{add_projectile, comp_spawn_projectile};
use mvs_sim_trace::harness::{self, Harness};

const TRACE: &str = "irongiant-combat";

fn start() -> Option<(Harness, FighterId)> {
    let h = harness::at(TRACE, 20)?;
    let f = h.fighter(0);
    Some((h, f))
}

fn idle(h: &mut Harness) {
    let inputs = vec![InputState::default(); h.w.players.len()];
    h.step(&inputs);
}

#[test]
fn the_bomb_explodes_where_it_lands_and_ends() {
    let Some((mut h, f)) = start() else { return };
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y + 150.0);
    let bomb = h.w.names.get("C017_Bomb_C").expect("bomb name");
    let dir = [Some(0.0), Some(0.0), Some(-1.0)];
    let Some(id) = comp_spawn_projectile(&mut h.w, f, bomb, x, y, dir, 400.0, 400.0, None) else {
        panic!("the bomb is not made");
    };
    let mut exploded = false;
    for _ in 0..400 {
        idle(&mut h);
        let Some(p) = h.w.actors.projectiles.get(id) else { break };
        if let ProjectileExt::Bomb(b) = &p.ext
            && b.exploding
        {
            exploded = true;
            assert_eq!(p.vx, 0.0);
            assert_eq!(p.vy, 0.0);
        }
    }
    assert!(exploded, "the bomb never exploded");
    // EndExpantion destroys it after ExpansionTime
    assert!(h.w.actors.projectiles.get(id).is_none_or(|p| p.dead), "the bomb outlived its explosion");
}

#[test]
fn a_thrown_scrap_leaves_a_scrap_item_the_condition_sees() {
    let Some((mut h, f)) = start() else { return };
    assert!(scrap_condition(&mut h.w, f) || !h.w.fighter(f).items.spawned_actors.is_empty());
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y + 150.0);
    let scrap = h.w.names.get("C017_ThrownScrap_C").expect("scrap name");
    let mut np = Props::new();
    let mut d = Props::new();
    for (k, v) in [("X", 0.0), ("Y", 0.0), ("Z", -1.0)] {
        d.insert(k.to_owned(), Value::Num(v));
    }
    np.insert("InitialMovementDirection".to_owned(), Value::Obj(d));
    np.insert("InitialSpeed".to_owned(), Value::Num(400.0));
    np.insert("MaxSpeed".to_owned(), Value::Num(400.0));
    let attack = h.w.fighter(f).decay.attack_count;
    let mut pr = make_projectile(
        &mut h.w,
        scrap,
        ProjectileClass::C017ThrownScrap,
        Some(f),
        attack,
        &np,
        Some(V2 { x, y }),
        MakeOpts::default(),
    );
    pr.ext = ProjectileExt::ThrownScrap(Default::default());
    let pid = h.w.actors.projectiles.insert(pr);
    add_projectile(&mut h.w, pid, None);
    let scrap_item = h.w.names.get("Item_C017_Scrap_C").expect("item name");
    let mut found = false;
    for _ in 0..600 {
        idle(&mut h);
        found = h.w.actors.items.iter().any(|(_, it)| it.cls == scrap_item && !it.dead);
        if found {
            break;
        }
    }
    if !found {
        match h.w.actors.projectiles.get(pid) {
            Some(p) => panic!(
                "scrap at {} {} v {} {} dead {} spent {:?}",
                p.x,
                p.y,
                p.vx,
                p.vy,
                p.dead,
                matches!(&p.ext, ProjectileExt::ThrownScrap(s) if s.spent)
            ),
            None => panic!("scrap gone, items {}", h.w.actors.items.iter().count()),
        }
    }
    // the last spawned actor is the scrap: the condition is false
    let last = h.w.fighter(f).items.spawned_actors.last().copied();
    assert!(matches!(last, Some(ActorRef::Item(_))));
    assert!(!scrap_condition(&mut h.w, f));
}

#[test]
fn the_eye_laser_makes_bolts_that_end() {
    let Some((mut h, f)) = start() else { return };
    assert!(eye_laser(&mut h.w, f), "the eye laser is not asked for");
    let mut seen = false;
    for _ in 0..600 {
        idle(&mut h);
        let n =
            h.w.actors.actors.iter().filter(|(_, a)| h.w.names.str(a.cls) == "C017_AntenaBolts_C" && !a.dead).count();
        seen |= n > 0;
    }
    assert!(seen, "no C017_AntenaBolts_C");
    let n = h.w.actors.actors.iter().filter(|(_, a)| h.w.names.str(a.cls) == "C017_AntenaBolts_C" && !a.dead).count();
    assert_eq!(n, 0, "the bolts outlived their 5.25 s");
}
