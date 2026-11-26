//! The match camera and its shake: PFGCamera's camera mode 0 fed each
//! tick from the World, beside the sim (the sim keeps only on_camera), and the Camera of each frame.
//!
//! All the rig's maths is in UE units on the Y/Z plane (UE Y = -sim x, UE Z = sim y); X is the depth toward the
//! camera. Per tick (PFGCamera ReceiveTick):
//!  - the BBox (AABB_C) refresh is rate limited: when BoundingBoxRateLimiter <= 0 it is recomputed and the limiter set
//!    to 0.1, otherwise the limiter loses DeltaTime; between refreshes the old Min / Max / ObjectCount are kept;
//!  - Arm = clamp(max(hW / tan(FOV/2), hH * aspect / tan(FOV/2)), CameraArmMin, CameraArmMax), hW / hH the half
//!    extents of the buffered, blast-box-clamped box plus 500, FOV the CameraComponent's current value (last tick's,
//!    UE's default 90 on the first tick);
//!  - CameraComponent.FieldOfView = aspect / 1.777778 * CameraFOV (horizontal; bConstrainAspectRatio is false);
//!  - Target = (arm cos A, midY, midZ + arm sin A), pitch A, through ClampCameraMoveToBlastBox;
//!  - CameraCenterPoint eases toward it with TEase EaseOut, alpha = lerp(LowAlphaEase, HighAlphaEase,
//!    clamp(dist / EaseLerpValue)) and exponent ZoomOut (current X < target X) or ZoomIn, not scaled by DeltaTime.
//!
//! The PFGCamera actor (TrainingContainer ContainerCamera: root at relative (0, 0, 100), yaw 180) rides
//! CameraCenterPoint with that relative transform: the lens is 100 above the eased point along its up axis and looks
//! down CameraAngle. Priority framing is inert: CurrentPriorityValue copies PriorityDurationRemaining, which is only
//! ever decreased.
//!
//! Onscreen actors (RegisteredOnscreenActors: Comp_Actor_KeepOnCamera Register = Array_AddUnique, Unregister =
//! Array_RemoveItem, so the array is in registration order): the fighters, the ring-out anchors, Morty's marker and
//! its camera focus (projectiles keep KeepOnCamera? false). The refresh (@1915..2483) sets ObjectCount to the array's
//! length (read only as ObjectCount >= 1) and, with one actor or more, resets the box and adds each in order. AABB Add
//! (the AABB_C Blueprint): the first point after Reset sets Min = Max = the point (@2057, Initialized false); each
//! later one goes through the SelectVector on IsBaseFighter (@1032..1831), a local of Add set only where
//! cast<BP_FighterCharacter_C>(Actor) succeeds (@218 -> @422): a fighter (a dead one too) adds its plain point, any
//! other actor its location moved by BufferVector toward the box (Min = min(Min, P + Buffer), Max = max(Max, P -
//! Buffer)): it widens the box only by what it is further out than 400. The onscreen sphere components
//! (RegisteredOnscreenSphereCollision, AddOnscreenComponent @PFGCamera 1352) follow the actors in the refresh
//! (@2617..2977), each buffered as an actor that is no fighter; that loop runs only when an actor is visible and never
//! counts in ObjectCount. The one user in the build is Pawn_C017's CameraTracker sphere (BeginPlay @12470 to EndPlay
//! @12504).
//!
//! The blast box shrink (FFAGameMode's ShrinkBlastBox, Mutator_ShrinkBlastBoxLastPoint's): the tick copies CameraAngle
//! / CameraFOV from the CameraCenter actor (@1636), whose AdjustCameraForShrinkingBlastBox(time) plays a 1.0 long
//! linear timeline at rate 1 / time setting CameraAngle = Lerp(OriginalCameraAngle, ShrunkCameraAngle, Alpha)
//! (CameraCenter ubergraph @15). The tick's BlastBoxTopLeftCorner / BlastBoxBottomRightCorner are fetched once, while
//! its BlastBox reference is not valid (@1831 -> @12651..13444), so the bounding box stays clamped to the unshrunk
//! corners; ClampCameraMoveToBlastBox asks the blast box again on every call (@255..652) and so follows the corners
//! AdjustCornerPositions rewrites.

use indexmap::IndexMap;
use mvs_data::stage::{NumOrBool, StageData};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, MarkerId};
use mvs_sim::fighter::FighterId;
use mvs_sim::{Event, World};

use crate::frame::{Camera, M4};
use crate::wind_cam::{Lean, WindCam};

const ASPECT_REF: f64 = 1.777778;
const UE_DEFAULT_FOV: f64 = 90.0;
/// The rig's tick (the 60 Hz step).
const TICK: f64 = 1.0 / 60.0;
const PAD: f64 = 500.0;
const LENS_UP: f64 = 100.0;
/// AABB_C CDO BufferVector (400, 400, 400).
const BUFFER: f64 = 400.0;
/// The projection's near and far planes, cm.
// Deviation: a reversed-depth projection with a far plane (a perspective camera of near 0.1 m, far
// 100000 m), where the build's FReversedZPerspectiveMatrix has none ([2][2] 0).
pub const NEAR: f64 = 0.1;
pub const FAR: f64 = 100000.0;

fn dsin(d: f64) -> f64 {
    (d * std::f64::consts::PI / 180.0).sin()
}

fn dcos(d: f64) -> f64 {
    (d * std::f64::consts::PI / 180.0).cos()
}

fn dtan(d: f64) -> f64 {
    (d * std::f64::consts::PI / 180.0).tan()
}

/// A point the camera follows, in sim units (x = -UE Y, y = UE Z). A fighter's also carries its mesh's relative
/// offset (mesh_x along its facing, mesh_z up): AABB reads the Mesh component's world location, so the hit wobble and
/// the bounce offset move the point; mesh_z is the whole RelativeLocation.Z, the class's own (Shaggy -193) plus the
/// bounce offset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraTarget {
    pub x: f64,
    pub y: f64,
    /// An actor that is no fighter: its actor location, buffered unless it is the first.
    pub actor: bool,
    pub crouched: bool,
    pub facing: f64,
    pub mesh_x: f64,
    pub mesh_z: f64,
}

// ---- camera shake --------------------------------------------------------------------------------------------

/// A MatineeCameraShake oscillator: amplitude, frequency, InitialOffset EOO_OffsetZero (else random).
#[derive(Clone, Copy, Debug)]
struct Osc {
    amp: f64,
    freq: f64,
    zero: bool,
}

