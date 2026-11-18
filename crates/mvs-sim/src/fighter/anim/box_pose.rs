//! The pose of the box tracks (the hit and hurt shapes) at the actor's scale, and the pitch the anim graph's
//! ModifyBone nodes give them.

use super::GroundState;
use super::MontageSlot;
use super::blend_spaces::set_of;
use super::montages::{char_data, instance_data, slot_uid};
use crate::Real;
use crate::combat::boxes::{BoxSampler, sample_of};
use crate::fighter::FighterId;
use crate::name;
use crate::names::Name;
use crate::world::World;

/// A Float32Array store.
#[allow(clippy::cast_possible_truncation, reason = "a Float32Array store rounds to float32")]
fn f32_of(x: Real) -> f32 {
    x
}

/// The fighter's box sampler (`f.boxes`).
fn sampler(w: &World, f: FighterId) -> &BoxSampler {
    let ch = w.fighter(f).ch;
    w.fixed.samplers.get(&ch).unwrap_or_else(|| panic!("no box sampler for {}", w.names.str(ch)))
}

/// A frame's numbers (`f.boxes.index.stride`).
fn stride(w: &World, f: FighterId) -> usize {
    let cd = char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    usize::try_from(cd.boxes.stride).expect("a box stride")
}

/// `f.boxes.sample(anim, time, out)`: the box tracks of `anim` at `time` into `out` (sized to a frame); false
/// without the sequence.
fn sample(w: &World, f: FighterId, anim: Name, time: Real, out: &mut Vec<f32>) -> bool {
    out.resize(stride(w, f), 0.0);
    sample_of(w, w.fighter(f).ch, w.names.str(anim), time, out)
}

/// `poseBoxes`: a pose of the box tracks at the actor's scale, its shapes on the root capsule placed at the capsule
/// (`BoxSampler.onCapsule`). False (`out` as it was sized) without the sequence.
pub fn pose_boxes(w: &World, f: FighterId, anim: Name, time: Real, out: &mut Vec<f32>) -> bool {
    if !scaled_boxes(w, f, anim, time, out) {
        return false;
    }
    let h = &w.fighter(f).hit;
    sampler(w, f).on_capsule(out, h.mesh_x, h.mesh_z);
    true
}

/// A pose buffer of the World's pool ([`put_pose`] hands it back; a sample inside a sample takes another).
pub fn take_pose(w: &World) -> Vec<f32> {
    w.scratch.poses.borrow_mut().pop().unwrap_or_default()
}

/// A buffer of [`take_pose`], [`posed_boxes`] or [`sampled_boxes`] back to the World's pool (one not given back
/// is made anew at the next take).
pub fn put_pose(w: &World, v: Vec<f32>) {
    w.scratch.poses.borrow_mut().push(v);
}

/// A skeleton's scratch (`Skel.tmp`): the FK and row buffers and five poses, for a skeleton of
/// `n` bones.
#[derive(Clone, Debug)]
pub struct SkelWork {
    pub n: usize,
    pub sc: mvs_anim::skel_pose::SkelScratch,
    pub p: [mvs_anim::skel_pose::SkelPose; 5],
}

/// The World's skeleton scratch for a skeleton of `n` bones ([`put_skel_work`] hands it back).
pub fn take_skel_work(w: &World, n: usize) -> SkelWork {
    use mvs_anim::skel_pose::{SkelPose, SkelScratch};
    match w.scratch.skel.borrow_mut().take() {
        Some(k) if k.n == n => k,
        _ => SkelWork { n, sc: SkelScratch::new(n), p: std::array::from_fn(|_| SkelPose::new(n)) },
    }
}

/// A [`take_skel_work`] scratch back to the World.
pub fn put_skel_work(w: &World, k: SkelWork) {
    *w.scratch.skel.borrow_mut() = Some(k);
}

