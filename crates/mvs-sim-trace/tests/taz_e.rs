//! Taz's projectile eat no trace match runs (`chars/taz/projectile_eat.rs`, `actors/tunnel_carry.rs`):
//! Buff_C015HitProjectile on an enemy projectile, its collision off, the pull to Taz's mouth while
//! ShrinkDownProjectileTimeline shrinks it, the stored projectile and the meter, and the destroy at the timeline's end.
//! Against an oracle probe of the same World (Taz against Shaggy, Shaggy's sandwich thrown at him), the values it printed.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::taz::projectile_eat::HitProjExt;
use mvs_sim::chars::taz::state::taz_of;
use mvs_sim::combat::elem::{ElemExt, spawn_actor_buff};
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

const NONE: [mvs_sim::input::InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

/// The probe's `eat`: Taz landed (`probe.land()`: 13 ticks), Shaggy at x -100 facing him, two ticks, his
/// Shaggy_RockThrow_Ground_Montage, 19 ticks; then spawnActorBuff(sandwich, Buff_C015HitProjectile_C, 0, 0, true, w,
/// Taz) and the ticks to the destroy.
#[test]
fn taz_eats_the_shot() {
    let setup = MatchSetup { players: vec![player("taz", false), player("shaggy", true)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..13 {
        h.step(&NONE);
    }
    let (t, s) = (h.fighter(0), h.fighter(1));
    {
        let b = &mut h.w.fighter_mut(s).body;
        b.x = -100.0;
        b.prev_x = -100.0;
        b.facing_dir = -1.0;
    }
    h.step(&NONE);
    h.step(&NONE);
    let m = h.w.lit("Shaggy_RockThrow_Ground_Montage");
    start_montage(&mut h.w, s, m, &PROXY_NONE, None);
    for _ in 0..19 {
        h.step(&NONE);
    }
    assert_eq!(h.w.flow.tick, 34);
    let (id, p) = h.w.actors.projectiles.iter().next().expect("Shaggy's sandwich");
    assert_eq!(h.w.names.str(p.cls), "ShaggySandwichProjectile_C");
    assert_eq!((p.x, p.y, p.hurt_radius), (-377.678_07, 387.342_74, 32.0));
    assert_eq!((h.w.fighter(t).body.x, h.w.fighter(t).body.facing_dir), (-800.0, 1.0));
    let a = ActorRef::Projectile(id);
    let cls = h.w.lit("Buff_C015HitProjectile_C");
    spawn_actor_buff(&mut h.w, a, cls, 0, 0.0, true, Some(t)).expect("the buff");

    let ext = |h: &harness::Harness| -> HitProjExt {
        let p = h.w.actors.projectiles.get(id).expect("the sandwich");
        let e = p.elem.as_ref().map(|e| e.buffs.iter().map(|b| b.ext).collect::<Vec<_>>()).unwrap_or_default();
        let [ElemExt::C015HitProjectile(e)] = e[..] else { panic!("one eat buff: {e:?}") };
        e
    };
    let want = HitProjExt {
        act: Some(true),
        t: Some(0.0),
        rate: Some(5.0),
        sx: Some(1.0),
        fl: Some(false),
        inst: Some(0),
        carry: mvs_sim::actors::tunnel_carry::CarryExt { life: None, hurt: Some(32.0) },
    };
    assert_eq!(ext(&h), want);
    let p = h.w.actors.projectiles.get(id).unwrap();
    assert_eq!((p.x, p.y, p.vx, p.scale, p.no_collision, p.hurt_radius), (-450.0, 180.412, 0.0, 1.0, true, 0.0));
    let ts = taz_of(&h.w, t).expect("Taz's state");
    assert_eq!(h.w.names.str(ts.stored_projectile), "ShaggySandwichProjectile_C");
    assert_eq!(
        (ts.stored_starting_speed, ts.stored_max_speed, ts.projectiles_being_eaten, ts.eat_level),
        (3250.0, 3250.0, 1.0, 1.0)
    );

    let xs = [
        -479.166_9,
        -508.333_8,
        -537.500_73,
        -566.667_6,
        -595.834_5,
        -625.001_4,
        -654.168_33,
        -683.335_2,
        -712.502_1,
        -741.668_95,
        -770.835_9,
    ];
    let scales = [
        0.916_666_03,
        0.833_332,
        0.749_998,
        0.666_664,
        0.583_330_04,
        0.499_996,
        0.416_661_98,
        0.333_328,
        0.249_994_04,
        0.166_660_07,
        0.083_326_1,
    ];
    let times = [
        0.083_334,
        0.166_668,
        0.250_002,
        0.333_336,
        0.416_67,
        0.500_004,
        0.583_338,
        0.666_672,
        0.750_005_96,
        0.833_339_9,
        0.916_673_9,
    ];
    for k in 0..11 {
        h.step(&NONE);
        let tick = h.w.flow.tick;
        let p = h.w.actors.projectiles.get(id).expect("the sandwich");
        assert_eq!((p.x, p.y, p.scale, p.dead), (xs[k], 180.412, scales[k], false), "tick {tick}");
        assert_eq!(ext(&h), HitProjExt { t: Some(times[k]), ..want }, "tick {tick}");
    }
    h.step(&NONE);
    assert_eq!(h.w.flow.tick, 46);
    assert_eq!(h.w.actors.projectiles.iter().count(), 0, "the sandwich destroyed");
    let ts = taz_of(&h.w, t).expect("Taz's state");
    assert_eq!((ts.projectiles_being_eaten, ts.eat_level), (0.0, 1.0));
    assert_eq!(h.w.names.str(ts.stored_projectile), "ShaggySandwichProjectile_C");
}
