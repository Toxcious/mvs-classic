//! A path no trace match runs: Shaggy's raged down special (ShaggyShockwaveActor_C) whose stage sphere overlaps a
//! projectile of its own team, Shaggy's own sandwich. shockwave_hit calls pfgDoDamage(null, the sandwich, HitDamage,
//! ..., OnHitBuffs = UseVacuumHit ? [] : On Hit Buffs Param) @1235: the call's rows are the caller's on every read,
//! the ally rows (@5931-6835) included, so the route never asks for the shockwave's own HitBoxDamage. On a fresh World
//! of Shaggy against Shaggy, the sandwich is held where each next stage fires; the port panicked there before the
//! rows were passed down. Without a store it checks nothing.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef, ProjectileId};
use mvs_sim::chars::shaggy::shockwave::overlap_types;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::traces::sphere_overlap_actors;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::registry::props::obj;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];
/// `OBJ.HitBox`: the shockwave spheres' object type.
const OBJ_HIT_BOX: mvs_sim::Real = 7.0;
const COMPS: [&str; 4] = ["Shockwave0", "Shockwave1", "Shockwave2", "Shockwave3"];

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() }
}

fn place(h: &mut Harness, f: FighterId, x: mvs_sim::Real, facing: mvs_sim::Real) {
    let b = &mut h.w.fighter_mut(f).body;
    b.x = x;
    b.prev_x = x;
    b.facing_dir = facing;
}

fn sandwich(h: &Harness) -> Option<ProjectileId> {
    h.w.actors.projectiles.iter().find(|(_, p)| !p.dead).map(|(id, _)| id)
}

fn wave(h: &Harness) -> Option<ActorId> {
    h.w.actors
        .actors
        .iter()
        .find(|(_, a)| matches!(a.state, WorldActorState::Shockwave(_)) && !a.dead)
        .map(|(id, _)| id)
}

/// The next stage's sphere: its component, centre and radius (`fire`'s x, the last ground height), or `None` once the
/// wave fired its last stage.
fn next_sphere(h: &Harness, id: ActorId) -> Option<(usize, mvs_sim::Real, mvs_sim::Real, mvs_sim::Real)> {
    let a = h.w.actors.actors.get(id)?;
    let WorldActorState::Shockwave(sw) = &a.state else { return None };
    if sw.then != Some(mvs_sim::chars::shaggy::shockwave::Then::Stage) {
        return None;
    }
    let k = a.size_scale.unwrap_or(1.0);
    let i = usize::try_from(sw.stage).ok()?;
    let sp = sw.spheres[i];
    Some((i, sw.x + sp[0] * sw.facing * k, sw.height, sp[2] * k))
}

#[test]
fn the_shockwave_hits_its_own_teams_sandwich() {
    let setup = MatchSetup { players: vec![player("shaggy", 0), player("shaggy", 1)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (s, e) = (h.fighter(0), h.fighter(1));
    place(&mut h, s, -900.0, 1.0);
    place(&mut h, e, 1400.0, -1.0);
    h.step(&NONE);
    let throw = h.w.lit("Shaggy_RockThrow_Ground_Montage");
    start_montage(&mut h.w, s, throw, &PROXY_NONE, None);
    let mut sw = None;
    for _ in 0..40 {
        h.step(&NONE);
        sw = sandwich(&h);
        if sw.is_some() {
            break;
        }
    }
    let sw = sw.expect("Shaggy's throw spawns his sandwich");
    let rage = h.w.lit("Shaggy_Rage_D_Montage");
    start_montage(&mut h.w, s, rage, &PROXY_NONE, None);
    let cls = h.w.lit("ShaggyShockwaveActor_C");
    let comps = actor_def(&h.w, ActorRef::Fighter(s), cls).expect("the shockwave's class data").components.clone();
    let (mut id, mut met) = (None, 0);
    for _ in 0..120 {
        // the sandwich held still, out of the fighters' way, until the wave is there
        let at = id.and_then(|id| next_sphere(&h, id));
        if let Some(p) = h.w.actors.projectiles.get_mut(sw) {
            let (x, y) = at.map_or((-200.0, 600.0), |(_, x, y, r)| (x, y + r * 0.5));
            (p.x, p.y, p.vx, p.vy, p.grav_scale) = (x, y, 0.0, 0.0, 0.0);
        }
        if let (Some(w), Some((i, x, y, r))) = (id, at) {
            let c = obj(&comps, COMPS[i]).expect("the stage's sphere component");
            let types = overlap_types(c);
            let found = sphere_overlap_actors(&mut h.w, x, y, r, &types, &[Some(ActorRef::Actor(w))], OBJ_HIT_BOX);
            if found.contains(&ActorRef::Projectile(sw)) {
                met += 1;
            }
        }
        h.step(&NONE);
        if id.is_none() {
            id = wave(&h);
        }
        if h.w.actors.projectiles.get(sw).is_none_or(|p| p.dead) {
            break;
        }
        if let Some(w) = id
            && h.w.actors.actors.get(w).is_none_or(|a| a.dead)
        {
            break;
        }
    }
    assert!(id.is_some(), "Shaggy_Rage_D_Montage spawns the shockwave");
    assert!(met > 0, "no stage of the wave met the sandwich");
}
