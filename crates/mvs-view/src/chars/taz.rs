//! Taz's render module: the entry, the AnimBP's pose hook and the pawn props
//!.
//!
//! The skin's skeletal meshes and materials (C015_Default: BodyMesh C015_SkelMesh, Mouth1..3 and their _MI twins,
//! Tongue, Teeth, TeethSmall, Tornado, TornadoSmall, Baguette1 / 2; the main mesh C015Core_SkelMesh draws nothing) are
//! drawn by the shared path; the slot graph is the sim's (TazState.slot_mix).
//! Facing (turns): C015_MirrorTable has BoneFlipAxis None on fml_un_C_pelvis_att, fml_un_C_face_att,
//! fml_un_C_faceLower_att and every bone below them; Root, fml_un_COM, the prop, baguette, tornado1..7 and vfx bones
//! are mirrored. C015_AnimBP's mirror node (bEnabled <- FacingLeft) and On Facing Changed's RelativeScale3D
//! (1, -1, 1) on every VisMesh SkeletalMeshComponent (BodyMesh, the mouths, Tongue, Teeth, TeethSmall, Tornado,
//! TornadoSmall): the view's turns rule.
//!
//! The pose hook: C015_AnimBP's sub-mesh toggles, from the CDO's visibility states.
//! BlueprintUpdateAnimation (@3830) -> SetPropVisibility (@3869 -> ub @4645):
//!   - Visibility_ForceUpdate (BlueprintInitializeAnimation @4563 after a Delay 0, the bind @2954, IsReset @7366 on
//!     BaseFighter's OnFighterReset): every state's ForceUpdate set, the flag cleared (@4645..4979).
//!   - behind VisibilityManager valid and InitialVisibilitySet (@4990..5029; both true in a match).
//!   - Toggle Submesh Visibility (the function, @0..1287) on BodyVisible (@5111). Body shown: Mouth1, Mouth2, Mouth3,
//!     Tongue, Teeth (@5191, 6381, 6414, 6447, 6480); TeethSmall = !Teeth.Visibility && Mouth2.Visibility, written on
//!     the tag TeethSmall when it changes (@6512..6749); no mouth shown: Mouth1 shown on its mirror tag facing left
//!     (the other tag hidden), Visibility true, PreviousFrameCurveValue 1 (@6818..7124). Body hidden: Mouth1 (both
//!     tags), Mouth2 (both), Mouth3 (both), Tongue, Teeth and TeethSmall each hidden when its Visibility is set, the
//!     Visibility cleared (@5554..6380; no success test, prev and ForceUpdate untouched).
//!   - then Tornado, SmallTornado, Anvil, Baguette1, Boot, Pot, Bowl, Salt, Baguette2, Pepper (the push_flow stack
//!     @5061..5106 unwinds 5257 first).
//!
//! Toggle Submesh Visibility: prev = the curve (@46, before the test); c = ClampAngle(curve, 0, 1) (the plain clamp);
//! vis = c >= 0.5 || c > prev, so a negative key reads shown; (vis XOR Visibility) || ForceUpdate:
//! SetSubMeshVisibility(FacingLeft && MirrorMeshTag != None ? MirrorMeshTag : MeshTagName, vis); success:
//! Visibility = vis, ForceUpdate false; no mesh of the tag: ForceUpdate true (Anvil, Boot and Bowl have none in
//! C015_Default: the salad's tag is salad), tried again each update.
//! On Facing Changed (@0..2064, BPI_SetIsFacingLeft @4419): each of Mouth1..3 whose Visibility is set moved to its
//! other tag: facing left MeshTagName hidden and MirrorMeshTag shown, facing right the reverse. The port runs it before
//! the update of the tick the fighter's facing changed in.
//! SetSubMeshVisibility is the manager's (Comp_FighterVisibilityManager @0..1504): shown is drawn only at
//! InvisibilityCount 0 (out of HiddenMeshes either way), hidden is drawn hidden and leaves AlwaysVisibleMeshes. So
//! under an AddInvisibility a write of true shows nothing, and a write of false hides a mesh the count left shown
//! (C015_Side_Special's tornado, AnimNotify_SetSubmeshAlwaysVisible): `cut` below.
//! The AnimBP's write holds until the sim changes that mesh's visibility (a notify, SetupInitialVisibility at the
//! respawn); the sim's changes of the tick come first (the anim update follows the game tick), then this update's
//! writes. At the respawn the states are kept and every state is written anew (IsReset's ForceUpdate).
//! The curves: the clips' own (crate::curves) by the sequence the tick shows (the fighter's anim and anim time), the
//! slot mix's tracks (TazState.slot_mix) lerped in by their weights, and the view's crossfade.
//!
//! The big head (crate::anim head scale): C015_AnimBP's ModifyBone_7 (fml_un_C_neck_a, scale HeadScale x 0.8 @3125),
//! ModifyBone_5 (fml_un_C_faceLower_att, x 0.7 @3357, translation Additive (12.466, 0, 21.275)) and ModifyBone_6
//! (fml_un_C_face_att, x 0.8 @3589, translation Additive (-0.001, -17.357, 8.476)), bone space, each Alpha =
//! HeadScale.X > 1 (@3161, 3393, 3625).
//!
//! The pawn props: Pawn_C015's StaticMeshComponents, children of CharacterMesh0 in the SCS with no StaticMesh of their
//! own: PepperShaker (tag Pepper, socket fml_un_C_prop2), SaltShaker (Salt, fml_un_C_prop1), SoupPot (Pot,
//! fml_un_C_prop1) and Salad (salad, fml_un_C_prop1, bVisible false). The skin's static SubMeshes give them their
//! meshes and materials (C015_Default: Pot C015_Cooking_Pot_Mesh, Pepper C015_Pepper_Shaker_Mesh, Salt
//! C015_Salt_Shaker_Mesh; the draws' `<skin>.<tag>` entries), with SelectMaterialColor's pick (selected_variant).
//! Shown by C015_AnimBP's PotVisible, SaltVisible and PepperVisible (the hook's `shown`); BowlVisible's tag Bowl names
//! no mesh (the salad's tag is salad), so the salad is never shown and is not made. CharacterMesh0 is not scaled by
//! OnFacingChanged (no VisMesh tag): the props are on the view's bone, the plain reflection facing left.
//!
//! Deviations and unverified models:
//!   - Deviation: the AnimBP's writes are the view's: they do not reach the sim's sub_hidden / always_visible, which
//!     the build's manager holds (a sim reader of those sets sees the notifies' writes alone).
//!   - Unverified: a curve's value in a crossfade and under the slot mix, and the one tick the update lags the
//!     evaluation: as Rick's and LeBron's.
//!   - Unverified: OnFighterReset's place among the respawn's writes (BaseFighter ResetFighterLogic @1106 against
//!     SetupInitialVisibility): taken as after, so the AnimBP's writes are the later ones.
//!   - a respawn is seen as a set() after ticks without one (the view is not posed while the fighter is dead), where
//!     dead() would be called each dead tick.
//!   - Deviation: a prop is placed each frame at the frame's alpha, on the socket of the pose just presented, not
//!     once a tick on the tick's pose.
//!   - Deviation: Baguette1 / Baguette2 have no VisMesh tag (one scale less than the body's) and C015_AnimBP's
//!     Constraint (Baguette2 onto VB fml_un_C_prop1_fml_un_C_Baguette1) and Constraint_1 (Baguette1 onto VB
//!     prop2_Baguette2), Parent type, alpha FacingLeft, put each baguette on the other's prop bone facing left: drawn
//!     as the plain reflection.
//!   - Deviation: the Tornado meshes are weighted to tornado1..7 alone (mirrored, BoneFlipAxis Z) under the VisMesh
//!     scale: in the build the mirrored bones' pose reflected once more in depth; drawn as the plain reflection.
//!   - Not ported (C015_Default's meshes weight no vertex to these bones): ModifyBone / _1 / _2 / _4
//!     (fml_un_Summertime_hat_bow_1..4) and ModifyBone_3 (fml_un_C_hat_att), and the Trail node on hat_bow_4: the
//!     Summertime skin's bow; ModifyBone_8 (VB Spine): a virtual bone the view does not hold.
//!   - Deviation: BP_CharLights_C015 is not drawn: only BP_CharacterPreview spawns its light rig, and no match does.
//!   - a preview actor (BP_CharacterPreview, the podium, the menus) runs the same toggle (PoseHook::preview_step): the
//!     AnimBP takes the visibility manager from its owning actor (@377-562), and its SetupInitialVisibility after the
//!     0.2 s Delay (BP_CharacterPreview @8699, @8754) sets InitialVisibilitySet, the gate of @5029. No sim: a part is
//!     written on the frames the AnimBP writes its tag. Unverified: where SetupInitialVisibility falls against the
//!     montage's start: taken as the first posed frame, with that frame's curves as the evaluation before.
//!   - not ported: the check of the data's slot graph against
//!     the AnimBP's Slot nodes, Buff_Chicken's polymorph mesh, the warm-up lists. The reset (a new pawn on one
//!     view: the Lab's reset) is PoseHook::reset.

