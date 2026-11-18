//! The mortar aim: C019_MortarAim, an `actors::lists::MortarAim` of the World's typed lists
//! (its offset, its activation and its fall to the ground; actors.rs's seismic boom when it lands).
//!
//! C019_MortarAim_C (Morty's grounded side special). Sim space: x = -UE Y, y = UE Z.
//! AnimNotify_Spawner with AttachToActor and socket None (Comp_Spawner HandleActorPostSpawn 10232-10515):
//! K2_AttachToActor(the owner), then SetActorRelativeTransform(the notify's Transforms), so the actor sits at the
//! owner's actor location + (0, 0, -175) with the owner's rotation and follows it. Its Aim component is at relative
//! (CurrentOffset, 0, 0): CurrentOffset ahead of the owner along its facing.
//! ReceiveBeginPlay: FacingLeft from the owner, CurrentOffset = StartingOffset.
//! ReceiveTick (locally controlled owner): MoveX from the owner's BPI_GetMoveAxisValues, turned around when
//! FacingLeft; nothing when it is 0. A BoxTraceSingle on trace type 3 (ProjectileGroundTrace), half extent
//! (50, 150, 0), from the probe point + (0, 0, 50) to the probe point - (0, 0, 150); the probe point is the Aim's
//! location with Y moved 200 to the stick's side when |MoveX| >= 0.3, else with Y = the Aim's X (as authored: the
//! depth coordinate, 0 in the play plane). With a hit CurrentOffset moves by MoveSpeed x DeltaSeconds, up for a
//! stick towards the cached facing, clamped to [0, MaxRange]; the owner's AnimBP gets
//! BPI_SetAimDirection(CurrentOffset / MaxRange).
//! ActivateObject: Comp_Actor_Destructible.DamageDisabled = false, the cursor emitter off, SeismicBoom(the Aim's
//! location), which starts with Delay(0.05).
//! The probe's blockers (ProjectileGroundTrace, ignoring only the actor itself): the stage block (profile Floor), a
//! stage's pass-through platforms (profile Platform) and the pieces of any C019_Ship with their collision on
//! (`World::blockers_into(BlockKind::Trace)`). The box has no height and goes from 125 to 325 under the owner's
//! actor location: with Morty's capsule (half height 180) from 55 above his feet to 145 under them, so the floor,
//! platform or piece he stands on is met.
//! Hurtbox: a capsule (radius 50, half height 200, profile DamageDestructible: overlaps the HitBox channel, and the
//! HitBox profile overlaps it) at relative (0, 0, 400) with scale (0.5, 0.5, 2) under the Aim, whose scale is
//! (2, 2, 0.5) under the root (bAbsoluteScale): by the SCS AttachParent chain its world scale is 1, its centre
//! 0.5 x 400 = 200 above the Aim, so an upright capsule from 50 to 350 above the actor at the Aim's place. No node
//! of the graph touches it: it is on for the actor's whole life, and what a spawned actor's or an item's hitbox does
//! on it is actors.rs's `mortar_hurt`.
// Unverified: the actor is ticked with the other spawned actors, after the fighters, and the Delay counts the
// activation frame's tick; the Aim follows the owner's current facing while the stick rule keeps the cached one;
// ActivateObject is taken to run once; the probe is an overlap of the swept box's bounds in the play plane.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::MortarId;
use crate::actors::lists::MortarAim;
use crate::actors::projectile::attach_parent;
use crate::combat::HitPacket;
use crate::consts::DELTA_SECONDS;
use crate::fighter::common::{actor_data, actor_def, latent_frames};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::{max, min, to_int32};
use crate::registry::props::{flag, num, obj};
use crate::world::World;
use crate::world::blast::{blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use indexmap::IndexSet;
use mvs_collision::Ground;
use mvs_data::value::Props;

fn mo_ref(w: &World, mo: MortarId) -> &MortarAim {
    w.actors.mortars.get(mo).expect("a live mortar")
}

fn mo_mut(w: &mut World, mo: MortarId) -> &mut MortarAim {
    w.actors.mortars.get_mut(mo).expect("a live mortar")
}

/// The class's data (`p`).
fn class_p(w: &World, x: &MortarAim) -> Props {
    actor_def(w, ActorRef::Fighter(x.owner), x.cls).map(|d| d.p.clone()).unwrap_or_default()
}

/// `aimX`: the Aim component's location.
pub fn aim_x(x: &MortarAim) -> Real {
    x.x + x.offset * x.facing
}

/// `aimAlpha`: CurrentOffset / MaxRange (BPI_SetAimDirection); `p` the class's data.
fn aim_alpha(x: &MortarAim, p: &Props) -> Real {
    let r = num(p, "MaxRange").unwrap_or(0.0);
    x.offset / if r != 0.0 && !r.is_nan() { r } else { 1.0 }
}

/// `new MortarAim(f, r.attackId, a, r.p)`.
pub fn new_mortar(w: &mut World, f: FighterId, r: &SpawnRequest) -> MortarAim {
    let notify = crate::world::spawning::request_props(w, f, r);
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(f), r.cls).expect("the mortar's class data");
    let (p, cs) = (&def.p, &def.components);
    let empty = Props::new();
    let hit = HitPacket::from_props(&w.names, obj(p, "HitBoxDamage").unwrap_or(&empty), None);
    let dz = obj(&notify, "Transforms").and_then(|t| obj(t, "Translation")).and_then(|t| num(t, "Z")).unwrap_or(0.0);
    let fc = facing(w, f);
    let hb = obj(cs, "Hurtbox");
    let hp = attach_parent(cs, "Hurtbox");
    let l = hb.and_then(|h| obj(h, "RelativeLocation"));
    let hs = hb.and_then(|h| obj(h, "RelativeScale3D"));
    let s = |k: &str| hs.and_then(|s| num(s, k)).unwrap_or(1.0);
    let (sx, sy, sz) = ((hp.scale[0] * s("X")).abs(), (hp.scale[1] * s("Y")).abs(), (hp.scale[2] * s("Z")).abs());
    let hurt_r = hb.and_then(|h| num(h, "CapsuleRadius")).unwrap_or(0.0) * min(sx, sy);
    let hurt_seg = max(hb.and_then(|h| num(h, "CapsuleHalfHeight")).unwrap_or(0.0) * sz - hurt_r, 0.0);
    let hurt_up = hp.loc[2] + hp.scale[2] * l.and_then(|l| num(l, "Z")).unwrap_or(0.0);
    let mut x = MortarAim {
        lifetime_pause: None,
        dodge: None,
        cls: r.cls,
        elem: None,
        seq: -1,
        x: w.fighter(f).body.x,
        y: actor_y(w, f) + dz,
        facing: fc,
        age: 0,
        dead: false,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        hit,
        charge: None,
        facing_left: fc < 0.0,
        offset: num(p, "StartingOffset").unwrap_or(0.0),
        active: false,
        boom_x: 0.0,
        boom_y: 0.0,
        boom_in: -1,
        dz,
        hurt_r,
        hurt_seg,
        hurt_up,
        attack_type: num(p, "ProjectileAttackType").map_or(0, to_int32),
        overlapped_by: IndexSet::new(),
        owner: f,
        attack_id: r.attack_id,
    };
    // ServerSetOffset(StartingOffset), ubergraph 8917.
    w.fighter_mut(f).pose.aim_direction = min(max(aim_alpha(&x, p), 0.0), 1.0);
    // SpawnedActorParent's BeginPlay blast-box loop at the spawn place.
    let (force, doe) = (flag(p, "ForceSpawnPositionInsideBlastBox"), flag(p, "DestroyOnExitedArena?"));
    if blast_box_loop(&spawn_box(w), &mut x.x, &mut x.y, None, force, &mut x.blast_bounds, doe) {
        x.dead = true;
    }
    x
}

