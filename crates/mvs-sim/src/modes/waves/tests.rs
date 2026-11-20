//! The director's rules that need no World: the cap's climb, the damage's fall, the Giants and the cast's draw. The
//! runs on a World are `mvs-sim-trace`'s `tests/waves.rs`.

use super::*;

#[test]
fn the_cap_climbs_from_three_to_six() {
    assert_eq!(alive_cap(0), CAP_START);
    assert_eq!(alive_cap(CAP_STEP - 1), CAP_START);
    assert_eq!(alive_cap(CAP_STEP), CAP_START + 1);
    assert_eq!(alive_cap(CAP_STEP * 3), CAP_MAX);
    assert_eq!(alive_cap(10_000), CAP_MAX);
    let mut last = 0;
    for n in 0..200 {
        let c = alive_cap(n);
        assert!(c >= last && (CAP_START..=CAP_MAX).contains(&c));
        last = c;
    }
}

#[test]
fn the_damage_falls_to_its_floor() {
    assert_eq!(starting_damage(0), DAMAGE_START);
    assert!(starting_damage(10) < starting_damage(0));
    assert_eq!(starting_damage(100_000), DAMAGE_MIN);
}

#[test]
fn every_tenth_enemy_is_a_giant() {
    let giants: Vec<i32> = (1..=40).filter(|&n| is_giant(n)).collect();
    assert_eq!(giants, vec![10, 20, 30, 40]);
}

#[test]
fn the_cast_is_distinct_and_seeded() {
    let roster: Vec<String> = ["batman", "shaggy", "superman", "taz", "velma", "finn", "jake"].map(String::from).into();
    let a = draw_cast(7, &roster);
    assert_eq!(a.len(), CAST_SIZE);
    assert_eq!(a, draw_cast(7, &roster));
    for (i, k) in a.iter().enumerate() {
        assert!(roster.contains(k));
        assert!(!a[..i].contains(k));
    }
    let others = (0..20).map(|s| draw_cast(s, &roster)).filter(|c| *c != a).count();
    assert!(others > 0, "every seed drew one cast");
    assert_eq!(draw_cast(1, &roster[..2]).len(), 2);
    assert!(draw_cast(1, &[]).is_empty());
}
