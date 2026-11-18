//! A fighter's view: the crossfades, the Slot node's mix
//! over a montage blending out, the hit turn, the additive and layered nodes, the head scale, the draw between two
//! ticks, the bones' component-space walk, and the props hung on its bones. The view's space (metres, Y up) as the
//! view keeps it; the poser turns the result into UE's.

use mvs_sim::names::Name;

use crate::anim::{Clips, HeadScaleStep, Pose, Rig, apply_additive, blend_pose, sample_clip};
use crate::math::{self, Mat4};
use crate::mesh::MeshInfo;

// Base_AnimBP's hit rotation: the bone FAnimNode_ModifyBone replaces the rotation of.
const HIT_TURN_BONE: &str = "fml_un_COM";

// A kept bone K under a mirrored parent P is, in component space (row vectors), K * F_P * T(P) * M_Y: its own local
// transform on the mirrored parent. P is Root or fml_un_COM here, both self-partnered with BoneFlipAxis Y (and
// SkelMirrorAxis Y).
// Arya_MirrorTable (422 bones: None on 396): fml_un_C_pelvis_att, fml_un_C_arya_face_att and every bone below them,
// and on no other; no mirrored bone sits under a kept one.
const KEPT_ROOTS: [&str; 4] = ["fml_un_C_pelvis_att", "fml_un_C_Face_att", "TridentAtt", "fml_un_C_arya_face_att"];
// C015_MirrorTable (Taz): BoneFlipAxis None on fml_un_C_pelvis_att, fml_un_C_face_att, fml_un_C_faceLower_att and
// every bone below them, the three under fml_un_COM. Taken only where no bone above is kept already: C020's
// fml_un_C_face_att hangs under its fml_un_C_pelvis_att, a kept root of its own.
const KEPT_ROOTS_FREE: [&str; 2] = ["fml_un_C_face_att", "fml_un_C_faceLower_att"];

/// BoneFlipAxis as a scale in a bone's own axes (three (x, y, z) = UE (X, Z, Y)); None: the axis is None (kept).
pub fn flip_scale(axis: &str) -> Option<[f64; 3]> {
    match axis {
        "X" => Some([-1.0, 1.0, 1.0]),
        "Y" => Some([1.0, 1.0, -1.0]),
        "Z" => Some([1.0, -1.0, 1.0]),
        "XYZ" => Some([-1.0, -1.0, -1.0]),
        "None" => None,
        _ => panic!("prop mirror: BoneFlipAxis {axis}"),
    }
}

/// The crossfade's switch key: a montage's clip and montage (`clip.name|montage`), else
/// the sim's animKey (`'m' + anim`: the dash montage's clip; `'g' + locoGen`: the state machines' entry; '' none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKey {
    Empty,
    Clip(usize, Name),
    Anim(Name),
    Gen(i32),
}

/// pose()'s `loco`: the state machines' crossfade time and curve, and the switch key.
#[derive(Clone, Copy, Debug)]
pub struct Loco {
    pub dur: f64,
    pub hermite: bool,
    pub key: ViewKey,
}

/// A slot mix entry (pose()'s `mix`): a clip of the view's clips (None: not loaded), its time and weight.
#[derive(Clone, Copy, Debug)]
pub struct MixArg {
    pub clip: Option<usize>,
    pub time: f64,
    pub w: f64,
}

/// A static mesh component on a skeleton socket: its bone, the socket and the component's
/// relative transforms (right facing; and the left attach under its flip group), shown while `on`.
#[derive(Debug)]
pub struct Prop {
    pub tag: &'static str,
    pub bone: usize,
    pub right: Mat4,
    pub left: Option<Mat4>,
    pub on: bool,
}

/// The left attach of a prop: (socketLoc, socketRot, loc, rot, flip): flip is the BoneFlipAxis scale of the bone the
/// build attaches to facing left (the left socket's bone on the mirrored pose).
pub type LeftAttach = ([f64; 3], [f64; 3], [f64; 3], [f64; 3], [f64; 3]);

/// `PropAttach`, UE units.
#[derive(Clone, Copy, Debug)]
pub struct PropAttach<'a> {
    pub bone: &'a str,
    pub socket_loc: [f64; 3],
    pub socket_rot: Option<[f64; 3]>,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
    pub abs_scale: bool,
    /// (socketLoc, socketRot, loc, rot, flip) of the left attach; its bone is not read (the flip group hangs on
    /// `bone`, the left bone's mirror partner).
    pub left: Option<LeftAttach>,
}

/// The hold layer: a single-frame pose on a bone subtree, faded by a bool.
#[derive(Debug)]
struct Layer {
    pose: Pose,
    idx: Vec<usize>,
    time: f64,
}

