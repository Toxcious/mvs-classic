//! Garnet's render module: the entry, the AnimBP's pose hook and the classes' mesh
//! rules. Her trails are AnimNotify_PlayVFX of her montages (the generic notify VFX).
//!
//! The pose beside the shared one (Garnet_AnimBP). The graph, input first (each node's
//! LinkID is its index among the class's AnimNode_ / MirrorNode properties):
//!   LinkedAnimLayer -> Slot DefaultSlot (Base) -> TwoWayBlend (IsOnGround: A the InAir slot, B the OnGround slot) ->
//!   TwoWayBlend_2 (AimDirection: A AimCenter, B AimUp) -> TwoWayBlend_1 (AimDirectionInverted: A that, B AimDown) ->
//!   cached PostAim -> Slot JumpSlot -> Slot DefaultSlot -> the mirror node (FacingLeft) -> cached MirroredAnim ->
//!   LocalToComponent -> ModifyBone_1 (fml_un_C_head) -> ModifyBone_12 (fml_un_C_grnt_face_att) -> ModifyBone
//!   (fml_un_C_grnt_Glasses) -> ModifyBone_15 (VB Spine) -> _14 / _13 (VB L_Hand / VB R_Hand) -> _11 .. _7 (the left
//!   thumb .. pinky) -> _6 .. _2 (the right thumb .. pinky) -> ComponentToLocal.
//!
//! Ported here:
//! - ModifyBone_1 / _12 (scale BMM_Additive in BCS_BoneSpace, Scale <- HeadScale x 0.9 every evaluation: the exposed
//!   input handlers @1313 / @1234): HeadScale is the shared step (the entry's head_scale, factor 1); the constant 0.9
//!   is a factor here, every tick (the product of two local scale factors is the same in either order).
//! - ModifyBone (fml_un_C_grnt_Glasses; scale BMM_Replace in BCS_ComponentSpace, Scale <- ComponentMax(HeadScale x 0.9
//!   x 1.2, (1, 1, 1)) @1392-1711, the CDO's (2, 2, 2) never shown): the bone's component-space scale becomes that,
//!   its local scale the value over its parent's component scale (the FTransform rule: componentwise products down
//!   the chain fml_un_COM -> fml_un_C_grnt_face_att, the head step's factor on face_att counted in), 1.08 at the
//!   default HeadScale of 1.
//! - ModifyBone_11 .. _2 (the ten finger roots fml_un_{L,R}_hand_{thumb,index,middle,ring,pinky}; scale BMM_Replace
//!   (0, 0, 0) in BCS_ComponentSpace, alpha <- HandScale): HandScale = GetCurveValue(showRightGlove) @2473-2514 in the
//!   update, the curve of the evaluation before; FAnimNode_SkeletalControlBase's ActualAlpha is the input clamped to
//!   [0, 1] and the node is skipped at or under ZERO_ANIMWEIGHT_THRESH; its blend of the component-space transforms
//!   by the alpha leaves the rotation and translation and scales the bone's component scale by 1 - alpha: its local
//!   scale times 1 - alpha, the finger's bones under it following. (The gloves cover the hands; the curve shrinks
//!   the fingers away inside them.)
//! - The Visor (SetSubmeshVisibility @2820, behind VisorActive): ToggleSubmeshVisibility(showGlasses, Visor,
//!   VisorVisible), threshold 0.9. VisorActive = AppliedSkin != C012_S02 (@572-632): the drawn skin's name, so
//!   C012_S02 never runs the toggle and keeps the Visor hidden. OnSkinApplied @2948 -> @572: VisorVisible false and
//!   SetSubMeshVisibility(Visor, false): the part is hidden from the start until the curve shows it. The AnimBP's own
//!   state of the part over the sim's (sub_hidden) until the sim changes the part. OnSetInitialVisibility @2954:
//!   VisorVisible false (a respawn).
//!   The curves: the clips' own (crate::curves::CurveTable) by the sequence the tick shows (the fighter's anim and
//!   anim time) and the view's crossfade.
//!
//! Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation (as Steven's
//! and Bugs's poses).
//! Not ported:
//! - ModifyBone_15 / _14 / _13 (virtual bones VB Spine / VB L_Hand / VB R_Hand: the view holds none).
//! - A check of the data's slot graph against the AnimBP's Slot nodes (a warning of the data only).
//!
//! The mirror node is the view's reflection; the ModifyBones sit after it, where a uniform scale is the same.
//! Deviation: BP_CharLights_Garnet: no match spawns a CharLights rig in the port, so nothing of it is drawn.