use mvs_sim::World;
use mvs_sim::actors::{ActorRef, ProjectileId};
use mvs_sim::chars::taz::state::taz_of;
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::{Rows, V3, compose_rows};

use super::{CharRender, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_view::{StaticView, Xform, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule};
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::curves::CurveTable;
use crate::frame::{ActorPose, MeshPose};
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_7",
            bone: "fml_un_C_neck_a",
            factor: 0.8,
            gated: true,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_5",
            bone: "fml_un_C_faceLower_att",
            factor: 0.7,
            gated: true,
            moves: [12.466, 0.0, 21.275],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_6",
            bone: "fml_un_C_face_att",
            factor: 0.8,
            gated: true,
            moves: [-0.001, -17.357, 8.476],
            scale: true,
        },
    ],
    turns: true,
    pose: Some(taz_pose),
    pawn_actors: Some(pawn_props),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (PICKUP, MeshRule { rot: None, mesh: Some(pickup_mesh) }),
    (TRASH, MeshRule { rot: None, mesh: Some(trash_mesh) }),
    (ANVIL, MeshRule { rot: None, mesh: Some(anvil_mesh) }),
];

/// The float curves its AnimBP reads by name (GetCurveValue), in the order of STATES.
const CURVES: CurveTable = CurveTable::new(&[
    "mouth01_Vis",
    "mouth02_Vis",
    "mouth03_Vis",
    "tongue_Vis",
    "teethVis",
    "body_Vis",
    "tornado_Vis",
    "small_tornado_Vis",
    "show_taz_anvil",
    "show_taz_baguette1",
    "show_taz_boot",
    "show_taz_cooking_pot",
    "show_taz_salad_bowl",
    "show_taz_salt_shaker",
    "show_taz_baguette2",
    "show_taz_pepper_shaker",
]);
const NC: usize = 16;