/// A character AnimBP's own nodes in the view's pose (`sampleRule`, `takesLayer`, `mixRule` and
/// `poseRule`), called by [`CharView::pose`] with the view's state in a [`PoseCx`]. Every method's default is the
/// shared pose's (no rule).
pub trait PoseRules {
    /// `takesLayer`: with no montage, pose() and the source track leave the layer's slot mix and its turn of
    /// fml_un_COM to `sample`, which takes the layer from [`PoseCx::sample_layer`] under its own nodes.
    fn takes_layer(&self) -> bool {
        false
    }

    /// `sampleRule`: the AnimBP's own asset player in place of `clip` at `time`, written into `out` (true), else the
    /// clip is sampled as it is (false). Called before the crossfade, for the montage (`montage`) or the state
    /// machines; `again`: the tick's second call (the montage's), after one with no montage for the source pose under
    /// it ([`CharView::slot_under`]): a rule that keeps state does not step it twice.
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &mut self,
        _cx: &mut PoseCx<'_>,
        _clip: usize,
        _time: f64,
        _montage: Option<Name>,
        _dt: f64,
        _out: &mut Pose,
        _again: bool,
    ) -> bool {
        false
    }

    /// `mixRule`: the AnimBP's own asset player for one part of the slot mix (true: it wrote `out`).
    fn mix(&mut self, _cx: &mut PoseCx<'_>, _clip: usize, _time: f64, _out: &mut Pose) -> bool {
        false
    }

    /// `poseRule`: the tick's final local pose `p`, after the crossfade and the hold layer, before the head scale and
    /// the bones take it.
    fn rule(&mut self, _cx: &mut PoseCx<'_>, _p: &mut Pose, _dt: f64) {}
}

/// The view's state a [`PoseRules`] reads while pose() runs (the character view's members the rules take).
pub struct PoseCx<'a> {
    pub clips: &'a mut Clips,
    pub rig: &'a Rig,
    pub rest: &'a Pose,
    /// Per bone its parent bone, -1 a top bone.
    pub parents: &'a [i32],
    /// `rootIndex` of the sample calls.
    pub root_index: i32,
    pub facing_left: bool,
    /// `fadeFromShown`: a montage that starts next fades in from the pose as it was shown (a rule's layers included).
    pub fade_from_shown: &'a mut bool,
    /// `fadeWeight`: the weight of the target in the view's crossfade, 1 with none running (in `sample` and `mix`,
    /// the last pose's).
    pub fade_weight: f64,
    /// `fadeCount`: the crossfades started so far.
    pub fade_count: u32,
    turn_index: Option<usize>,
    turn_above: &'a [usize],
    layer_mix: &'a [MixArg],
    layer_turn: Option<f64>,
    mix_pose: &'a mut Pose,
    mix_out: &'a mut Pose,
}

impl PoseCx<'_> {
    /// `sampleClip` over the view's rig.
    pub fn sample(&mut self, clip: usize, time: f64, out: &mut Pose) {
        sample_clip(self.clips, clip, self.rig, self.rest, time, out, self.root_index);
    }

    /// `sampleLayer`: the linked layer's pose of the call in progress (a `takes_layer` rule's): the clip, the slot mix,
    /// the turn of fml_un_COM.
    pub fn sample_layer(&mut self, clip: usize, time: f64, out: &mut Pose) {
        self.sample(clip, time, out);
        for m in self.layer_mix {
            let Some(mc) = m.clip else { continue };
            if m.w <= 0.0 {
                continue;
            }
            sample_clip(self.clips, mc, self.rig, self.rest, m.time, self.mix_pose, self.root_index);
            blend_pose(out, self.mix_pose, m.w, self.mix_out);
            out.copy(self.mix_out);
        }
        if let Some(p) = self.layer_turn {
            turn_hit(out, self.turn_index, self.turn_above, p);
        }
    }

    /// `turnCom`: fml_un_COM's component-space rotation replaced by `q` (the model's space), a character's own
    /// ModifyBone on the bone.
    pub fn turn_com(&self, p: &mut Pose, q: [f64; 4]) {
        turn_com(p, self.turn_index, self.turn_above, q);
    }

    /// fml_un_COM's index on the rig.
    pub fn com(&self) -> Option<usize> {
        self.turn_index
    }

    /// The product of the local rotations of `bone`'s ancestors in `p`, from the top bone down (its parent's
    /// component-space rotation).
    pub fn above_rot(&self, p: &Pose, bone: usize) -> [f64; 4] {
        let mut chain = Vec::new();
        let mut a = self.parents[bone];
        while a >= 0 {
            chain.push(a as usize);
            a = self.parents[a as usize];
        }
        chain.iter().rev().fold([0.0, 0.0, 0.0, 1.0], |q, &i| math::qmul(q, p.quat(i)))
    }
}

