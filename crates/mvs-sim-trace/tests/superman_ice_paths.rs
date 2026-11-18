//! Superman's ice pellet: its passive Buff_Elemental_Ice's ElementalIceOverlap after the class's own HitBox handler
//! (`chars::superman::ice` elemental_overlap, `combat::elem` elem_sphere_overlap), which no trace match reaches: a
//! same-team spawned actor gets Elemental_Ice 1; an enemy one holding Elemental_Fire loses 2 stacks of it besides the
//! class's own 1 (a FireWallHazard made not Iceproof), and nothing more when it is Iceproof (the wall's passive). From
//! superman-combat a tick after the first frame where a pellet and Superman's FireSlam fire wall are live; the fire wall
//! is moved onto the pellet and its elementals and owner changed where each path starts.

use mvs_sim::actors::{ActorRef, ProjectileClass, ProjectileId};
use mvs_sim::combat::BuffClass;
use mvs_sim::combat::elem::elem_of;
use mvs_sim_trace::harness::{self, Harness};

/// The pellet and the fire wall of the frame, the wall moved so its hurt capsule's centre is on the pellet.
fn setup() -> Option<(Harness, ProjectileId, ProjectileId)> {
    let live = |w: &mvs_sim::World, c: ProjectileClass| {
        (0..).map_while(|i| w.actors.projectiles.nth(i)).find(|&id| {
            let p = w.actors.projectiles.get(id).unwrap();
            p.class == c && !p.dead
        })
    };
    let mut h = harness::at_baseline_where("superman-combat", 0, |w| {
        live(w, ProjectileClass::SupermanIceBreath).is_some() && live(w, ProjectileClass::FireWallHazard).is_some()
    })?;
    // the spawn tick overlaps nothing yet: the paths start one tick in
    h.step_recorded();
    let pellet = live(&h.w, ProjectileClass::SupermanIceBreath).expect("a pellet at the frame");
    let wall = live(&h.w, ProjectileClass::FireWallHazard).expect("a fire wall at the frame");
    let (px, py) = {
        let p = h.w.actors.projectiles.get(pellet).unwrap();
        (p.x, p.y)
    };
    // the wall's hurt capsule stands 270 above its root (hurtOffY in the trace)
    let p = h.w.actors.projectiles.get_mut(wall).unwrap();
    p.x = px;
    p.y = py - 270.0;
    Some((h, pellet, wall))
}

fn drop_elem(h: &mut Harness, id: ProjectileId, c: BuffClass) {
    let st = h.w.actors.projectiles.get_mut(id).unwrap().elem.as_mut().expect("the wall's elementals");
    st.buffs.retain(|b| b.class != c);
}

fn stacks(h: &Harness, id: ProjectileId, c: BuffClass) -> Option<i32> {
    elem_of(&h.w, ActorRef::Projectile(id), c).map(|b| b.stacks)
}

#[test]
fn a_same_team_actor_gets_elemental_ice() {
    let Some((mut h, _, wall)) = setup() else { return };
    // (its Elemental_Fire stays: a fire wall without it ends)
    drop_elem(&mut h, wall, BuffClass::BuffElementalIceproof);
    assert_eq!(stacks(&h, wall, BuffClass::BuffElementalIce), None);
    h.step_recorded();
    assert_eq!(stacks(&h, wall, BuffClass::BuffElementalIce), Some(1), "ElementalIceOverlap @1375");
}

#[test]
fn an_enemy_actor_with_elemental_fire_loses_two_more() {
    let Some((mut h, _, wall)) = setup() else { return };
    let shaggy = h.fighter(1);
    let superman = h.fighter(0);
    drop_elem(&mut h, wall, BuffClass::BuffElementalIceproof);
    {
        let p = h.w.actors.projectiles.get_mut(wall).unwrap();
        p.owner = Some(shaggy);
        p.lookup_fighter = Some(superman); // its class data stays Superman's
    }
    let before = stacks(&h, wall, BuffClass::BuffElementalFire).expect("the wall's fire");
    assert!(before > 3, "enough stacks to see both removals: {before}");
    h.step_recorded();
    assert_eq!(
        stacks(&h, wall, BuffClass::BuffElementalFire),
        Some(before - 3),
        "the class's 1 (@1424), then the elemental's 2 (@1170)"
    );
}

#[test]
fn an_iceproof_enemy_wall_loses_only_the_class_stack() {
    let Some((mut h, _, wall)) = setup() else { return };
    let shaggy = h.fighter(1);
    let superman = h.fighter(0);
    {
        let p = h.w.actors.projectiles.get_mut(wall).unwrap();
        p.owner = Some(shaggy);
        p.lookup_fighter = Some(superman);
    }
    let before = stacks(&h, wall, BuffClass::BuffElementalFire).expect("the wall's fire");
    h.step_recorded();
    assert_eq!(stacks(&h, wall, BuffClass::BuffElementalFire), Some(before - 1), "Iceproof: ElementalIceOverlap @666");
}