/// The CDO's states (MeshTagName, MirrorMeshTag), in the variable order; state k reads curve k.
const STATES: [(&str, Option<&str>); NC] = [
    ("Mouth1", Some("Mouth1_MI")), // Mouth1Visible
    ("Mouth2", Some("Mouth2_MI")), // Mouth2Visible
    ("Mouth3", Some("Mouth3_MI")), // Mouth3Visible
    ("Tongue", None),              // TongueVisible
    ("Teeth", None),               // TeethVisible
    ("BodyMesh", None),            // BodyVisible
    ("Tornado", None),             // Tornado Visible
    ("TornadoSmall", None),        // SmallTornadoVisible
    ("Anvil", None),               // AnvilVisible
    ("Baguette1", None),           // Baguette1Visible
    ("Boot", None),                // BootVisible
    ("Pot", None),                 // PotVisible
    ("Bowl", None),                // BowlVisible
    ("Salt", None),                // SaltVisible
    ("Baguette2", None),           // Baguette2Visible
    ("Pepper", None),              // PepperVisible
];
const M1: usize = 0;
const M2: usize = 1;
const M3: usize = 2;
const TONGUE: usize = 3;
const TEETH: usize = 4;
const BODY: usize = 5;
/// Tornado .. Pepper, in the update's order.
const AFTER: [usize; 10] = [6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const MOUTHS: [usize; 3] = [M1, M2, M3];
const TEETH_SMALL: &str = "teethsmall";
/// The pawn's static props the view does not hold as parts (pawn_props): their tags have a mesh in the manager.
const PROP_TAGS: [&str; 4] = ["pot", "salt", "pepper", "salad"];

/// `tazPose`'s state on one view.
#[derive(Debug)]
struct TazPose {
    /// Every tag the update writes (lower case), and per tag the view's part (None: not a part) and whether the
    /// manager has a mesh of it.
    tags: Vec<String>,
    part_of: Vec<Option<usize>>,
    has_mesh: Vec<bool>,
    st_tag: [usize; NC],
    st_mirror: [Option<usize>; NC],
    teeth_small: usize,
    tgt: [f64; NC],
    cur: [f64; NC],
    from: [f64; NC],
    fades: u32,
    /// The states (Visibility, PreviousFrameCurveValue, ForceUpdate), TeethSmallVisible, FacingLeft.
    visible: [bool; NC],
    prev: [f64; NC],
    force: [bool; NC],
    teeth_small_vis: bool,
    facing_left: bool,
    facing_seen: bool,
    /// Per tag: the AnimBP's last write (None: none), the sim's last visibility of the mesh (None: not seen), whether
    /// the sim has it in AlwaysVisibleMeshes, and whether a hide of the AnimBP took it out of that set since (cut).
    own: Vec<Option<bool>>,
    sim_vis: Vec<Option<bool>>,
    sim_av: Vec<bool>,
    cut: Vec<bool>,
    /// The World tick of the last set() (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    live: bool,
    /// PoseOf::slaved: no AnimBP of its own (set does nothing).
    slaved: bool,
    /// Per tag: the AnimBP wrote it in this update (a preview's parts).
    wrote: Vec<bool>,
    /// A preview's next step is its first (its curves seeded from that frame).
    preview_first: bool,
}

fn taz_pose(_view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let mut tags: Vec<String> = Vec::new();
    let mut add = |t: &str| {
        let t = t.to_lowercase();
        if !tags.contains(&t) {
            tags.push(t);
        }
    };
    for (t, m) in STATES {
        add(t);
        if let Some(m) = m {
            add(m);
        }
    }
    add(TEETH_SMALL);
    for t in PROP_TAGS {
        add(t);
    }
    let ix = |t: &str| tags.iter().position(|x| x.eq_ignore_ascii_case(t)).expect("a state's tag");
    let st_tag = STATES.map(|(t, _)| ix(t));
    let st_mirror = STATES.map(|(_, m)| m.map(ix));
    let teeth_small = ix(TEETH_SMALL);
    let part_of: Vec<Option<usize>> =
        tags.iter().map(|t| of.part_keys.iter().position(|k| k.eq_ignore_ascii_case(t))).collect();
    let has_mesh = tags.iter().zip(&part_of).map(|(t, p)| p.is_some() || PROP_TAGS.contains(&t.as_str())).collect();
    let n = tags.len();
    Box::new(TazPose {
        part_of,
        has_mesh,
        st_tag,
        st_mirror,
        teeth_small,
        tgt: [0.0; NC],
        cur: [0.0; NC],
        from: [0.0; NC],
        fades: 0,
        visible: [false; NC],
        prev: [0.0; NC],
        // BlueprintInitializeAnimation's Visibility_ForceUpdate
        force: [true; NC],
        teeth_small_vis: false,
        facing_left: false,
        facing_seen: false,
        own: vec![None; n],
        sim_vis: vec![None; n],
        sim_av: vec![false; n],
        cut: vec![false; n],
        last_tick: None,
        live: false,
        slaved: of.slaved,
        wrote: vec![false; n],
        preview_first: false,
        tags,
    })
}

impl TazPose {
    /// SetSubMeshVisibility on tag `t`: whether the manager has a mesh of it.
    fn write(&mut self, t: usize, vis: bool) -> bool {
        self.own[t] = Some(vis);
        self.wrote[t] = true;
        if !vis {
            self.cut[t] = true;
        }
        self.has_mesh[t]
    }

    fn tag_of(&self, k: usize) -> usize {
        match self.st_mirror[k] {
            Some(m) if self.facing_left => m,
            _ => self.st_tag[k],
        }
    }

    /// Toggle Submesh Visibility on state `k`.
    fn toggle(&mut self, k: usize) {
        let raw = self.cur[k];
        self.prev[k] = raw;
        let c = raw.clamp(0.0, 1.0);
        let vis = c >= 0.5 || c > self.prev[k];
        if vis == self.visible[k] && !self.force[k] {
            return;
        }
        if self.write(self.tag_of(k), vis) {
            self.visible[k] = vis;
            self.force[k] = false;
        } else {
            self.force[k] = true;
        }
    }

    fn hide(&mut self, k: usize) {
        if !self.visible[k] {
            return;
        }
        self.write(self.st_tag[k], false);
        if let Some(m) = self.st_mirror[k] {
            self.write(m, false);
        }
        self.visible[k] = false;
    }

    /// On Facing Changed, then SetPropVisibility on the curves of the evaluation before.
    fn update(&mut self, left: bool) {
        if self.facing_seen && left != self.facing_left {
            for k in MOUTHS {
                if !self.visible[k] {
                    continue;
                }
                let (t, m) = (self.st_tag[k], self.st_mirror[k].expect("a mouth's mirror tag"));
                self.write(if left { t } else { m }, false);
                self.write(if left { m } else { t }, true);
            }
        }
        self.facing_left = left;
        self.facing_seen = true;
        self.toggle(BODY);
        if self.visible[BODY] {
            for k in [M1, M2, M3, TONGUE, TEETH] {
                self.toggle(k);
            }
            let ts = !self.visible[TEETH] && self.visible[M2];
            if ts != self.teeth_small_vis {
                self.teeth_small_vis = ts;
                self.write(self.teeth_small, ts);
            }
            if !self.visible[M1] && !self.visible[M2] && !self.visible[M3] {
                let (t, m) = (self.st_tag[M1], self.st_mirror[M1].expect("Mouth1's mirror tag"));
                let (on, off) = if self.facing_left { (m, t) } else { (t, m) };
                self.write(on, true);
                self.write(off, false);
                self.visible[M1] = true;
                self.prev[M1] = 1.0;
            }
        } else {
            for k in [M1, M2, M3, TONGUE, TEETH] {
                self.hide(k);
            }
            if self.teeth_small_vis {
                self.write(self.teeth_small, false);
                self.teeth_small_vis = false;
            }
        }
        for k in AFTER {
            self.toggle(k);
        }
    }

    /// A preview's parts: those the AnimBP wrote in this update; the others as the preview's manager has them.
    fn preview_parts(&mut self, view: &mut CharView) {
        for t in 0..self.tags.len() {
            if let (true, Some(p)) = (self.wrote[t], self.part_of[t]) {
                view.set_part_visible(p, self.own[t] == Some(true));
            }
        }
        self.wrote.fill(false);
    }
}

impl PoseRules for TazPose {
    /// This evaluation's curves (the view's crossfade of the targets), for the next update.
    fn rule(&mut self, cx: &mut PoseCx<'_>, _p: &mut Pose, _dt: f64) {
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..NC {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl PoseHook for TazPose {
    /// `preview()`: a preview actor's view (no fighter): the AnimBP anew, run by preview_step.
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.reset(&mut |_, _, _| {});
        self.wrote.fill(false);
        self.fades = view.fade_count();
        self.preview_first = true;
    }

    /// `previewStep(anim, time, left)`: On Facing Changed and SetPropVisibility on the curves of the
    /// evaluation before, the parts the update wrote, then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        if std::mem::take(&mut self.preview_first) {
            // Unverified (the module's note): the evaluation before the first update is this frame's
            CURVES.fill(clips, s.anim, s.time, &mut self.cur);
            self.from = self.cur;
        }
        self.update(s.facing_left);
        self.preview_parts(view);
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    /// The AnimBP's update of the tick, before the view's pose: the sim's changes of the tick, the update on the
    /// curves of the evaluation before, the parts' visibility, then this tick's curve targets.
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        self.live = !self.slaved;
        if !self.live {
            return;
        }
        let fi = w.fighter(f);
        if self.last_tick.is_some_and(|t| w.flow.tick > t + 1) {
            self.force = [true; NC]; // IsReset (OnFighterReset): Visibility_ForceUpdate
        }
        self.last_tick = Some(w.flow.tick);
        let name = |t: &str| w.names.get(t);
        // the sim's changes of the tick: its write is the later one
        for t in 0..self.tags.len() {
            let n = name(&self.tags[t]);
            let vis = self.has_mesh[t] && !n.is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            if self.sim_vis[t].is_some_and(|s| s != vis) {
                self.own[t] = None;
            }
            self.sim_vis[t] = Some(vis);
            let av = fi.vis.always_visible.as_ref().is_some_and(|s| n.is_some_and(|n| s.contains(&n)));
            if !av || !self.sim_av[t] {
                self.cut[t] = false;
            }
            self.sim_av[t] = av;
        }
        self.update(fi.body.facing_dir < 0.0);
        self.wrote.fill(false);
        // the parts: under an AddInvisibility (the view's parts-only mode) the meshes the count leaves shown and no
        // hide of the AnimBP took out since; else the AnimBP's write until the sim changes the mesh
        let kept = if fi.vis.invisible { fi.vis.invis_shown.as_ref() } else { fi.vis.always_visible.as_ref() };
        for t in 0..self.tags.len() {
            let Some(p) = self.part_of[t] else { continue };
            let on = if view.parts_only {
                kept.is_some_and(|s| name(&self.tags[t]).is_some_and(|n| s.contains(&n))) && !self.cut[t]
            } else {
                self.own[t].unwrap_or(self.sim_vis[t] == Some(true))
            };
            view.part_on[p] = on;
        }
        let anim = w.names.str(fi.anim.anim);
        CURVES.fill(clips, anim, f64::from(fi.anim.anim_time), &mut self.tgt);
        if let Some(s) = taz_of(w, f) {
            for x in &s.slot_mix.slot_mix {
                let clip = clips.get(w.names.str(x.anim));
                for k in 0..NC {
                    self.tgt[k] += (CURVES.value_of(clips, clip, k, f64::from(x.time)) - self.tgt[k]) * f64::from(x.w);
                }
            }
        }
    }

    /// Whether the mesh of a tag (lower case) is shown: the AnimBP's last write while the sim has not changed the
    /// mesh since, else the sim's.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        // init(): the crossfade's count needs no reset (cur and from are both 0)
        self.tgt = [0.0; NC];
        self.cur = [0.0; NC];
        self.from = [0.0; NC];
        self.visible = [false; NC];
        self.prev = [0.0; NC];
        self.force = [true; NC]; // BlueprintInitializeAnimation's Visibility_ForceUpdate
        self.teeth_small_vis = false;
        self.facing_seen = false;
        self.own.fill(None);
        self.sim_vis.fill(None);
        self.sim_av.fill(false);
        self.cut.fill(false);
        self.last_tick = None; // wasDead = false
    }

    fn shown(&self, tag: &str, sim: bool) -> bool {
        match self.tags.iter().position(|t| t == tag) {
            Some(t) if self.live => self.own[t].unwrap_or(sim),
            _ => sim,
        }
    }
}

