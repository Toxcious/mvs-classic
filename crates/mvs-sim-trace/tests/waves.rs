//! The port's own Waves mode on a World of the dev store (`mvs_sim::modes::waves`): the director's cap and Giants,
//! the end rules, the arena's reuse and a run's determinism. A bot holds the player's seat. With no store the tests
//! pass with a note.

use mvs_sim::modes::waves::{self, WavesEnd};
use mvs_sim::setup::WavesVariant;
use mvs_sim::world::World;
use mvs_sim_trace::waves as run;

const CAST: [&str; 5] = ["shaggy", "batman", "superman", "wonderwoman", "taz"];

fn world(variant: WavesVariant, level: i32, seed: i32) -> Option<World> {
    world_of(variant, "superman", level, seed)
}

fn world_of(variant: WavesVariant, character: &str, level: i32, seed: i32) -> Option<World> {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
        return None;
    };
    let setup = run::bot_setup(variant, character, level, CAST.map(String::from).into());
    let data = run::data_for(&store, &setup, "trainingroomsmall").expect("the data");
    Some(run::world(&data, &setup, seed).expect("the World"))
}

/// The Giant mutator's class on a fighter.
fn is_giant(w: &World, f: mvs_sim::fighter::FighterId) -> bool {
    let cls = w.data.mutators.map.get("Mutator_Giant").expect("the Giant mutator");
    let cls = w.lit(cls);
    w.fighter(f).buffs.buffs.iter().any(|b| b.cls == cls)
}

#[test]
fn the_director_keeps_the_cap_and_makes_every_tenth_a_giant() {
    let Some(mut w) = world(WavesVariant::Endless, 3, 11) else { return };
    let mut seen: Vec<(i32, mvs_sim::fighter::FighterId)> = Vec::new();
    let player = w.waves.as_ref().unwrap().player.unwrap();
    for _ in 0..60 * 60 * 6 {
        run::step(&mut w);
        let r = w.waves.as_ref().unwrap();
        assert!(i32::try_from(r.enemies.len()).unwrap() <= waves::alive_cap(r.ring_outs), "more enemies than the cap");
        // (one spawn a pass at most: the newest is the list's last)
        if r.spawned > seen.last().map_or(0, |s| s.0) {
            let n = r.spawned;
            let f = *r.enemies.last().expect("the new enemy");
            seen.push((n, f));
            let fr = w.fighter(f);
            assert_eq!(fr.team.team, 1);
            assert_eq!(fr.life.slot, Some(waves::ENEMY_SLOT));
            assert_eq!(is_giant(&w, f), waves::is_giant(n), "enemy {n}");
            if waves::is_giant(n) {
                assert_eq!(fr.stats.damage, 0.0);
            } else {
                assert!(fr.stats.damage > 0.0 && fr.stats.starting_damage == fr.stats.damage);
            }
            assert!(fr.invuln.invuln_count > 0, "spawn invulnerability");
        }
        if r.end.is_some() {
            break;
        }
    }
    let r = w.waves.as_ref().unwrap();
    assert!(seen.iter().any(|&(n, _)| n >= 10), "no tenth enemy in the run: {} spawned", r.spawned);
    assert_eq!(w.fighter(player).life.slot, Some(0));
    assert_eq!(w.fighter(player).team.team, 0);
    // (the arena holds the player, the enemies alive and the ones gone in the last REUSE_AFTER ticks)
    assert!(w.fighters.len() <= 1 + r.enemies.len() + r.out.len() + r.gone.len());
}