#[derive(Clone, Copy, Debug)]
struct ShakeDef {
    dur: f64,
    blend_in: f64,
    blend_out: f64,
    x: Osc,
    z: Osc,
    /// RotOscillation: pitch, yaw, roll (degrees).
    rot: [Osc; 3],
    /// bSingleInstance: a play of the class restarts the one playing instead of adding another.
    single: bool,
}

const fn osc(amp: f64, freq: f64, zero: bool) -> Osc {
    Osc { amp, freq, zero }
}

const NO_OSC: Osc = osc(0.0, 0.0, true);

/// Panda_Main/Blueprints/CameraShakes: the MatineeCameraShake CDOs (location oscillation only, all bSingleInstance),
/// and Panda_Main/PreMatch/PM_CameraShake (Seq_Clash's camera shake track: rotation oscillation only).
fn shake_def(cls: &str) -> Option<ShakeDef> {
    let d =
        |dur, blend_in, blend_out, x, z| ShakeDef { dur, blend_in, blend_out, x, z, rot: [NO_OSC; 3], single: true };
    Some(match cls {
        // OscillationDuration 1.65, OscillationBlendOutTime 0.5; Pitch 0.25 at 50, Yaw 0.25 at 50 EOO_OffsetZero,
        // Roll 0.5 at 100.
        // Unverified: OscillationBlendInTime is not stored: the class default 0.1 is 4.26's (the ctor 1428edaf0 is
        // not read).
        "PM_CameraShake_C" => ShakeDef {
            dur: 1.65,
            blend_in: 0.1,
            blend_out: 0.5,
            x: NO_OSC,
            z: NO_OSC,
            rot: [osc(0.25, 50.0, false), osc(0.25, 50.0, true), osc(0.5, 100.0, false)],
            // bSingleInstance false (the CDO stores it): Seq_Clash's sections each play their own instance.
            single: false,
        },
        "CameraShakeVeryLight_C" => d(0.035, 0.015, 0.035, osc(2.0, 100.0, false), osc(5.0, 100.0, true)),
        "CameraShakeLight_C" => d(0.075, 0.04, 0.025, osc(8.0, 100.0, false), osc(12.0, 100.0, true)),
        "CameraShakeMedium_C" => d(0.095, 0.04, 0.025, osc(15.0, 100.0, true), osc(22.0, 100.0, false)),
        "CameraShakeLarge_C" => d(0.132, 0.02, 0.04, osc(44.0, 40.0, true), osc(48.0, 72.0, false)),
        "CameraShakeWall_C" => d(0.13, 0.02, 0.04, osc(20.0, 80.0, true), osc(20.0, 80.0, false)),
        "CameraShakeBounces_C" => d(0.1, 0.04, 0.025, osc(3.5, 70.0, false), osc(6.0, 70.0, true)),
        _ => return None,
    })
}

/// The CRT's rand() (MSVC: state * 214013 + 2531011, bits 16..30).
#[derive(Clone, Debug)]
struct CrtRand(u32);

impl CrtRand {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        (self.0 >> 16) & 0x7fff
    }
}

/// One playing shake (UMatineeCameraShake as read from the exe: StartShake 14290a8c0, UpdateShake 14290cac0,
/// IsFinished 142902950; vtable 144221260).
#[derive(Clone, Debug)]
struct Shake {
    cls: String,
    d: ShakeDef,
    remaining: f64,
    blending_in: bool,
    blending_out: bool,
    in_time: f64,
    out_time: f64,
    off_x: f64,
    off_z: f64,
    off_rot: [f64; 3],
    /// ShakeScale: the play's scale (a sequence section's PlayScale), set by each StartShake.
    scale: f64,
    finished: bool,
    x: f64,
    z: f64,
    rot: [f64; 3],
}

impl Shake {
    fn new(cls: &str, d: ShakeDef) -> Shake {
        Shake {
            cls: cls.to_owned(),
            d,
            remaining: 0.0,
            blending_in: false,
            blending_out: false,
            in_time: 0.0,
            out_time: 0.0,
            off_x: 0.0,
            off_z: 0.0,
            off_rot: [0.0; 3],
            scale: 1.0,
            finished: false,
            x: 0.0,
            z: 0.0,
            rot: [0.0; 3],
        }
    }

    fn start(&mut self, rand: &mut CrtRand, scale: f64) {
        let d = self.d;
        self.scale = scale;
        if self.remaining > 0.0 {
            // Already playing: the duration restarts; a blend-out turns back into a blend-in. The engine zeroes
            // CurrentBlendOutTime before using it, so the blend-in resumes at full weight.
            self.remaining = d.dur;
            if self.blending_out {
                self.blending_out = false;
                self.out_time = 0.0;
                self.blending_in = d.blend_in > 0.0;
                self.in_time = if d.blend_in > 0.0 { d.blend_in * (1.0 - self.out_time / d.blend_out) } else { 0.0 };
            }
        } else {
            // A random initial offset is (rand() & 0x7fff) * 2 pi / 32767.
            // Unverified: the CRT stream is the camera's own from the CRT's seed 1; the build's rand() is shared by
            // every engine caller, so its place in the stream is not known.
            let mut initial = |o: Osc| {
                if o.zero { 0.0 } else { f64::from(rand.next() & 0x7fff) * 2.0 * std::f64::consts::PI / 32767.0 }
            };
            self.off_x = initial(d.x);
            self.off_z = initial(d.z);
            // The rotation's offsets after the location's (StartShake's order).
            // Unverified: an oscillator of no amplitude draws none here, where the build draws for every oscillator that
            // is not EOO_OffsetZero (about six draws against these two or three).
            for k in 0..3 {
                if d.rot[k].amp != 0.0 {
                    self.off_rot[k] = initial(d.rot[k]);
                }
            }
            self.remaining = d.dur;
            if d.blend_in > 0.0 {
                self.blending_in = true;
                self.in_time = 0.0;
            }
        }
    }

