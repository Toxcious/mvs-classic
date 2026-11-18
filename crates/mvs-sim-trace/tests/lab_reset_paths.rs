//! The Lab reset's new pawns (`world::reset::new_pawn`) and the spawn's served despawns (`match_side::spawn`): paths
//! no trace runs, since a trace resets only at its World's start.

use std::sync::atomic::{AtomicU32, Ordering};

use mvs_sim::World;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::RunSubState;
use mvs_sim::fighter::control::{DodgeEntry, DodgeState};
use mvs_sim::fighter::match_side::{self, DespawnRequest};
use mvs_sim_trace::harness;

/// The calls of the test's AttackEnded hook.
static ENDED: AtomicU32 = AtomicU32::new(0);

fn count_end(_: &mut World, _: FighterId) {
    ENDED.fetch_add(1, Ordering::SeqCst);
}

/// A reset's new pawn: a new AnimInstance (the run machine at its entry), a new hit box handler (no dodging fighters,
/// its gates open) and no AttackEnded from the old pawn's attack.
#[test]
fn a_reset_gives_new_pawns_their_own_state() {
    let Some(mut h) = harness::at("wonderwoman-wonderwoman", 100) else { return };
    // The first frame the first fighter is in an attack.
    let f = h.fighter(0);
    let mut n = 0;
    while h.w.fighter(f).charge.attack_is_active == 0 {
        h.step_recorded();
        n += 1;
        assert!(n < 1500, "no attack of the first fighter in the trace");
    }
    let hooks = Box::leak(Box::new(*h.w.fighter(f).hooks.char));
    hooks.attack_ended = Some(count_end);
    h.w.fighter_mut(f).hooks.char = hooks;
    ENDED.store(0, Ordering::SeqCst);
    h.w.fighter_mut(f).sm.run_sub = Some(RunSubState::CrouchRun);
    let mut d = DodgeState { gate: 1e6, fx_gate: 1e6, quest_gate: 1e6, ..DodgeState::default() };
    d.fighters.insert(2, DodgeEntry { id: Some(7), ..DodgeEntry::default() });
    h.w.fighter_mut(f).evade.dodge = Some(d);

    mvs_sim::world::reset::reset(&mut h.w);

    let fr = h.w.fighter(f);
    assert_eq!(ENDED.load(Ordering::SeqCst), 0, "the old pawn's attack end ran AttackEnded on the new one");
    assert_eq!(fr.charge.attack_is_active, 0);
    assert_eq!(fr.sm.run_sub, None);
    let d = fr.evade.dodge.as_ref().expect("the handler's dodge state is kept, emptied");
    assert!(d.fighters.is_empty());
    assert_eq!((d.gate, d.fx_gate, d.quest_gate), (0.0, 0.0, 0.0));
}

/// A despawn in the spawner's requests when the pawn is spawned again is served, not dropped (the ends the spawn
/// runs ask theirs there).
#[test]
fn a_spawn_serves_the_despawns_asked_ahead_of_it() {
    let Some(mut h) = harness::at("batman-batman", 0) else { return };
    // The first frame a fighter has a live projectile of its own.
    let mut n = 0;
    let (f, id, cls) = loop {
        let found = h.w.actors.projectiles.iter().find_map(|(id, p)| {
            let by = mvs_sim::actors::projectile::spawner(p)?;
            (!p.dead).then_some((by, id, p.cls))
        });
        if let Some(x) = found {
            break x;
        }
        h.step_recorded();
        n += 1;
        assert!(n < 2000, "no projectile in the trace");
    };
    h.w.fighter_mut(f).requests.despawn_requests.push(DespawnRequest::All(cls));
    let (x, y, facing) = {
        let b = &h.w.fighter(f).body;
        (b.x, b.y, b.facing_dir)
    };
    match_side::spawn(&mut h.w, f, x, y, facing);
    assert!(h.w.actors.projectiles.get(id).is_none_or(|p| p.dead), "the despawn was dropped");
    assert!(h.w.fighter(f).requests.despawn_requests.is_empty());
}
