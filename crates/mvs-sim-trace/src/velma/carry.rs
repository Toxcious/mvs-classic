//! Velma's carry as the record writes it: VelmaCarryRun_Actor_C's `put` and `load`.
//!
//! `pickUps` and `watch` hold whole buff entries in the oracle (`{ f, b }` and the entries): written through
//! `buff_entry_put`. A load cannot make the reference (the fighters' buffs are loaded after the actors), so it leaves a
//! pending serial and [`resolve_run_buffs`] takes the fighter's first Buff_VelmaAttachToAlly_C.
// Unverified: the entry a pick-up or a watch loads as is the first Buff_VelmaAttachToAlly_C of its fighter's list (a
// fighter holds one at a time).

use mvs_sim::actors::WorldActorState;
use mvs_sim::chars::velma::capture::ATTACH_TO_ALLY_N;
use mvs_sim::chars::velma::carry::{PickUp, VelmaCarryRun};
use mvs_sim::combat::BuffRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::world::World;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::black_adam::chain::buff_entry_put;
use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

/// A buff reference a load has not resolved.
const PENDING: u32 = u32::MAX - 4;

macro_rules! run_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "attackId" = attack_id, "base" = base, "baseMaxAccel" = base_max_accel, "bound" = bound,
            "canHit" = can_hit, "canJump" = can_jump, "carriedAlly" = carried_ally, "carriedEnemy" = carried_enemy,
            "carryInt" = carry_int, "cruise" = cruise, "ending" = ending, "facingLeft" = facing_left, "hit" = hit,
            "hitIds" = hit_ids, "hitTimes" = hit_times, "hitWait" = hit_wait, "insideHit" = inside_hit,
            "insideJump" = inside_jump, "jumpGate" = jump_gate, "jumpOnAlly" = jump_on_ally, "jumpOnUi" = jump_on_ui,
            "kbSeq" = kb_seq, "pct" = pct, "section" = section, "stunAge" = stun_age, "stunWas" = stun_was,
            "team" = team, "turnGate" = turn_gate, "turnWait" = turn_wait,
        )
    };
}

/// A VelmaCarryRun_Actor_C as the record writes it: the place (vx and vy readonly 0) and its own keys.
pub fn run_put(s: &VelmaCarryRun, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    run_keys!(plain_keys put s, x, o;);
    let picks = s
        .pick_ups
        .iter()
        .map(|e| {
            Value::Obj(Obj::new(vec![
                ("f".to_owned(), e.f.to_value(x)),
                ("b".to_owned(), buff_entry_put(Some(e.b), x)),
            ]))
        })
        .collect();
    o.push(("pickUps".to_owned(), Value::List(picks)));
    o.push(("watch".to_owned(), Value::List(s.watch.iter().map(|b| buff_entry_put(Some(*b), x)).collect())));
    o.push(("p".into(), static_object()));
}

/// [`run_put`]'s inverse; the keys it knows go into `known`.
pub fn run_load(s: &mut VelmaCarryRun, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        run_keys!(plain_keys load s, ob, l;);
        s.pick_ups.clear();
        if let Some(Value::List(a)) = ob.get("pickUps") {
            for (i, e) in a.iter().enumerate() {
                l.at(&format!("pickUps[{i}]"), |l| {
                    let Value::Obj(e) = e else { return l.miss("a pick-up that is no object") };
                    let f = e.get("f").and_then(|v| FighterId::from_value(v, l));
                    if let Some(f) = f {
                        s.pick_ups.push(PickUp { f, b: BuffRef { fighter: f, serial: PENDING } });
                    }
                });
            }
        }
        s.watch.clear();
        if let Some(Value::List(a)) = ob.get("watch") {
            for _ in a {
                // The fighter is the pick-up's with the same place in the list (a watch entry is a pick-up's buff).
                let at = s.watch.len();
                match s.pick_ups.get(at) {
                    Some(p) => s.watch.push(BuffRef { fighter: p.f, serial: PENDING }),
                    None => l.miss("a watched buff with no pick-up at its place"),
                }
            }
        }
    });
    known.extend(run_keys!(plain_keys names));
    known.extend(["pickUps", "watch"]);
}

/// Turns the pending references [`run_load`] left into the entries of the held fighters' lists.
pub fn resolve_run_buffs(w: &mut World, l: &mut Lx) {
    type Fixes = Vec<(usize, Option<u32>)>;
    let mut fix: Vec<(mvs_sim::actors::ActorId, Fixes, Fixes)> = Vec::new();
    for (id, a) in w.actors.actors.iter() {
        let WorldActorState::VelmaCarryRun(s) = &a.state else { continue };
        let find = |f: FighterId| {
            w.fighter(f).buffs.buffs.iter().find(|b| b.cls == ATTACH_TO_ALLY_N && b.frames > 0.0).map(|b| b.serial)
        };
        let picks =
            s.pick_ups.iter().enumerate().filter(|(_, p)| p.b.serial == PENDING).map(|(i, p)| (i, find(p.f))).collect();
        let watch = s
            .watch
            .iter()
            .enumerate()
            .filter(|(_, b)| b.serial == PENDING)
            .map(|(i, b)| (i, find(b.fighter)))
            .collect();
        fix.push((id, picks, watch));
    }
    for (id, picks, watch) in fix {
        let Some(a) = w.actors.actors.get_mut(id) else { continue };
        let WorldActorState::VelmaCarryRun(s) = &mut a.state else { continue };
        for (i, serial) in picks {
            match serial {
                Some(serial) => s.pick_ups[i].b.serial = serial,
                None => l.miss(format_args!("a carry run's pick-up buff is in no list of its fighter")),
            }
        }
        for (i, serial) in watch {
            match serial {
                Some(serial) => s.watch[i].serial = serial,
                None => l.miss(format_args!("a carry run's watched buff is in no list of its fighter")),
            }
        }
    }
}