use mvs_sim::World;
use mvs_sim::actors::{ActorRef, ProjectileId};
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::{V3, compose_rows};

use super::{CharRender, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_view::{SkinnedActor, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, Rows};
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::skins::Variant;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // Garnet_AnimBP ModifyBone_1 (fml_un_C_head) and ModifyBone_12 (fml_un_C_grnt_face_att), scale Additive in bone
    // space <- HeadScale; the pose hook adds the AnimBP's constant 0.9.
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_1",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_12",
            bone: "fml_un_C_grnt_face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    pose: Some(garnet_pose),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    ("GarnetGauntletsProjectile_C", MeshRule { rot: None, mesh: Some(glove_mesh) }),
    ("GarnetGauntletsProjectileAir_C", MeshRule { rot: None, mesh: Some(glove_mesh) }),
];

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&["showRightGlove", "showGlasses"]);
const GLOVE: usize = 0;
const SHOW_GLASSES: usize = 1;

/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: f64 = 1e-5;
/// ToggleSubmeshVisibility @109 / @462.
const TOGGLE: f64 = 0.9;
/// @1234 / @1313 / @1392.
const HEAD_ADJ: f64 = 0.9;
/// @1480-1564.
const GLASSES_ADJ: f64 = 1.2;
/// ModifyBone_1 / _12 (the entry's head_scale).
const HEAD_BONES: [&str; 2] = ["fml_un_C_head", "fml_un_C_grnt_face_att"];
const GLASSES: &str = "fml_un_C_grnt_Glasses";
const FINGERS: [&str; 5] = ["thumb", "index", "middle", "ring", "pinky"];
/// The sub-mesh key.
const VISOR: &str = "Visor";
/// NotEqual_ObjectObject(SkinManager.AppliedSkin, SkinData'C012_S02') @572.
const NO_VISOR_SKIN: &str = "C012_S02";

/// `garnetPose`'s state on one view.
#[derive(Debug)]
struct GarnetPose {
    /// A fighter's view this tick (set ran, not slaved); PoseOf::slaved: no AnimBP of its own (set does nothing, no
    /// finger scale).
    live: bool,
    slaved: bool,
    visor_active: bool,
    head: Vec<usize>,
    fingers: Vec<usize>,
    glasses: Option<usize>,
    /// The glasses' ancestors, nearest first, and whether the head step scales each after this rule.
    chain: Vec<(usize, bool)>,
    visor_part: Option<usize>,
    /// Its tag in the sim's sub_hidden.
    visor_tag: Option<String>,
    /// The view's HeadScale of the tick.
    head_scale: f64,
    /// The last tick set() ran at (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    /// HandScale (CDO 0).
    hand_scale: f64,
    /// The curves: the sequence's (tgt), the evaluation's (cur) and the pose the running fade started from (from).
    tgt: [f64; 2],
    cur: [f64; 2],
    from: [f64; 2],
    fades: u32,
    /// VisorVisible.
    visor_visible: bool,
    /// The AnimBP's last write of the Visor (-1 none, 0 hidden, 1 shown): OnSkinApplied's hide at the start.
    own: i8,
    /// The sim's last visibility of it (-1 not seen).
    sim_vis: i8,
}

