//! Iron Giant's fighter state as the record writes it (`GiantState`): the hover rage, the ally turret and the AnimBP's
//! variables as rows of their own, a crouch listener by its actor's reference (a dead one left out, as the oracle
//! writer leaves it), the turret's PilotBuff by its place in its owner's buff list (-1 none, -2 ended), resolved by
//! [`resolve_pilot_buffs`] once every fighter has its buffs.

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::CharState;
use mvs_sim::chars::iron_giant::GiantState;
use mvs_sim::chars::iron_giant::hover_rage::HoverRage;
use mvs_sim::chars::iron_giant::pose::GiantAnim;
use mvs_sim::chars::iron_giant::turret::AllyTurret;
use mvs_sim::combat::BuffRef;
use mvs_sim::{Fighter, World};
use mvs_trace::{AllyTurretRecord, CharRecord, GiantAnimRecord, GiantRecord, HoverRageRecord, RefRecord, WorldRecord};

use crate::conv::{Rec, rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(HoverRage => HoverRageRecord {
    fuel: fuel, using_fuel: using_fuel, fuel_use_type: fuel_use_type, attack_fuel_percent: attack_fuel_percent,
    attack_section: attack_section, attack_montage: attack_montage, empty_jumped: empty_jumped, rage: rage,
    using_weapon_mode: using_weapon_mode, respawn_rage: respawn_rage, fuel_ui: fuel_ui, fuel_ui_value: fuel_ui_value,
    fuel_rtpc: fuel_rtpc, frames: frames,
});

rec_struct!(GiantAnim => GiantAnimRecord {
    loop_t: loop_t, aim_vy: aim_vy, aim_vz: aim_vz, aim_dist: aim_dist, aim_alpha2: aim_alpha2,
    split_active: split_active, jump_check: jump_check, split: split, falling: falling, moving: moving, back: back,
    lean: lean,
});

/// The turret's rows but `buff` (the buff reference: [`ally_turret_put`], [`resolve_pilot_buffs`]).
macro_rules! turret_rows {
    ($mode:ident $s:ident, $x:ident, $r:ident) => {
        rec_rows!($mode $s, $x, $r;
            in_weapon_mode: in_weapon_mode, enter: enter, seed: seed, over: over, jump_on_ally: jump_on_ally,
            ally_prev: ally_prev, has_pilot: has_pilot, pilot: pilot, buff_on: buff_on, pilot_prev: pilot_prev,
            reticle: reticle, aim_x: aim_x, aim_y: aim_y, aim_vx: aim_vx, aim_vy: aim_vy, left: left, right: right,
            bottom: bottom, top: top, held: held, fire: fire, fire_rate: fire_rate, fire_t: fire_t, fire_at: fire_at,
            shot: shot, bomb: bomb, kick: kick, move_open: move_open, stop_open: stop_open,
        )
    };
}

fn ally_turret_put(t: &AllyTurret, x: &Wx) -> AllyTurretRecord {
    let mut r = AllyTurretRecord {
        buff: t.buff.map_or(-1.0, |b| {
            x.w.fighter(b.fighter).buffs.buffs.iter().position(|e| e.serial == b.serial).map_or(-2.0, |i| i as f64)
        }),
        ..AllyTurretRecord::default()
    };
    turret_rows!(put t, x, r);
    r
}

/// [`ally_turret_put`]'s inverse but the buff (left `None` for [`resolve_pilot_buffs`]).
fn ally_turret_load(t: &mut AllyTurret, r: &AllyTurretRecord, l: &mut Lx) {
    turret_rows!(load t, r, l);
    t.buff = None;
}

/// `GiantState` as the record writes it.
pub fn giant_put(s: &GiantState, x: &Wx) -> GiantRecord {
    let mut r = GiantRecord {
        ally_turret: s.ally_turret.as_ref().map(|t| ally_turret_put(t, x)),
        crouch_listeners: s.crouch_listeners.as_ref().map(|l| {
            l.iter()
                .map(|&a| ActorRef::Actor(a))
                .filter(|&a| !mvs_sim::actors::world_actor::actor_dead(x.w, a))
                .map(|a| Rec::<RefRecord>::put(&a, x))
                .collect()
        }),
        ..GiantRecord::default()
    };
    rec_rows!(put s, x, r;
        hover_rage: hover_rage, is_max_rage: is_max_rage, alternate_mode: alternate_mode, gliding: gliding,
        can_attack_hover: can_attack_hover, attack_force_hover: attack_force_hover,
        hover_ignore_fuel: hover_ignore_fuel, hover_fast_fall_block: hover_fast_fall_block,
        ally_rage_delay: ally_rage_delay, out_of_rage_bound: out_of_rage_bound, giant_blocking: giant_blocking,
        turret_aim_number: turret_aim_number, turret_aim_on: turret_aim_on, turret_aim_start_left: turret_aim_start_left,
        giant_anim: giant_anim, hold_valid: hold_valid, hold_alpha: hold_alpha, giant_crouched: giant_crouched,
    );
    r
}

/// [`giant_put`]'s inverse (the turret's PilotBuff: [`resolve_pilot_buffs`]).
pub fn giant_load(s: &mut GiantState, r: &GiantRecord, l: &mut Lx) {
    rec_rows!(load s, r, l;
        hover_rage: hover_rage, is_max_rage: is_max_rage, alternate_mode: alternate_mode, gliding: gliding,
        can_attack_hover: can_attack_hover, attack_force_hover: attack_force_hover,
        hover_ignore_fuel: hover_ignore_fuel, hover_fast_fall_block: hover_fast_fall_block,
        ally_rage_delay: ally_rage_delay, out_of_rage_bound: out_of_rage_bound, giant_blocking: giant_blocking,
        turret_aim_number: turret_aim_number, turret_aim_on: turret_aim_on, turret_aim_start_left: turret_aim_start_left,
        giant_anim: giant_anim, hold_valid: hold_valid, hold_alpha: hold_alpha, giant_crouched: giant_crouched,
    );
    l.at("ally_turret", |l| match (&mut s.ally_turret, &r.ally_turret) {
        (t, None) => *t = None,
        (Some(t), Some(r)) => ally_turret_load(t, r, l),
        (t @ None, Some(r)) => {
            let mut n = AllyTurret::default();
            ally_turret_load(&mut n, r, l);
            *t = Some(n);
        }
    });
    l.at("crouch_listeners", |l| {
        s.crouch_listeners = r.crouch_listeners.as_ref().map(|v| {
            v.iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    l.at(&format!("[{i}]"), |l| match <ActorRef as Rec<RefRecord>>::make(e, l) {
                        Some(ActorRef::Actor(a)) => Some(a),
                        Some(a) => {
                            l.miss(format_args!("{a:?} is no world actor"));
                            None
                        }
                        None => None,
                    })
                })
                .collect()
        });
    });
}

