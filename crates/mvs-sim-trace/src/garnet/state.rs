//! Garnet's fighter state as the record writes it (`GarnetState`):
//! a record per file (`gauntlets`, `marker`, `dash`, `hold` and `stopproj` hold nothing), the slot mix as the other
//! characters', the two gauntlet projectiles by reference.

use mvs_sim::actors::{ActorRef, ProjectileId};
use mvs_sim::chars::garnet::GarnetState;
use mvs_sim::chars::garnet::state::{GarnetAimState, GarnetGroundState, GarnetSingState, GarnetZoneState};
use mvs_trace::{
    GarnetAimRecord, GarnetGroundRecord, GarnetPawnRecord, GarnetRecord, GarnetSingRecord, GarnetZoneRecord, RefRecord,
};

use crate::conv::{Rec, rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(GarnetAimState => GarnetAimRecord {
    aim_direction: aim_direction, aim_direction_before_reset: aim_direction_before_reset, reset_on: reset_on,
    reset_pos: reset_pos, aim_flag: aim_flag, set_aim_value_from_movement: set_aim_value_from_movement,
    read_aim_as_horizontal_movement: read_aim_as_horizontal_movement,
});
rec_struct!(GarnetGroundState => GarnetGroundRecord {
    left_positions: left_positions, right_positions: right_positions, left_x: left_x, left_y: left_y,
    right_x: right_x, right_y: right_y, checking_left: checking_left, checking_right: checking_right,
});
rec_struct!(GarnetZoneState => GarnetZoneRecord {
    charging_electricity: charging_electricity, electricity_fully_charged: electricity_fully_charged,
});
rec_struct!(GarnetSingState => GarnetSingRecord {
    overlapping_sing_actors: overlapping_sing_actors, inside: inside, singing_vfx: singing_vfx,
    song_entering: song_entering, song_retries: song_retries, song_retry_pending: song_retry_pending,
});

/// A gauntlet projectile by reference (`REF`).
fn gauntlet_put(p: Option<ProjectileId>, x: &Wx) -> Option<RefRecord> {
    p.map(|p| ActorRef::Projectile(p).put(x))
}

/// [`gauntlet_put`]'s inverse.
fn gauntlet_load(r: &Option<RefRecord>, l: &mut Lx) -> Option<ProjectileId> {
    match r.as_ref().map(|r| ActorRef::make(r, l)) {
        None => None,
        Some(Some(ActorRef::Projectile(p))) => Some(p),
        Some(Some(a)) => {
            l.miss(format_args!("{a:?} where a projectile is held"));
            None
        }
        Some(None) => None,
    }
}

/// `GarnetState` as the record writes it.
pub fn garnet_put(s: &GarnetState, x: &Wx) -> GarnetRecord {
    let mut r = GarnetRecord {
        pawn: GarnetPawnRecord {
            marker_location_before_destroy_x: f64::from(s.pawn.marker_location_before_destroy_x),
            marker_location_before_destroy_y: f64::from(s.pawn.marker_location_before_destroy_y),
            slot_mix: crate::fighter::slot_mix_put(&s.pawn.slot_mix.slot_mix, x),
        },
        idle_gauntlet: gauntlet_put(s.idle_gauntlet, x),
        projectile_gauntlets: gauntlet_put(s.projectile_gauntlets, x),
        ..GarnetRecord::default()
    };
    rec_rows!(put s, x, r;
        aim: aim, ground: ground, zone: zone, sing: sing, location_before_move_x: location_before_move_x,
        location_before_move_y: location_before_move_y, charging_to_marker: charging_to_marker,
        is_active_singing: is_active_singing,
    );
    r
}

/// [`garnet_put`]'s inverse.
pub fn garnet_load(s: &mut GarnetState, r: &GarnetRecord, l: &mut Lx) {
    s.pawn.marker_location_before_destroy_x = mvs_sim::math::narrow(r.pawn.marker_location_before_destroy_x);
    s.pawn.marker_location_before_destroy_y = mvs_sim::math::narrow(r.pawn.marker_location_before_destroy_y);
    s.pawn.slot_mix.slot_mix = crate::fighter::slot_mix_load(&r.pawn.slot_mix, l);
    s.idle_gauntlet = l.at("idle_gauntlet", |l| gauntlet_load(&r.idle_gauntlet, l));
    s.projectile_gauntlets = l.at("projectile_gauntlets", |l| gauntlet_load(&r.projectile_gauntlets, l));
    rec_rows!(load s, r, l;
        aim: aim, ground: ground, zone: zone, sing: sing, location_before_move_x: location_before_move_x,
        location_before_move_y: location_before_move_y, charging_to_marker: charging_to_marker,
        is_active_singing: is_active_singing,
    );
}