/// The props by the pawn's component name: their tag (lower case, as the manager files it).
const PROPS: [(&str, &str); 3] =
    [("SoupPot_GEN_VARIABLE", "pot"), ("SaltShaker_GEN_VARIABLE", "salt"), ("PepperShaker_GEN_VARIABLE", "pepper")];

/// One pawn prop: its tag, its view and its attach.
#[derive(Debug)]
struct PawnProp {
    tag: &'static str,
    view: StaticView,
    socket: String,
    rel: Xform,
}

/// `pawnProps`: fighter `f`'s props, with the mesh and materials its skin gives each component (the skin's static
/// entry of the component's tag; skinStatic).
fn pawn_props(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let d = &w.char_data(f).fighter;
    let pick =
        usize::try_from(w.fighter(f).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let skin = crate::skins::skin_of(d, pick).ok();
    let draws = host.draws_of(w, f);
    let mut out: Vec<Box<dyn PawnActor>> = Vec::new();
    for (name, tag) in PROPS {
        let c = d.components.pawn.get(name);
        let key = c.zip(skin.as_ref()).and_then(|(c, s)| {
            let tags = c.p.as_obj()?.get("ComponentTags")?.as_arr()?;
            tags.iter().filter_map(|t| t.as_str()).find_map(|t| draws?.actor_key(&format!("{}.{t}", s.name)))
        });
        let variant = host.selected_variant(w, f);
        let (key, variant) = match key {
            Some(k) => (Some(k), Some(variant)),
            // (skinStatic's fallback: the components have no StaticMesh of their own, so nothing)
            None => c.and_then(|c| c.p.as_obj()).and_then(|p| host.skin_static(w, f, p)).unzip(),
        };
        let view = key.as_deref().zip(variant).and_then(|(k, v)| StaticView::new(host, w, f, k, v));
        let (Some(c), Some(view)) = (c, view) else {
            eprintln!("warning: Taz: the prop {name} is not drawn (its component, mesh or materials are not loaded)");
            continue;
        };
        let p = c.p.as_obj();
        let rel = Xform {
            loc: xyz(p.and_then(|p| p.get("RelativeLocation")), 0.0),
            rot: rel_rows(p.and_then(|p| p.get("RelativeRotation"))),
            scale: xyz(p.and_then(|p| p.get("RelativeScale3D")), 1.0),
        };
        let socket = c.socket.clone().unwrap_or_else(|| "None".to_owned());
        out.push(Box::new(PawnProp { tag, view, socket, rel }));
    }
    out
}

impl PawnActor for PawnProp {
    /// `placePawnProps`: shown by the AnimBP's write while the sim has not changed the mesh since, else the sim's.
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        if !cx.shown(w, self.tag) {
            return;
        }
        let Some(at) = host.socket(w, cx.fid, &self.socket, &self.rel) else { return };
        self.view.set(&at);
        let mut meshes = Vec::new();
        self.view.draw(&mut meshes);
        cx.outline(true, &mut meshes);
        out.push(ActorPose { meshes, ..ActorPose::default() });
    }
}

