//! Gizmo's music, bow slow and arrow, the paths no match runs (`chars/gizmo/music.rs`, `bow.rs`): the Music buffs'
//! stack, evade and knock-back handlers, the pickup's gate and its pick-up by an ally, the piano note's bubble, the
//! debuff's Silenced. The expected values are read off the oracle's source, not an oracle probe.
//! Without a store the tests check nothing and say so.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::combat::buffs::{buff, buff_of, buff_signal, spawn_buff};
use mvs_sim::events::UiStateOp;
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::name;
use mvs_sim::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot: false, ..MatchPlayer::default() }
}

/// Gizmo (fighter 0) and Shaggy (fighter 1) on one team, ten ticks in.
fn pair() -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player("gizmo", 0), player("shaggy", 0)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (g, s) = (h.fighter(0), h.fighter(1));
    Some((h, g, s))
}

fn ui_ids(h: &Harness, f: FighterId) -> Vec<String> {
    h.w.fighter(f)
        .controls
        .ui_state_ops
        .iter()
        .map(|o| match o {
            UiStateOp::Add(p) => format!("add {} {}", h.w.names.str(p.id), p.current_value),
            UiStateOp::Remove(n) => format!("remove {}", h.w.names.str(*n)),
        })
        .collect()
}

/// Music_Buff on an ally: its begin puts the state widget up (Count / MaxStacks), a knock-back takes a stack and the
/// last one ends it with the widget's remove.
#[test]
fn the_music_buff_shows_its_stacks_and_a_knock_back_takes_one() {
    let Some((mut h, g, s)) = pair() else { return };
    let cls = name!("C023A_Music_Buff_C");
    h.w.fighter_mut(s).controls.ui_state_ops.clear();
    spawn_buff(&mut h.w, s, cls, 0.0, Some(2), Some(g), true, Some(s));
    let b = buff_of(&h.w, s, cls).expect("the buff");
    assert_eq!(buff(&h.w, b).map(|x| x.stacks), Some(2.0));
    assert!(ui_ids(&h, s).iter().any(|o| o.starts_with("add MusicDebuff")), "{:?}", ui_ids(&h, s));
    buff_signal(&mut h.w, s, BuffSignal::KnockedBack, BuffSignalArg::default());
    assert_eq!(buff(&h.w, b).map(|x| x.stacks), Some(1.0));
    buff_signal(&mut h.w, s, BuffSignal::KnockedBack, BuffSignalArg::default());
    assert!(buff_of(&h.w, s, cls).is_none(), "the last stack ends the buff");
    assert!(ui_ids(&h, s).iter().any(|o| o == "remove MusicDebuff"));
}

/// An evade with the Music_Buff on: Gizmo's Music_Projectile is spawned attached to the ally (riding its actor
/// location, facing it) and a stack goes.
#[test]
fn an_evade_spawns_the_music_projectile_on_the_ally() {
    let Some((mut h, g, s)) = pair() else { return };
    let cls = name!("C023A_Music_Buff_C");
    spawn_buff(&mut h.w, s, cls, 0.0, Some(2), Some(g), true, Some(s));
    let b = buff_of(&h.w, s, cls).expect("the buff");
    let arg = BuffSignalArg { dir: Some(6.0), ..BuffSignalArg::default() };
    buff_signal(&mut h.w, s, BuffSignal::EvadeTriggered, arg);
    assert_eq!(buff(&h.w, b).map(|x| x.stacks), Some(1.0));
    let found = h.w.actors.projectiles.iter().find_map(|(_, p)| match &p.ext {
        ProjectileExt::MusicProjectile(m) => Some((m.ally, p.scale, p.attached)),
        _ => None,
    });
    assert_eq!(found, Some((Some(s), 4.0, true)));
    h.step(&NONE);
    let (_, p) =
        h.w.actors.projectiles.iter().find(|(_, p)| matches!(p.ext, ProjectileExt::MusicProjectile(_))).expect("alive");
    assert_eq!((p.x, p.y), (h.w.fighter(s).body.x, mvs_sim::fighter::actor_y(&h.w, s)));
}

/// Music_DeBuff at its max stacks: Silenced for 8 s on the owner and the debuff gone.
#[test]
fn the_music_debuff_at_max_stacks_silences() {
    let Some((mut h, g, s)) = pair() else { return };
    let cls = name!("C023A_Music_DeBuff_C");
    let max = 8;
    for _ in 0..max {
        spawn_buff(&mut h.w, s, cls, 0.0, Some(1), Some(g), true, Some(s));
    }
    h.step(&NONE);
    assert!(buff_of(&h.w, s, name!("C023A_Buff_Silenced_C")).is_some(), "{:?}", ui_ids(&h, s));
}