/// [`pose_boxes`] into a buffer of the World's pool.
pub fn posed_boxes(w: &World, f: FighterId, anim: Name, time: Real) -> Option<Vec<f32>> {
    let mut out = take_pose(w);
    if pose_boxes(w, f, anim, time, &mut out) {
        Some(out)
    } else {
        put_pose(w, out);
        None
    }
}

pub(crate) fn scaled_boxes(w: &World, f: FighterId, anim: Name, time: Real, out: &mut Vec<f32>) -> bool {
    if !sample(w, f, anim, time, out) {
        return false;
    }
    scale_boxes(w, f, out);
    true
}

/// `scaleBoxes`: a sampled pose at the actor's scale, sizeScale, or actorScale where a graph set a non-uniform one.
pub fn scale_boxes(w: &World, f: FighterId, s: &mut [f32]) {
    let b = &w.fighter(f).body;
    if let Some([xy, z]) = b.actor_scale {
        sampler(w, f).scale3(s, xy, z);
    } else if b.size_scale != 1.0 {
        sampler(w, f).scale(s, b.size_scale);
    }
}

/// `sampleBoxes`: the box tracks of the current pose (anim / animTime with the slot mix), turned by the hit
/// rotation. A character whose AnimBP lays poses over that clip under its shapes composes them itself
/// (CharHooks.pose: the hit rotation is in its pose then). False without the sequence.
pub fn sample_boxes(w: &World, f: FighterId, out: &mut Vec<f32>) -> bool {
    if !mesh_boxes(w, f, out) {
        return false;
    }
    let h = &w.fighter(f).hit;
    sampler(w, f).on_capsule(out, h.mesh_x, h.mesh_z);
    true
}

/// [`sample_boxes`] short of placing the root capsule's shapes at the capsule: the shapes as the mesh holds them.
fn mesh_boxes(w: &World, f: FighterId, out: &mut Vec<f32>) -> bool {
    let (anim, time) = (w.fighter(f).anim.anim, w.fighter(f).anim.anim_time);
    if !sample(w, f, anim, time, out) {
        return false;
    }
    let hooks = w.fighter(f).hooks.char;
    let posed = hooks.pose.is_some_and(|pose| pose(w, f, out));
    scale_boxes(w, f, out);
    if !posed {
        // its AnimBP's slot mix (SlotMix)
        if let Some(mix) = hooks.slot_mix
            && let Some(mo) = mix(w, f)
            && !mo.slot_mix.is_empty()
        {
            // Deviation: the mix's scratch is not part of the state record (mixBuf); it is a buffer of the World's
            // pool.
            let mut buf = take_pose(w);
            for m in &mo.slot_mix {
                if !scaled_boxes(w, f, m.anim, m.time, &mut buf) {
                    continue;
                }
                for (v, &b) in out.iter_mut().zip(&buf) {
                    let s = crate::real(*v);
                    *v = f32_of(s + (crate::real(b) - s) * m.w);
                }
            }
            put_pose(w, buf);
        }
        if let Some(pitch) = hit_pitch(w, f)
            && pitch != 0.0
            && !pitch.is_nan()
        {
            // a turn by 0 leaves them as they are (sin 0 = 0, cos 0 = 1)
            turn_boxes(w, f, out, pitch);
        }
    }
    // its AnimBP's source under a montage below full weight
    if let Some(under) = hooks.slot_source
        && shown_weight(w, f).is_some_and(|wt| wt < 1.0)
    {
        under(w, f, out);
    }
    true
}

/// The fighters' hurt shapes as the step found them (`Scratch::struck`): the pose each mesh holds when the step
/// begins, before any tick of it. Taken at the start of every step, for every fighter of the match.
pub fn take_struck_poses(w: &mut World) {
    let mut kept = std::mem::take(&mut w.scratch.struck);
    let ids = w.pawns.as_ref().unwrap_or(&w.players);
    kept.truncate(ids.len());
    while kept.len() < ids.len() {
        kept.push(StruckPose::default());
    }
    for (k, &f) in kept.iter_mut().zip(ids) {
        k.f = Some(f);
        k.ok = mesh_boxes(w, f, &mut k.boxes);
    }
    w.scratch.struck = kept;
}