    fn update(&mut self, dt: f64) {
        let d = self.d;
        self.x = 0.0;
        self.z = 0.0;
        self.rot = [0.0; 3];
        if self.remaining > 0.0 {
            self.remaining = (self.remaining - dt).max(0.0);
        }
        if self.blending_in {
            self.in_time += dt;
        }
        if self.blending_out {
            self.out_time += dt;
        }
        let mut done = false;
        if self.remaining == 0.0 {
            done = true;
        } else if self.remaining < d.blend_out {
            self.blending_out = true;
            self.out_time = d.blend_out - self.remaining;
        }
        if self.blending_in && self.in_time > d.blend_in {
            self.blending_in = false;
        }
        if self.blending_out && self.out_time > d.blend_out {
            self.out_time = d.blend_out;
        }
        if done {
            self.finished = true;
            return;
        }
        let w_in = if self.blending_in { self.in_time / d.blend_in } else { 1.0 };
        let w_out = if self.blending_out { 1.0 - self.out_time / d.blend_out } else { 1.0 };
        let w = w_in.min(w_out) * self.scale.max(0.0);
        if w <= 0.0 {
            return;
        }
        if d.x.amp != 0.0 {
            self.off_x += dt * d.x.freq;
            self.x = d.x.amp * self.off_x.sin() * w;
        }
        if d.z.amp != 0.0 {
            self.off_z += dt * d.z.freq;
            self.z = d.z.amp * self.off_z.sin() * w;
        }
        for k in 0..3 {
            if d.rot[k].amp != 0.0 {
                self.off_rot[k] += dt * d.rot[k].freq;
                self.rot[k] = d.rot[k].amp * self.off_rot[k].sin() * w;
            }
        }
    }
}

/// UCameraModifier_CameraShake (vtable 144223f88): the playing shakes, summed. AddCameraShake 1428eefc0 restarts a
/// live instance of a bSingleInstance class through StartShake; ModifyCamera 142903340 updates every active shake,
/// then removes the finished ones. Played through PlayWorldCameraShake at the origin with inner 0, outer 900000 and
/// falloff 0, so CalcRadialShakeScale is pow(x, 0) = 1; ApplyPlaySpace 1428f05d0 (CameraLocal) puts X along the view
/// direction and Z along the camera's up.
#[derive(Clone, Debug)]
pub struct CameraShaker {
    active: Vec<Shake>,
    rand: CrtRand,
    /// The summed camera-local offset (UE units) of the last update.
    pub x: f64,
    pub z: f64,
    /// The summed rotation (pitch, yaw, roll, degrees) of the last update, composed in the camera's space
    /// (`turned_axes`).
    pub rot: [f64; 3],
}

impl Default for CameraShaker {
    fn default() -> CameraShaker {
        CameraShaker { active: Vec::new(), rand: CrtRand(1), x: 0.0, z: 0.0, rot: [0.0; 3] }
    }
}

impl CameraShaker {
    /// Plays the shake class `cls` (a class not among the build's shakes plays nothing).
    pub fn play(&mut self, cls: &str) {
        self.play_scaled(cls, 1.0);
    }

    /// Plays `cls` at the scale `scale` (UpdateAndApplyCameraShake's max(ShakeScale x Alpha, 0), Alpha 1).
    pub fn play_scaled(&mut self, cls: &str, scale: f64) {
        let Some(d) = shake_def(cls) else { return };
        match self.active.iter_mut().find(|s| d.single && s.cls == cls && !s.finished) {
            Some(s) => s.start(&mut self.rand, scale),
            None => {
                let mut s = Shake::new(cls, d);
                s.start(&mut self.rand, scale);
                self.active.push(s);
            }
        }
    }

    pub fn update(&mut self, dt: f64) {
        self.x = 0.0;
        self.z = 0.0;
        self.rot = [0.0; 3];
        for s in &mut self.active {
            s.update(dt);
            self.x += s.x;
            self.z += s.z;
            for k in 0..3 {
                self.rot[k] += s.rot[k];
            }
        }
        self.active.retain(|s| !s.finished);
    }

    /// Whether a playing shake moves the camera along its forward X / up Z from a random initial offset.
    pub fn random_axes(&self) -> (bool, bool) {
        let on = |o: Osc| o.amp != 0.0 && !o.zero;
        let live = || self.active.iter().filter(|s| !s.finished);
        (live().any(|s| on(s.d.x)), live().any(|s| on(s.d.z)))
    }

    pub fn clear(&mut self) {
        self.active.clear();
        self.x = 0.0;
        self.z = 0.0;
        self.rot = [0.0; 3];
    }
}

/// FRotationMatrix of a rotator (pitch, yaw, roll, degrees): its X (forward), Y (right) and Z (up) axes in UE's world.
pub fn rotation_axes(rot: [f64; 3]) -> [[f64; 3]; 3] {
    let [(sp, cp), (sy, cy), (sr, cr)] = rot.map(|d| (dsin(d), dcos(d)));
    [
        [cp * cy, cp * sy, sp],
        [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp],
        [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp],
    ]
}

/// The axes of `base` turned by `delta` in its own space (ApplyPlaySpace CameraLocal: RotationMatrix(delta) x
/// RotationMatrix(base)).
pub fn turned_axes(base: [f64; 3], delta: [f64; 3]) -> [[f64; 3]; 3] {
    let (b, d) = (rotation_axes(base), rotation_axes(delta));
    d.map(|r| std::array::from_fn(|k| r[0] * b[0][k] + r[1] * b[1][k] + r[2] * b[2][k]))
}

/// UE's TranslatedWorldToView of a lens with the axes `axes` (`rotation_axes`): its columns are the view's right,
/// up and forward, as `look_at`'s.
pub fn view_of_axes(axes: [[f64; 3]; 3]) -> M4 {
    let cols = [axes[1], axes[2], axes[0]];
    let mut m = [[0.0; 4]; 4];
    for (j, col) in cols.iter().enumerate() {
        for k in 0..3 {
            m[k][j] = col[k];
        }
    }
    m[3][3] = 1.0;
    m
}

// ---- the camera rig -----------------------------------------------------------------------------

/// The point of view as placed (UE units and axes): the lens and its right / up / forward axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pov {
    pub pos: [f64; 3],
    pub right: [f64; 3],
    pub up: [f64; 3],
    pub front: [f64; 3],
}

/// The visible rectangle on the blast box's plane.
struct Frustum {
    tl_y: f64,
    tl_z: f64,
    br_y: f64,
    br_z: f64,
    w: f64,
    h: f64,
}

