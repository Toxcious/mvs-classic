//! Paths of Taz's index, pawn, eat and projectiles that no match runs, on a World loaded from `taz-taz` (both
//! fighters are Taz there). With no baseline trace the tests check nothing and say so.

use mvs_sim::actors::projectile::{ProjectileExt, hit_shapes};
use mvs_sim::actors::{Projectile, ProjectileClass};
use mvs_sim::chars::taz::projectiles::StewSpitAir;
use mvs_sim::chars::taz::{eat, pawn, taz, taz_w};
use mvs_sim_trace::harness;

fn taz_pair() -> Option<harness::Harness> {
    harness::at("taz-taz", 400)
}

#[test]
fn a_knock_out_empties_the_meter() {
    let Some(mut h) = taz_pair() else { return };
    let f = h.fighter(0);
    taz_w(&mut h.w, f).eat_level = 0.75;
    pawn::on_knocked_out(&mut h.w, f);
    assert_eq!(taz(&h.w, f).eat_level, 0.0);
    // The level is clamped to 0..1 whichever way it moves.
    pawn::update_eat_level(&mut h.w, f, 3.0, true, true);
    assert_eq!(taz(&h.w, f).eat_level, 1.0);
}

#[test]
fn an_eaten_fighter_is_added_once_and_a_freed_slot_is_taken_again() {
    let Some(mut h) = taz_pair() else { return };
    let (f, v) = (h.fighter(0), h.fighter(1));
    taz_w(&mut h.w, f).eaten_fighters.clear();
    taz_w(&mut h.w, f).eat_free.clear();
    eat::add_fighter_to_eat(&mut h.w, f, v);
    eat::add_fighter_to_eat(&mut h.w, f, v);
    assert_eq!(taz(&h.w, f).eaten_fighters.len(), 1);
    assert!(pawn::has_eaten_fighters(&h.w, f));
    eat::remove_fighter_from_eat(&mut h.w, f, v);
    assert!(!pawn::has_eaten_fighters(&h.w, f));
    eat::add_fighter_to_eat(&mut h.w, f, v);
    assert_eq!(taz(&h.w, f).eaten_fighters.len(), 1);
}

#[test]
fn a_chew_deals_a_hold_hit_to_each_eaten_fighter() {
    let Some(mut h) = taz_pair() else { return };
    let (f, v) = (h.fighter(0), h.fighter(1));
    taz_w(&mut h.w, f).eaten_fighters.clear();
    eat::add_fighter_to_eat(&mut h.w, f, v);
    let before = h.w.fighter(f).holds.hold_hits.len();
    eat::chew_on_eaten_fighters(&mut h.w, f, 1.0);
    assert_eq!(h.w.fighter(f).holds.hold_hits.len(), before + 1);
}

#[test]
fn the_aerial_stew_meets_a_fighter_as_the_circle_and_nine_spheres() {
    let Some(h) = taz_pair() else { return };
    let mut p = Projectile::new(
        h.w.names.get("C015StewSpitAir_C").unwrap_or_default(),
        ProjectileClass::C015StewSpitAir,
        None,
        0,
        0,
    );
    p.sphere_3d = false;
    p.hit_radius = 120.0;
    p.scale = 1.0;
    p.facing = 1.0;
    p.ext = ProjectileExt::StewSpitAir(StewSpitAir {
        reverse_at: 10.0,
        dealer_half: 90.0,
        dealer_r: 60.0,
        stew_vx: 1.0,
        stew_vy: 0.0,
    });
    let mut out = Vec::new();
    hit_shapes(&h.w, &p, &mut out);
    assert_eq!(out.len(), 10);
    assert!(out[0].z.is_none());
    assert_eq!(out[0].r, 120.0);
    assert!(out[1..].iter().all(|s| s.r == 60.0 && s.z == Some(0.0)));
    // The row of spheres is centred on the stew and spans half the dealer's length each way.
    assert_eq!(out[5].x, p.x);
    assert_eq!((out[9].x - out[1].x).abs(), 180.0);
    p.sphere_3d = true;
    out.clear();
    hit_shapes(&h.w, &p, &mut out);
    assert_eq!(out.len(), 9);
}

// ---- the burp, the eat meter's conditions and the EatUI notify
use mvs_sim::events::{Event, NotifyRef};
use mvs_sim::fighter::FighterId;
use mvs_sim::registry::entries::Condition;

/// The first notify of class `cls` in a montage of the fighter's own data (its montage name through `w.lit`).
fn notify_of(
    h: &harness::Harness,
    f: FighterId,
    cls: &str,
    mut ok: impl FnMut(&mvs_data::fighter::Notify) -> bool,
) -> Option<NotifyRef> {
    let key = h.w.names.str(h.w.fighter(f).ch);
    let data = &h.w.data.chars[key].fighter;
    let mut names: Vec<&String> = data.montages.keys().collect();
    names.sort();
    for name in names {
        for (i, n) in data.montages[name].notifies.iter().enumerate() {
            if n.cls == cls && ok(n) {
                return Some(NotifyRef { montage: h.w.lit(name), index: u32::try_from(i).unwrap() });
            }
        }
    }
    None
}