/// A [`PoseCx`] over the view's fields (a macro: the caller holds other fields of the view mutably).
macro_rules! pose_cx {
    ($v:ident, $clips:expr, $mix:expr, $turn:expr) => {
        PoseCx {
            clips: $clips,
            rig: &$v.rig,
            rest: &$v.rest,
            parents: &$v.parents,
            root_index: $v.root_index,
            facing_left: $v.facing_left,
            fade_from_shown: &mut $v.fade_from_shown,
            fade_weight: $v.fade_weight,
            fade_count: $v.fade_count,
            turn_index: $v.turn_index,
            turn_above: &$v.turn_above,
            layer_mix: $mix,
            layer_turn: $turn,
            mix_pose: &mut $v.cx_pose,
            mix_out: &mut $v.cx_out,
        }
    };
}

/// Bones by name, each with a scale in its own axes (CharView::part_bone_scale).
pub type BoneScales = &'static [(&'static str, [f64; 3])];

/// `CharacterView`'s pose and placement.
#[derive(Debug)]
pub struct CharView {
    pub rig: Rig,
    pub rest: Pose,
    /// Per bone its parent bone, -1 a top bone.
    pub parents: Vec<i32>,
    root_index: i32,
    turn_index: Option<usize>,
    turn_above: Vec<usize>,
    cur: Pose,
    target: Pose,
    from: Pose,
    out: Pose,
    shown: Pose,
    before: Pose,
    /// What the last present() drew.
    pub drawn: Pose,
    posed: bool,
    blend_t: f64,
    blend_dur: f64,
    blend_hermite: bool,
    fade_weight: f64,
    fade_count: u32,
    last_key: ViewKey,
    last_montage: Option<Name>,
    last_blend_out: f64,
    mix_pose: Pose,
    mix_out: Pose,
    add_a: Pose,
    under: Option<(Option<usize>, f64, f64, ViewKey)>,
    under_pose: Pose,
    gone_dur: f64,
    src_on: bool,
    src_key: ViewKey,
    src_t: f64,
    src_dur: f64,
    src_hermite: bool,
    src_from: Pose,
    src_cur: Pose,
    layer: Option<Layer>,
    layer_w: f64,
    pub hit_pitch: Option<f64>,
    pub layer_pitch: Option<f64>,
    pub head_step: Option<HeadScaleStep>,
    pub head_scale: f64,
    pub props: Vec<Prop>,
    pub mesh_scale: f64,
    pub mesh_z: f64,
    pub size_scale: f64,
    pub facing_left: bool,
    /// The view root's place (root.position, three space).
    root_pos: [f64; 3],
    /// `fadeFromShown` (PoseCx::fade_from_shown).
    pub fade_from_shown: bool,
    /// Per part (skin sub-mesh) whether it shows (`partOn`, setPartVisible).
    pub part_on: Vec<bool>,
    /// `partsOnlyOn`: the parts alone are drawn (setPartsOnly).
    pub parts_only: bool,
    /// `setPartBoneScale`: per part so set, the bones (by name) its own AnimBP scales after it copied the pose, each
    /// scale in the bone's own axes (the view's (x, y, z) = UE (X, Z, Y)): the part is drawn on draw bones under them.
    /// Set by a pose hook when the view is made.
    pub part_bone_scale: Vec<(usize, BoneScales)>,
    cx_pose: Pose,
    cx_out: Pose,
    /// `turns`: the bones the mirror node leaves alone that hang under mirrored ones (keptRoots), each with the bones
    /// above it from the top down.
    kept_roots: Vec<(usize, Vec<usize>)>,
}

