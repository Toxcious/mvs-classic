//! Stripe's perk paths no trace match runs (`chars/stripe/perks.rs`, `chars/stripe/dash.rs`): Buff_C023B_Leap's
//! LaunchNextLeap on the BeforeLeapInitiated signal, Buff_C023B_PerkAttach's transfer on SendAttachedDispatch, and
//! Buff_C023B_Dynamite's explosion on the bomb's self bounce. stripe-stripe runs the bomb and the dash only without
//! these perks, so the tests start from a frame of it and give the buffs themselves.

use mvs_sim::chars::stripe::state::stripe;
use mvs_sim::combat::buffs::{buff_of, buff_signal, spawn_buff};
use mvs_sim::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_sim_trace::harness;

const AT: usize = 200;

#[test]
fn leap_takes_the_leap_only_for_its_own_team() {
    let Some(mut h) = harness::at("stripe-stripe", AT) else { return };
    let f = h.fighter(0);
    let cls = h.w.lit("Buff_C023B_Leap_C");
    spawn_buff(&mut h.w, f, cls, 0.0, Some(0), Some(f), true, None);
    assert!(!stripe(&h.w, f).perks.launch_next_leap, "false at the buff's BeginPlay");
    let other = h.w.fighter(f).team.team + 1;
    buff_signal(
        &mut h.w,
        f,
        BuffSignal::BeforeLeapInitiated,
        BuffSignalArg { team: Some(other), ..Default::default() },
    );
    assert!(!stripe(&h.w, f).perks.launch_next_leap, "another team's projectile: no long leap");
    let own = h.w.fighter(f).team.team;
    buff_signal(&mut h.w, f, BuffSignal::BeforeLeapInitiated, BuffSignalArg { team: Some(own), ..Default::default() });
    assert!(stripe(&h.w, f).perks.launch_next_leap, "his own team's projectile: the long leap");
}

#[test]
fn feeding_after_midnight_moves_the_debuffs_to_the_enemy() {
    let Some(mut h) = harness::at("stripe-stripe", AT) else { return };
    let (f, enemy) = (h.fighter(0), h.fighter(1));
    let attach = h.w.lit("Buff_C023B_PerkAttach_C");
    spawn_buff(&mut h.w, f, attach, 0.0, Some(0), Some(f), true, None);
    assert!(buff_of(&h.w, f, attach).is_some(), "the PerkAttach buff");
    // the first debuff class the match's data has
    let mut fear = None;
    for n in ["Buff_Weakened_C", "Buff_Fear_C", "Buff_Ice_C", "Buff_Slow_C", "Buff_FearStack_C"] {
        let Some(c) = h.w.names.get(n) else { continue };
        spawn_buff(&mut h.w, f, c, 2.0, Some(1), Some(enemy), true, None);
        if buff_of(&h.w, f, c).is_some() {
            fear = Some(c);
            break;
        }
    }
    let fear = fear.expect("a debuff class in the data");
    mvs_sim::chars::stripe::perks::attached_to_enemy(&mut h.w, f, enemy);
    assert!(buff_of(&h.w, f, fear).is_none(), "the debuff left Stripe");
    assert!(buff_of(&h.w, enemy, fear).is_some(), "and is on the enemy");
}

#[test]
fn dynamite_explodes_on_the_self_bounce_only_while_the_buff_lives() {
    let Some(mut h) = harness::at("stripe-stripe", AT) else { return };
    let (f, enemy) = (h.fighter(0), h.fighter(1));
    let (fx, fy) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    {
        let b = &mut h.w.fighter_mut(enemy).body;
        b.x = fx + 100.0;
        b.prev_x = fx + 100.0;
        b.y = fy;
        b.prev_y = fy;
    }
    let before = h.w.events.len();
    mvs_sim::chars::stripe::perks::self_bounce(&mut h.w, f);
    assert_eq!(h.w.events.len(), before, "no Dynamite: no explosion");
    let cls = h.w.lit("Buff_C023B_Dynamite_C");
    spawn_buff(&mut h.w, f, cls, 0.0, Some(0), Some(f), true, None);
    mvs_sim::chars::stripe::perks::self_bounce(&mut h.w, f);
    assert!(h.w.events.len() > before, "the explosion's effect and sound");
}
