//! Steven's fighter state as the record writes it (`StevenState`):
//! a record per file (`tether`, `barrier` and `bubble` hold nothing), the Watermelon by its fighter id, the spawner's
//! class-checked actors by reference.
//!
//! Unverified: a Watermelon destroyed while the pawn still holds it (`watermelonSteven`, `classChecked`) is written
//! by the oracle with its id; `Wx::fid` answers -1 for a fighter gone from the World, so such a frame needs a book of
//! gone fighters' ids (as `projectile_seqs` for projectiles) when the Watermelon's port reaches it.

use mvs_sim::chars::steven::StevenState;
use mvs_sim::chars::steven::state::{
    PendingSpawn, StevenGlideState, StevenPawnState, StevenShieldState, StevenWatermelonState,
};
use mvs_trace::{
    PendingSpawnRecord, StevenGlideRecord, StevenPawnRecord, StevenRecord, StevenShieldRecord, StevenWatermelonRecord,
};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(StevenPawnState => StevenPawnRecord {
    gliding: gliding, anim_gliding: anim_gliding, glide_once_closed: glide_once_closed,
    original_gravity_scale: original_gravity_scale, up_shield_ammo: up_shield_ammo,
    up_shield_ammo_max: up_shield_ammo_max, watermelon_steven: watermelon_steven, class_checked: class_checked,
});
rec_struct!(StevenShieldState => StevenShieldRecord {
    charges: charges, shielding: shielding, ally: ally, ally_bound: ally_bound,
});
rec_struct!(PendingSpawn => PendingSpawnRecord { x: x, y: y });
rec_struct!(StevenWatermelonState => StevenWatermelonRecord {
    is_watermelon_steven: is_watermelon_steven, pending_spawn: pending_spawn, cleanup_pending: cleanup_pending,
});
rec_struct!(StevenGlideState => StevenGlideRecord {
    w: w, from_w: from_w, to_w: to_w, blend: blend, elapsed: elapsed, last: last, sm: sm, sm_time: sm_time,
    sm_first: sm_first,
});

/// `StevenState` as the record writes it.
pub fn steven_put(s: &StevenState, x: &Wx) -> StevenRecord {
    let mut r = StevenRecord::default();
    rec_rows!(put s, x, r; pawn: pawn, shield: shield, watermelon: watermelon, glide: glide);
    r
}

/// [`steven_put`]'s inverse.
pub fn steven_load(s: &mut StevenState, r: &StevenRecord, l: &mut Lx) {
    rec_rows!(load s, r, l; pawn: pawn, shield: shield, watermelon: watermelon, glide: glide);
}
