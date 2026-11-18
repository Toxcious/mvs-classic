//! Shared paths that no match runs until the characters' ports land, on a fresh World or a trace's:
//! `combat::move_to_actor_2` (Gizmo's dash to an ally), `combat::dynamic_move` (Marvin's warp), the side montages and
//! the stop of `actors::actor_montage` (Jerry's actor, Jake's horse) and `combat::hits` bind_hold_charge.
//! Without a store or the traces the tests check nothing and say so.
//! Not reached: a ReachedActor binder (none is entered before Gizmo's port), the hit pause's Delay of
//! Buff_MoveToActor2, a dynamic move class with overrides (Marvin's), the Struggling listeners (Jake's) and the hold
//! charge handlers' calls (no character binds them yet).

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::actor_montage::{
    mesh_instances, montage_active, montage_is_playing, montage_play, montage_play_side, montage_stop,
};
use mvs_sim::combat::BuffClass;
use mvs_sim::combat::buffs::{BuffRef, buff, buff_mut, spawn_buff};
use mvs_sim::combat::dynamic_move::{begin_move_to_actor, dynamic_move_of};
use mvs_sim::combat::hits::{HoldChargeHandlers, bind_hold_charge};
use mvs_sim::combat::move_to_actor_2::{begin_move_to_actor_2, bind_reached_actor, move_to_actor_2_of};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::registry::CharHooks;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot: false, ..MatchPlayer::default() }
}

fn pair(a: &str, b: &str) -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player(a, 0), player(b, 0)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (x, y) = (h.fighter(0), h.fighter(1));
    Some((h, x, y))
}

fn warned(w: &World) -> Vec<String> {
    let mut v = w.warned.borrow_mut();
    let out = v.iter().map(|s| s.to_string()).collect();
    v.clear();
    out
}

fn spawn(w: &mut World, f: FighterId, cls: &str) -> BuffRef {
    let c = w.lit(cls);
    spawn_buff(w, f, c, 0.0, None, Some(f), false, Some(f)).unwrap_or_else(|| panic!("{cls} spawns: {:?}", warned(w)))
}

/// Buff_MoveToActor2 on Gizmo toward a team mate (as AtemptToReachAlly's call: no eases, the offset half a height up):
/// the source to the mate's place plus the offset, the timeline's play rate 1 / OverTime, a second call that only
/// takes the launch, a re-plan once the mate has left the planned point by more than BufferDistance (to the location
/// alone), and the end after OverTime, the source gone.
#[test]
fn a_move_to_actor_2_follows_its_actor() {
    let Some((mut h, g, m)) = pair("gizmo", "shaggy") else { return };
    let w = &mut h.w;
    let b = spawn(w, g, "Buff_MoveToActor2_C");
    let e = move_to_actor_2_of(w, b).expect("the extension");
    assert!(e.kb && e.buffer == 100.0 && !e.began);
    bind_reached_actor(w, b, "NoSuchBinder");
    assert!(warned(w).iter().any(|s| s.contains("no binder entered as NoSuchBinder")));
    let (mx, my) = (w.fighter(m).body.x, mvs_sim::fighter::actor_y(w, m));
    begin_move_to_actor_2(w, g, b, Some(ActorRef::Fighter(m)), false, false, 0.5, [0.0, 40.0], [0.0, 0.0]);
    let e = move_to_actor_2_of(w, b).expect("the extension");
    assert!(e.began && e.playing && e.pos == 0.0);
    assert_eq!((e.cx, e.cy), (mx, (my + 40.0)));
    assert_eq!(e.rate, (1.0 / 0.5));
    let id = e.id;
    let s = w.fighter(g).moves.force_move.as_ref().expect("the source");
    assert_eq!((s.id, s.tx, s.ty, s.mode), (id, e.cx, e.cy, 2));
    // The DoOnce: a second call takes the launch alone.
    begin_move_to_actor_2(w, g, b, None, true, true, 9.0, [5.0, 5.0], [300.0, 400.0]);
    let e = move_to_actor_2_of(w, b).expect("the extension");
    assert_eq!((e.lx, e.ly, e.over_time, e.dest), (300.0, 400.0, 0.5, Some(ActorRef::Fighter(m))));
    h.step(&NONE);
    let w = &mut h.w;
    let e = move_to_actor_2_of(w, b).expect("the extension");
    assert!(e.pos > 0.0 && e.pos < 0.1, "one timeline tick: {}", e.pos);
    // The mate leaves by 300: the next update plans to its location alone, for the rest of the time.
    w.fighter_mut(m).body.x += 300.0;
    h.step(&NONE);
    let w = &h.w;
    let e = move_to_actor_2_of(w, b).expect("the extension");
    let mx = w.fighter(m).body.x;
    assert!((e.cx - mx).abs() < 50.0, "re-planned: {} {}", e.cx, mx);
    let s = w.fighter(g).moves.force_move.as_ref().expect("the new source");
    assert_eq!((s.id, s.mode), (e.id, 0));
    assert!(s.dur < 0.5, "the remaining time: {}", s.dur);
    let mut n = 2;
    while buff(&h.w, b).is_some() {
        h.step(&NONE);
        n += 1;
        assert!(n < 60, "the move ends");
    }
    assert!((29..=32).contains(&n), "0.5 s of timeline: {n} steps");
}

