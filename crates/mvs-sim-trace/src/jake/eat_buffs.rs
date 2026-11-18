//! Buff_JakEat's own variables on its hold as the record writes them (`Hold.own`, an
//! object of its six keys; `holds.rs` writes a hold's `own` through it).

use mvs_sim::chars::jake::eat_buffs::JakEatOwn;

use crate::conv::plain_struct;

plain_struct!(JakEatOwn {
    "collOff" = coll_off, "deathSave" = death_save, "gate" = gate, "gateWait" = gate_wait, "tries" = tries,
    "wait" = wait,
});