/// PFGCamera (Panda_Main/Blueprints/PFGCamera), camera mode 0, and its CameraCenter.
#[derive(Clone, Debug)]
pub struct CameraRig {
    /// CameraCenterPoint's location (UE), and the last tick's.
    c: [f64; 3],
    p: [f64; 3],
    fov: f64,
    aspect: f64,
    limiter: f64,
    count: usize,
    min_y: f64,
    min_z: f64,
    max_y: f64,
    max_z: f64,
    pub shaker: CameraShaker,
    /// The point of view as placed by `place`.
    pub pov: Pov,
    /// The PFGCamera actor's root as placed: the lens without the shake (which moves the view, not the actor); what is
    /// attached to the camera actor's root follows it.
    pub root: [f64; 3],
    /// The CameraCenterPoint's location (UE) as the frame draws it (between the last tick's and the latest), its
    /// placed one until then: what follows the camera's centre reads it (Comp_Parallax).
    pub center: [f64; 3],
    eased: bool,
    /// The camera's defaults (the stage's PFGCamera values), each with the CDO's when the stage has none.
    arm_default: f64,
    arm_min: f64,
    arm_max: f64,
    edge_h: f64,
    edge_top: f64,
    edge_bottom: f64,
    camera_fov: f64,
    ease_lerp: f64,
    low_ease: f64,
    high_ease: f64,
    zoom_out_exp: f64,
    zoom_in_exp: f64,
    low_linear: f64,
    high_linear: f64,
    tl: [f64; 2],
    br: [f64; 2],
    /// ClampCameraMoveToBlastBox's corners (the blast box's current ones) and their insets from its bounds
    /// (horizontal, top, bottom).
    ctl: [f64; 2],
    cbr: [f64; 2],
    inset: [f64; 3],
    /// CameraCenter OriginalCameraAngle, ShrunkCameraAngle and CameraAngle.
    angle0: f64,
    angle1: f64,
    angle: f64,
    /// CameraCenterPoint's pitch: the eased transform's rotation (the target's is CameraAngle), and last tick's.
    rot: f64,
    prot: f64,
    /// AABB_C adds a fighter at the Mesh location + (BPI_GetCapsuleCollision) CapsuleHalfHeight; while
    /// CharacterMovement IsCrouching + (CapsuleHalfHeight - CapsuleRadius), so the crouch drops it by CapsuleRadius.
    point_z: f64,
    crouch_drop: f64,
}

impl CameraRig {
    /// The rig of `stage`'s camera; `point_z` and `crouch_drop` are BaseFighter's Capsule half height and radius
    /// (no fighter class overrides them).
    pub fn new(stage: &StageData, point_z: f64, crouch_drop: f64) -> CameraRig {
        let n = |k: &str, d: f64| match stage.camera.get(k) {
            Some(NumOrBool::Num(v)) => *v,
            _ => d,
        };
        let eased = !matches!(stage.camera.get("UseEasedMovement"), Some(NumOrBool::Bool(false)));
        let cc = [stage.camera_center[0], stage.camera_center[1], stage.camera_center[2]];
        let corners = &stage.blast_box_corners;
        let tl = [corners.top_left[0], corners.top_left[1]];
        let br = [corners.bottom_right[0], corners.bottom_right[1]];
        let bb = stage.blast_boxes.get("CollisionComp").unwrap_or_else(|| panic!("the stage has no CollisionComp"));
        let angle0 = n("CameraAngle", 5.0);
        CameraRig {
            c: cc,
            p: cc,
            fov: UE_DEFAULT_FOV,
            aspect: 16.0 / 9.0,
            limiter: 0.0,
            count: 0,
            min_y: 0.0,
            min_z: 0.0,
            max_y: 0.0,
            max_z: 0.0,
            shaker: CameraShaker::default(),
            pov: Pov { pos: [0.0; 3], right: [0.0, -1.0, 0.0], up: [0.0, 0.0, 1.0], front: [-1.0, 0.0, 0.0] },
            root: [0.0; 3],
            center: cc,
            eased,
            arm_default: n("DefaultCameraArmLength", 7000.0),
            arm_min: n("CameraArmMin", 6000.0),
            arm_max: n("CameraArmMax", 99999.0),
            edge_h: n("HorizontalEdgeBuffer", 375.0),
            edge_top: n("TopEdgeBuffer", 325.0),
            edge_bottom: n("BottomEdgeBuffer", 300.0),
            camera_fov: n("CameraFOV", 26.0),
            ease_lerp: n("EaseLerpValue", 1000.0),
            low_ease: n("LowAlphaEase", 0.006),
            high_ease: n("HighAlphaEase", 0.016),
            zoom_out_exp: n("ZoomOutEasingExponent", 4.1),
            zoom_in_exp: n("ZoomInEasingExponent", 1.8),
            low_linear: n("LowAlphaLinear", 0.05),
            high_linear: n("HighAlphaLinear", 0.1),
            tl,
            br,
            ctl: tl,
            cbr: br,
            inset: [bb.y[1] - tl[0], bb.z[1] - tl[1], br[1] - bb.z[0]],
            angle0,
            angle1: stage.camera_shrunk_angle.unwrap_or(angle0),
            angle: angle0,
            rot: angle0,
            prot: angle0,
            point_z,
            crouch_drop,
        }
    }

    /// `a`: BlastBoxPlayerExit.ShrinkPercent (World's blast_shrink); `blast`: its CollisionComp bounds as they are now,
    /// in sim axes. Called before `tick`. The two timelines start in the same ShrinkBlastBox call with the same length
    /// and rate, so the CameraCenter's Alpha is taken as the blast box's.
    // Unverified: which of the two timeline components and the PFGCamera ticks first is not read (the camera may see
    // the last frame's angle or corners).
    pub fn set_blast_shrink(&mut self, a: f64, blast: &mvs_sim::world::BlastBounds) {
        self.angle = self.angle0 + (self.angle1 - self.angle0) * a;
        self.ctl[0] = -f64::from(blast.left) - self.inset[0];
        self.ctl[1] = f64::from(blast.top) - self.inset[1];
        self.cbr[0] = -f64::from(blast.right) + self.inset[0];
        self.cbr[1] = f64::from(blast.bottom) + self.inset[2];
    }

    /// SetCameraMovement(Eased) (ubergraph 18835): UseEasedMovement, read only by the SelectTransform between the eased
    /// and the linear step. The game mode's IntroSequence turns it off for its first 3.0 s.
    pub fn set_camera_movement(&mut self, eased: bool) {
        self.eased = eased;
    }

    pub fn resize(&mut self, aspect: f64) {
        self.aspect = aspect;
    }

    /// One Blueprint tick, in lockstep with the 60 Hz sim after the fighters step (the PFGCamera actor ticks after
    /// both fighters' meshes). `targets`: RegisteredOnscreenActors in its order; `spheres`: the registered sphere
    /// components' world locations (sim units).
    pub fn tick(&mut self, targets: &[CameraTarget], spheres: &[[f64; 2]]) {
        self.p = self.c;
        self.prot = self.rot;
        self.step(targets, spheres);
        // PlayerCameraManager: the modifiers after the view target
        self.shaker.update(TICK);
    }