// ---- C015_HealPickup_C and C015Trash_C ----
//
// The chicken's heal pickup: its SCS Chicken_Drumstick (the static mesh Chicken_Drumstick, scale 0.6, at (30, 0, 0),
// rotation (90, 180, 180)) under the Capsule root (bAbsoluteScale). No override and no graph of its sets a material:
// the mesh's own slot material (the draws' entry). Its HealVisualMulticast (PS_Heal on the picker) is the effects'.
// Deviation: the body's own rotation is not drawn: the sim steps a sphere and keeps its spin rate alone
// (AngularDamping 200), so the mesh holds the spawn's rotation (actor_rot).
// Not ported: a drumstick whose class no character of the match has (the items part's DrumStick item's draw).
// C015Trash (the burp's default): its StaticMesh is Apple (Finn's) under the Capsule, scale 8; SetTeamColor (@494 ->
// @10..304) puts Finn_Apple_D_Mat on slot 0 locally online, else by FTrunc(BPI_GetColorValue): 0 Finn_Apple_Red_D_Mat,
// 1 Finn_Apple_Blue_D_Mat: ActorHost::variant (the draws' entry 'Apple'). Its ParticleSystem (bVisible false, no
// tick) is the shared path's. C015StewSpit's and C015StewSpitAir's StaticMesh components have no mesh asset.
// Deviation: no outline (the projectiles' custom depth stencil).

