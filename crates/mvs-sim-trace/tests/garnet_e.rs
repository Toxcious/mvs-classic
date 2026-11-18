//! Garnet's stop projectile and sing paths no trace match runs (`chars/garnet/stop_projectile.rs`,
//! `chars/garnet/sing.rs`, `actors/tunnel_carry.rs`): Buff_GarnetStopProjectile on an enemy projectile (the collision
//! off, the added sphere, the stationary root), its reflect by Garnet's Sing Sphere coming back over it, and the sing
//! that gives it. Each against an oracle probe of the same World (Garnet against Shaggy, Shaggy's sandwich thrown at
//! her), the values the probe printed.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::garnet::garnet;
use mvs_sim::chars::garnet::sing::SingExt;
use mvs_sim::chars::garnet::stop_projectile::{StopExt, stop_ext_of};
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::combat::elem::{ElemExt, spawn_actor_buff};
use mvs_sim::combat::phases::SphereKey;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn place(h: &mut Harness, f: FighterId, x: mvs_sim::Real, facing: Option<mvs_sim::Real>) {
    let b = &mut h.w.fighter_mut(f).body;
    b.x = x;
    b.prev_x = x;
    if let Some(d) = facing {
        b.facing_dir = d;
    }
}

/// The probe's start: a fresh Garnet against Shaggy, landed (`probe.land()`: 10 ticks), Shaggy at x -100 facing her,
/// two ticks, his Shaggy_RockThrow_Ground_Montage; `garnet_in` the inputs of Garnet's ticks from there.
fn start(garnet_in: InputState, ticks: usize) -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player("garnet", false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (g, s) = (h.fighter(0), h.fighter(1));
    place(&mut h, s, -100.0, Some(-1.0));
    h.step(&NONE);
    h.step(&NONE);
    let m = h.w.lit("Shaggy_RockThrow_Ground_Montage");
    start_montage(&mut h.w, s, m, &PROXY_NONE, None);
    for _ in 0..ticks {
        h.step(&[garnet_in, InputState::default()]);
    }
    Some((h, g, s))
}

fn sandwich(h: &Harness) -> (ActorRef, &mvs_sim::actors::Projectile) {
    let (id, p) = h.w.actors.projectiles.iter().next().expect("Shaggy's sandwich");
    assert_eq!(h.w.names.str(p.cls), "ShaggySandwichProjectile_C");
    (ActorRef::Projectile(id), p)
}

/// The probe's `stop`: the sandwich 19 ticks out, Garnet moved away (her Sing Sphere off it), the buff given by
/// spawnActorBuff(sandwich, Buff_GarnetStopProjectile_C, 0, 0, true, w, Garnet); three ticks held; Garnet back at
/// -800, whose sphere's overlap begins on the added sphere: the reflect toward Shaggy.
#[test]
fn the_stop_holds_the_shot_and_her_sphere_reflects_it() {
    let Some((mut h, g, s)) = start(InputState::default(), 19) else { return };
    place(&mut h, g, -1400.0, None);
    h.step(&NONE);
    assert_eq!(h.w.flow.tick, 32);
    let (a, p) = sandwich(&h);
    assert_eq!(p.x, -431.844_73);
    assert_eq!(p.y, 387.342_74);
    assert_eq!((p.vx, p.hurt_radius, p.no_collision, p.mobility), (-3250.0, 32.0, false, None));
    let cls = h.w.lit("Buff_GarnetStopProjectile_C");
    let serial = spawn_actor_buff(&mut h.w, a, cls, 0, 0.0, true, Some(g)).expect("the buff");
    let want = StopExt {
        instigator: 0,
        adjusted: true,
        x: -431.844_73,
        y: 387.342_74,
        r: 150.0,
        find_scale: 100.0,
        carry: mvs_sim::actors::tunnel_carry::CarryExt { life: None, hurt: Some(32.0) },
        ..StopExt::default()
    };
    for k in 0..4 {
        if k > 0 {
            h.step(&NONE);
        }
        assert_eq!(stop_ext_of(&h.w, a, serial), Some(want), "tick {}", h.w.flow.tick);
        let (_, p) = sandwich(&h);
        assert_eq!((p.x, p.vx, p.hurt_radius, p.no_collision, p.mobility), (-431.844_73, -3250.0, 0.0, true, Some(1)));
    }
    assert_eq!(h.w.flow.tick, 35);
    place(&mut h, g, -800.0, None);
    for _ in 0..14 {
        h.step(&NONE);
        let (_, p) = sandwich(&h);
        let t = h.w.flow.tick;
        assert!(p.elem.as_ref().is_none_or(|e| e.buffs.is_empty()), "tick {t}: the buff ended");
        assert_eq!(p.owner, Some(g), "tick {t}: reflected to Garnet");
        assert_eq!((p.x, p.y, p.vx, p.vy), (-377.678_07, 387.342_74, 3250.0, 0.0), "tick {t}");
        assert_eq!((p.hurt_radius, p.no_collision, p.mobility), (32.0, false, Some(1)), "tick {t}");
    }
    let _ = s;
}