/// Each Iron Giant's PilotBuff from its record, once every fighter of the World has its buffs (their serials): the
/// entry at the record's place in `buff_on`'s list, an ended one (-2) as a serial no entry has.
pub fn resolve_pilot_buffs(w: &mut World, r: &WorldRecord, l: &mut Lx) {
    let ids: Vec<_> = w.pawns.clone().unwrap_or_else(|| w.players.clone());
    for (i, (&id, fr)) in ids.iter().zip(&r.fighters).enumerate() {
        let CharRecord::IronGiant(g) = &fr.state else { continue };
        let Some(t) = &g.ally_turret else { continue };
        if t.buff == -1.0 {
            continue;
        }
        let at = format!("fighters[{i}].state.ally_turret.buff");
        let Some(on) = l.at(&at, |l| match l.fighter(t.buff_on) {
            Some(on) => Some(on),
            None => {
                l.miss("a PilotBuff with no owner");
                None
            }
        }) else {
            continue;
        };
        let b = if t.buff == -2.0 {
            // (never handed out: serials count up from 0)
            Some(BuffRef { fighter: on, serial: u32::MAX - 1 })
        } else {
            let f: &Fighter = w.fighter(on);
            match f.buffs.buffs.get(t.buff as usize) {
                Some(e) => Some(BuffRef { fighter: on, serial: e.serial }),
                None => {
                    let n = t.buff;
                    l.at(&at, |l| l.miss(format_args!("PilotBuff {n} past its owner's buffs")));
                    None
                }
            }
        };
        if let CharState::IronGiant(s) = &mut w.fighter_mut(id).cs
            && let Some(at) = s.ally_turret.as_mut()
        {
            at.buff = b;
        }
    }
}