    fn step(&mut self, targets: &[CameraTarget], spheres: &[[f64; 2]]) {
        if self.limiter <= 0.0 {
            self.count = targets.len();
            self.min_y = f64::INFINITY;
            self.min_z = f64::INFINITY;
            self.max_y = f64::NEG_INFINITY;
            self.max_z = f64::NEG_INFINITY;
            for (i, t) in targets.iter().enumerate() {
                let y = -(t.x + t.facing * t.mesh_x);
                let z = t.y
                    + t.mesh_z
                    + if t.actor { 0.0 } else { self.point_z - if t.crouched { self.crouch_drop } else { 0.0 } };
                // the first point is the box; a fighter is never buffered
                let b = if t.actor && i > 0 { BUFFER } else { 0.0 };
                self.min_y = self.min_y.min(y + b);
                self.max_y = self.max_y.max(y - b);
                self.min_z = self.min_z.min(z + b);
                self.max_z = self.max_z.max(z - b);
            }
            // Unverified: with no visible actor the build keeps the old box and never reaches the components.
            if !targets.is_empty() {
                for s in spheres {
                    self.min_y = self.min_y.min(-s[0] + BUFFER);
                    self.max_y = self.max_y.max(-s[0] - BUFFER);
                    self.min_z = self.min_z.min(s[1] + BUFFER);
                    self.max_z = self.max_z.max(s[1] - BUFFER);
                }
            }
            self.limiter = 0.1;
        } else {
            self.limiter = f64::from(self.limiter as f32 - TICK as f32);
        }

        let angle = self.angle;
        let mut arm = self.arm_default;
        let (mut mid_y, mut mid_z) = (self.c[1], self.c[2]);
        if self.count >= 1 {
            let top_y = (self.max_y + self.edge_h).min(self.tl[0]);
            let top_z = (self.max_z + self.edge_top).min(self.tl[1]);
            let bot_y = (self.min_y - self.edge_h).max(self.br[0]);
            let bot_z = (self.min_z - self.edge_bottom).max(self.br[1]);
            let tan = dtan(self.fov / 2.0);
            let by_w = ((top_y - bot_y) / 2.0 + PAD) / tan;
            let by_h = ((top_z - bot_z) / 2.0 + PAD) * self.aspect / tan;
            arm = by_w.max(by_h).max(self.arm_min).min(self.arm_max);
            mid_y = (top_y + bot_y) / 2.0;
            mid_z = (top_z + bot_z) / 2.0;
        }
        self.fov = (self.aspect / ASPECT_REF) * self.camera_fov;
        if self.count < 1 {
            return; // only the rotation is applied
        }

        let (tx, ty, tz) = (arm * dcos(angle), mid_y, mid_z + arm * dsin(angle));
        let [kx, ky, kz] = self.clamp_to_blast_box(tx, ty, tz, angle);
        let [cx, cy, cz] = self.c;
        let dist = ((tx - cx) * (tx - cx) + (ty - cy) * (ty - cy) + (tz - cz) * (tz - cz)).sqrt();
        let a = (dist / self.ease_lerp).clamp(0.0, 1.0);
        let t = if self.eased {
            let alpha = self.low_ease + (self.high_ease - self.low_ease) * a;
            let exp = if cx < tx { self.zoom_out_exp } else { self.zoom_in_exp };
            // TEase EaseOut = FMath::InterpEaseOut
            1.0 - (1.0 - alpha).powf(exp)
        } else {
            self.low_linear + (self.high_linear - self.low_linear) * a
        };
        self.c = [cx + (kx - cx) * t, cy + (ky - cy) * t, cz + (kz - cz) * t];
        // TEase / TLerp of the transform: the rotation is the normalised lerp of the two quaternions (a pitch alone
        // here). Only the shrink moves the target's pitch.
        if self.rot != angle {
            let (h0, h1) = (self.rot * std::f64::consts::PI / 360.0, angle * std::f64::consts::PI / 360.0);
            let s = h0.sin() + (h1.sin() - h0.sin()) * t;
            let c = h0.cos() + (h1.cos() - h0.cos()) * t;
            self.rot = s.atan2(c) * 360.0 / std::f64::consts::PI;
        }
    }

    /// The visible rectangle on the blast box's plane for a camera at depth `x` (the vertical half angle taken as
    /// FOV / aspect / 2).
    fn frustum(&self, x: f64, y: f64, z: f64, angle: f64) -> Frustum {
        let vh = self.fov / self.aspect / 2.0;
        let up = dtan(vh - angle) * x;
        let down = -dtan(vh + angle) * x;
        let left = dtan(self.fov / 2.0) * x;
        Frustum { tl_y: y + left, tl_z: z + up, br_y: y - left, br_z: z + down, w: 2.0 * left, h: up - down }
    }

    /// ClampCameraMoveToBlastBox.
    fn clamp_to_blast_box(&self, mut x: f64, mut y: f64, mut z: f64, angle: f64) -> [f64; 3] {
        let ([tl_y, tl_z], [br_y, br_z]) = (self.ctl, self.cbr);
        let (bb_w, bb_h) = (tl_y - br_y, tl_z - br_z);
        let mut fr = self.frustum(x, y, z, angle);
        let (wb, hb) = (fr.w - bb_w, fr.h - bb_h);
        if wb > 0.0 && hb > 0.0 {
            let vh = self.fov / self.aspect / 2.0;
            x = (bb_w / 2.0 / dtan(self.fov / 2.0)).max(bb_h / (dtan(vh - angle) + dtan(vh + angle)));
            fr = self.frustum(x, y, z, angle);
        }
        let (top, left, bottom, right) = (tl_z < fr.tl_z, tl_y < fr.tl_y, br_z > fr.br_z, br_y > fr.br_y);
        if top && hb <= 0.0 {
            z -= fr.tl_z - tl_z;
        } else if bottom && hb <= 0.0 {
            z += br_z - fr.br_z;
        } else if hb > 0.0 {
            z += hb / 2.0 - (fr.tl_z - tl_z);
        }
        fr = self.frustum(x, y, z, angle);
        if left && wb <= 0.0 {
            y -= fr.tl_y - tl_y;
        } else if right && wb <= 0.0 {
            y += br_y - fr.br_y;
        } else if wb > 0.0 {
            y = 0.0;
        }
        [x, y, z]
    }