/// Buff_DynamicMoveToActor on Shaggy toward Marvin (whose data holds the class): the source at once, the 0.1 s timer's
/// re-plans (each source for the time left), the Delay's end after Duration, and the end's removal of the source. The
/// World is not stepped (Marvin's own frame is his tracks' and panics before them): the buff's hooks are called as
/// the owner's frame calls them, timer pass then buff pass, on the owner's clock moved one frame at a time.
#[test]
fn a_dynamic_move_plans_again_on_its_timer() {
    let setup = MatchSetup { players: vec![player("shaggy", 0), player("marvin", 0)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let (a, o) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    // No fighter's data holds the parent class (Marvin's holds its children, entered by his port): a carrier buff of
    // Shaggy's data is made a Buff_DynamicMoveToActor and begun by its entry's hook, as the spawn would.
    let b = spawn(w, a, "Buff_Shaggy_ExtraHungry_C");
    let hooks = w.registry.buffs.get("Buff_DynamicMoveToActor_C").expect("entered").hooks;
    buff_mut(w, b).expect("the carrier").class = BuffClass::BuffDynamicMoveToActor;
    (hooks.begin.expect("begin"))(w, a, b, Some(a), Some(0.0));
    assert_eq!(dynamic_move_of(w, b).expect("the extension").id, -1);
    let (ox, oy) = (w.fighter(o).body.x, mvs_sim::fighter::actor_y(w, o));
    assert!(!begin_move_to_actor(w, a, b, Some(ActorRef::Fighter(o)), 0.5, true, true, false, [0.0, 0.0]));
    let e = dynamic_move_of(w, b).expect("the extension");
    let first = e.id;
    assert!(first > 0 && e.arm_at >= 0.0 && e.end_at > e.arm_at, "{e:?}");
    let s = w.fighter(a).moves.force_move.as_ref().expect("the source");
    assert_eq!((s.id, s.mode, s.ease, s.dur), (first, 2, true, 0.5));
    assert_eq!((s.tx, s.ty), (ox, oy));
    let (timer, tick, end) = (hooks.timer.expect("timer"), hooks.tick.expect("tick"), hooks.end.expect("end"));
    let mut ids = vec![first];
    let mut n = 0;
    loop {
        let c = &mut w.fighter_mut(a).clock;
        c.tick += 1;
        c.time += mvs_sim::consts::DELTA_SECONDS;
        n += 1;
        assert!(!timer(w, a, b), "the parent's timer never ends the buff");
        let id = dynamic_move_of(w, b).expect("the extension").id;
        if ids.last() != Some(&id) {
            ids.push(id);
            let s = w.fighter(a).moves.force_move.as_ref().expect("the new source");
            assert!(s.dur < 0.5 && s.dur > 0.0, "the time left: {}", s.dur);
        }
        if tick(w, a, b) {
            break;
        }
        assert!(n < 60, "the move ends");
    }
    assert!((4..=5).contains(&(ids.len() - 1)), "a re-plan per 0.1 s: {ids:?}");
    assert!((29..=31).contains(&n), "Delay(0.5): {n} frames");
    end(w, a, b);
    assert!(w.fighter(a).moves.force_move.is_none(), "the source removed");
    assert!(dynamic_move_of(w, b).expect("the extension").arm_at < 0.0, "the timer cleared");
}

/// The montages of rick-rick's Meeseeks (frame 2440) played beside its current one: the side instance listed after
/// `m`, the montage-less queries on `m`, a stop that names another montage doing nothing and one that names none
/// stopping `m`.
#[test]
fn a_meeseeks_plays_a_side_montage_and_stops() {
    let Some(mut h) = harness::at("rick-rick", 2440) else { return };
    let w = &mut h.w;
    let id = w.actors.projectiles.nth(0).expect("the Meeseeks");
    let a = ActorRef::Projectile(id);
    let look = w.lit("Meeseeks_Attack_LookAtMe_Montage");
    let golf = w.lit("Meeseeks_Attack_GolfSwing_Montage");
    let mesh = w.lit("Meeseeks_SkelMesh");
    warned(w);
    assert!(montage_play(w, a, look, 1.0, 0.0, false));
    assert!(montage_play_side(w, a, golf, 1.0, 0.0));
    assert!(!montage_play_side(w, a, mesh, 1.0, 0.0));
    assert!(warned(w).iter().any(|s| s.contains("actor montage missing: Meeseeks_SkelMesh")));
    let inst: Vec<_> = mesh_instances(w, a).iter().map(|p| p.montage.name).collect();
    assert_eq!(inst, vec![look, golf]);
    assert_eq!(montage_active(w, a), Some(look));
    assert!(montage_is_playing(w, a, look) && !montage_is_playing(w, a, golf));
    montage_stop(w, a, Some(golf));
    assert_eq!(montage_active(w, a), Some(look), "a stop of another montage");
    montage_stop(w, a, None);
    assert_eq!(montage_active(w, a), None, "stopped");
    assert!(!montage_is_playing(w, a, look));
}

/// bind_hold_charge enters the handlers on the character's hooks (a second bind is an error of the module).
#[test]
fn hold_charge_handlers_bind_once() {
    fn noop(_: &mut World, _: FighterId) {}
    let mut c = CharHooks::default();
    bind_hold_charge(&mut c, HoldChargeHandlers { reached_max: Some(noop), ended: None, used: Some(noop) });
    let h = c.hold_charge.expect("bound");
    assert!(h.reached_max.is_some() && h.ended.is_none() && h.used.is_some());
    let again = std::panic::catch_unwind(move || {
        let mut c = c;
        bind_hold_charge(&mut c, HoldChargeHandlers::default());
    });
    assert!(again.is_err());
}
