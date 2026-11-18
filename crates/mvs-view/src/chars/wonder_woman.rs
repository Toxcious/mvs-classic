//! Wonder Woman's render module: the entry, the AnimBP's pose hook
//! and the Ground Down blast's glow on her body material.
//!
//! The meshes: the sword and the shield are a section of the body mesh (WW_BodySkelMesh, material
//! WonderWomanWeaponBase_MIC), skinned to fml_un_C_prop1 (the sword: sword_handle, hilt, back1..10, front1..10 under
//! it) and fml_un_C_prop2 (the shield: shield_Center). Both bones are children of Root, so the animation alone
//! leaves them where the clip's own tracks put them; the hands hold them through the AnimBP's four
//! FAnimNode_Constraint nodes. The hair is two components on the body's pose (HairBaseMesh and HairInvertedMesh
//! SetMasterPoseComponent(Mesh, true)), the skin's sub-meshes RightHair and LeftHair.
//!
//! The facing. The build poses her facing left through WAnimationMirrorNode (WonderWoman_MirrorTable) on an actor
//! turned half a turn; the port draws a left facing as the reflection of the right-facing view. The mirror node's
//! rule on the table's rows for these bones gives, for the reflected view:
//!   - fml_un_L_Prop <-> fml_un_R_Prop (flip Y) and fml_un_C_prop1 (flip Y, its own partner): Constraint_1 on the
//!     mirrored pose is the reflection of Constraint_2 (prop1 on fml_un_R_Prop) on the right-facing pose: Constraint_2
//!     at both facings.
//!   - fml_un_L_ShieldPivot_b <-> fml_un_R_ShieldPivot_b (flip XYZ) and ModifyBone_8 (prop2, Pitch 180 at
//!     FacingLeft): Constraint_3 with ModifyBone_8 on the mirrored pose is the reflection of Constraint (prop2 on
//!     fml_un_L_ShieldPivot_b): Constraint at both facings, no ModifyBone_8.
//!   - ModifyBone_7 (fml_un_C_hair_att, scale HeadScale x (1, -1, 1), alpha FacingLeft): the reflection of the
//!     right-facing hair: no scale here.
//!
//! Deviation: what the reflection does not give: the bones under prop1 are flip X rows with back_i <-> front_i, and
//! the body's, the sword's and the shield's textures are not mirrored in the build; here they are the right-facing
//! ones mirrored.
//!
//! The hair components (Pawn_WonderWomanV3 SetRotationLogic @2985..3101: SetSubMeshVisibility(LeftHair, FacingLeft)
//! and (RightHair, !FacingLeft)). LeftHair is the mesh with the triangles wound the other way, the one that faces
//! outward under hair_att's negative scale; in the reflected view it is RightHair that faces outward. So the view
//! draws RightHair at both facings and never LeftHair; the hair is gone only when both are hidden.
//!
//! The constraints (FAnimNode_Constraint): at alpha 1 the bone's component-space rotation becomes the target's,
//! taken through quaternion -> FRotator -> quaternion, its component-space translation the target's, its scale stays;
//! the children keep their local transforms. Alpha (ubergraph @2473..2915, @4447..5538): base(X) = (S == 0), S the
//! SwordSocket / ShieldSocket byte: 2 at DisableWeaponSockets >= 0.1; 0 under 0.1 with no montage playing; else
//! Comp_ItemSocketTracker.Get Item Socket(1 sword / 2 shield), written by AnimNotify_ResocketItem (NotifyBegin
//! Assign Socket(item, socket) for each of its "Items and Sockets", NotifyEnd Assign Socket(item, 0)).
//! The holster: ModifyBone_6 (fml_un_C_prop1, translation and rotation Replace in world space, alpha ItemOverride;
//! after the sword's constraints): the skeleton's socket SwordHolster (on fml_un_C_spine_d, location (-28.331896,
//! 0.000061035156, 17.151218), rotation Pitch -52.150257, Yaw 117.39286, Roll -123.27512). ItemOverride is
//! Comp_ItemManager's OnItemOverride: true at a pick-up, false at ClearHeldItem and the item's loss.
//!
//! Unverified: the rotator round trip is the stock 4.26 FQuat::Rotator and FRotator::Quaternion (14136e550 /
//! 14136a6d0 identified by use). Deviation: computed in doubles on the view's float32 pose.
//! Deviation: the socket tracker is derived here from the shown montage's ResocketItem windows at its position (the
//! sim treats the notify as a no-op); a window's begin and end are taken at their times.
//! Unverified: IsAnyMontagePlaying is the sim's slot instance, blending out and terminated included.
//! DisableWeaponSockets (AnimBPV2 @2473-2621): at 0.1 or more on the shown clip, both sockets are 2 and the
//! constraints off (the clip's own prop keys hold the props). Only the S05 skin's select, victory and defeat
//! sequences carry it (a constant 1), so it is read on the menu scenes' preview only (preview_step); no match clip
//! has it.
//! Deviation: the holster's socket is read from this tick's pose in component space (the build reads last tick's in
//! world space); the right-facing rule is drawn at both facings. ItemOverride is taken as the fighter's held item.
//! Unverified: the scale of the chain above the target and the bone is FTransform's (per-axis, no shear).
//! The other nodes: ModifyBone_2 (the head scale below), the VB bones (not drawn), AnimDynamics and Trail (blHair
//! bones: no vertex of the default skin's meshes is weighted to them). The lasso's aim layers are the sim's slot mix.