/// A pickup of Gizmo's spawned at his ally: its gate opens after 0.3 s, then the ally in PickUpCheck takes it and
/// gets one stack of the Music_Buff.
#[test]
fn a_music_pickup_gives_its_ally_a_stack() {
    let Some((mut h, g, s)) = pair() else { return };
    let cls_pickup = h.w.lit("C023A_MusicPickup_C");
    let (x, y) = (h.w.fighter(s).body.x, mvs_sim::fighter::actor_y(&h.w, s));
    let id = mvs_sim::world::spawning::comp_spawn_projectile(
        &mut h.w,
        g,
        cls_pickup,
        x,
        y,
        [Some(0.0), Some(0.0), Some(0.0)],
        0.0,
        0.0,
        None,
    )
    .expect("the pickup spawns");
    let gate = match &h.w.actors.projectiles.get(id).expect("listed").ext {
        ProjectileExt::MusicPickup(m) => (m.can_start_following, m.can_follow, m.pickup_gate, m.linger_at),
        _ => panic!("a pickup"),
    };
    assert_eq!(gate, (true, true, false, -1));
    for _ in 0..40 {
        h.step(&NONE);
    }
    let cls = name!("C023A_Music_Buff_C");
    let b = buff_of(&h.w, s, cls).expect("the ally holds the buff");
    assert_eq!(buff(&h.w, b).map(|x| x.stacks), Some(1.0));
    assert!(h.w.actors.projectiles.get(id).is_none_or(|p| p.dead), "the pickup is gone");
    let _ = ActorRef::Fighter(s);
}

/// The piano note's bubble: the hit circle grows with the timeline from the tick after the spawn.
#[test]
fn the_piano_note_bubble_grows_the_hit_circle() {
    let Some((mut h, g, _)) = pair() else { return };
    let cls_note = h.w.lit("C023A_PianoNote_C");
    let (x, y) = (h.w.fighter(g).body.x, mvs_sim::fighter::actor_y(&h.w, g));
    let id = mvs_sim::world::spawning::comp_spawn_projectile(
        &mut h.w,
        g,
        cls_note,
        x,
        y,
        [Some(0.0), Some(1.0), Some(0.0)],
        0.0,
        0.0,
        None,
    )
    .expect("the note spawns");
    let r0 = h.w.actors.projectiles.get(id).expect("listed").hit_radius;
    h.step(&NONE);
    h.step(&NONE);
    let p = h.w.actors.projectiles.get(id).expect("alive");
    let ProjectileExt::PianoNote(n) = &p.ext else { panic!("a note") };
    assert!(n.bubble > 0.0 && n.born_at >= 0);
    assert!(p.hit_radius != r0, "the circle follows the bubble");
}

/// The Dance perk: a piano note spawned while Gizmo has Buff_C023A_Dance takes DanceCount in its first On Hit Buffs
/// row (the match's seeded run loses that instance variable: the record does not hold it).
#[test]
fn the_dance_perk_swaps_the_notes_first_hit_buff_for_dance_count() {
    let Some((mut h, g, _)) = pair() else { return };
    let dance = h.w.lit("Buff_C023A_Dance_C");
    spawn_buff(&mut h.w, g, dance, 0.0, Some(1), Some(g), true, Some(g));
    let cls_note = h.w.lit("C023A_PianoNote_C");
    let (x, y) = (h.w.fighter(g).body.x, mvs_sim::fighter::actor_y(&h.w, g));
    let id = mvs_sim::world::spawning::comp_spawn_projectile(
        &mut h.w,
        g,
        cls_note,
        x,
        y,
        [Some(0.0), Some(1.0), Some(0.0)],
        0.0,
        0.0,
        None,
    )
    .expect("the note spawns");
    let p = h.w.actors.projectiles.get(id).expect("listed");
    let applied = mvs_sim::actors::projectile::class_p(&h.w, p)
        .get("On Hit Buffs")
        .and_then(|v| v.as_obj())
        .and_then(|o| o.get("OnHitBuffs"))
        .and_then(|v| v.as_arr())
        .and_then(|r| r.first())
        .and_then(|r| r.as_obj())
        .and_then(|r| r.get("AppliedBuffs"))
        .and_then(|v| v.as_str().map(str::to_owned));
    assert_eq!(applied.as_deref(), Some("C023A_DanceCount_Debuff_C"));
}