/// `hurtTouches`: whether a circle (a hitbox on the play plane) touches the Hurtbox.
pub fn hurt_touches(x: &MortarAim, px: Real, py: Real, r: Real) -> bool {
    if x.dead || x.hurt_r <= 0.0 {
        return false;
    }
    let cy = x.y + x.hurt_up;
    let dx = px - aim_x(x);
    let dy = py - max(cy - x.hurt_seg, min(py, cy + x.hurt_seg));
    let rr = r + x.hurt_r;
    dx * dx + dy * dy <= rr * rr
}

/// The marker's CheckTestLocations against the mortar's Hurtbox (`Marker.safeLocation`'s mortar loop): the Hurtbox
/// under the Aim (the root's scale is absolute): its offset takes the Aim's scale, its size both scales (a capsule's
/// radius by the smaller of X / Y, its half height by Z), against the capsule at (`qx`, `qy`) of radius `r` and
/// segment half `h`.
pub fn hurt_meets_capsule(w: &World, x: &MortarAim, qx: Real, qy: Real, r: Real, h: Real) -> bool {
    let c = actor_def(w, ActorRef::Fighter(x.owner), x.cls).map(|d| &d.components);
    let a_s = c.and_then(|c| obj(c, "Aim")).and_then(|a| obj(a, "RelativeScale3D"));
    let hb = c.and_then(|c| obj(c, "Hurtbox"));
    let hs = hb.and_then(|h| obj(h, "RelativeScale3D"));
    let g = |o: Option<&Props>, k: &str| o.and_then(|o| num(o, k)).unwrap_or(1.0);
    let sr = min(g(a_s, "X") * g(hs, "X"), g(a_s, "Y") * g(hs, "Y"));
    let sz = g(a_s, "Z") * g(hs, "Z");
    let br = hb.and_then(|h| num(h, "CapsuleRadius")).unwrap_or(0.0) * sr;
    let bh = max(hb.and_then(|h| num(h, "CapsuleHalfHeight")).unwrap_or(0.0) * sz - br, 0.0);
    let lz = hb.and_then(|h| obj(h, "RelativeLocation")).and_then(|l| num(l, "Z")).unwrap_or(0.0);
    let by = x.y + lz * g(a_s, "Z");
    let dx = aim_x(x) - qx;
    let dy = max((by - qy).abs() - bh - h, 0.0);
    dx * dx + dy * dy < (r + br) * (r + br)
}

