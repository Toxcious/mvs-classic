//! Velma's mark, aim projectile and capture paths that no trace match runs (`chars/velma/mark.rs`, `capture.rs`), on a
//! fresh World of Velma and Shaggy: the homing buff's mark (on an enemy and on an ally, a second one destroying the
//! first), the aim projectile's mark target and life, the capture of an enemy and of an ally (the held place, the
//! end, the knockback's release). Without a store the tests check nothing and say so.

use mvs_sim::actors::{ActorRef, ProjectileExt, ProjectileId};
use mvs_sim::chars::velma::capture::{ATTACH_TO_ALLY_N, knocked_back, start_capture};
use mvs_sim::chars::velma::mark::{AIM_PROJECTILE, SPAWN_HOMING_TARGET};
use mvs_sim::chars::velma::velma;
use mvs_sim::combat::buffs::buff_of;
use mvs_sim::combat::buffs::spawn_buff;
use mvs_sim::fighter::{FighterId, actor_y};
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, team: Some(team), ..MatchPlayer::default() }
}

fn pair(ally: bool) -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup {
        players: vec![player("velma", false, 0), player("shaggy", true, u8::from(!ally))],
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (v, s) = (h.fighter(0), h.fighter(1));
    Some((h, v, s))
}

/// The live marks of the World, in list order.
fn marks(h: &Harness) -> Vec<ProjectileId> {
    h.w.actors
        .projectiles
        .iter()
        .filter(|(_, p)| !p.dead && matches!(p.ext, ProjectileExt::Mark(_)))
        .map(|(id, _)| id)
        .collect()
}

fn mark_of(h: &Harness, id: ProjectileId) -> &mvs_sim::chars::velma::mark::Mark {
    match &h.w.actors.projectiles.get(id).expect("a mark").ext {
        ProjectileExt::Mark(m) => m,
        e => panic!("not a mark: {e:?}"),
    }
}

/// Buff_VelmaSpawnHomingTarget on an enemy: a mark made at him and stuck to him, riding his actor location; the buff
/// ends at once; a second one destroys the first.
#[test]
fn the_homing_buff_marks_an_enemy_and_a_second_replaces_it() {
    let Some((mut h, v, s)) = pair(false) else { return };
    let cls = h.w.names.get(SPAWN_HOMING_TARGET).expect("an interned class");
    spawn_buff(&mut h.w, s, cls, 5.0, Some(1), Some(v), true, None);
    let m = marks(&h);
    assert_eq!(m.len(), 1, "one mark");
    let k = mark_of(&h, m[0]);
    assert_eq!(k.target, Some(s));
    assert!(k.stuck && k.to_enemy && !k.to_ally && k.gate_done);
    assert_eq!(velma(&h.w, v).mark, Some(m[0]));
    assert!(buff_of(&h.w, s, cls).is_none(), "the buff ended");
    h.step(&NONE);
    let p = h.w.actors.projectiles.get(m[0]).expect("the mark");
    assert_eq!((p.x, p.y), (h.w.fighter(s).body.x, actor_y(&h.w, s)), "rides the enemy");
    spawn_buff(&mut h.w, s, cls, 5.0, Some(1), Some(v), true, None);
    assert!(h.w.actors.projectiles.get(m[0]).is_none_or(|p| p.dead), "the first mark is destroyed");
    assert_eq!(marks(&h).len(), 1, "one live mark");
}

/// The same buff on a team mate: the mark's ally hit (Buff_SpeedUp_C on him).
#[test]
fn the_homing_buff_marks_an_ally_and_speeds_him_up() {
    let Some((mut h, v, s)) = pair(true) else { return };
    let cls = h.w.names.get(SPAWN_HOMING_TARGET).expect("an interned class");
    spawn_buff(&mut h.w, s, cls, 5.0, Some(1), Some(v), true, None);
    let m = marks(&h);
    assert_eq!(m.len(), 1);
    let k = mark_of(&h, m[0]);
    assert!(k.to_ally && !k.to_enemy && k.stuck);
    assert!(buff_of(&h.w, s, name!("Buff_SpeedUp_C")).is_some(), "the ally is sped up");
}

/// VelmaAimProjectile: its AimAtActor is the owner's last mark, its homing is set, and it ends with its life span.
#[test]
fn the_aim_projectile_takes_the_mark_and_ends_with_its_life() {
    let Some((mut h, v, _)) = pair(false) else { return };
    let (x, y) = (h.w.fighter(v).body.x, actor_y(&h.w, v));
    let mark = comp_spawn_projectile(
        &mut h.w,
        v,
        name!("VelmaMarkProjectile_C"),
        x,
        y,
        [Some(1.0), None, None],
        100.0,
        100.0,
        None,
    )
    .expect("a mark");
    let aim_cls = h.w.names.get(AIM_PROJECTILE).expect("an interned class");
    let aim = comp_spawn_projectile(&mut h.w, v, aim_cls, x, y, [None, None, None], 0.0, 0.0, None)
        .expect("an aim projectile");
    let ProjectileExt::AimProjectile(a) = &h.w.actors.projectiles.get(aim).expect("aim").ext else {
        panic!("an aim projectile's state")
    };
    assert_eq!(a.aim_at, Some(ActorRef::Projectile(mark)));
    assert!(a.life_at > 0.0 && a.life_at.is_finite());
    assert!(h.w.actors.projectiles.get(aim).expect("aim").buff_home.is_some(), "homing on the mark");
    for _ in 0..200 {
        h.step(&NONE);
    }
    assert!(h.w.actors.projectiles.get(aim).is_none_or(|p| p.dead), "ended by its life span");
}

/// Buff_VelmaAttachToAlly on an enemy: held above her socket, then released when the HoldPose check passes (she plays
/// no HoldPose), with no launch.
#[test]
fn an_enemy_is_captured_and_let_go() {
    let Some((mut h, v, s)) = pair(false) else { return };
    let b = start_capture(&mut h.w, v, s, ActorRef::Fighter(v)).expect("the capture lives");
    assert_eq!(buff_of(&h.w, s, ATTACH_TO_ALLY_N), Some(b));
    h.step(&NONE);
    assert!(actor_y(&h.w, s) > actor_y(&h.w, v), "held above her");
    for _ in 0..40 {
        h.step(&NONE);
    }
    assert!(buff_of(&h.w, s, ATTACH_TO_ALLY_N).is_none(), "the HoldPose check ended it");
}

/// The same on a team mate, and her knockback lets him go at once.
#[test]
fn a_held_ally_is_released_by_her_knockback() {
    let Some((mut h, v, s)) = pair(true) else { return };
    start_capture(&mut h.w, v, s, ActorRef::Fighter(v)).expect("the capture lives");
    h.step(&NONE);
    assert!(buff_of(&h.w, s, ATTACH_TO_ALLY_N).is_some());
    knocked_back(&mut h.w, v, 0.0, 0.0, 0.0, None);
    assert!(buff_of(&h.w, s, ATTACH_TO_ALLY_N).is_none(), "released");
}