    /// The frame's camera at `a` between the last tick (0) and the latest (1): the lens = centre + pitch(A) x (0, 0,
    /// 100), the view direction = pitch(A) x yaw(180) x forward = (-cos A, 0, -sin A), the shake in camera space
    /// (X forward, Z up = (-sin A, 0, cos A)) held for the whole tick; then `wind` (`wind_cam`) pushes the lens along UE
    /// Y and turns it about UE Z, the actor's root with it.
    pub fn place(&mut self, a: f64, wind: Lean) -> Camera {
        let angle = self.prot + (self.rot - self.prot) * a;
        let c = [0, 1, 2].map(|i| self.p[i] + (self.c[i] - self.p[i]) * a);
        self.center = c;
        let (dx, dz) = (-dcos(angle), -dsin(angle));
        let (sx, sz) = (self.shaker.x, self.shaker.z);
        let pos = [
            c[0] - LENS_UP * dsin(angle) + sx * dx - sz * dsin(angle),
            c[1] + wind.pan,
            c[2] + LENS_UP * dcos(angle) + sx * dz + sz * dcos(angle),
        ];
        self.root = [c[0] - LENS_UP * dsin(angle), c[1] + wind.pan, c[2] + LENS_UP * dcos(angle)];
        // the turn about UE Z: (x, y) -> (x cos - y sin, x sin + y cos)
        let (ws, wc) = (dsin(wind.yaw), dcos(wind.yaw));
        let turn = |v: [f64; 3]| [v[0] * wc - v[1] * ws, v[0] * ws + v[1] * wc, v[2]];
        let (right, up, front) = (turn([0.0, -1.0, 0.0]), turn([-dsin(angle), 0.0, dcos(angle)]), turn([dx, 0.0, dz]));
        self.pov = Pov { pos, right, up, front };
        Camera {
            origin: pos,
            translated_world_to_view: look_at(pos, [0, 1, 2].map(|i| pos[i] + front[i] * 1000.0)),
            view_to_clip: self.projection(),
        }
    }

    /// The vertical field of view the render camera takes (degrees) and its reversed-depth projection, as
    /// UE's ViewToClip of a view in cm.
    fn projection(&self) -> M4 {
        let vfov = 2.0 * (dtan(self.fov / 2.0) / self.aspect).atan() * 180.0 / std::f64::consts::PI;
        perspective(vfov, self.aspect, NEAR, FAR)
    }
}

/// UE's ViewToClip of a view in cm for a render camera of vertical field of view `vfov` (degrees), `aspect`
/// (width / height) and its near and far planes in the view's metres: three.js PerspectiveCamera.updateProjectionMatrix
/// and Matrix4.makePerspective with reversed depth.
pub fn perspective(vfov: f64, aspect: f64, near: f64, far: f64) -> M4 {
    let top = near * (std::f64::consts::PI / 180.0 * 0.5 * vfov).tan();
    let height = 2.0 * top;
    let width = aspect * height;
    let left = -0.5 * width;
    let x = 2.0 * near / ((left + width) - left);
    let y = 2.0 * near / (top - (top - height));
    let c = near / (far - near);
    let d = (far * near) / (far - near);
    // the clip of a view point (x, y, z forward) in cm: the render camera's of (x, y, -z) / 100, times 100
    [[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, -c, 1.0], [0.0, 0.0, 100.0 * d, 0.0]]
}

/// UE's TranslatedWorldToView of a lens at `eye` looking at `target` with the world's Z up (three.js Matrix4.lookAt in
/// the view's space): its columns are the view's right, up and forward in UE axes.
pub fn look_at(eye: [f64; 3], target: [f64; 3]) -> M4 {
    // the view's space: (x, y, z) = UE (X, Z, Y) x 0.01
    let page = |v: [f64; 3]| [v[0] / 100.0, v[2] / 100.0, v[1] / 100.0];
    let (e, t) = (page(eye), page(target));
    let norm = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let cross =
        |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let z = norm([e[0] - t[0], e[1] - t[1], e[2] - t[2]]);
    let x = norm(cross([0.0, 1.0, 0.0], z));
    let y = cross(z, x);
    // UE's view axes against the render camera's (looking down -Z): right x, up y, forward -z; each back in UE's axes
    let ue = |v: [f64; 3], k: f64| [v[0] * k, v[2] * k, v[1] * k];
    let cols = [ue(x, 1.0), ue(y, 1.0), ue(z, -1.0)];
    let mut m = [[0.0; 4]; 4];
    for (j, col) in cols.iter().enumerate() {
        for k in 0..3 {
            m[k][j] = col[k];
        }
    }
    m[3][3] = 1.0;
    m
}

// ---- the camera feed -------------------------------------------------------------------------------

/// What is on PFGCamera's RegisteredOnscreenActors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Onscreen {
    Fighter(FighterId),
    Marker(MarkerId),
    /// A marker's C019MarkerCameraFocus.
    Focus(MarkerId),
    /// A spawned world actor on the camera.
    Actor(ActorId),
    Anchor(u64),
}

/// A RingOutVfxAnchor: a Comp_Actor_KeepOnCamera actor at a ring-out effect's place.
#[derive(Clone, Copy, Debug)]
struct KoAnchor {
    id: u64,
    target: CameraTarget,
    ticks: i64,
}

/// (the port's own, dev) The 3D feel test's plane for the camera (`mvs_sim::world::depth3d`): its angle at the two
/// last steps (radians), O's sim x and the fighters' mean depth at the two last steps (cm).
#[derive(Clone, Copy, Debug)]
struct DepthCam {
    prev_theta: f64,
    theta: f64,
    origin: f64,
    prev_d: f64,
    d: f64,
}

/// The match's camera: the rig, what it follows each tick from the World (PFGCamera's onscreen actors and the pawns'
/// tracker spheres, in their registration order) and the step's camera events.
#[derive(Clone, Debug)]
pub struct MatchCamera {
    pub rig: CameraRig,
    /// (the port's own, dev) The free camera that replaces the drawn view; None: the game's.
    pub free: Option<crate::free_cam::FreeCam>,
    /// The rig's point of view of the last frame, before the free camera replaced it.
    pub game_pov: Pov,
    /// Wind's hold on the lens (the port's own rule).
    wind: WindCam,
    /// (the port's own, dev) The 3D feel test's plane as the last step left it; None: off.
    depth: Option<DepthCam>,
    /// RegisteredOnscreenActors in its order, with each one's point of the tick.
    order: IndexMap<Onscreen, CameraTarget>,
    anchors: Vec<KoAnchor>,
    next_anchor: u64,
    /// World blast_shrink_tick as the camera last saw it.
    shrink_shown: i32,
    /// The feed's buffers, kept from tick to tick: the tick's spheres, its onscreen actors and `order`'s targets.
    spheres: Vec<[f64; 2]>,
    now: IndexMap<Onscreen, CameraTarget>,
    ordered: Vec<CameraTarget>,
}