const PICKUP: &str = "C015_HealPickup_C";
const TRASH: &str = "C015Trash_C";

/// The drumstick: its place, rotation and scale under the root.
#[derive(Debug)]
struct PickupMesh {
    view: StaticView,
    rel: V3,
    local: Rows,
    scale: V3,
}

fn pickup_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.get("Chicken_Drumstick")?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    // (a drumstick whose class no character of the match has, the items part's DrumStick item's: the items part's
    // draw)
    let view = pr
        .owner
        .and_then(|o| StaticView::new(host, w, mvs_sim::fighter::common::actor_def_owner(w, o, pr.cls), mesh, ""))
        .or_else(|| StaticView::item(host, &format!("{PICKUP}.Chicken_Drumstick"), "template"));
    let Some(view) = view else {
        eprintln!("warning: Taz: the heal pickup is not drawn (its mesh or materials are not loaded)");
        return None;
    };
    let par = &c.parent;
    let prot: Rows =
        std::array::from_fn(|k| par.rot.get(k).copied().unwrap_or(if k % 4 == 0 { 1.0 } else { 0.0 })).map(f64::from);
    // the component's place under the root (FTransform composition)
    let rel = std::array::from_fn(|k| {
        f64::from(par.loc[k]) + (0..3).map(|j| f64::from(par.scale[j]) * c.loc[j] * prot[3 * j + k]).sum::<f64>()
    });
    Some(Box::new(PickupMesh {
        view,
        rel: rel.map(f64::from),
        local: compose_rows(&c.rot, &prot),
        scale: std::array::from_fn(|k| f64::from(par.scale[k]) * c.scale[k]),
    }))
}