/// `MortarAim.activate`.
pub fn mortar_activate(w: &mut World, mo: MortarId) {
    if mo_ref(w, mo).active {
        return;
    }
    follow(w, mo);
    let x = mo_mut(w, mo);
    x.active = true;
    x.boom_x = aim_x(x);
    x.boom_y = x.y;
    x.boom_in = to_int32(latent_frames(0.05));
}

fn follow(w: &mut World, mo: MortarId) {
    let f = mo_ref(w, mo).owner;
    let (fx, fy, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    let x = mo_mut(w, mo);
    x.x = fx;
    x.y = fy + x.dz;
    x.facing = fc;
}

/// `MortarAim.step(ground, boxes)`: one actor tick; true when the Delay of SeismicBoom ends. `boxes`: what blocks
/// the probe besides the stage block (`w.stage.ground`), with the polygon solids (met by the shape traces' overlap).
pub fn mortar_step(w: &mut World, mo: MortarId, boxes: crate::world::stage_query::Shapes) -> bool {
    mo_mut(w, mo).age += 1;
    follow(w, mo);
    let p = class_p(w, mo_ref(w, mo));
    let doe = flag(&p, "DestroyOnExitedArena?");
    let x = mo_mut(w, mo);
    if blast_timer_exit(doe, &mut x.blast_at, &x.blast_bounds, crate::real(x.age - 1), x.x, x.y) {
        x.dead = true;
        return false;
    }
    let f = x.owner;
    let mx = w.fighter(f).controls.inp.x;
    let ground = w.stage.ground;
    let x = mo_ref(w, mo);
    let adj = if x.facing_left { -mx } else { mx };
    if adj != 0.0 {
        let ax = aim_x(x);
        let px = if mx <= -0.3 {
            ax - 200.0
        } else if mx >= 0.3 {
            ax + 200.0
        } else {
            0.0
        };
        // The swept box (flat in Z) covers x within 150 of the probe point from y + 50 down to y - 150; a start
        // inside the block is a hit too.
        let hit = over(x, &ground, px)
            || boxes.boxes.iter().any(|g| over(x, g, px))
            || boxes.polys.iter().any(|q| q.box_overlaps(px, x.y - 50.0, 150.0, 100.0));
        if hit {
            let d = num(&p, "MoveSpeed").unwrap_or(0.0) * DELTA_SECONDS;
            let x = mo_mut(w, mo);
            x.offset = min(max(x.offset + if adj > 0.0 { d } else { -d }, 0.0), num(&p, "MaxRange").unwrap_or(0.0));
            let a = min(max(aim_alpha(x, &p), 0.0), 1.0);
            w.fighter_mut(f).pose.aim_direction = a;
        }
    }
    let x = mo_mut(w, mo);
    if x.boom_in > 0 {
        x.boom_in -= 1;
        if x.boom_in == 0 {
            return true;
        }
    }
    false
}

fn over(x: &MortarAim, g: &Ground, px: Real) -> bool {
    px + 150.0 >= crate::math::narrow(g.left)
        && px - 150.0 <= crate::math::narrow(g.right)
        && crate::math::narrow(g.top) >= x.y - 150.0
        && crate::math::narrow(g.bottom) <= x.y + 50.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aim() -> MortarAim {
        MortarAim {
            lifetime_pause: None,
            dodge: None,
            cls: crate::names::Name::NONE,
            elem: None,
            seq: 0,
            x: 100.0,
            y: 0.0,
            facing: -1.0,
            age: 0,
            dead: false,
            blast_at: 0.0,
            blast_bounds: new_blast_bounds(),
            hit: HitPacket::default(),
            charge: None,
            facing_left: true,
            offset: 50.0,
            active: false,
            boom_x: 0.0,
            boom_y: 0.0,
            boom_in: -1,
            dz: 0.0,
            hurt_r: 50.0,
            hurt_seg: 150.0,
            hurt_up: 200.0,
            attack_type: 0,
            overlapped_by: IndexSet::new(),
            owner: FighterId::new(0, 0),
            attack_id: 0,
        }
    }

    #[test]
    fn hurt_touches_tests_the_capsule_at_the_aim() {
        let mut a = aim();
        // the aim at 100 - 50 = 50; the capsule's axis from y 50 to 350
        assert!(hurt_touches(&a, 50.0, 200.0, 0.0));
        assert!(hurt_touches(&a, 110.0, 350.0, 10.0));
        assert!(!hurt_touches(&a, 111.0, 350.0, 10.0));
        assert!(!hurt_touches(&a, 50.0, -11.0, 10.0));
        a.dead = true;
        assert!(!hurt_touches(&a, 50.0, 200.0, 0.0));
    }

    #[test]
    fn the_probe_meets_a_box_within_its_sweep() {
        let a = aim();
        let g = Ground { left: 0.0, right: 10.0, bottom: -200.0, top: -150.0 };
        assert!(over(&a, &g, 160.0));
        assert!(!over(&a, &g, 161.0));
        let low = Ground { left: 0.0, right: 10.0, bottom: -300.0, top: -151.0 };
        assert!(!over(&a, &low, 0.0));
    }

    #[test]
    fn aim_alpha_takes_one_for_a_missing_range() {
        let a = aim();
        assert_eq!(aim_alpha(&a, &Props::new()), 50.0);
    }
}