use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::{data_of, latent_frames};

use super::{CharRender, PawnComponents, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actors::{MeshRule, Rows};
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::curves::CurveTable;
use crate::pose::{MeshMap, Rec, set_scalar};
use crate::rotator::{rotator_quat, through_rotator};
use crate::skins::FighterDraws;
use crate::view::{CharView, PoseCx, PoseRules};
use mvs_vfx::math::rotation_rows;

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &HEAD,
    pose: Some(wonder_woman_pose),
    components: Some(PawnComponents { make: glow_make, place: place_glow, reset: None }),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name: the lasso's rotation (`rot`).
pub(crate) const RULES: &[(&str, MeshRule)] =
    &[("LassoProjectileFastV3_C", MeshRule { rot: Some(lasso_rot), mesh: None })];

/// LassoProjectileFastV3's rotation: the spawn's (the owner's facing; the ProjectileMovement has no
/// bRotationFollowsVelocity).
fn lasso_rot(_w: &World, pr: &Projectile) -> Option<Rows> {
    let h = std::f64::consts::FRAC_PI_2;
    Some(rotation_rows(0.0, if pr.facing > 0.0 { -h } else { h }, 0.0))
}

/// WonderWoman_AnimBPV2's ModifyBone_2 (scale Additive, bone space, from HeadScale; Pawn_WonderWomanV3 @3287).
const HEAD: [HeadScaleNode; 1] = [HeadScaleNode {
    node: "ModifyBone_2",
    bone: "fml_un_C_head",
    factor: 1.0,
    gated: false,
    moves: [0.0; 3],
    scale: true,
}];

const SWORD: &str = "fml_un_C_prop1";
const SHIELD: &str = "fml_un_C_prop2";
/// Constraint_2's target.
const SWORD_TARGET: &str = "fml_un_R_Prop";
/// Constraint's target.
const SHIELD_TARGET: &str = "fml_un_L_ShieldPivot_b";
/// The socket SwordHolster's bone.
const HOLSTER_BONE: &str = "fml_un_C_spine_d";
/// The build's X, Y, Z.
const HOLSTER_LOC: [f64; 3] = [-28.331896, 0.000061035156, 17.151218];
/// Pitch, Yaw, Roll.
const HOLSTER_ROT: [f64; 3] = [-52.150257, 117.39286, -123.27512];
const RESOCKET: &str = "AnimNotify_ResocketItem_C";
/// Comp_ItemSocketTracker's items.
const ITEM_SWORD: f64 = 1.0;
const ITEM_SHIELD: f64 = 2.0;

/// The pose hook's state: the bones (None: missing), the holster socket in the pose's axes, the tick's alphas.
#[derive(Debug)]
struct WonderWomanPose {
    sword: Option<usize>,
    shield: Option<usize>,
    sword_to: Option<usize>,
    shield_to: Option<usize>,
    spine: Option<usize>,
    /// The holster socket's relative transform ((x, y, z) = the build's (X, Z, Y) / 100; a quaternion (x, y, z, w) =
    /// the build's (X, Z, Y, -W)).
    h_t: [f64; 3],
    h_r: [f64; 4],
    sword_on: bool,
    shield_on: bool,
    holster: bool,
    /// The hair components' parts of the view.
    hair_left: Option<usize>,
    hair_right: Option<usize>,
    /// A preview's view: its character's montages by name (montageOf), whose ResocketItem windows set the sockets.
    preview_montages: Option<Box<mvs_data::fighter::Map<mvs_data::fighter::Montage>>>,
}

fn wonder_woman_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let names = &view.rig.names;
    let bone = |b: &str| {
        let i = names.iter().position(|n| n == b);
        if i.is_none() {
            eprintln!("warning: WonderWoman pose: bone missing: {b}");
        }
        i
    };
    let q = rotator_quat(HOLSTER_ROT[0], HOLSTER_ROT[1], HOLSTER_ROT[2]);
    let part = |k: &str| of.part_keys.iter().position(|p| p.eq_ignore_ascii_case(k));
    Box::new(WonderWomanPose {
        sword: bone(SWORD),
        shield: bone(SHIELD),
        sword_to: bone(SWORD_TARGET),
        shield_to: bone(SHIELD_TARGET),
        spine: bone(HOLSTER_BONE),
        h_t: [HOLSTER_LOC[0] / 100.0, HOLSTER_LOC[2] / 100.0, HOLSTER_LOC[1] / 100.0],
        h_r: [q[0], q[2], q[1], -q[3]],
        sword_on: true,
        shield_on: true,
        holster: false,
        hair_left: part("lefthair"),
        hair_right: part("righthair"),
        preview_montages: None,
    })
}