/// The step's end: the poses [`take_struck_poses`] took no longer hold.
pub fn drop_struck_poses(w: &mut World) {
    for k in &mut w.scratch.struck {
        k.f = None;
    }
}

/// One fighter's pose of [`take_struck_poses`] (its buffer kept across steps).
#[derive(Clone, Debug, Default)]
pub struct StruckPose {
    /// The fighter, `None` outside the step that took it.
    pub f: Option<FighterId>,
    /// The sample found the sequence (`boxes` holds a pose).
    pub ok: bool,
    pub boxes: Vec<f32>,
}

/// `vic.sampleBoxes(out)` for LeBron's ball's overlap with a fighter's hurt shapes: the pose the fighter held at the
/// step's start, at the root where he stands now (the capsule's own shapes and the mesh offset of now). The live
/// read: the ball meets LeBron's HurtBox_LeftFoot at t49 with the t48 pose only, and misses it with the t49 one.
/// The test of the fighter's own update (`World::overlap_mid_frame`), outside a step, and for a fighter the step
/// began without (a pawn spawned in it) take the pose of now ([`sample_boxes`]).
/// Unverified: why the build's overlap sees the older pose is not decoded. HurtBox is QueryOnly, so its moved
/// transform meets queries at once, and no rule found makes every spawned actor's overlap see the step's start; the
/// other actors' and items' overlaps keep the pose of now until one is read live.
pub fn struck_boxes(w: &World, f: FighterId, out: &mut Vec<f32>) -> bool {
    if w.overlap_mid_frame {
        return sample_boxes(w, f, out);
    }
    let Some(k) = w.scratch.struck.iter().find(|k| k.f == Some(f)) else {
        return sample_boxes(w, f, out);
    };
    if !k.ok {
        return false;
    }
    out.clone_from(&k.boxes);
    let h = &w.fighter(f).hit;
    sampler(w, f).on_capsule(out, h.mesh_x, h.mesh_z);
    true
}

/// [`sample_boxes`] into a buffer of the World's pool.
pub fn sampled_boxes(w: &World, f: FighterId) -> Option<Vec<f32>> {
    let mut out = take_pose(w);
    if sample_boxes(w, f, &mut out) {
        Some(out)
    } else {
        put_pose(w, out);
        None
    }
}

/// The shown montage's weight (`f.shownMontage.weight`), `None` with none shown.
fn shown_weight(w: &World, f: FighterId) -> Option<Real> {
    let dash = crate::fighter::shown_montage(w, f)?;
    let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main })?;
    let m = instance_data(&w.data, w, f, uid)?;
    let fi = w.fighter(f);
    let s = if dash { fi.montages.dash_montage.as_ref() } else { fi.montages.montage.as_ref() }?;
    Some(crate::math::narrow(s.weight(m)))
}

/// AirState's four states whose top ModifyBone takes FallLean.
fn leaning(gs: GroundState) -> bool {
    matches!(gs, GroundState::Fall | GroundState::Jump1 | GroundState::Jump2 | GroundState::FastFall)
}

/// `layerPitch`: the layer's own pitch with the montage aside (the pose under the slots, which a montage blending
/// out mixes with: the Slot node's source carries it).
///
/// Base_AnimBP's hit rotation (see [`hit_pitch`]) and the same node with its own pitch in two more places of the
/// layer: ModifyBone_8 / _9 / _10 / _11 at the top of the AirState states Fall, Jump1, Jump2 and FastFall take
/// FallLean, ModifyBone_13 at the top of Bounce takes BouncePitchAngleDegrees (handlers @15039, @14888, @13822,
/// @13574, @24377: Rotation = MakeRotator(0, value, 0), Alpha 1, no binding; same bone, mode and space). Each sits
/// in its own state, so one pitch a tick; it is returned at 0 too (the Replace still puts the bone's own rotation
/// away: the drawn pose, and the shapes of the clips the box import bakes with it).
pub fn layer_pitch(w: &World, f: FighterId) -> Option<Real> {
    let fi = w.fighter(f);
    if fi.anim.carried.is_some() {
        return None; // the carried pose stands in for the machine (hit_pitch)
    }
    let gs = fi.sm.gs;
    if leaning(gs) {
        return Some(fi.react.fall_lean);
    }
    if gs == GroundState::Bounce {
        return Some(fi.react.bounce_pitch);
    }
    hit_pitch(w, f)
}

