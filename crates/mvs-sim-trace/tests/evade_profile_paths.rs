//! A path no trace match runs: Wonder Woman's evade shield (BlockCollision, OverlapAllDynamic) against Shaggy's
//! sandwich. The sandwich's root Capsule cooks WorldDynamic Ignore on profile ProjectileNavNoPlatform; the build applies
//! the profile over the cooked array (FBodyInstance::LoadProfileData 142c77210, read live), so the shield meets it.
//! Without a store the test checks nothing and says so.

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::wonder_woman::perks::evade_meets_body;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::traces::{OBJ_PHYSICS_BODY, OBJ_WORLD_DYNAMIC, body_info, response_to};
use mvs_sim::registry::props::obj;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

fn player(c: &str) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() }
}

#[test]
fn the_evade_block_meets_the_sandwich_root() {
    let setup = MatchSetup { players: vec![player("wonderwoman"), player("shaggy")], ..MatchSetup::default() };
    let Some(h) = harness::fresh(&setup, 1) else { return };
    let s = h.fighter(1);
    let cls = h.w.lit("ShaggySandwichProjectile_C");
    let def = actor_def(&h.w, ActorRef::Fighter(s), cls).expect("the sandwich's class data");
    let root = obj(&def.components, "Capsule");
    let info = body_info(root, None).expect("the root has a body");
    assert_eq!(info.kind, OBJ_PHYSICS_BODY, "ProjectileNavNoPlatform's object type, not the cooked one");
    assert_eq!(
        response_to(root, mvs_sim::real(OBJ_WORLD_DYNAMIC)),
        mvs_sim::name!("ECR_Block"),
        "the profile's WorldDynamic Block over the cooked Ignore"
    );
    assert!(evade_meets_body(root), "the shield meets the sandwich");
}