/// v = q v conjugate(q).
fn rotate(q: [f64; 4], v: &mut [f64; 3]) {
    let [x, y, z] = *v;
    let [qx, qy, qz, qw] = q;
    let (tx, ty, tz) = (2.0 * (qy * z - qz * y), 2.0 * (qz * x - qx * z), 2.0 * (qx * y - qy * x));
    v[0] = x + qw * tx + qy * tz - qz * ty;
    v[1] = y + qw * ty + qz * tx - qx * tz;
    v[2] = z + qw * tz + qx * ty - qy * tx;
}

/// a b (Hamilton product).
fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The component-space transform (T, R, S) of bone `i` in `p`, with an optional child transform (ct, cr) under it:
/// from the bone up, each parent's local transform put in front (FTransform's product).
fn component(
    p: &Pose,
    parents: &[i32],
    i: usize,
    child: Option<([f64; 3], [f64; 4])>,
) -> ([f64; 3], [f64; 4], [f64; 3]) {
    let (mut t, mut r) = child.unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
    let mut s = [1.0; 3];
    let mut k = i as i32;
    while k >= 0 {
        let ku = k as usize;
        let (k3, k4) = (ku * 3, ku * 4);
        let sc = [0, 1, 2].map(|j| f64::from(p.s[k3 + j]));
        for j in 0..3 {
            t[j] *= sc[j];
        }
        let q = [0, 1, 2, 3].map(|j| f64::from(p.r[k4 + j]));
        rotate(q, &mut t);
        for (v, d) in t.iter_mut().zip(&p.t[k3..k3 + 3]) {
            *v += f64::from(*d);
        }
        r = qmul(q, r);
        for j in 0..3 {
            s[j] *= sc[j];
        }
        k = parents[ku];
    }
    (t, r, s)
}

/// Bone `b`'s component-space rotation and translation set to the target's (at alpha 1; with `child` the socket's,
/// ModifyBone_6): the rotation through the rotator, the bone's scale and its children's local transforms kept.
fn constrain(p: &mut Pose, parents: &[i32], b: usize, target: usize, child: Option<([f64; 3], [f64; 4])>) {
    let (mut ct, mut cr, _) = component(p, parents, target, child);
    // to the build's axes, through the rotator, and back
    let u = [cr[0], cr[2], cr[1], -cr[3]];
    let l = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2] + u[3] * u[3]).sqrt();
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    let u = through_rotator(u.map(|v| v / l));
    cr = [u[0], u[2], u[1], -u[3]];
    let (b3, b4) = (b * 3, b * 4);
    let pa = parents[b];
    if pa < 0 {
        put(&mut p.t[b3..b3 + 3], &ct);
        put(&mut p.r[b4..b4 + 4], &cr);
        return;
    }
    let (pt, pr, ps) = component(p, parents, pa as usize, None);
    // local = conjugate(pR) (cT - pT) / pS, conjugate(pR) cR
    let inv = [-pr[0], -pr[1], -pr[2], pr[3]];
    for j in 0..3 {
        ct[j] -= pt[j];
    }
    rotate(inv, &mut ct);
    for j in 0..3 {
        p.t[b3 + j] = (if ps[j] != 0.0 { ct[j] / ps[j] } else { 0.0 }) as f32;
    }
    put(&mut p.r[b4..b4 + 4], &qmul(inv, cr));
}

/// Doubles stored in the float32 pose.
fn put(out: &mut [f32], v: &[f64]) {
    for (d, v) in out.iter_mut().zip(v) {
        *d = *v as f32;
    }
}

