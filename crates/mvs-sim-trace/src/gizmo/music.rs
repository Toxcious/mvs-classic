//! Gizmo's music actors and buffs as the record writes them.

use mvs_sim::chars::gizmo::music::{MusicExt, MusicPickup, MusicProjectile, PianoNote};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

/// A C023A_MusicPickup_C as the record writes it: the keys its class adds to the projectile.
pub fn pickup_put(s: &MusicPickup, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o;
        "damageFx" = damage_fx, "pickupGate" = pickup_gate, "isHomingOnAlly" = is_homing_on_ally,
        "canStartFollowing" = can_start_following, "canFollow" = can_follow, "target" = target,
        "endBound" = end_bound, "popBound" = pop_bound, "lingerAt" = linger_at,
        "inPick" = in_pick, "inMagnet" = in_magnet, "inFriend" = in_friend,);
}

/// [`pickup_put`]'s inverse; the keys it knows go into `known`.
pub fn pickup_load(s: &mut MusicPickup, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l;
        "damageFx" = damage_fx, "pickupGate" = pickup_gate, "isHomingOnAlly" = is_homing_on_ally,
        "canStartFollowing" = can_start_following, "canFollow" = can_follow, "target" = target,
        "endBound" = end_bound, "popBound" = pop_bound, "lingerAt" = linger_at,
        "inPick" = in_pick, "inMagnet" = in_magnet, "inFriend" = in_friend,);
    known.extend(plain_keys!(names
        "damageFx" = damage_fx, "pickupGate" = pickup_gate, "isHomingOnAlly" = is_homing_on_ally,
        "canStartFollowing" = can_start_following, "canFollow" = can_follow, "target" = target,
        "endBound" = end_bound, "popBound" = pop_bound, "lingerAt" = linger_at,
        "inPick" = in_pick, "inMagnet" = in_magnet, "inFriend" = in_friend,));
}

/// A C023A_Music_Projectile_C as the record writes it: its ally.
pub fn music_projectile_put(s: &MusicProjectile, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "ally" = ally,);
}

/// [`music_projectile_put`]'s inverse; the keys it knows go into `known`.
pub fn music_projectile_load(s: &mut MusicProjectile, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "ally" = ally,);
    known.extend(plain_keys!(names "ally" = ally,));
}

/// A C023A_PianoNote_C as the record writes it: the Bubble timeline.
pub fn piano_note_put(s: &PianoNote, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "bubble" = bubble, "bornAt" = born_at, "bubbleBase" = bubble_base,);
}

/// [`piano_note_put`]'s inverse; the keys it knows go into `known`.
pub fn piano_note_load(s: &mut PianoNote, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "bubble" = bubble, "bornAt" = born_at, "bubbleBase" = bubble_base,);
    known.extend(plain_keys!(names "bubble" = bubble, "bornAt" = born_at, "bubbleBase" = bubble_base,));
}

plain_struct!(MusicExt { "from" = from });

/// C023A_Music_Buff_C / C023A_Music_DeBuff_C's extension as the oracle writes it.
pub fn music_put(e: &MusicExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`music_put`]'s inverse.
pub fn music_load(e: &mut MusicExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
