//! Velma's fighter state as the record writes it (`VelmaState`): a
//! record per group of files (`beam`, `marks`, `capture` and `gadgets` hold nothing), the slot mix and its timed lists,
//! the police and the call's last spawn by fighter id, the live carry run and mark by reference; and the police
//! minion's (`VelmaPoliceState`, VELMA_POLICE).

use mvs_sim::chars::velma::state::{VelmaCarryState, VelmaEvidenceState, VelmaPoliceCallState};
use mvs_sim::chars::velma::{VelmaPoliceState, VelmaState};
use mvs_trace::{
    VelmaCarryRecord, VelmaEvidenceRecord, VelmaPawnRecord, VelmaPoliceCallRecord, VelmaPoliceRecord, VelmaRecord,
};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(VelmaEvidenceState => VelmaEvidenceRecord {
    period: period, start: start, fired: fired, timer_at: timer_at,
});
rec_struct!(VelmaPoliceCallState => VelmaPoliceCallRecord { delay1: delay1, delay8: delay8, last_spawn: last_spawn });
rec_struct!(VelmaCarryState => VelmaCarryRecord { holding_ally: holding_ally });

/// `VelmaState` as the record writes it.
pub fn velma_put(s: &VelmaState, x: &Wx) -> VelmaRecord {
    let m = &s.pawn.slot_mix;
    let mut r = VelmaRecord {
        pawn: VelmaPawnRecord {
            slot_mix: crate::fighter::slot_mix_put(&m.slot_mix, x),
            slot_blend: crate::fighter::slot_blend_put(m.slot_blend.as_ref(), x),
        },
        ..VelmaRecord::default()
    };
    rec_rows!(put s, x, r;
        evidence: evidence, call: call, carry: carry, aim_direction: aim_direction, carrying_ally: carrying_ally,
        anim_hit_stun: anim_hit_stun, allow_police_call: allow_police_call, police: police, evidence_ui: evidence_ui,
        carry_run: carry_run, mark: mark, glasses_off_count: glasses_off_count,
    );
    r
}

/// [`velma_put`]'s inverse.
pub fn velma_load(s: &mut VelmaState, r: &VelmaRecord, l: &mut Lx) {
    let m = &mut s.pawn.slot_mix;
    m.slot_mix = crate::fighter::slot_mix_load(&r.pawn.slot_mix, l);
    let b = m.slot_blend.get_or_insert_with(Default::default);
    l.at("slot_blend", |l| crate::fighter::slot_blend_load(b, &r.pawn.slot_blend, l));
    rec_rows!(load s, r, l;
        evidence: evidence, call: call, carry: carry, aim_direction: aim_direction, carrying_ally: carrying_ally,
        anim_hit_stun: anim_hit_stun, allow_police_call: allow_police_call, police: police, evidence_ui: evidence_ui,
        carry_run: carry_run, mark: mark, glasses_off_count: glasses_off_count,
    );
}

/// `VelmaPoliceState` as the record writes it.
pub fn police_put(s: &VelmaPoliceState, x: &Wx) -> VelmaPoliceRecord {
    let mut r = VelmaPoliceRecord::default();
    rec_rows!(put s, x, r;
        arrest_enabled: arrest_enabled, performed_arrest: performed_arrest, targeted_enemy: targeted_enemy,
        initial_x: initial_x, initial_y: initial_y, delay3: delay3, target_loop: target_loop,
        visuals_closed: visuals_closed, inside: inside, pin: pin,
    );
    r
}

/// [`police_put`]'s inverse.
pub fn police_load(s: &mut VelmaPoliceState, r: &VelmaPoliceRecord, l: &mut Lx) {
    rec_rows!(load s, r, l;
        arrest_enabled: arrest_enabled, performed_arrest: performed_arrest, targeted_enemy: targeted_enemy,
        initial_x: initial_x, initial_y: initial_y, delay3: delay3, target_loop: target_loop,
        visuals_closed: visuals_closed, inside: inside, pin: pin,
    );
}