/// The probe's `sing`: Garnet holds special from Shaggy's throw on (40 ticks). The notify's SetSinging(true) at tick
/// 26 gives her Buff_GarnetStackingSinging (its Increase Stack Timer 30 ticks, pending the first); the sandwich met in
/// the sphere at 31 gets the stop buff with the sphere's overlap recorded (`o0.p0`), and its collision off ends the
/// sphere's overlap (both lists empty); the sing's end at 54 pauses the timer and sets the 5 s stack life.
// Runs her neutral special whole: its release (the Cancel row at 53) takes the GarnetIsNotSinging / Sing-ready
// conditions of `chars/garnet/pawn.rs`.
#[test]
fn the_sing_stops_a_shot_in_her_sphere() {
    let special = InputState { special: true, ..InputState::default() };
    let Some((mut h, g, _)) = start(special, 0) else { return };
    let stacking = h.w.lit("Buff_GarnetStackingSinging_C");
    let sing = |h: &Harness| -> Option<(mvs_sim::Real, mvs_sim::Real, SingExt)> {
        let b = h.w.fighter(g).buffs.buffs.iter().find(|b| b.cls == stacking)?;
        match &b.ext {
            BuffExt::StackingSinging(e) => Some((b.stacks, b.stack_frames, e.clone())),
            _ => None,
        }
    };
    let ext = |active: mvs_sim::Real, inc: mvs_sim::Real, paused: bool| SingExt {
        active,
        inc,
        paused,
        prev: 1.0,
        ..SingExt::default()
    };
    for k in 0..90 {
        let inp = if k < 40 { special } else { InputState::default() };
        h.step(&[inp, InputState::default()]);
        let t = h.w.flow.tick;
        let gs = garnet(&h.w, g);
        assert_eq!(gs.is_active_singing, (26..54).contains(&t), "tick {t}: IsActiveSinging");
        assert!(
            gs.sing.inside.is_empty() && gs.sing.overlapping_sing_actors.is_empty(),
            "tick {t}: the sphere's lists"
        );
        match t {
            ..26 => assert!(sing(&h).is_none(), "tick {t}: no singing buff yet"),
            26..54 => {
                let (n, life, e) = sing(&h).expect("the singing buff");
                assert_eq!((n, life), (1.0, mvs_sim::Real::INFINITY), "tick {t}");
                assert_eq!(format!("{e:?}"), format!("{:?}", ext(1.0, 56.0 - mvs_sim::real(t), false)), "tick {t}");
            }
            _ => {
                let (n, life, e) = sing(&h).expect("the singing buff");
                assert_eq!((n, life), (1.0, 355.0 - mvs_sim::real(t)), "tick {t}");
                assert_eq!(format!("{e:?}"), format!("{:?}", ext(0.0, 3.0, true)), "tick {t}");
            }
        }
        if t >= 31 {
            let (a, p) = sandwich(&h);
            assert_eq!((p.x, p.y, p.no_collision, p.hurt_radius), (-377.678_07, 387.342_74, true, 0.0));
            let e = p.elem.as_ref().map(|e| e.buffs.iter().map(|b| b.ext).collect::<Vec<_>>()).unwrap_or_default();
            let [ElemExt::GarnetStopProjectile(e)] = e[..] else { panic!("tick {t}: one stop buff on {a:?}: {e:?}") };
            assert_eq!((e.instigator, e.adjusted, e.x, e.r, e.find_scale), (0, true, -377.678_07, 150.0, 100.0));
            assert_eq!(e.keys.iter().collect::<Vec<_>>(), vec![SphereKey::Pawn(0, 0)], "tick {t}");
        }
    }
}

/// The set holds past the 17th key (the oracle's set has no bound) and drops, never panics, past its cap.
#[test]
fn the_sphere_key_set_takes_more_than_sixteen_keys() {
    use mvs_sim::combat::phases::{SPHERE_KEYS, SphereKeys};
    let mut keys = SphereKeys::default();
    for i in 0..17 {
        assert!(keys.insert(SphereKey::Pawn(7, i)));
    }
    assert_eq!(keys.len(), 17);
    for i in 17..u32::try_from(SPHERE_KEYS).unwrap() + 5 {
        keys.insert(SphereKey::Pawn(7, i));
    }
    assert_eq!(keys.len(), SPHERE_KEYS);
    assert!(keys.is_full());
    assert!(!keys.insert(SphereKey::Pawn(7, 9999)), "past the cap a key is dropped");
    assert!(!keys.contains(&SphereKey::Pawn(7, 9999)));
}