impl MatchCamera {
    /// The camera of `world`'s stage drawn at `aspect` (width / height).
    pub fn new(world: &World, aspect: f64) -> MatchCamera {
        use mvs_sim::consts::{BF_CAPSULE_HALF, BF_CAPSULE_RADIUS};
        let mut rig = CameraRig::new(&world.data.stage, f64::from(BF_CAPSULE_HALF), f64::from(BF_CAPSULE_RADIUS));
        rig.resize(aspect);
        MatchCamera {
            game_pov: rig.pov,
            free: None,
            rig,
            wind: WindCam::default(),
            depth: None,
            order: IndexMap::new(),
            anchors: Vec::new(),
            next_anchor: 0,
            shrink_shown: -1,
            spheres: Vec::new(),
            now: IndexMap::new(),
            ordered: Vec::new(),
        }
    }

    /// A ring-out effect's RingOutVfxAnchor at its place (UE Y, Z), destroyed by a timer after `hold` s (the sim's
    /// max(CameraHoldTime, 0.01)). A timer set before the manager's tick is pending and starts counting after that
    /// tick, so it fires in the timer tick of the frame hold x 60 after the ring-out, after that frame's camera tick:
    /// the camera sees the anchor on the KO tick and the hold x 60 ticks after it. Called before `step` of the tick.
    pub fn ko_anchor(&mut self, y: f64, z: f64, hold: f64) {
        let ticks = 1 + (hold * 60.0).round() as i64;
        let target = CameraTarget { x: -y, y: z, actor: true, ..CameraTarget::default() };
        self.anchors.push(KoAnchor { id: self.next_anchor, target, ticks });
        self.next_anchor += 1;
    }

    /// One presentation tick, after each World step and before its events are drained: the step's camera events
    /// (shakes, the eased movement), then `feedCamera` (presentTick's last part) and the rig's tick.
    pub fn step(&mut self, w: &World) {
        for e in &w.events {
            match e {
                Event::CameraShake { cls } => self.rig.shaker.play(w.names.str(*cls)),
                Event::CameraMovement { on } => self.rig.set_camera_movement(*on),
                _ => {}
            }
        }
        self.feed(w);
        self.wind.step(w);
        self.depth = w.depth3d.as_ref().map(|s| {
            let ds: Vec<f64> = w.players.iter().map(|&f| f64::from(w.fighter(f).body.d)).collect();
            let d = if ds.is_empty() { 0.0 } else { ds.iter().sum::<f64>() / ds.len() as f64 };
            DepthCam {
                prev_theta: f64::from(s.prev_theta),
                theta: f64::from(s.theta),
                origin: f64::from(s.origin),
                prev_d: self.depth.map_or(d, |c| c.d),
                d,
            }
        });
    }

    /// The Lab's reset: the shakes end and the wind lets go.
    pub fn reset(&mut self) {
        self.rig.shaker.clear();
        self.wind.reset();
    }

    /// The frame's camera at `alpha` between the last tick (0) and the latest (1).
    pub fn camera(&mut self, alpha: f32) -> Camera {
        let mut game = self.rig.place(f64::from(alpha), self.wind.at(f64::from(alpha)));
        if let Some(c) = &self.depth {
            // (the port's own 3D feel test: the rig's view turned about O with the plane and moved out along its
            // normal by the fighters' mean depth)
            let a = f64::from(alpha);
            let pi = std::f64::consts::PI;
            let step = (c.theta - c.prev_theta + pi).rem_euclid(2.0 * pi) - pi;
            let t = crate::depth_view::Turn::of(
                c.prev_theta + step * a,
                c.origin,
                c.origin,
                c.prev_d + (c.d - c.prev_d) * a,
            );
            let pov = t.pov(&self.rig.pov);
            self.rig.pov = pov;
            self.rig.root = t.point(self.rig.root);
            let target = [0, 1, 2].map(|k| pov.pos[k] + pov.front[k] * 1000.0);
            game = Camera { origin: pov.pos, translated_world_to_view: look_at(pov.pos, target), ..game };
        }
        self.game_pov = self.rig.pov;
        let Some(free) = &self.free else { return game };
        // the poser and the listener read the rig's pov and root: they follow the drawn view
        let (camera, pov) = free.camera(&self.game_pov, &game);
        self.rig.pov = pov;
        self.rig.root = pov.pos;
        camera
    }