impl ActorMesh for PickupMesh {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        _w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.view.visible = shown;
        if !shown {
            return;
        }
        let (x, y) = at.at(alpha);
        let (r, s) = (&at.rot, at.scale);
        let mut loc = [0.0, -x, y];
        for (k, l) in loc.iter_mut().enumerate() {
            *l += s * (self.rel[0] * r[k] + self.rel[1] * r[3 + k] + self.rel[2] * r[6 + k]);
        }
        let scale = self.scale.map(|v| v * s);
        self.view.set(&Xform { loc, rot: compose_rows(&self.local, r), scale });
        self.view.draw(out);
    }
}

/// The apple: its owner (SetTeamColor follows it) and its scale.
#[derive(Debug)]
struct TrashMesh {
    id: ProjectileId,
    view: StaticView,
    scale: V3,
}

fn trash_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.get("StaticMesh")?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    let owner = pr.owner?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, pr.cls);
    let view = StaticView::new(host, w, from, mesh, host.variant(w, owner))?;
    Some(Box::new(TrashMesh { id, view, scale: c.scale }))
}

impl ActorMesh for TrashMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.view.visible = shown;
        if !shown {
            return;
        }
        // (SetTeamColor follows the owner: a reflect's SetTeamColor, as Rick's rocket)
        if let Some(o) = w.actors.projectiles.get(self.id).and_then(|p| p.owner) {
            self.view.dress(host, host.variant(w, o));
        }
        let (x, y) = at.at(alpha);
        let scale = self.scale.map(|v| v * at.scale);
        self.view.set(&Xform { loc: [0.0, -x, y], rot: at.rot, scale });
        self.view.draw(out);
    }
}