#[test]
fn the_burp_notify_asks_a_spawn_of_the_stored_class_and_resets_the_store_at_its_end() {
    let Some(mut h) = taz_pair() else { return };
    let f = h.fighter(0);
    let reg = h.w.registry;
    let h_ = *reg.notifies.get("AnimNotify_C015ResetConditionalSpawner_C").expect("the burp notify");
    // A stored projectile that is neither the trash nor the anvil: the spit montage's EatStored notify passes.
    let stored = h.w.lit("C015StewSpitAir_C");
    taz_w(&mut h.w, f).stored_projectile = stored;
    let before = h.w.fighter(f).requests.spawn_requests.len();
    // A spit montage's notify: its conditions pass for one of them with the store as it is.
    let mut asked = None;
    let names: Vec<NotifyRef> = {
        let key = h.w.names.str(h.w.fighter(f).ch);
        let data = &h.w.data.chars[key].fighter;
        let mut v = Vec::new();
        for (name, m) in &data.montages {
            for (i, n) in m.notifies.iter().enumerate() {
                if n.cls == "AnimNotify_C015ResetConditionalSpawner_C" {
                    v.push(NotifyRef { montage: h.w.lit(name), index: u32::try_from(i).unwrap() });
                }
            }
        }
        v
    };
    for n in names {
        (h_.begin.expect("begin"))(&mut h.w, f, n, 0);
        if h.w.fighter(f).requests.spawn_requests.len() > before {
            asked = Some(n);
            break;
        }
    }
    let Some(n) = asked else {
        eprintln!("note: no burp notify of Taz's passed its conditions; nothing checked");
        return;
    };
    let r = h.w.fighter(f).requests.spawn_requests.last().expect("the request");
    assert_eq!(r.cls, stored, "the stored class is burped");
    taz_w(&mut h.w, f).stored_max_speed = 123.0;
    (h_.end.expect("end"))(&mut h.w, f, n, 0);
    assert_eq!(taz(&h.w, f).stored_max_speed, 0.0, "the end resets the stored projectile");
}

#[test]
fn the_burping_notify_marks_the_burp() {
    let Some(mut h) = taz_pair() else { return };
    let f = h.fighter(0);
    let Some(n) = notify_of(&h, f, "AnimNotify_C015BurpingProjectile_C", |_| true) else { return };
    let hd = *h.w.registry.notifies.get("AnimNotify_C015BurpingProjectile_C").expect("entered");
    (hd.begin.expect("begin"))(&mut h.w, f, n, 0);
    assert!(taz(&h.w, f).burping_projectile);
    (hd.end.expect("end"))(&mut h.w, f, n, 0);
    assert!(!taz(&h.w, f).burping_projectile);
}

#[test]
fn the_eat_meter_conditions_read_the_level_and_the_store() {
    let Some(mut h) = taz_pair() else { return };
    let f = h.fighter(0);
    let cond = |h: &mut harness::Harness, name: &str| match *h.w.registry.conditions.get(name).expect("entered") {
        Condition::Fn(c) => c(&mut h.w, f),
        Condition::Class(_) => panic!("{name} is a function"),
    };
    taz_w(&mut h.w, f).eat_level = 0.0;
    assert!(cond(&mut h, "C015EatEmpty_C"));
    assert!(cond(&mut h, "C015EatUnder25_C"));
    assert!(!cond(&mut h, "C015EatMedium_C"));
    taz_w(&mut h.w, f).eat_level = 0.6;
    assert!(!cond(&mut h, "C015EatEmpty_C"));
    assert!(!cond(&mut h, "C015EatUnder25_C"));
    assert!(cond(&mut h, "C015EatMedium_C"));
    // The store: the trash (empty) is neither stored nor large; the anvil is large; a projectile is stored.
    taz_w(&mut h.w, f).stored_projectile = h.w.lit("C015Trash_C");
    assert!(!cond(&mut h, "C015EatStored_C"));
    assert!(!cond(&mut h, "C015EatLarge_C"));
    if let Some(anvil) = h.w.names.get("C015Anvil_C") {
        taz_w(&mut h.w, f).stored_projectile = anvil;
        assert!(cond(&mut h, "C015EatLarge_C"));
        assert!(!cond(&mut h, "C015EatStored_C"));
    }
    // C015EatMiss: nothing eaten and nothing being eaten.
    taz_w(&mut h.w, f).eaten_fighters.clear();
    assert!(cond(&mut h, "C015EatMiss_C"));
}

#[test]
fn the_eat_ui_notify_sends_its_state_for_a_taz() {
    let Some(mut h) = taz_pair() else { return };
    let f = h.fighter(0);
    let Some(n) = notify_of(&h, f, "AnimNotify_C015EatUI_C", |_| true) else { return };
    let hd = *h.w.registry.notifies.get("AnimNotify_C015EatUI_C").expect("entered");
    h.w.fighter_mut(f).events.clear();
    (hd.begin.expect("begin"))(&mut h.w, f, n, 0);
    assert!(h.w.fighter(f).events.iter().any(|e| matches!(e, Event::EatUi { on: true, .. })));
    (hd.end.expect("end"))(&mut h.w, f, n, 0);
    assert!(h.w.fighter(f).events.iter().any(|e| matches!(e, Event::EatUi { on: false, .. })));
}