    fn feed(&mut self, w: &World) {
        // BPI_ShrinkBlastBox (FFAGameMode ShrinkBlastBox, Mutator_ShrinkBlastBoxLastPoint): the CameraCenter's angle
        // and the camera clamp's corners follow the blast box; a reset world (blast_shrink_tick back at -1) gives the
        // rig its unshrunk box once more.
        if w.flow.blast_shrink_tick >= 0 || self.shrink_shown >= 0 {
            self.rig.set_blast_shrink(f64::from(w.flow.blast_shrink), &w.stage.blast);
        }
        self.shrink_shown = w.flow.blast_shrink_tick;
        // AABB Add keeps dead fighters (frozen by DisableMovement): in a 1v1 RemoveCameraFocus never unregisters them
        // (it needs more than 2 registered actors), so the camera holds on the KO point until the respawn; with more
        // fighters the dead one leaves 0.5 s after the ring-out and is back at its respawn. Before a fighter's intro
        // and during the other side's it is not registered (the sim's on_camera).
        // A pawn's tracker sphere (CharRender.camera_tracker, Pawn_C017's CameraTracker) stays on the camera's
        // component list from BeginPlay to EndPlay, whatever its Comp_Actor_KeepOnCamera does: the capsule's location
        // + the sphere's RelativeLocation.
        let spheres = &mut self.spheres;
        let now = &mut self.now;
        spheres.clear();
        now.clear();
        // (the fighters in the World's order: a minion is never on the camera, its on_camera false)
        // Unverified: a destroyed mode pawn leaves the feed with its record; PFGCamera's registration of it is native
        // and not read.
        for f in w.all_ids() {
            let fi = w.fighter(f);
            if let Some(z) = tracker_z(w, f) {
                spheres.push([f64::from(fi.body.x), f64::from(fi.body.y) + z]);
            }
            if !fi.vis.on_camera || fi.capsule.capsule_off {
                continue;
            }
            let class_z = w.data.chars[w.names.str(fi.ch)].boxes.mesh_z;
            let t = CameraTarget {
                x: f64::from(fi.body.x),
                y: f64::from(fi.body.y),
                actor: false,
                crouched: fi.capsule.cmc_crouched,
                facing: f64::from(fi.body.facing_dir),
                mesh_x: f64::from(fi.hit.mesh_x),
                mesh_z: class_z + f64::from(fi.hit.mesh_z),
            };
            now.insert(Onscreen::Fighter(f), t);
        }
        // C019Marker StartMoveAlongPath unregisters its owner until the timeline ends; the marker itself
        // (Comp_Actor_KeepOnCamera, KeepOnCamera? true) and its C019MarkerCameraFocus actor are onscreen actors. The
        // focus actor is spawned by StartMoveAlongPath (@5919) and destroyed when the timeline ends (@7731), before the
        // owner registers again (@7839): the owner is then the array's last.
        let actor = |x: f64, y: f64| CameraTarget { x, y, actor: true, ..CameraTarget::default() };
        for (id, m) in w.actors.markers.iter() {
            if m.dead {
                continue;
            }
            now.insert(Onscreen::Marker(id), actor(f64::from(m.x), f64::from(m.y)));
            if m.moving {
                now.insert(Onscreen::Focus(id), actor(f64::from(m.focus_x), f64::from(m.focus_y)));
            }
        }
        // A spawned actor with its Comp_Actor_KeepOnCamera registered (SimActor.onCamera): Jerry's actor
        // and the UFO always, the BMO by its flag, Rick's portal aimer while LocallyControlled and alive;
        // its place.
        for (id, a) in w.actors.actors.iter() {
            let at = match &a.state {
                WorldActorState::JerryActor(s) => Some((s.x, s.y)),
                WorldActorState::Ufo(s) => Some((s.x, s.y)),
                WorldActorState::BmoActor(s) if s.on_camera => Some((s.x, s.y)),
                WorldActorState::PortalAimer(s) if s.locally_controlled && !a.dead => Some((s.x, s.y)),
                _ => None,
            };
            if let Some((x, y)) = at {
                now.insert(Onscreen::Actor(id), actor(f64::from(x), f64::from(y)));
            }
        }
        for k in &self.anchors {
            now.insert(Onscreen::Anchor(k.id), k.target);
        }
        // Those still registered keep their place, the new ones follow in the order above.
        // Unverified: the order of two registered in one tick is the port's.
        self.order.retain(|k, _| now.contains_key(k));
        for (k, t) in now.drain(..) {
            self.order.insert(k, t);
        }
        self.ordered.clear();
        self.ordered.extend(self.order.values().copied());
        self.rig.tick(&self.ordered, &self.spheres);
        self.anchors.retain_mut(|k| {
            k.ticks -= 1;
            k.ticks > 0
        });
    }
}

/// `cameraTracker`: the Z of fighter `f`'s tracker component's RelativeLocation (None: its
/// character has no tracker).
// Deviation: a tracker the data lacks (a broken import) is not an error: it warns once and the fighter has
// no sphere, so the match goes on.
fn tracker_z(w: &World, f: FighterId) -> Option<f64> {
    let ch = w.names.str(w.fighter(f).ch);
    let name = crate::chars::render_of(ch).camera_tracker?;
    let z = (|| {
        let c = w.char_data(f).fighter.components.pawn.get(name)?;
        c.p.as_obj()?.get("RelativeLocation")?.as_obj()?.get("Z")?.as_f64()
    })();
    if z.is_none() {
        static W: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let mut seen = W.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let what = format!("{ch}: {name}");
        if !seen.contains(&what) {
            eprintln!("warning: {ch}'s fighter data has no camera tracker {name} with a RelativeLocation.Z: no sphere");
            seen.push(what);
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crt_rand_is_msvc_rand() {
        // MSVC rand() after srand(1): 41, 18467, 6334
        let mut r = CrtRand(1);
        assert_eq!([r.next(), r.next(), r.next()], [41, 18467, 6334]);
    }

    #[test]
    fn a_shake_ends_after_its_duration() {
        let mut s = CameraShaker::default();
        s.play("CameraShakeLight_C");
        let mut ticks = 0;
        while !s.active.is_empty() {
            s.update(TICK);
            ticks += 1;
        }
        // 0.075 s: the fifth update reaches 0
        assert_eq!(ticks, 5);
        assert_eq!((s.x, s.z), (0.0, 0.0));
    }

    /// PM_CameraShake turns the camera only, within its amplitudes at the play's scale, and ends after 1.65 s.
    #[test]
    fn the_pre_match_shake_turns() {
        let mut s = CameraShaker::default();
        s.play_scaled("PM_CameraShake_C", 0.5);
        let (mut ticks, mut most) = (0, [0.0f64; 3]);
        while !s.active.is_empty() {
            s.update(TICK);
            assert_eq!((s.x, s.z), (0.0, 0.0));
            for (m, r) in most.iter_mut().zip(s.rot) {
                *m = m.max(r.abs());
            }
            ticks += 1;
        }
        // 1.65 s: 99 sixtieths leave a rounding's remainder, the 100th update reaches 0
        assert_eq!(ticks, 100);
        assert!(most[0] > 0.1 && most[0] <= 0.125 && most[1] <= 0.125 && most[2] > 0.2 && most[2] <= 0.25);
        assert_eq!(s.rot, [0.0; 3]);
    }

    /// An unturned rotator's view is look_at's along its forward vector.
    #[test]
    fn axes_view_is_look_at() {
        let rot = [-12.0, 75.0, 0.0];
        let a = turned_axes(rot, [0.0; 3]);
        let l = look_at([0.0; 3], a[0].map(|v| v * 100.0));
        let v = view_of_axes(a);
        for r in 0..4 {
            for c in 0..4 {
                assert!((l[r][c] - v[r][c]).abs() < 1e-9, "{r} {c}: {} {}", l[r][c], v[r][c]);
            }
        }
    }

    #[test]
    fn look_at_down_the_ue_minus_x() {
        let m = look_at([1000.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        // right = -Y, up = Z, forward = -X
        assert_eq!([m[0][0], m[1][0], m[2][0]], [0.0, -1.0, 0.0]);
        assert_eq!([m[0][1], m[1][1], m[2][1]], [0.0, 0.0, 1.0]);
        assert_eq!([m[0][2], m[1][2], m[2][2]], [-1.0, 0.0, 0.0]);
    }
}