impl CharView {
    /// A view of the main mesh `body` (skeletal) on `rig` (its bones bound to the clips).
    pub fn new(body: &MeshInfo, rig: Rig, mesh_scale: f64, mesh_z: f64) -> CharView {
        let sk = body.skeleton.as_ref().expect("a view's main mesh is skeletal");
        let n = sk.bones.len();
        let parents = sk.parents.clone();
        let root_index = rig.index("Root").map_or(-1, |i| i as i32);
        let turn_index = rig.index(HIT_TURN_BONE);
        let mut turn_above = Vec::new();
        if let Some(t) = turn_index {
            let mut p = parents[t];
            while p >= 0 {
                turn_above.insert(0, p as usize);
                p = parents[p as usize];
            }
        }
        let rest = body.rest.clone();
        let p = || Pose::new(n);
        CharView {
            rig,
            parents,
            root_index,
            turn_index,
            turn_above,
            cur: rest.clone(),
            target: p(),
            from: p(),
            out: p(),
            shown: p(),
            before: p(),
            drawn: rest.clone(),
            rest,
            posed: false,
            blend_t: 1.0,
            blend_dur: 0.0,
            blend_hermite: false,
            fade_weight: 1.0,
            fade_count: 0,
            last_key: ViewKey::Empty,
            last_montage: None,
            last_blend_out: 0.0,
            mix_pose: p(),
            mix_out: p(),
            add_a: p(),
            under: None,
            under_pose: p(),
            gone_dur: -1.0,
            src_on: false,
            src_key: ViewKey::Empty,
            src_t: 1.0,
            src_dur: 0.0,
            src_hermite: false,
            src_from: p(),
            src_cur: p(),
            layer: None,
            layer_w: 0.0,
            hit_pitch: None,
            layer_pitch: None,
            head_step: None,
            head_scale: 1.0,
            props: Vec::new(),
            mesh_scale,
            mesh_z,
            size_scale: 1.0,
            facing_left: false,
            root_pos: [0.0; 3],
            fade_from_shown: false,
            part_on: Vec::new(),
            parts_only: false,
            part_bone_scale: Vec::new(),
            cx_pose: p(),
            cx_out: p(),
            kept_roots: Vec::new(),
        }
    }

    /// `turns`: the character's mirror table leaves the body alone; the kept roots are
    /// placed on the mirrored bones above them facing left (bones_cs).
    pub fn set_turns(&mut self) {
        self.kept_roots = kept_roots(&self.rig.names, &self.parents);
    }

    /// Whether bone `b` is a kept bone (keptBy: a kept root or under one).
    pub fn kept(&self, b: usize) -> bool {
        is_kept(&self.kept_roots, &self.parents, b)
    }

    /// `setPartVisible`.
    pub fn set_part_visible(&mut self, part: usize, on: bool) {
        self.part_on[part] = on;
    }

    /// `place`: the sim position (UE units) and facing as the view root's place.
    pub fn place(&mut self, sim_x: f64, sim_y: f64, facing: f64) {
        self.root_pos = [0.0, (sim_y + self.mesh_z * self.size_scale) / 100.0, -sim_x / 100.0];
        self.facing_left = facing < 0.0;
    }

    /// The model node's world matrix (three space): the root (its place, scale.z -1 facing left) times the model
    /// (the mesh scale with the actor's, yaw 90 degrees: facing right the actor yaws -90).
    pub fn model_world(&self) -> Mat4 {
        let root =
            math::compose(self.root_pos, [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, if self.facing_left { -1.0 } else { 1.0 }]);
        let h = std::f64::consts::FRAC_PI_2 / 2.0;
        let k = self.mesh_scale * self.size_scale;
        let model = math::compose([0.0; 3], [0.0, h.sin(), 0.0, h.cos()], [k, k, k]);
        math::mul(&root, &model)
    }

    /// `slotUnder`: the montage of the coming pose() is drawn at weight w over this source pose.
    // Deviation: the source track starts with the first tick the source is drawn, so a crossfade the state machines
    // were in when the blend-out began is not in it (the build's machines ran under the montage).
    pub fn slot_under(&mut self, clip: Option<usize>, time: f64, w: f64, key: ViewKey) {
        self.under = Some((clip, time, w, key));
    }

    /// `montageGone`: the montage shown by the last pose() is gone in the coming one, stopped: its crossfade time.
    pub fn montage_gone(&mut self, dur: f64) {
        self.gone_dur = dur;
    }

    /// `shownMontage`: the montage the last pose() showed.
    pub fn shown_montage(&self) -> Option<Name> {
        self.last_montage
    }

    /// `fadeCount`: the crossfades the view has started.
    pub fn fade_count(&self) -> u32 {
        self.fade_count
    }

    /// `setLayer`: AnimNode_LayeredBoneBlend (local space, BlendDepth 0) of the single-frame `clip` on the subtree
    /// under `branch`, faded over `blend_time`.
    pub fn set_layer(&mut self, clips: &mut Clips, clip: usize, branch: &str, blend_time: f64) {
        let b = self.rig.index(branch).unwrap_or_else(|| panic!("layer bone missing: {branch}"));
        let mut pose = Pose::new(self.rest.n);
        sample_clip(clips, clip, &self.rig, &self.rest, 0.0, &mut pose, self.root_index);
        // (Object3D.traverse: the bone, then each child's subtree in order)
        let mut idx = Vec::new();
        let mut stack = vec![b];
        while let Some(i) = stack.pop() {
            idx.push(i);
            let kids: Vec<usize> = (0..self.parents.len()).filter(|&k| self.parents[k] == i as i32).collect();
            stack.extend(kids.into_iter().rev());
        }
        self.layer = Some(Layer { pose, idx, time: blend_time });
    }

