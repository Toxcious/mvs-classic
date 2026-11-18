//! Combat paths no trace match runs: a buff actor's sourceless hit as PreviousAttacker (`combat::electric`), the
//! ShieldBreakHit at Steven's last shield charge (`chars::steven::shield`), Buff_Flatten (`combat::flatten`) and a
//! spawned actor's own hit pause (`combat::spawned_hits`). On fresh Worlds; without a store they check nothing.

use mvs_sim::actors::ActorRef;
use mvs_sim::combat::buffs::{buff_of, end_buff, spawn_buff};
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn pair(a: &str, b: &str) -> Option<Harness> {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player(a, 0), player(b, 1)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    Some(h)
}

#[test]
fn a_sourceless_electric_hit_makes_the_buff_actor_previous_attacker() {
    let Some(mut h) = pair("wonderwoman", "shaggy") else { return };
    let (ww, s) = (h.fighter(0), h.fighter(1));
    let cls = h.w.lit("Buff_Low_Damage_Electric_C");
    h.w.fighter_mut(s).attackers.previous_attacker = Some(ActorRef::Fighter(ww));
    let before = h.w.fighter(s).stats.damage;
    let b = spawn_buff(&mut h.w, s, cls, 5.0, Some(0), None, true, None).expect("the buff spawns");
    h.step(&NONE);
    assert!(h.w.fighter(s).stats.damage > before, "the Trigger's pulse hit the wearer");
    assert_eq!(h.w.fighter(s).attackers.previous_attacker, Some(ActorRef::Buff(b)), "the buff actor");
    assert!(h.w.fighter(s).attackers.hit_by_multiple_attackers, "another attacker than the last");
}

#[test]
fn stevens_last_shield_charge_launches_him() {
    let Some(mut h) = pair("steven", "shaggy") else { return };
    let st = h.fighter(0);
    mvs_sim::chars::steven::state::steven_w(&mut h.w, st).shield.charges = 1.0;
    let dmg = h.w.fighter(st).stats.damage;
    mvs_sim::chars::steven::shield::decrement_shield_from_hit(&mut h.w, st);
    let f = h.w.fighter(st);
    assert!(f.hit.hitpause > 0.0, "ShieldBreakHit's HitPauseTime");
    assert!(f.hit.launch.is_some() || f.hit.hitstun > 0.0, "launched by BaseKnockBack 500");
    assert_eq!(f.stats.damage, dmg, "Damage 0");
}

#[test]
fn flatten_sets_the_bounce_and_clears_the_angles() {
    let Some(mut h) = pair("velma", "shaggy") else { return };
    let (v, s) = (h.fighter(0), h.fighter(1));
    let cls = h.w.lit("Buff_Flatten_C");
    {
        let r = &mut h.w.fighter_mut(s).react;
        r.bounce_pitch = 30.0;
        r.knock_back_angle = 45.0;
    }
    let b = spawn_buff(&mut h.w, s, cls, 2.0, Some(0), Some(v), true, Some(v)).expect("the buff spawns");
    let f = h.w.fighter(s);
    assert_eq!(f.hit.anim_bounce, Some(3.0), "OnIsBouncing(3)");
    assert_eq!(f.react.bounce_pitch, 0.0);
    assert_eq!(f.react.knock_back_angle, 0.0);
    assert!(f.hit.hitpause >= 299.0, "a pause of 5 s: {}", f.hit.hitpause);
    end_buff(&mut h.w, s, b);
    h.step(&NONE);
    assert!(buff_of(&h.w, s, cls).is_none(), "ended");
    let f = h.w.fighter(s);
    assert!(f.hit.hitpause <= 1.0, "the pause of 0.01 s: {}", f.hit.hitpause);
}

#[test]
fn a_spit_destroyed_in_its_own_pause_is_held_and_dies_at_the_pause_end() {
    let Some(mut h) = harness::at_baseline("creature-shaggy", 0) else { return };
    let spit = h.w.lit("CreatureSpit_C");
    let mut found = None;
    for _ in 0..3000 {
        h.step_recorded();
        found = h.w.actors.projectiles.iter().find(|(_, p)| p.cls == spit && !p.dead).map(|(id, _)| id);
        if found.is_some() {
            break;
        }
    }
    let id = found.expect("a spit in creature-shaggy");
    let a = ActorRef::Projectile(id);
    assert!(mvs_sim::combat::spawned_hits::receives_hit_pause(&h.w, a), "CreatureSpit_C's Comp_HitPause");
    let hit = mvs_sim::combat::HitPacket { hit_pause_time: Some(0.1), ..mvs_sim::combat::HitPacket::default() };
    mvs_sim::combat::spawned_hits::dealer_hit_pause(&mut h.w, a, &hit, 0.1);
    let p = h.w.actors.projectiles.get(id).expect("live");
    let n = p.hit_pause.expect("paused");
    assert!((6..=7).contains(&n), "0.1 s: {n} frames");
    let (x, y) = (p.x, p.y);
    // DestroySpawnedActor(false) in the pause: DestroyAfterHitPause, the actor kept till the pause ends
    mvs_sim::world::spawning::destroy_spawned_actor(&mut h.w, a, 0.0);
    let p = h.w.actors.projectiles.get(id).expect("not destroyed");
    assert!(!p.dead && p.destroy_after_pause);
    h.step(&NONE);
    let p = h.w.actors.projectiles.get(id).expect("held");
    assert_eq!((p.x, p.y), (x, y), "held still");
    assert_eq!(p.hit_pause, Some(n - 1));
    // OnHitPauseEnded -> BPI_HitPauseEnd @5176 -> DestroySpawnedActor(false)
    for _ in 1..n {
        h.step(&NONE);
    }
    assert!(h.w.actors.projectiles.get(id).is_none_or(|p| p.dead), "destroyed at the pause's end");
}
