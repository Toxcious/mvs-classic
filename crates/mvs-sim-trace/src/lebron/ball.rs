//! `Ball` (BasketBallProjectile_C and its Shot, Pass and AllyPass children: a projectile's `ext`):
//! the keys a ball adds to its projectile's `rest`.

use mvs_sim::chars::lebron::ball::Ball;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a ball adds to its projectile's `rest`.
pub fn put(p: &Ball, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "allowTeleportTo" = allow_teleport_to, "audioSound" = audio_sound, "bounceSound" = bounce_sound,
        "bounces" = bounces, "hitActivated" = hit_activated, "hitEnemy" = hit_enemy,
        "ignorePlatformsCheck" = ignore_platforms_check, "inside" = inside, "mid" = mid, "noLook" = no_look,
        "others" = others, "passVx" = pass_vx, "passVy" = pass_vy, "reopenAt" = reopen_at,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut Ball, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "allowTeleportTo" = allow_teleport_to, "audioSound" = audio_sound, "bounceSound" = bounce_sound,
        "bounces" = bounces, "hitActivated" = hit_activated, "hitEnemy" = hit_enemy,
        "ignorePlatformsCheck" = ignore_platforms_check, "inside" = inside, "mid" = mid, "noLook" = no_look,
        "others" = others, "passVx" = pass_vx, "passVy" = pass_vy, "reopenAt" = reopen_at,);
    known.extend(plain_keys!(names
        "allowTeleportTo" = allow_teleport_to, "audioSound" = audio_sound, "bounceSound" = bounce_sound,
        "bounces" = bounces, "hitActivated" = hit_activated, "hitEnemy" = hit_enemy,
        "ignorePlatformsCheck" = ignore_platforms_check, "inside" = inside, "mid" = mid, "noLook" = no_look,
        "others" = others, "passVx" = pass_vx, "passVy" = pass_vy, "reopenAt" = reopen_at,));
}