    /// `addProp`: a static mesh component on a socket of the bone `at.bone`, hidden until set_prop shows it.
    pub fn add_prop(&mut self, tag: &'static str, at: &PropAttach<'_>) {
        let bone = self.rig.index(at.bone).unwrap_or_else(|| panic!("prop bone missing: {}", at.bone));
        let k = if at.abs_scale { 1.0 / self.mesh_scale } else { 1.0 };
        let build = |socket_loc: [f64; 3], socket_rot: Option<[f64; 3]>, loc: [f64; 3], rot: [f64; 3]| {
            let socket = math::compose(
                math::gv(socket_loc, 0.01),
                socket_rot.map_or([0.0, 0.0, 0.0, 1.0], math::gq),
                [1.0, 1.0, 1.0],
            );
            let p = math::gv(loc, 0.01);
            let s = math::gv(at.scale, 1.0);
            let obj = math::compose([p[0] * k, p[1] * k, p[2] * k], math::gq(rot), [s[0] * k, s[1] * k, s[2] * k]);
            math::mul(&socket, &obj)
        };
        let right = build(at.socket_loc, at.socket_rot, at.loc, at.rot);
        // Facing left the view reflects the right-facing scene across three z, so the mirrored pose's twin bone is the
        // reflected bone itself with the twin's BoneFlipAxis negated (XYZ for the wrists in Shaggy_MirrorTable, Y for
        // Bugs's fml_un_L_PropOffset).
        let left = at.left.map(|(sl, sr, l, r, f)| {
            let flip = math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], f);
            math::mul(&flip, &build(sl, Some(sr), l, r))
        });
        self.props.push(Prop { tag, bone, right, left, on: false });
    }

    /// `setProp`.
    pub fn set_prop(&mut self, tag: &str, visible: bool) {
        if let Some(p) = self.props.iter_mut().find(|p| p.tag == tag) {
            p.on = visible;
        }
    }

    /// `restart`: the view starts as a new one (a second view taken again: crate::second), its pose held.
    pub fn restart(&mut self) {
        self.last_key = ViewKey::Empty;
        self.last_montage = None;
        self.last_blend_out = 0.0;
        self.under = None;
        self.gone_dur = -1.0;
        self.src_on = false;
        self.blend_t = 1.0;
        self.fade_weight = 1.0;
        self.layer_w = 0.0;
        self.posed = false;
    }

    /// The cut of the next present: it starts from the latest pose.
    pub fn cut(&mut self) {
        self.before.copy(&self.shown);
    }

    // One tick of the source track (srcStep): the source clip's pose into src_cur.
    #[allow(clippy::too_many_arguments)]
    fn src_step(
        &mut self,
        clips: &mut Clips,
        clip: usize,
        time: f64,
        key: ViewKey,
        dt: f64,
        loco: &Loco,
        rules: Option<&mut (dyn PoseRules + 'static)>,
    ) {
        if !self.src_on {
            self.src_on = true;
            self.src_t = 1.0;
        } else if key != self.src_key {
            let dur = loco.dur;
            if dur > 0.0 {
                self.src_from.copy(&self.src_cur);
                self.src_t = 0.0;
                self.src_dur = dur;
                self.src_hermite = loco.hermite;
            } else {
                self.src_t = 1.0;
            }
        }
        self.src_key = key;
        let takes = rules.as_ref().is_some_and(|r| r.takes_layer());
        let ruled = match rules {
            Some(r) => {
                let turn = if takes { self.layer_pitch } else { None };
                let mut cx = pose_cx!(self, clips, &[], turn);
                r.sample(&mut cx, clip, time, None, dt, &mut self.under_pose, false)
            }
            None => false,
        };
        if !ruled {
            sample_clip(clips, clip, &self.rig, &self.rest, time, &mut self.under_pose, self.root_index);
        }
        // the layer's own ModifyBone, unless the rule took it under its nodes
        if let Some(p) = self.layer_pitch
            && !(ruled && takes)
        {
            turn_hit(&mut self.under_pose, self.turn_index, &self.turn_above, p);
        }
        if self.src_t < 1.0 {
            self.src_t = (self.src_t + dt / self.src_dur).min(1.0);
            let t = self.src_t;
            let a = if self.src_hermite { t * t * (3.0 - 2.0 * t) } else { t };
            blend_pose(&self.src_from, &self.under_pose, a, &mut self.src_cur);
        } else {
            self.src_cur.copy(&self.under_pose);
        }
    }

    // The drawn blend-out is over: the main crossfade goes on as the source track (srcTakeOver).
    fn src_take_over(&mut self) {
        self.last_key = self.src_key;
        self.last_montage = None;
        self.cur.copy(&self.src_cur);
        if self.src_t < 1.0 {
            self.from.copy(&self.src_from);
            self.blend_t = self.src_t;
            self.blend_dur = self.src_dur;
            self.blend_hermite = self.src_hermite;
        } else {
            self.blend_t = 1.0;
        }
    }

    /// `pose`: show `clip` at `time`; `montage` and its blend times decide the crossfade, `loco` the state machines';
    /// `rules`: the character's own nodes.
    #[allow(clippy::too_many_arguments)]
    pub fn pose(
        &mut self,
        clips: &mut Clips,
        clip: Option<usize>,
        time: f64,
        montage: Option<Name>,
        blend_in: f64,
        blend_out: f64,
        dt: f64,
        layer_on: bool,
        loco: &Loco,
        add: Option<(usize, f64)>,
        mix: &[MixArg],
        mut rules: Option<&mut (dyn PoseRules + 'static)>,
    ) {
        let under = self.under.take();
        let gone_dur = std::mem::replace(&mut self.gone_dur, -1.0);
        let Some(clip) = clip else { return };
        if let Some(l) = &self.layer {
            let lt = l.time;
            self.layer_w = if lt > 0.0 {
                (self.layer_w + (if layer_on { dt } else { -dt }) / lt).clamp(0.0, 1.0)
            } else if layer_on {
                1.0
            } else {
                0.0
            };
        }
        let key = match montage {
            Some(m) => ViewKey::Clip(clip, m),
            None => loco.key,
        };
        let mixed = montage.is_some() && under.is_some_and(|u| u.0.is_some() && u.2 < 1.0);
        if mixed {
            let (src, t, _, k) = under.expect("mixed");
            self.src_step(clips, src.expect("mixed"), t, k, dt, loco, rules.as_deref_mut());
        } else {
            if self.src_on && montage.is_none() && self.last_montage.is_some() && gone_dur < 0.0 {
                self.src_take_over();
            }
            self.src_on = false;
        }
        if key != self.last_key {
            let mut dur = loco.dur;
            self.blend_hermite = montage.is_none() && self.last_montage.is_none() && loco.hermite;
            if montage.is_some() && montage != self.last_montage {
                dur = blend_in;
            } else if montage.is_none() && self.last_montage.is_some() {
                dur = if gone_dur >= 0.0 { gone_dur } else { self.last_blend_out };
            } else if montage.is_some() && montage == self.last_montage {
                dur = 0.0; // section / segment change inside a montage
            }
            if self.last_key != ViewKey::Empty && dur > 0.0 {
                if montage.is_some() && self.fade_from_shown && self.posed {
                    self.from.copy(&self.shown);
                } else {
                    self.from.copy(&self.cur);
                }
                self.blend_t = 0.0;
                self.blend_dur = dur;
                self.fade_count = self.fade_count.wrapping_add(1);
            } else {
                self.blend_t = 1.0;
            }
            self.last_key = key;
            self.last_montage = montage;
            if montage.is_some() {
                self.last_blend_out = blend_out;
            }
        }
        // with no montage a takes_layer rule has the layer's slot mix and turn (sample_layer)
        let own = montage.is_none() && rules.as_ref().is_some_and(|r| r.takes_layer());
        let ruled = match rules.as_deref_mut() {
            Some(r) => {
                let (lm, lt) = if own { (mix, self.hit_pitch) } else { (&[][..], None) };
                let mut cx = pose_cx!(self, clips, lm, lt);
                r.sample(&mut cx, clip, time, montage, dt, &mut self.target, mixed)
            }
            None => false,
        };
        if !ruled {
            sample_clip(clips, clip, &self.rig, &self.rest, time, &mut self.target, self.root_index);
        }
        let layered = own && ruled;
        for m in if layered { &[][..] } else { mix } {
            let Some(mc) = m.clip else { continue };
            if m.w <= 0.0 {
                continue;
            }
            // The kick slot blend list (and C019_AnimBP's slot blends): the other slot's pose at the same montage
            // position, mixed by its weight.
            let mixed_by = match rules.as_deref_mut() {
                Some(r) => {
                    let mut cx = pose_cx!(self, clips, &[], None);
                    r.mix(&mut cx, mc, m.time, &mut self.mix_pose)
                }
                None => false,
            };
            if !mixed_by {
                sample_clip(clips, mc, &self.rig, &self.rest, m.time, &mut self.mix_pose, self.root_index);
            }
            blend_pose(&self.target, &self.mix_pose, m.w, &mut self.mix_out);
            self.target.copy(&self.mix_out);
        }
        if mixed {
            let w = under.expect("mixed").2;
            blend_pose(&self.src_cur, &self.target, w.max(0.0), &mut self.mix_out);
            self.target.copy(&self.mix_out);
        }
        // Deviation: Base_AnimBP's ModifyBone_5 sits under the state's own crossfades (the intro to the loop, 0.1 s);
        // here the sampled clip is turned before the view's one crossfade, so a pose fading out keeps the pitch of
        // its last tick.
        if let Some(p) = self.hit_pitch
            && !layered
        {
            turn_hit(&mut self.target, self.turn_index, &self.turn_above, p);
        }
        if let Some((c, t)) = add {
            apply_additive(clips, c, &self.rig, &self.rest, t, &mut self.target, &mut self.add_a);
        }
        if self.blend_t < 1.0 {
            self.blend_t = (self.blend_t + dt / self.blend_dur).min(1.0);
            let t = self.blend_t;
            self.fade_weight = if self.blend_hermite { t * t * (3.0 - 2.0 * t) } else { t };
            blend_pose(&self.from, &self.target, self.fade_weight, &mut self.out);
            self.cur.copy(&self.out);
        } else {
            self.fade_weight = 1.0;
            self.cur.copy(&self.target);
        }
        // (the last shown pose becomes before: the two buffers swap)
        if self.posed {
            std::mem::swap(&mut self.before, &mut self.shown);
        } else {
            self.before.copy(&self.cur);
        }
        self.shown.copy(&self.cur);
        if let Some(l) = &self.layer
            && self.layer_w > 0.0
        {
            blend_pose(&self.cur, &l.pose, self.layer_w, &mut self.out);
            for &i in &l.idx {
                self.shown.copy_bone(&self.out, i);
            }
        }
        if let Some(r) = rules {
            let mut cx = pose_cx!(self, clips, &[], None);
            r.rule(&mut cx, &mut self.shown, dt);
        }
        if let Some(h) = &self.head_step
            && self.head_scale != 1.0
        {
            h.apply(&mut self.shown, self.head_scale);
        }
        if !self.posed {
            self.before.copy(&self.shown);
        }
        self.posed = true;
    }

    /// `present`: the pose drawn between the previous tick's (a = 0) and the latest tick's (a = 1), into `drawn`. A
    /// scale goes geometrically between the two (a x (b / a)^t, both positive).
    pub fn present(&mut self, a: f64) {
        if !self.posed {
            return;
        }
        if a >= 1.0 {
            self.drawn.copy(&self.shown);
            return;
        }
        blend_pose(&self.before, &self.shown, a, &mut self.drawn);
        for i in 0..self.drawn.s.len() {
            let (p, q) = (f64::from(self.before.s[i]), f64::from(self.shown.s[i]));
            if p != q && p > 0.0 && q > 0.0 {
                self.drawn.s[i] = (p * (q / p).powf(a)) as f32;
            }
        }
    }

    /// The drawn pose's bones in the model's space (three space, the model node's own transform not in it): the
    /// matrix product down each chain, with the engine's rule under a parent of non-uniform positive scale
    /// (the local's 3x3 as diag(1 / Sp) L diag(Sp), Sp the parent's component-space scale).
    pub fn bones_cs(&self, out: &mut Vec<Mat4>) {
        self.pose_cs(&self.drawn, out);
    }

    /// The latest tick's pose (not the frame's) as [`Self::bones_cs`] has it.
    pub fn shown_bones_cs(&self, out: &mut Vec<Mat4>) {
        self.pose_cs(&self.shown, out);
    }

    fn pose_cs(&self, p: &Pose, out: &mut Vec<Mat4>) {
        let n = p.n;
        out.clear();
        let mut cs = vec![[1.0f64; 3]; n];
        for i in 0..n {
            let f = |v: &[f32], k: usize| f64::from(v[k]);
            let t = [f(&p.t, i * 3), f(&p.t, i * 3 + 1), f(&p.t, i * 3 + 2)];
            let s = [f(&p.s, i * 3), f(&p.s, i * 3 + 1), f(&p.s, i * 3 + 2)];
            let mut local = math::compose(t, p.quat(i), s);
            // placeKept: turns, facing left, the kept top bones on the mirrored bones above them. A VisMesh component
            // draws the kept bone K as K * F_P * T(P) (the node's pose, K * F_P * T(P) * M_Y, under the component's
            // (1, -1, 1) scale), turned with the actor; the view's root reflects everything under it, which is that
            // turn after M_Y, so K's local matrix has to be K * F_P * T(P) * M_Y * T(P)^-1: as column matrices,
            // T(P)^-1 * M_Y * T(P) * F_P in front of K's own (F_P = Y). That is the identity while T(P) is symmetric
            // across the component's Y (Root and fml_un_COM at their rest transforms), and turns the body the build's
            // way when it is not (C019_Attack_Arm_Combo3_Perk's Root yaw).
            if self.facing_left
                && let Some((_, above)) = self.kept_roots.iter().find(|k| k.0 == i)
                && !above.is_empty()
            {
                let lm = |j: usize| {
                    let f = |v: &[f32], k: usize| f64::from(v[k]);
                    let t = [f(&p.t, j * 3), f(&p.t, j * 3 + 1), f(&p.t, j * 3 + 2)];
                    let s = [f(&p.s, j * 3), f(&p.s, j * 3 + 1), f(&p.s, j * 3 + 2)];
                    math::compose(t, p.quat(j), s)
                };
                let mut c = lm(above[0]);
                for &a in &above[1..] {
                    c = math::mul(&c, &lm(a));
                }
                let my = math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, -1.0]);
                let q = math::mul(&math::mul(&math::mul(&math::invert(&c), &my), &c), &my);
                local = math::mul(&q, &local);
            }
            let pa = self.parents[i];
            if pa < 0 {
                cs[i] = s;
                out.push(local);
                continue;
            }
            let pa = pa as usize;
            let [px, py, pz] = cs[pa];
            cs[i] = [px * s[0], py * s[1], pz * s[2]];
            let m = if (px == py && py == pz)
                || !(px > 0.0 && py > 0.0 && pz > 0.0 && s[0] > 0.0 && s[1] > 0.0 && s[2] > 0.0)
            {
                math::mul(&out[pa], &local)
            } else {
                let sp = [px, py, pz];
                let mut l = local;
                for c in 0..3 {
                    for r in 0..3 {
                        l[c * 4 + r] = local[c * 4 + r] * sp[c] / sp[r];
                    }
                }
                math::mul(&out[pa], &l)
            };
            out.push(m);
        }
    }
}

