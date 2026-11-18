//! The taken pass's fields beyond Damage / KnockBackScaling / HitStunMult (`combat::buffs` apply_taken_hit_mods:
//! HitPauseTime, KnockBackDirectionVector) and the fire pulse's NotifyAllyOfHit and its PreviousAttacker when the
//! Instigator is no fighter, on the paths the matches may not take. On a fresh World; without a store it checks
//! nothing. OnHitEffectOverride has no test: the two classes with a taken one (Buff_AryaSetupStart,
//! Buff_SideDirectionalKnockbackIncrease) are referenced by nothing of the build.

use mvs_sim::combat::buffs::{BuffRef, TakenHit, apply_taken_hit_mods, buff_of, spawn_buff};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim_trace::harness::{self, Harness};

fn fresh(players: &[(&str, u8)]) -> Option<Harness> {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: players.iter().map(|&(c, t)| player(c, t)).collect(), ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    let none = vec![NO_INPUT; players.len()];
    for _ in 0..10 {
        h.step(&none);
    }
    Some(h)
}

fn spawn(w: &mut World, f: FighterId, cls: &str, dur: f32, by: Option<FighterId>) -> BuffRef {
    let c = w.lit(cls);
    spawn_buff(w, f, c, dur, None, by, false, by).unwrap_or_else(|| panic!("{cls} spawns"))
}

const HIT: TakenHit =
    TakenHit { damage: 10.0, scaling: 1.0, hit_stun: 1.0, hit_pause: 0.1, dir: (1.0, 0.5), effect: None };

#[test]
fn iron_giants_bolts_override_the_hit_pause_and_the_direction() {
    let Some(mut h) = fresh(&[("irongiant", 0), ("shaggy", 1)]) else { return };
    let f = h.fighter(0);
    let w = &mut h.w;
    spawn(w, f, "Buff_C017_Passive_C", 0.0, Some(f));
    let t = apply_taken_hit_mods(w, f, HIT, None, true, None, None);
    assert_eq!((t.scaling, t.hit_stun, t.hit_pause, t.dir), (0.0, 0.0, 0.0, (0.0, 0.0)));
    assert_eq!(t.damage, 10.0);
}

#[test]
fn a_bubbled_enemys_direction_is_multiplied_to_zero() {
    let Some(mut h) = fresh(&[("steven", 0), ("shaggy", 1)]) else { return };
    let (s, f) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    // Buff_StevenBubbleEnemy: every type Multiply with BuffParentActor's mods (0); HitPauseMod DoNothing.
    spawn(w, f, "Buff_StevenBubbleEnemy_C", 5.0, Some(s));
    let t = apply_taken_hit_mods(w, f, HIT, None, true, None, None);
    assert_eq!((t.damage, t.hit_pause, t.dir, t.effect), (0.0, HIT.hit_pause, (0.0, 0.0), None));
}

/// Steps until the fighter's damage changes (the first pulse), at most 300 frames.
fn to_pulse(h: &mut Harness, f: FighterId, n: usize) {
    let none = vec![NO_INPUT; n];
    let d0 = h.w.fighter(f).stats.damage;
    for _ in 0..300 {
        h.step(&none);
        if h.w.fighter(f).stats.damage != d0 {
            return;
        }
    }
    panic!("no fire pulse");
}

#[test]
fn a_team_mates_fire_pulse_notifies_the_ally() {
    for (mate, freed) in [(1, true), (2, false)] {
        let players = [("arya", 0), ("shaggy", 0), ("batman", 1)];
        let Some(mut h) = fresh(&players) else { return };
        let (f, by) = (h.fighter(0), h.fighter(mate));
        let enemy = h.fighter(2);
        let fear = h.w.lit("Buff_Fear_C");
        spawn(&mut h.w, f, "Buff_Fear_C", 10.0, Some(enemy));
        assert!(buff_of(&h.w, f, fear).is_some());
        spawn(&mut h.w, f, "Buff_Fire_C", 0.0, Some(by));
        to_pulse(&mut h, f, players.len());
        // Buff_Fear's DestroyAndBreak is bound to HitByAlly: a team mate's pulse ends it, an enemy's does not.
        assert_eq!(buff_of(&h.w, f, fear).is_none(), freed, "the Instigator {mate}");
    }
}

#[test]
/// A fire with no Instigator notes the buff actor as the attacker (DamageAndLaunch's LocalDmgDlrOwner @8521-8641); the
/// The oracle notes one only `if (b.src)`.
fn a_fire_with_no_fighter_instigator_notes_the_buff_actor() {
    let Some(mut h) = fresh(&[("shaggy", 0), ("batman", 1)]) else { return };
    let f = h.fighter(0);
    let b = spawn(&mut h.w, f, "Buff_Fire_C", 0.0, None);
    to_pulse(&mut h, f, 2);
    assert_eq!(h.w.fighter(f).attackers.previous_attacker, Some(mvs_sim::actors::ActorRef::Buff(b)));
}