/// `hitPitch`: Base_AnimBP's hit rotation, the pitch in degrees the HitStunned fly branch gives the bone
/// fml_un_COM, `None` while that branch is not the pose; the Fall / Jump / Bounce pitches of [`layer_pitch`] while
/// no montage is shown.
///
/// AnimGraph: MovementAndHitReact's HitStunned state is BlendListByBool_15(Has_N_HR && IsNeutralHit ||
/// IsBuffStunned), whose false pose is ComponentToLocal <- ModifyBone_5 <- LocalToComponent <-
/// BlendListByBool_12(KB_Facing) over the two intro machines: the neutral reacts, Recovery, Bounce and HitWobble do
/// not pass the node, and every blend around it is 0 s. The node (BoneToModify fml_un_COM, RotationMode
/// BMM_Replace, RotationSpace BCS_ComponentSpace, translation and scale ignored) takes its inputs from the handler
/// @21065-21526 (its copy @25286-25747):
///   Rotation = MakeRotator(Roll 0, Pitch, Yaw 0), Alpha = BoolToFloat(HitRotationOn),
///   Pitch = KBFacing_Bool ? (Rotate90OnHit ? DirectionSoft + 90 : DirectionSoft) x FlyforwardOrientMultiplier
///                         : (Rotate90OnHit ? DirectionSoft - 90 : DirectionSoft) x FlybackOrientMultiplier.
/// KBFacing_Bool (SetHorizontalAndVerticalSpeed @4425-4704, every update, with the facing of that update):
/// (KnockBackVelocity.Y >= 0 && FacingLeft) || (!FacingLeft && KnockBackVelocity.Y <= 0), the launch toward the
/// facing (UE Y = -x). HitRotationOn is true in the class defaults and only Buff_VelmaAttachToAlly writes it
/// (BPI_SetHitRotaion(false) @5352 of its ubergraph, Base_AnimBP @27946): Alpha 1, or 0 (`None`).
/// The montage slots are above the state machines in the port's pose (updatePose), so a shown montage hides the
/// turn. In the build too (node links of the cooked classes): Base_AnimBP has no slot node at all (Root <-
/// BlendListByBool <- the carried override | StateMachine MovementAndHitReact); it is the BaseMovementMachine layer
/// of the mesh's AnimBP (Source_AnimBP: LinkedAnimLayer BaseMovementMachine -> JumpSlot -> DefaultSlot -> Root;
/// Shaggy_AnimBP and c019_AnimBP have their slots in the same graph), so every slot takes the whole layer's output,
/// the turned fly pose included, as its source. A slot at a weight between 0 and 1 (a montage blending out) mixes
/// the two poses; here the turn is all or nothing. A montage is stopped by the knockback hit (Montage_Stop(0)) and
/// none starts in hitstun, so no blend-out lies over the fly pose.
/// Deviation: a C019 aim / move slot mixed over the layer's pose (the slot mix) is turned with it here and in the
/// drawing, where the build's slots come after the node.
/// The hurt shapes take no such mix ([`sample_boxes`] samples the montage's shapes whole until it is terminated,
/// then the layer's, turned; a character's CharHooks.slotSource aside: Batman's glide stack), so they keep this.
pub fn hit_pitch(w: &World, f: FighterId) -> Option<Real> {
    let fi = w.fighter(f);
    // The carried pose (Base_AnimBP's layer root on IsBeingCarried) is shown in place of the whole movement machine,
    // so none of its pitches. Its own ModifyBone_7 replaces fml_un_COM's rotation in parent bone space with
    // MakeRotator(0, COM_Override_Rotation, 0), 0 unless a car's wheelie sets it.
    // Unverified: that rotation of 0 is taken as the clip's own (no turn).
    if fi.anim.carried.is_some() {
        return None;
    }
    let gs = fi.sm.gs;
    if leaning(gs) {
        return if crate::fighter::shown_montage(w, f).is_some() { None } else { Some(fi.react.fall_lean) };
    }
    if gs == GroundState::Bounce {
        return if crate::fighter::shown_montage(w, f).is_some() { None } else { Some(fi.react.bounce_pitch) };
    }
    let br = fi.react.hr_branch;
    if gs != GroundState::HitStunned
        || (br != name!("b") && br != name!("f"))
        || fi.montages.montage.is_some()
        || fi.montages.dash_montage.is_some()
        || fi.react.hit_rotation_off == Some(true)
    {
        return None;
    }
    let an = set_of(w, f);
    let ds = fi.react.direction_soft;
    let kb = fi.react.hit_stun_kb_vx;
    // KBFacing_Bool
    let to_facing = if fi.body.facing_dir < 0.0 { kb <= 0.0 } else { kb >= 0.0 };
    let r90 = an.rotate_90_on_hit;
    Some(if to_facing {
        (if r90 { ds + 90.0 } else { ds }) * crate::math::narrow(an.fly_forward_orient)
    } else {
        (if r90 { ds - 90.0 } else { ds }) * crate::math::narrow(an.fly_back_orient)
    })
}