#[test]
fn the_players_ring_out_ends_the_run_and_the_spawns() {
    let Some(mut w) = world(WavesVariant::Endless, 0, 5) else { return };
    for _ in 0..300 {
        run::step(&mut w);
    }
    let player = w.waves.as_ref().unwrap().player.unwrap();
    waves::round_end(&mut w, player);
    let s = waves::status(&w).unwrap();
    assert_eq!(s.end, Some(WavesEnd::PlayerOut));
    let spawned = w.waves.as_ref().unwrap().spawned;
    for _ in 0..600 {
        run::step(&mut w);
    }
    let after = waves::status(&w).unwrap();
    assert_eq!(after.end, Some(WavesEnd::PlayerOut));
    assert_eq!(after.ticks, s.ticks, "the clock stops at the end");
    assert_eq!(w.waves.as_ref().unwrap().spawned, spawned, "no spawn after the end");
}

#[test]
fn the_hundredth_ring_out_clears_a_hundred_run() {
    let Some(mut w) = world(WavesVariant::RingOuts100, 0, 6) else { return };
    while w.waves.as_ref().unwrap().enemies.is_empty() {
        run::step(&mut w);
    }
    w.waves.as_mut().unwrap().ring_outs = 99;
    let e = w.waves.as_ref().unwrap().enemies[0];
    let player = w.waves.as_ref().unwrap().player.unwrap();
    w.fighter_mut(player).stats.damage = 50.0;
    waves::round_end(&mut w, e);
    let healed = (50.0 - waves::HEAL).max(0.0);
    assert_eq!(w.fighter(player).stats.damage, healed, "the player's heal at a ring-out");
    let s = waves::status(&w).unwrap();
    assert_eq!((s.ring_outs, s.target, s.end), (100, Some(100), Some(WavesEnd::Cleared)));
    run::step(&mut w);
    assert!(!w.pawns.as_ref().unwrap().contains(&e), "a rung-out enemy leaves at the next latent pass");
}

#[test]
fn a_run_is_the_same_from_one_seed() {
    let (Some(mut a), Some(mut b)) = (world(WavesVariant::Endless, 3, 21), world(WavesVariant::Endless, 3, 21)) else {
        return;
    };
    for t in 0..3000 {
        run::step(&mut a);
        run::step(&mut b);
        if t % 100 == 0 {
            assert_eq!(mvs_sim::state::hash_of(&a), mvs_sim::state::hash_of(&b), "tick {t}");
        }
    }
    assert!(a.waves.as_ref().unwrap().spawned > 3);
}

#[test]
fn a_spawn_passes_over_the_ids_in_use() {
    let Some(mut w) = world(WavesVariant::Endless, 3, 8) else { return };
    while w.waves.as_ref().unwrap().enemies.is_empty() {
        run::step(&mut w);
    }
    let player = w.waves.as_ref().unwrap().player.unwrap();
    let held = w.fighter(player).id;
    // (the counter's next id is the player's, as after MINION_IDS spawns of an Endless run)
    let base = mvs_sim::world::pawns::MINION_ID_BASE;
    w.pawn_seq = Some(mvs_sim::world::pawns::MINION_IDS + held - base);
    let before = w.waves.as_ref().unwrap().spawned;
    while w.waves.as_ref().unwrap().spawned == before {
        run::step(&mut w);
        assert!(w.waves.as_ref().unwrap().end.is_none(), "the run ended before a spawn");
    }
    let f = *w.waves.as_ref().unwrap().enemies.last().unwrap();
    let ids: Vec<i32> = w.pawns.as_ref().unwrap().iter().map(|&p| w.fighter(p).id).collect();
    assert_ne!(w.fighter(f).id, held);
    assert_eq!(ids.iter().filter(|&&i| i == w.fighter(f).id).count(), 1, "the new id is the only one: {ids:?}");
}