// ---- C015Anvil ----
//
// The EatProjectiles perk's anvil (mvs-sim chars/taz/perks.rs, an item; its class data is the perk part's, which
// item_def finds through the spawner). Its SCS: Visual (C015_Anvil_Mesh_C015_anvil at (0, 0, -60)) under the Capsule
// root (bAbsoluteScale). SpawnedActorParent SetTeamColor (@22710 -> @21872) puts the spawner's selected material
// pick of the class's Materials on slot 0 (ActorHost::selected_variant). The root's rotation: the item rows (facing
// yaw, pitch). Held, on the holder's hold socket (Comp_ItemManager.EquipOnSpawn), as Bugs's dynamite.
// Deviation: the ParticleSystem (PS_ShaggySandwichProjectile at (130, 0, 0)) is not drawn, and no outline.
// Deviation: the hold socket is read on the holder's main mesh (the copy mesh is not picked for a holder
// whose Comp_ItemManager has UseOverrideAttachParent).
// ApplyLocalUserColor: ActorHost::apply_local_user_colour (the class default is not set on the anvil).
const ANVIL: &str = "C015Anvil_C";
const ANVIL_MESH: &str = "C015_Anvil_Mesh_C015_anvil";

#[derive(Debug)]
struct AnvilMesh {
    id: mvs_sim::actors::ItemId,
    view: StaticView,
    c: crate::actor_view::Comp,
}

fn anvil_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let by = it.spawner?;
    let def = mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(by), it.cls)?;
    let c = crate::actor_view::comp_of(&def.components, "Visual")?;
    let mut view = StaticView::new(host, w, by, ANVIL_MESH, host.selected_variant(w, by))?;
    host.apply_local_user_colour(w, &mut view, &def.p, by);
    Some(Box::new(AnvilMesh { id, view, c }))
}

impl ActorMesh for AnvilMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(it) = w.actors.items.get(self.id) else { return };
        let hold = at.holder.map(|h| (h, mvs_sim::fighter::match_side::hold_socket(w, h)));
        let no_socket = hold.as_ref().is_some_and(|(_, s)| s == "None");
        if no_socket {
            warn_no_socket();
        }
        let on = shown && !no_socket;
        self.view.visible = on;
        if !on {
            return;
        }
        // (socketFollow's `missing`, a bone the view lacks: the holder's mesh origin)
        let held = hold.and_then(|(h, s)| host.hold_place(w, h, &s));
        let xf = match held {
            Some(s) => crate::actor_view::place_at(&self.c, &self.c.rot, self.c.rs, &s.rot, f64::from(it.scale), s.loc),
            None => {
                let (x, y) = at.at(alpha);
                crate::actor_view::place_under(&self.c, &crate::actor_view::item_rows(it), f64::from(it.scale), x, y)
            }
        };
        self.view.set(&xf);
        self.view.draw(out);
    }
}

/// The warning of a held anvil with no hold socket, once a run.
fn warn_no_socket() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| eprintln!("warning: {ANVIL}: a holder with no hold socket: the held item is hidden"));
}