fn garnet_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let bone = |b: &str| {
        let i = view.rig.index(b);
        if i.is_none() {
            eprintln!("warning: Garnet pose: bone missing: {b}");
        }
        i
    };
    let head: Vec<usize> = HEAD_BONES.iter().filter_map(|b| bone(b)).collect();
    let fingers: Vec<usize> = FINGERS
        .iter()
        .flat_map(|n| [format!("fml_un_L_hand_{n}"), format!("fml_un_R_hand_{n}")])
        .filter_map(|b| bone(&b))
        .collect();
    let glasses = bone(GLASSES);
    let mut chain = Vec::new();
    if let Some(g) = glasses {
        let mut p = view.parents[g];
        while p >= 0 {
            let pi = p as usize;
            chain.push((pi, HEAD_BONES.contains(&view.rig.names[pi].as_str())));
            p = view.parents[pi];
        }
    }
    let visor_part = of.part_keys.iter().position(|k| k == VISOR);
    if visor_part.is_none() {
        eprintln!("warning: Garnet pose: the skin has no sub-mesh {VISOR}");
    }
    Box::new(GarnetPose {
        visor_active: !of.skin.name.eq_ignore_ascii_case(NO_VISOR_SKIN),
        head,
        fingers,
        glasses,
        chain,
        visor_part,
        head_scale: 1.0,
        last_tick: None,
        live: false,
        slaved: of.slaved,
        hand_scale: 0.0,
        tgt: [0.0; 2],
        cur: [0.0; 2],
        from: [0.0; 2],
        fades: 0,
        visor_tag: visor_part.and_then(|k| of.skin.part_tags.get(k).cloned().flatten()),
        visor_visible: false,
        own: 0,
        sim_vis: -1,
    })
}

/// A local scale times `k` (the f32 pose).
fn scale_by(p: &mut Pose, i: usize, k: f64) {
    for c in 0..3 {
        p.s[i * 3 + c] = (f64::from(p.s[i * 3 + c]) * k) as f32;
    }
}

impl GarnetPose {
    /// ToggleSubmeshVisibility(showGlasses, Visor, VisorVisible) behind VisorActive, on the curves of the evaluation
    /// before: its write (None: none).
    fn visor_toggle(&mut self) -> Option<bool> {
        if !self.visor_active {
            return None;
        }
        let v = self.cur[SHOW_GLASSES];
        let write = if v > TOGGLE && !self.visor_visible {
            Some(true)
        } else if self.visor_visible && v < TOGGLE {
            Some(false)
        } else {
            None
        };
        self.visor_visible = v > TOGGLE;
        write
    }
}

impl PoseRules for GarnetPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, p: &mut Pose, _dt: f64) {
        // ModifyBone_1 / _12: HeadScale x 0.9 (the head step adds HeadScale)
        for &i in &self.head {
            scale_by(p, i, HEAD_ADJ);
        }
        // ModifyBone (the glasses): the component-space scale replaced
        if let Some(g) = self.glasses {
            let k = self.head_scale;
            let target = (k * HEAD_ADJ * GLASSES_ADJ).max(1.0);
            let mut s = [1.0f64; 3];
            for &(b, is_head) in &self.chain {
                let h = if is_head { k } else { 1.0 };
                for (c, v) in s.iter_mut().enumerate() {
                    *v *= f64::from(p.s[b * 3 + c]) * h;
                }
            }
            for (c, v) in s.iter().enumerate() {
                p.s[g * 3 + c] = if *v != 0.0 { (target / v) as f32 } else { 0.0 };
            }
        }
        // ModifyBone_11 .. _2: the fingers' component scale by 1 - HandScale
        let a = self.hand_scale.clamp(0.0, 1.0);
        if self.live && a > ZERO_W {
            for &i in &self.fingers {
                scale_by(p, i, 1.0 - a);
            }
        }
        // this evaluation's curves, for the next update
        if cx.fade_count != self.fades {
            self.fades = cx.fade_count;
            self.from = self.cur;
        }
        let w = cx.fade_weight;
        for k in 0..self.cur.len() {
            self.cur[k] = if w >= 1.0 { self.tgt[k] } else { self.from[k] + (self.tgt[k] - self.from[k]) * w };
        }
    }
}

