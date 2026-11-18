//! Taz's Tasty buffs on a spawned actor (`chars/taz/tasty.rs`), a path no trace match runs: Buff_ApplyTasty and
//! Buff_TastyOnHit on a projectile (his hits' AppliedBuffs on a projectile or an item, his burped projectile). The
//! expected values are the oracle's: a probe of the oracle that spawns the same buffs on a C015_HealPickup_C of Taz's
//! (the stacks, the extension, the Tasty given to the hit player and its UI state).

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::taz::tasty::{TastyActor, TastyOnHitActor};
use mvs_sim::combat::BuffClass;
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::combat::elem::{ElemExt, actor_buff_signal, spawn_actor_buff};
use mvs_sim::events::UiStateOp;
use mvs_sim::fighter::FighterId;
use mvs_sim::name;
use mvs_sim::registry::signals::{ActorBuffArg, ActorBuffSignal};
use mvs_sim::world::World;
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness;

/// An early frame of taz-shaggy: Taz is fighter 0, Shaggy fighter 1, and Shaggy has no Tasty yet.
const AT: usize = 5;

/// A C015_HealPickup_C of Taz's at (x, 300). Taz is burping at the baseline frame, so the flag is cleared
/// first: the burp's spawn hook would give the pickup a Buff_TastyOnHit of its own.
fn pickup(w: &mut World, taz: FighterId, x: f64) -> ActorRef {
    mvs_sim::chars::taz::state::taz_mut(w, taz).expect("Taz").burping_projectile = false;
    let id = comp_spawn_projectile(
        w,
        taz,
        name!("C015_HealPickup_C"),
        mvs_sim::math::narrow(x),
        300.0,
        [Some(0.0), Some(0.0), Some(1.0)],
        0.0,
        0.0,
        None,
    )
    .expect("the pickup spawns");
    ActorRef::Projectile(id)
}

/// The actor's buffs: class, stacks, extension.
fn buffs(w: &World, a: ActorRef) -> Vec<(BuffClass, i32, ElemExt)> {
    mvs_sim::actors::world_actor::actor_elem(w, a)
        .and_then(Option::as_ref)
        .map(|s| s.buffs.iter().map(|b| (b.class, b.stacks, b.ext)).collect())
        .unwrap_or_default()
}

fn tasty(stacks: i32, inst: i32) -> Vec<(BuffClass, i32, ElemExt)> {
    vec![(BuffClass::BuffTasty, stacks, ElemExt::Tasty(TastyActor { inst }))]
}

#[test]
fn apply_tasty_on_an_actor_gives_it_the_tasty() {
    let Some(mut h) = harness::at("taz-shaggy", AT) else { return };
    let taz = h.fighter(0);
    let id = h.w.fighter(taz).id;
    let a = pickup(&mut h.w, taz, 0.0);
    let apply = h.w.lit("Buff_ApplyTasty_C");
    // the ApplyTasty ends in its BeginPlay; a Tasty of its stacks, the Instigator's id kept
    assert_eq!(spawn_actor_buff(&mut h.w, a, apply, 3, 0.0, true, Some(taz)), None);
    assert_eq!(buffs(&h.w, a), tasty(3, id));
    // a Tasty there: CurrentStackAmount - 1 more
    spawn_actor_buff(&mut h.w, a, apply, 3, 0.0, true, Some(taz));
    assert_eq!(buffs(&h.w, a), tasty(5, id));
    // one stack on a Tasty that is there adds nothing
    spawn_actor_buff(&mut h.w, a, apply, 1, 0.0, true, Some(taz));
    assert_eq!(buffs(&h.w, a), tasty(5, id));
    // to the cap (MaxStacks 18); the chicken it spawns there is refused on an actor, the Tasty stays
    spawn_actor_buff(&mut h.w, a, apply, 30, 0.0, true, Some(taz));
    assert_eq!(buffs(&h.w, a), tasty(18, id));
}

#[test]
fn tasty_on_hit_on_an_actor_gives_the_hit_player_the_tasty_once() {
    let Some(mut h) = harness::at("taz-shaggy", AT) else { return };
    let (taz, shaggy) = (h.fighter(0), h.fighter(1));
    let id = h.w.fighter(taz).id;
    let tasty_of = |w: &World| -> Vec<(mvs_sim::Real, Option<FighterId>)> {
        let f = w.fighter(shaggy);
        let list = f.buffs.buffs.iter().filter(|b| b.cls == name!("Buff_Tasty_C") && b.frames > 0.0);
        list.map(|b| (b.stacks, if let BuffExt::Tasty(e) = &b.ext { e.inst } else { None })).collect()
    };
    assert!(tasty_of(&h.w).is_empty(), "Shaggy has a Tasty at frame {AT}");
    let a = pickup(&mut h.w, taz, 100.0);
    let cls = h.w.lit("Buff_TastyOnHit_C");
    spawn_actor_buff(&mut h.w, a, cls, 4, 0.0, true, Some(taz));
    let on_hit = vec![(BuffClass::BuffTastyOnHit, 4, ElemExt::TastyOnHit(TastyOnHitActor { inst: id, open: true }))];
    assert_eq!(buffs(&h.w, a), on_hit);
    let arg = ActorBuffArg { by: shaggy, kx: 0.0, ky: 0.0 };
    // another signal: nothing
    actor_buff_signal(&mut h.w, a, ActorBuffSignal::HitAnything, Some(&arg));
    assert_eq!(buffs(&h.w, a), on_hit);
    h.w.fighter_mut(shaggy).controls.ui_state_ops.clear();
    // HitEnemyPlayer: the hit player's Tasty of the buff's stacks, Taz its Instigator; the buff gone
    actor_buff_signal(&mut h.w, a, ActorBuffSignal::HitEnemyPlayer, Some(&arg));
    assert!(buffs(&h.w, a).is_empty());
    assert_eq!(tasty_of(&h.w), vec![(4.0, Some(taz))]);
    let ops = &h.w.fighter(shaggy).controls.ui_state_ops;
    let [UiStateOp::Add(p)] = ops.as_slice() else { panic!("one UI state added: {ops:?}") };
    assert_eq!(
        (p.id, p.state_type, p.display_type, p.current_value, p.max_value, p.timer),
        (name!("Tasty"), 1, 0, 4.0, 18.0, -1.0)
    );
    assert_eq!((p.icon_material, p.active, p.auto_remove_time, p.subicon), (name!("MI_State_Smell"), true, -1.0, None));
    // a second HitEnemyPlayer: the buff is gone, nothing more
    actor_buff_signal(&mut h.w, a, ActorBuffSignal::HitEnemyPlayer, Some(&arg));
    assert_eq!(tasty_of(&h.w), vec![(4.0, Some(taz))]);
}