/// The kept roots of a skeleton (`turns`): each with the bones above it, from the top.
fn kept_roots(names: &[String], parents: &[i32]) -> Vec<(usize, Vec<usize>)> {
    let kept_above = |b: usize| {
        let mut p = parents[b];
        while p >= 0 {
            let n = names[p as usize].as_str();
            if KEPT_ROOTS.contains(&n) || KEPT_ROOTS_FREE.contains(&n) {
                return true;
            }
            p = parents[p as usize];
        }
        false
    };
    let mut out = Vec::new();
    for (b, n) in names.iter().enumerate() {
        if KEPT_ROOTS.contains(&n.as_str()) || (KEPT_ROOTS_FREE.contains(&n.as_str()) && !kept_above(b)) {
            let mut above = Vec::new();
            let mut p = parents[b];
            while p >= 0 {
                above.insert(0, p as usize);
                p = parents[p as usize];
            }
            out.push((b, above));
        }
    }
    out
}

fn is_kept(roots: &[(usize, Vec<usize>)], parents: &[i32], b: usize) -> bool {
    let mut i = b as i32;
    while i >= 0 {
        if roots.iter().any(|k| k.0 == i as usize) {
            return true;
        }
        i = parents[i as usize];
    }
    false
}

/// Per bone of character `key`'s main mesh: a bone its view keeps unmirrored (a kept
/// root of a character with `turns`, a bone of its entry's `kept_by`, and the bones under them), for an attach that
/// follows the bone. All false for a character without `turns` and for a mesh without a skeleton.
pub fn kept_bones(key: &str, info: &MeshInfo) -> Vec<bool> {
    let Some(sk) = info.skeleton.as_ref().filter(|_| crate::chars::render_of(key).turns) else {
        return vec![false; info.bones().len()];
    };
    let mut roots = kept_roots(&sk.bones, &sk.parents);
    for k in crate::chars::render_of(key).kept_by {
        if let Some(b) = sk.bones.iter().position(|n| n == k) {
            roots.push((b, Vec::new()));
        }
    }
    (0..sk.bones.len()).map(|b| is_kept(&roots, &sk.parents, b)).collect()
}

/// `turnHit` / `turnCom`: fml_un_COM's component-space rotation replaced by the pitch about the depth axis.
fn turn_hit(p: &mut Pose, turn: Option<usize>, above: &[usize], pitch: f64) {
    let h = pitch * std::f64::consts::PI / 360.0;
    turn_com(p, turn, above, [0.0, 0.0, h.sin(), h.cos()]);
}

/// `turnCom`: the same Replace with any rotation `q` in the model's space.
fn turn_com(p: &mut Pose, turn: Option<usize>, above: &[usize], to: [f64; 4]) {
    let Some(b) = turn else { return };
    let mut q = [0.0, 0.0, 0.0, 1.0];
    for &a in above {
        q = math::qmul(q, p.quat(a));
    }
    let inv = [-q[0], -q[1], -q[2], q[3]];
    let r = math::qmul(inv, to);
    for (k, v) in r.iter().enumerate() {
        p.r[b * 4 + k] = *v as f32;
    }
}