impl WonderWomanPose {
    /// The tracker's sockets from a montage's ResocketItem windows at position `pos` (None: no montage).
    fn sockets(&mut self, notifies: Option<&[mvs_data::fighter::Notify]>, pos: f64) {
        let (mut s1, mut s2) = (0.0, 0.0);
        for n in notifies.unwrap_or_default() {
            if n.cls != RESOCKET || pos < n.t || pos > n.t + n.dur {
                continue;
            }
            let Some(rows) = n.p.get("Items and Sockets").and_then(Value::as_arr) else { continue };
            for r in rows {
                let Some(o) = r.as_obj() else { continue };
                let item = o.get("Item").and_then(Value::as_f64);
                let socket = o.get("Socket").and_then(Value::as_f64).unwrap_or(0.0);
                if item == Some(ITEM_SWORD) {
                    s1 = socket;
                } else if item == Some(ITEM_SHIELD) {
                    s2 = socket;
                }
            }
        }
        self.sword_on = s1 == 0.0;
        self.shield_on = s2 == 0.0;
    }
}

/// The AnimBP's curve: DisableWeaponSockets.
const CURVES: CurveTable = CurveTable::new(&["DisableWeaponSockets"]);

impl PoseRules for WonderWomanPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        let parents = cx.parents;
        if self.sword_on
            && let (Some(b), Some(t)) = (self.sword, self.sword_to)
        {
            constrain(p, parents, b, t, None); // Constraint_2 (and _1's reflection)
        }
        if self.holster
            && let (Some(b), Some(t)) = (self.sword, self.spine)
        {
            constrain(p, parents, b, t, Some((self.h_t, self.h_r))); // ModifyBone_6
        }
        if self.shield_on
            && let (Some(b), Some(t)) = (self.shield, self.shield_to)
        {
            constrain(p, parents, b, t, None); // Constraint (and _3 with ModifyBone_8)
        }
    }
}

impl PoseHook for WonderWomanPose {
    /// `preview(partKeys, montageOf)`: a view with no fighter: RightHair shown, LeftHair hidden, the sword
    /// and the shield in hand, no holster; the sockets from the shown montage's ResocketItem windows (preview_step).
    fn preview(&mut self, view: &mut CharView, of: &PreviewOf<'_>) {
        if let Some(l) = self.hair_left {
            view.set_part_visible(l, false);
        }
        if let Some(r) = self.hair_right {
            view.set_part_visible(r, true);
        }
        self.sword_on = true;
        self.shield_on = true;
        self.holster = false;
        self.preview_montages = Some(Box::new(of.fighter.montages.clone()));
    }

    /// The preview's sampleRule: the montage position of the segment's time, and the sockets at it.
    // Deviation: run once a frame before the pose, from the segment the scene shows (it could also run at each sample
    // of the view, with the same clip, time and montage).
    fn preview_step(&mut self, _view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        let Some(ms) = self.preview_montages.take() else { return };
        let m = ms.get(s.montage);
        let mut pos = -1.0;
        for g in m.iter().flat_map(|m| &m.slots).flat_map(|sl| &sl.segments) {
            if g.anim == s.anim && s.time >= g.a0 - 1e-6 && s.time <= g.a1 + 1e-6 && g.rate > 0.0 {
                pos = g.start + (s.time - g.a0) / g.rate;
            }
        }
        self.sockets(m.filter(|_| pos >= 0.0).map(|m| m.notifies.as_slice()), pos);
        // @2473-2621: DisableWeaponSockets at 0.1 or more sets both sockets to 2
        if CURVES.value(clips, s.anim, 0, s.time) >= 0.1 {
            self.sword_on = false;
            self.shield_on = false;
        }
        self.preview_montages = Some(ms);
    }

    /// Before the view's pose of the tick: the sockets from the slot instance's ResocketItem windows (a terminated
    /// instance included), the holster from the held item, the hair components.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, _clips: &Clips) {
        let fi = w.fighter(f);
        let sm = fi.montages.montage.as_ref().or(fi.montages.dash_montage.as_ref());
        let data = sm.and_then(|s| data_of(w, f).fighter.montages.get(w.names.str(s.montage.name)));
        self.sockets(data.map(|m| m.notifies.as_slice()), sm.map_or(0.0, |s| s.pos));
        self.holster = fi.items.held_item.is_some();
        // RightHair at both facings, LeftHair never; the sim hides one of the two by facing (SetRotationLogic): the
        // hair is gone only when both are hidden.
        let hidden = |t: &str| fi.pose.sub_hidden.iter().any(|n| w.names.str(*n) == t);
        if let Some(l) = self.hair_left {
            view.set_part_visible(l, false);
        }
        if let Some(r) = self.hair_right {
            view.set_part_visible(r, !(hidden("righthair") && hidden("lefthair")));
        }
    }
}