#[test]
fn a_minions_ring_out_is_the_minions_and_counts_for_nothing() {
    let Some(mut w) = world_of(WavesVariant::Endless, "steven", 3, 9) else { return };
    run::step(&mut w);
    let player = w.waves.as_ref().unwrap().player.unwrap();
    let (x, y) = (w.fighter(player).body.x, w.fighter(player).body.y);
    let o = mvs_sim::world::pawns::MinionSpawn {
        character: w.lit("steven"),
        owner: player,
        x,
        y,
        facing: 1.0,
        tree: None,
        at: mvs_sim::world::pawns::FirstTicks::Leftover,
    };
    let m = mvs_sim::world::pawns::spawn_minion(&mut w, o);
    run::step(&mut w);
    let ring_outs = w.waves.as_ref().unwrap().ring_outs;
    w.fighter_mut(m).body.x = 1.0e6;
    w.fighter_mut(m).life.blast_listed = true;
    mvs_sim::world::blast::check_blast(&mut w, m);
    assert_eq!(w.fighter(m).life.dead, mvs_sim::Real::INFINITY, "the minion's defeat (minion_ring_out)");
    assert!(!w.fighter(m).vis.invisible, "not the mode's ring-out");
    let r = w.waves.as_ref().unwrap();
    assert_eq!((r.ring_outs, r.end, r.player_out), (ring_outs, None, false));
}

#[test]
fn a_despawned_enemy_leaves_nothing_behind() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
    // (casts whose moves leave actors out: Garnet's markers and zones, Bugs's dispenser and safe, Harley's box,
    // Shaggy's sandwich and snack box)
    let cast: Vec<String> = ["garnet", "shaggy", "batman", "bugs", "harley"].map(String::from).into();
    let setup = run::bot_setup(WavesVariant::Endless, "superman", 3, cast);
    let data = run::data_for(&store, &setup, "trainingroomsmall").expect("the data");
    for seed in [7, 8] {
        let mut w = run::world(&data, &setup, seed).expect("the World");
        let mut despawns = 0;
        for t in 0..60 * 60 * 3 {
            let before = w.pawns.clone().unwrap_or_default();
            run::step(&mut w);
            let now = w.pawns.clone().unwrap_or_default();
            despawns += before.iter().filter(|f| !now.contains(f)).count();
            // (an arena slot taken again is a new enemy's: only owners out of the match are checked)
            let gone = |f: mvs_sim::fighter::FighterId| !now.contains(&f);
            for (_, p) in w.actors.projectiles.iter() {
                let left = p.owner.is_some_and(gone) && !p.dead;
                assert!(!left, "seed {seed} tick {t}: {} outlived its owner", w.names.str(p.cls));
            }
            for (_, a) in w.actors.actors.iter() {
                assert!(!gone(a.owner) || a.dead, "seed {seed} tick {t}: {} outlived its owner", w.names.str(a.cls));
            }
            for (_, it) in w.actors.items.iter() {
                let left = it.spawner.is_some_and(gone) && !it.dead && !it.held;
                assert!(!left, "seed {seed} tick {t}: an item outlived its owner");
            }
            if w.waves.as_ref().unwrap().end.is_some() {
                break;
            }
        }
        assert!(despawns >= 3, "seed {seed}: {despawns} despawns, too few to check");
    }
}

#[test]
fn a_hidden_fighters_ring_out_counts_once() {
    let Some(mut w) = world(WavesVariant::Endless, 3, 10) else { return };
    while w.waves.as_ref().unwrap().enemies.is_empty() {
        run::step(&mut w);
    }
    let e = w.waves.as_ref().unwrap().enemies[0];
    // (hidden by a move: an eat, a polymorph, a teleport)
    w.fighter_mut(e).vis.invisible = true;
    waves::round_end(&mut w, e);
    waves::round_end(&mut w, e);
    assert_eq!(w.waves.as_ref().unwrap().ring_outs, 1);
    assert!(!w.waves.as_ref().unwrap().enemies.contains(&e), "its place under the cap is free");
    let player = w.waves.as_ref().unwrap().player.unwrap();
    w.fighter_mut(player).vis.invisible = true;
    waves::round_end(&mut w, player);
    assert_eq!(waves::status(&w).unwrap().end, Some(WavesEnd::PlayerOut));
}