impl PoseHook for GarnetPose {
    /// `preview()`: a view with no fighter: HandScale stays 0 (HandScale @2473 runs only for a Character
    /// pawn owner, @2331). BindAnimationEvents' manager branch (@36-221, the owning actor's VisibilityManager) then
    /// its skin branch (@304-662): VisorActive by the applied skin, VisorVisible false and the Visor hidden. Its toggle
    /// runs in preview_step.
    // Unverified: BP_CharacterPreview's Comp_Pawn_SkinManager is found at the bind (without one the bind retries after
    // a Delay(0), @778, and the Visor is hidden a frame later).
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.hand_scale = 0.0;
        self.tgt = [0.0; 2];
        self.cur = [0.0; 2];
        self.from = [0.0; 2];
        self.visor_visible = false;
        if let Some(k) = self.visor_part {
            view.set_part_visible(k, false);
        }
    }

    /// The preview's update (as gizmo.rs's): SetSubmeshVisibility @2820 behind VisorActive tests only the
    /// VisibilityManager (the Fighter test @873 ends only its own branch): the Visor's toggle on the curves of the
    /// evaluation before, a write put on the part (the AnimBP is the part's only writer here), then the curves of the
    /// frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        if let (Some(on), Some(k)) = (self.visor_toggle(), self.visor_part) {
            view.set_part_visible(k, on);
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        self.live = !self.slaved;
        if !self.live {
            return;
        }
        let fi = w.fighter(f);
        self.head_scale = view.head_scale;
        // the respawn: SetupInitialVisibility's write of the parts and OnSetInitialVisibility (the view places no
        // dead fighter: a gap in the ticks is a death between)
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.own = -1;
            self.visor_visible = false;
        }
        self.last_tick = Some(tick);
        // BlueprintUpdateAnimation on the curves of the evaluation before: SetSubmeshVisibility @2056 (the toggle
        // runs only behind VisorActive, @2820), then HandScale @2473
        if let Some(on) = self.visor_toggle() {
            self.own = i8::from(on);
        }
        if let Some(k) = self.visor_part {
            // the AnimBP's write until the sim changes the part's visibility (the later write)
            let hidden =
                self.visor_tag.as_deref().and_then(|t| w.names.get(t)).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            let vis = if hidden { 0 } else { 1 };
            if self.sim_vis >= 0 && self.sim_vis != vis {
                self.own = -1;
            }
            self.sim_vis = vis;
            view.set_part_visible(k, if self.own >= 0 { self.own == 1 } else { vis == 1 });
        }
        self.hand_scale = self.cur[GLOVE];
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    fn shown(&self, tag: &str, sim: bool) -> bool {
        if tag.eq_ignore_ascii_case(VISOR) && self.own >= 0 { self.own == 1 } else { sim }
    }
}

// ---- the thrown gloves (GarnetGauntletsProjectile_C / GarnetGauntletsProjectileAir_C) ----
//
// LeftGauntlet / RightGauntlet (Garnet_GloveL / GloveR_SkelMesh, at (-420, 0, -300) / (-420, 0, -310), scale 1.4,
// under the Capsule root). SetTeamColor @5264-6399: SetMaterial(0, SelectMaterialColor(the LeftGauntlet sub-mesh's
// Materials of the owner's applied skin)) on both, so the right glove wears the left one's slot 0; @6720 / @6762
// PlayAnimation(Garnet_Attack_Special_F_GauntletsMidLaunchLoop, loop) on both, from the actor's first tick.
// Unverified: SetTeamColor runs at BeginPlay, the clip's time is the actor's age.
// Deviation: no first draw of the gloves is made under the loading screen:
// the match warm-up makes their GPU objects instead (ViewPoser::warm_meshes, mvs-render Renderer::prewarm_match).

const GLOVE_LOOP: &str = "Garnet_Attack_Special_F_GauntletsMidLaunchLoop";
const GLOVES: [&str; 2] = ["LeftGauntlet", "RightGauntlet"];
const STEP: f64 = 1.0 / 60.0;