/// No mesh component; own[0]: 1 while a clash's glow was written (its end writes 0 once).
fn glow_make(r: &mut Rec, _w: &World, _c: &Clips, _d: &FighterDraws, _m: &MeshMap) {
    r.own = vec![0.0];
}

// ---- WonderWoman_MeterBracerClash's glow ----

const CLASH: &str = "WonderWoman_MeterBracerClash_C";
/// One sim step (s).
const STEP: f64 = 1.0 / 60.0;
/// Timeline_0's TimelineLength.
const GLOW_LENGTH: f64 = 1.2;

/// Curve_WonderWomanGlow (Characters/WonderWomanV2/VFX): RCIM_Cubic (0, 0), RCIM_Cubic (0.75, 10.025749; leave
/// tangent 0.015454846), RCIM_Linear (1.2, 0.016418457); every other tangent 0. FRichCurve::Eval between two keys
/// whose first is cubic: the Bezier of v1, v1 + leave x dt / 3, v2 - arrive x dt / 3, v2. (t, v, arrive, leave).
const GLOW_KEYS: [[f64; 4]; 3] =
    [[0.0, 0.0, 0.0, 0.0], [0.75, 10.025749, 0.0, 0.015454846], [1.2, 0.016418457, 0.0, 0.0]];

fn glow_at(t: f64) -> f64 {
    let k = &GLOW_KEYS;
    if t <= k[0][0] {
        return k[0][1];
    }
    for i in 1..k.len() {
        let (a, b) = (k[i - 1], k[i]);
        if t > b[0] {
            continue;
        }
        let dt = b[0] - a[0];
        let u = (t - a[0]) / dt;
        let w = 1.0 - u;
        let p1 = a[1] + a[3] * dt / 3.0;
        let p2 = b[1] - b[2] * dt / 3.0;
        return w * w * w * a[1] + 3.0 * w * w * u * p1 + 3.0 * w * u * u * p2 + u * u * u * b[1];
    }
    k[k.len() - 1][1]
}

/// Timeline_0's position for the actor's age: SetVisuals(false) at BeginPlay @1235 plays it from
/// the start; ActivateObject @1700 -> EarlyTrigger(0.8) = SetNewTime(0.8) @3005. The activation's age is the
/// Delay(0.02) before the hitbox less.
// Unverified: the timeline's time is the actor's ticks of 1/60 s; a timeline that had ended (1.2 s without an
// activation) stays stopped at 0.8 after SetNewTime.
fn glow_time(age: i32, on_age: i32) -> f64 {
    if on_age < 0 {
        return (f64::from(age) * STEP).min(GLOW_LENGTH);
    }
    let at = f64::from(on_age) - f64::from(latent_frames(0.02));
    if at * STEP >= GLOW_LENGTH {
        return 0.8;
    }
    (0.8 + (f64::from(age) - at) * STEP).min(GLOW_LENGTH)
}

/// The missing material 0 warned of (once).
static NO_GLOW_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The blast's tick @2950 (Emissive_Color_Mult on the instigator's Mesh material 0, the curve at the timeline's
/// time) and ReceiveEndPlay @2899 (0 again): on her view, once a presentation tick.
// Unverified: CreateDynamicMaterialInstance on a slot that holds the skin's dynamic instance returns that instance
// (4.26 source, not read): the parameter is written on the body's own material (bodyMaterials[0], the first skin
// instance).
fn place_glow(r: &mut Rec, w: &World) {
    let id = w.fighter(r.fighter()).id;
    let mut glow = None;
    for (_, pr) in w.actors.projectiles.iter() {
        if pr.dead || pr.owner_id != id || w.names.str(pr.cls) != CLASH {
            continue;
        }
        let ProjectileExt::BracerClash(c) = &pr.ext else { continue };
        glow = Some(glow_at(glow_time(pr.age, c.on_age)));
    }
    // (the end: 0 once, when the last clash goes)
    let was = r.own.first().copied().unwrap_or(0.0) != 0.0;
    if let Some(o) = r.own.first_mut() {
        *o = if glow.is_some() { 1.0 } else { 0.0 };
    }
    let v = match glow {
        Some(g) => g,
        None if was => 0.0,
        None => return,
    };
    match r.flash_material(0) {
        Some(m) => set_scalar(m, "Emissive_Color_Mult", v),
        None => {
            if !NO_GLOW_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("warning: WonderWoman_BracerClash: the owner has no material 0 to glow");
            }
        }
    }
}