/// `turnBoxes`: FAnimNode_ModifyBone with BMM_Replace in component space. The bone keeps its place and takes the
/// rotator as its component-space rotation, and every bone under it follows. In the clips of the states with the
/// node the bone's own component rotation is the identity (it hangs on Root; the box import bakes those clips with
/// it set to the identity: its "COM Replace", Shaggy_HR_Floored's roll), so the replacement turns what rides the
/// bone (BoxShape.com: all 16 hurt capsules, the bone hit spheres and sockets) about the bone's point by the
/// rotator. A pitch P turns +X (forward) toward +Z (up) (FRotator(P, 0, 0).Quaternion() = (0, -sin P/2, 0,
/// cos P/2), the bone's rotation in a memory read of the build): in the play plane (fwd, up) -> (fwd cos P - up
/// sin P, fwd sin P + up cos P), the depth as it is (the mirror node's axis is the depth too, so it is the same turn
/// for both facings). A capsule's radius scale stays. Also a character's own ModifyBone of that kind on the bone
/// (CharHooks.pose: Creature_AnimBP's ModifyBone_1), on the box tracks before the actor's scale (a uniform scale
/// about the origin: the turn about the scaled point is the same). The turn is the build's: FRotator::Quaternion, then
/// the quaternion's RotateVector (FTransform's child composition), the bone's point added after.
pub fn turn_boxes(w: &World, f: FighterId, s: &mut [f32], pitch: Real) {
    let b = sampler(w, f);
    let Some(com) = b.com else { return };
    let com = usize::try_from(com).expect("a box offset");
    let (cx, cy) = (s[com], s[com + 1]);
    let at = |o: u32| usize::try_from(o).expect("a box offset");
    for &o in &b.com_pts {
        let o = at(o);
        let [fw, u] = crate::math::ue::turn_by_pitch(pitch, [s[o] - cx, s[o + 1] - cy]);
        s[o] = cx + fw;
        s[o + 1] = cy + u;
    }
    for &o in &b.com_dirs {
        let o = at(o);
        let [fw, u] = crate::math::ue::turn_by_pitch(pitch, [s[o], s[o + 1]]);
        s[o] = fw;
        s[o + 1] = u;
    }
}