#[derive(Debug)]
struct GloveView {
    view: SkinnedActor,
    rel: V3,
    rot: Rows,
    scale: V3,
}

#[derive(Debug)]
struct GloveMesh {
    /// The class data's owner (the clips' character).
    owner: FighterId,
    id: ProjectileId,
    views: Vec<GloveView>,
}

fn glove_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let own = pr.owner?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, own, pr.cls);
    let pick =
        usize::try_from(w.fighter(from).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let skin = crate::skins::skin_of(&w.char_data(from).fighter, pick).ok()?;
    // (the instigator's pick of the owner's applied skin's material sets)
    let variant = match host.colour(w, own).variant {
        Variant::Menu => Variant::Local,
        v => v,
    };
    let d = host.draws_of(w, from)?;
    let part = |key: &str| skin.parts.iter().find(|p| p.key.eq_ignore_ascii_case(key));
    let set_of = |key: &str| -> Option<Vec<Option<String>>> {
        let p = part(key)?;
        if p.own { d.actor(&p.mesh, None).map(|a| a.1) } else { d.set(&skin.name, &p.key, variant).ok() }
    };
    let Some(left) = set_of(GLOVES[0]).filter(|l| !l.is_empty()) else {
        eprintln!("warning: Garnet: no material set of the LeftGauntlet sub-mesh (the thrown gloves are not drawn)");
        return None;
    };
    let mut views = Vec::new();
    for key in GLOVES {
        let (Some(c), Some(p)) = (cs.get(key), part(key)) else {
            eprintln!("warning: Garnet: the thrown glove {key} is not drawn (no component or sub-mesh)");
            continue;
        };
        // SetMaterial(0, the left glove's pick) on both; the other slots the mesh's own set
        let keys = if key == GLOVES[0] {
            left.clone()
        } else {
            let mut k = set_of(key).unwrap_or_default();
            match k.first_mut() {
                Some(s) => *s = left[0].clone(),
                None => k.push(left[0].clone()),
            }
            k
        };
        let Some(info) = p.path.as_deref().and_then(|m| host.mesh(m)) else { continue };
        let par = &c.parent;
        let pr_rot: Rows = std::array::from_fn(|k| par.rot[k]).map(f64::from);
        let ps = par.scale;
        let d: V3 =
            std::array::from_fn(|j| (0..3).map(|i| f64::from(ps[i]) * c.loc[i] * pr_rot[i * 3 + j]).sum::<f64>());
        let rel = std::array::from_fn(|k| f64::from(par.loc[k]) + d[k]);
        views.push(GloveView {
            view: SkinnedActor::new(info, &keys),
            rel: rel.map(f64::from),
            rot: compose_rows(&c.rot, &pr_rot),
            scale: std::array::from_fn(|k| f64::from(ps[k]) * c.scale[k]),
        });
    }
    if views.is_empty() {
        return None;
    }
    if host.clips_of(w, from).is_none_or(|c| c.get(GLOVE_LOOP).is_none()) {
        eprintln!("warning: Garnet: clip missing: {GLOVE_LOOP}");
    }
    Some(Box::new(GloveMesh { owner: from, id, views }))
}

impl ActorMesh for GloveMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let age = w.actors.projectiles.get(self.id).map_or(0, |p| p.age);
        let (x, y) = at.at(alpha);
        for g in &mut self.views {
            g.view.visible = shown;
            if !shown {
                continue;
            }
            let r: V3 = std::array::from_fn(|j| at.scale * (0..3).map(|i| g.rel[i] * at.rot[i * 3 + j]).sum::<f64>());
            let loc = [r[0], -x + r[1], y + r[2]];
            let scale = std::array::from_fn(|k| at.scale * g.scale[k]);
            g.view.set(&Xform { loc, rot: compose_rows(&g.rot, &at.rot), scale });
            if let Some(clips) = host.clips_of(w, self.owner)
                && let Some(c) = clips.get(GLOVE_LOOP)
            {
                g.view.play(clips, c, f64::from(age) * STEP, true);
            }
            g.view.draw(out);
        }
    }
}
