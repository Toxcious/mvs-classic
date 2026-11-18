//! Velma's render module: the entry, the AnimBP's pose hook, the
//! pawn props, the evidence's clue mesh and the book's mesh. Her effects are mvs-render's fx/chars/velma.rs. Her
//! police: the car's minion view and the VelmaJenkinsPin's mesh, below; their effects mvs-render's
//! fx/chars/velma_police.rs.
//!
//! The head scale: Velma_AnimBP's ModifyBone_8 (fml_un_C_head) and ModifyBone_5 (fml_un_C_Face_att), bone space,
//! scale HeadScale, no gate. On Facing Changed (BPI_SetIsFacingLeft @8311): RelativeScale3D (1, -1, 1) facing left on
//! every VisMesh component (BodyMesh alone): the view's turns rule. The Book is a skin sub-mesh child of Body without
//! the VisMesh tag, posed as a copy under the one scale (copied_props, as Rick's).
//!
//! The pose hook: Velma_AnimBP's sub-mesh toggles.
//! BlueprintInitializeAnimation (@8196): the init chain @113..@617 takes the owner's Comp_FighterVisibilityManager and
//! hides every entry of Props (["None", Megaphone, Flashlight, MagnifyingGlass, Vial, Camera, Lightbulb, Evidence,
//! Tablet, Calculator, Book]) with SetSubMeshVisibility(tag, false) @561; no manager: Delay(0.0) @618 and again.
//! BlueprintUpdateAnimation (@8162) -> SetPropVisibility (@8181 -> ub @8335): ToggleSubmeshVisibility(curve, tag,
//! state) for megaphone @8385, then (the push_flow stack @8335..@8380 unwinds 8541 before 8463) flashlight @8541,
//! magnifying glass @8463, vial @8619, camera @8697, lightbulb @8775, evidence @8853, tablet @8931, calculator @9009,
//! book @9087; then TogglePropVisibility(Props, prop1PropActive, ActivePropIndex) @9165.
//! ToggleSubmeshVisibility (@0..553): behind VisibilityManager valid; curve > 0.9 and the state false, or the state
//! true and curve < 0.9: SetSubMeshVisibility(tag, curve > 0.9) @266; the state becomes curve > 0.9 either way (@401;
//! the call's success is not read). Threshold 0.9: a curve of exactly 0.9 leaves a shown mesh shown.
//! TogglePropVisibility (@0..569): n = Round(curve); n != ActiveIndex: SetSubMeshVisibility(Props[ActiveIndex], false)
//! @169, SetSubMeshVisibility(Props[n], true) @309, ActiveIndex = n (@487, CDO 0). Props[0] is None (no mesh); an
//! index past the array reads None (Array_Get's default) and is taken here as no mesh.
//! Both functions write the same tags (Megaphone is shown by prop1PropActive 1 in Velma_Special_MegaphoneBeam, which
//! carries no show_vlma_megaphone key): the manager's last write wins, as the `own` writes below.
//! SetSubMeshVisibility is the manager's (Comp_FighterVisibilityManager): shown is drawn only at InvisibilityCount 0;
//! the view's parts-only mode (an AddInvisibility) keeps the sim's AlwaysVisibleMeshes alone (as Taz's).
//! The AnimBP's write holds until the sim changes that mesh's visibility (a notify, SetupInitialVisibility at the
//! respawn), as Taz's: the sim's changes of the tick come first, then this update's writes.
//! The curves: the clips' own (crate::curves) by the sequence the tick shows, the slot mix's tracks
//! (VelmaPawnState.slot_mix) lerped in by their weights, and the view's crossfade.
//! A respawn keeps the pawn and its AnimBP: Velma_AnimBP's CustomEvent (@15, bound in BindAnimationEvents) sets Is
//! Respawning for 0.1 s alone (the dynamics' alphas), so the prop states are kept and the respawn's
//! SetupInitialVisibility is the sim's write (the later one until the AnimBP's next change).
//!
//! The pawn props: Pawn_Velma's StaticMeshComponents, children of CharacterMesh0 on the socket fml_un_C_prop1, each
//! bVisible false in the SCS, shown by the pose hook's toggles (`shown`): Megaphone, Flashlight, MagnifyingGlass,
//! Vial, Camera, Lightbulb (yaw 90, tags Lightbulb and NoOutline), Evidence, Tablet, Calculator. The skin's static
//! SubMeshes give them their meshes and materials (keyed 'Velma.<key>' in the draws), with SelectMaterialColor's pick
//! (selected_variant). CharacterMesh0 is not scaled by OnFacingChanged (no VisMesh tag): the props are on the view's
//! bone, the plain reflection facing left, as Taz's.
//!
//! Deviations and unverified models:
//!   - Deviation: the AnimBP's writes are the view's: they do not reach the sim's sub_hidden (as Taz's and Rick's).
//!   - Unverified: a curve's value in a crossfade and under the slot mix, and the one tick the update lags the
//!     evaluation: as Taz's and Rick's.
//!   - Deviation: the init's hide is taken at the pawn's first update (and at the view's reset: a Lab reset's new
//!     pawn).
//!   - Unverified: the props' socket is the last rendered pose's (as Rick's, Bugs' and Taz's).
//!   - The props draw custom depth with the fighter's stencil but the Lightbulb (NoOutline: chars::outlined).
//!   - Deviation: Velma_AnimBP's AnimDynamics (23 nodes) and Trail nodes (5) are not ported, as no character's are;
//!     the witch hat's ModifyBones and curves belong to other skins; the VB ModifyBones move virtual bones the view
//!     does not hold.
//!   - Deviation: BP_CharLights_Velma is not drawn: no match spawns a CharLights rig.
//!   - a preview actor (the podium, the menus) runs the same toggles from its first posed frame, as Taz's
//!     (PoseHook::preview_step).
//!   - not ported: the check of the data's slot graph against the AnimBP's Slot nodes, the warm-up lists (the props'
//!     and actors' warm-ups). The reset (a new pawn on one view) is PoseHook::reset.

use mvs_data::fighter::FighterData;
use mvs_sim::World;
use mvs_sim::actors::actor_montage::montage_of;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef, ProjectileId};
use mvs_sim::chars::velma::police::{JENKINS_PIN, JenkinsPin, POLICE_DATA};
use mvs_sim::chars::velma::state::velma_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::names::Name;
use mvs_vfx::math::{Rows, V3, compose_rows, rotation_rows};

use super::{CharRender, MinionView, PawnActor, PawnCx, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_anim::{
    ActorMachine, ActorSkeleton, ActorSlot, BlendOption, MachineDef, MachineQuery, PoseDef, SeqDef, StateDef,
    TransitionDef,
};
use crate::actor_view::{IDENT, SkinnedActor, StaticView, Xform, rel_rows, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule};
use crate::anim::{Clips, HeadScaleNode, Pose};
use crate::curves::{CurveKey, CurveTable, rich_curve};
use crate::frame::{ActorPose, MeshPose};
use crate::pose::Rec;
use crate::skins::{Skin, Variant};
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_8",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_5",
            bone: "fml_un_C_Face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    turns: true,
    copied_props: true,
    pose: Some(velma_pose),
    pawn_actors: Some(pawn_props),
    minions: &[POLICE_CAR],
    ..CharRender::GENERIC
};

/// The companion's entry (`velmapolice`: a key of Velma's match, never a player's character): no rule of
/// its own; its view is Velma's minion view (POLICE_CAR).
pub(crate) const POLICE_RENDER: CharRender = CharRender::GENERIC;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (mvs_sim::chars::velma::evidence::EVIDENCE, MeshRule { rot: None, mesh: Some(clue_mesh) }),
    (mvs_sim::chars::velma::evidence::EVIDENCE_NO_GRAV, MeshRule { rot: None, mesh: Some(clue_mesh) }),
    (mvs_sim::chars::velma::gadgets::BOOK, MeshRule { rot: None, mesh: Some(book_mesh) }),
    (JENKINS_PIN, MeshRule { rot: None, mesh: Some(pin_mesh) }),
];

/// The float curves its AnimBP reads by name (GetCurveValue).
const CURVES: CurveTable = CurveTable::new(&[
    "show_vlma_megaphone",
    "show_vlma_magnifyingGlass",
    "show_vlma_flashlight",
    "show_vlma_vial",
    "show_vlma_camera",
    "show_vlma_lightbulb",
    "show_vlma_evidence",
    "show_vlma_tablet",
    "show_vlma_calculator",
    "show_vlma_book",
    "prop1PropActive",
]);
const NC: usize = 11;
/// ToggleSubmeshVisibility's (curve, tag), in the update's order.
const TOGGLES: [(usize, &str); 10] = [
    (0, "megaphone"),
    (2, "flashlight"),
    (1, "magnifyingglass"),
    (3, "vial"),
    (4, "camera"),
    (5, "lightbulb"),
    (6, "evidence"),
    (7, "tablet"),
    (8, "calculator"),
    (9, "book"),
];
const PROP_CURVE: usize = 10;
/// The CDO's Props past its None (lower case, as the manager files the tags): Props[n] is TAGS[n - 1].
const TAGS: [&str; 10] = [
    "megaphone",
    "flashlight",
    "magnifyingglass",
    "vial",
    "camera",
    "lightbulb",
    "evidence",
    "tablet",
    "calculator",
    "book",
];
/// The pawn's static props (pawn_props): their tags have a mesh in the manager though the view holds no part of them.
const PROP_TAGS: [&str; 9] =
    ["megaphone", "flashlight", "magnifyingglass", "vial", "camera", "lightbulb", "evidence", "tablet", "calculator"];

/// `velmaPose`'s state on one view.
#[derive(Debug)]
struct VelmaPose {
    /// Per tag (TAGS): the view's part (None: not a part) and whether the manager has a mesh of it.
    part_of: [Option<usize>; 10],
    has_mesh: [bool; 10],
    /// ToggleSubmeshVisibility's tag per toggle.
    st_tag: [usize; 10],
    tgt: [f64; NC],
    cur: [f64; NC],
    from: [f64; NC],
    fades: u32,
    /// The *Visible variables.
    state: [bool; 10],
    /// ActivePropIndex.
    active: i64,
    /// The init's hide of Props done.
    inited: bool,
    /// Per tag: the AnimBP's last write (None: none), the sim's last visibility of the mesh (None: not seen), whether
    /// the sim has it in AlwaysVisibleMeshes, and whether a hide of the AnimBP took it out of that set since (cut).
    own: [Option<bool>; 10],
    sim_vis: [Option<bool>; 10],
    sim_av: [bool; 10],
    cut: [bool; 10],
    live: bool,
    /// PoseOf::slaved: no AnimBP of its own (set does nothing).
    slaved: bool,
    /// PoseOf::stand_in: no character state (no slot mix).
    stand_in: bool,
    /// Per tag: the AnimBP wrote it in this update (a preview's parts).
    wrote: [bool; 10],
    /// A preview's next step is its first (its curves seeded from that frame).
    preview_first: bool,
}

fn velma_pose(_view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let part_of = TAGS.map(|t| of.part_keys.iter().position(|k| k.eq_ignore_ascii_case(t)));
    let mut has_mesh = [false; 10];
    for (k, t) in TAGS.iter().enumerate() {
        has_mesh[k] = part_of[k].is_some() || PROP_TAGS.contains(t);
    }
    let st_tag = TOGGLES.map(|(_, t)| TAGS.iter().position(|x| *x == t).expect("a toggle's tag"));
    Box::new(VelmaPose {
        part_of,
        has_mesh,
        st_tag,
        tgt: [0.0; NC],
        cur: [0.0; NC],
        from: [0.0; NC],
        fades: 0,
        state: [false; 10],
        active: 0,
        inited: false,
        own: [None; 10],
        sim_vis: [None; 10],
        sim_av: [false; 10],
        cut: [false; 10],
        live: false,
        slaved: of.slaved,
        stand_in: of.stand_in,
        wrote: [false; 10],
        preview_first: false,
    })
}

impl VelmaPose {
    /// SetSubMeshVisibility on tag `t` (None: the None entry, no mesh).
    fn write(&mut self, t: Option<usize>, vis: bool) {
        let Some(t) = t else { return };
        self.own[t] = Some(vis);
        self.wrote[t] = true;
        if !vis {
            self.cut[t] = true;
        }
    }

    /// Props[n]'s tag.
    fn prop_tag(n: i64) -> Option<usize> {
        (1..=TAGS.len() as i64).contains(&n).then(|| (n - 1) as usize)
    }

    /// SetPropVisibility on the curves of the evaluation before.
    fn update(&mut self) {
        if !self.inited {
            self.inited = true;
            for t in 0..TAGS.len() {
                if self.has_mesh[t] {
                    self.write(Some(t), false);
                }
            }
        }
        for (k, (c, _)) in TOGGLES.iter().enumerate() {
            let v = self.cur[*c];
            let vis = v > 0.9;
            let was = self.state[k];
            if (vis && !was) || (was && v < 0.9) {
                self.write(Some(self.st_tag[k]), vis);
            }
            self.state[k] = vis;
        }
        // Round: half away from zero (floor(x + 0.5) for the curve's non-negative keys)
        let n = (self.cur[PROP_CURVE] + 0.5).floor() as i64;
        if n != self.active {
            self.write(Self::prop_tag(self.active), false);
            self.write(Self::prop_tag(n), true);
            self.active = n;
        }
    }

    /// A preview's parts: those the AnimBP wrote in this update; the others as the preview's manager has them.
    fn preview_parts(&mut self, view: &mut CharView) {
        for t in 0..TAGS.len() {
            if let (true, Some(p)) = (self.wrote[t], self.part_of[t]) {
                view.set_part_visible(p, self.own[t] == Some(true));
            }
        }
        self.wrote.fill(false);
    }
}

impl PoseRules for VelmaPose {
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

impl PoseHook for VelmaPose {
    /// `preview()`: a preview actor's view (no fighter): the AnimBP anew, run by preview_step.
    fn preview(&mut self, view: &mut CharView, _of: &PreviewOf<'_>) {
        self.live = false;
        self.reset(&mut |_, _, _| {});
        self.wrote = [false; 10];
        self.fades = view.fade_count();
        self.preview_first = true;
    }

    /// `previewStep(anim, time)`: SetPropVisibility on the curves of the evaluation before, the parts the
    /// update wrote, then the curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        if std::mem::take(&mut self.preview_first) {
            CURVES.fill(clips, s.anim, s.time, &mut self.cur);
            self.from = self.cur;
        }
        self.update();
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
        let name = |t: &str| w.names.get(t);
        for (t, tag) in TAGS.iter().enumerate() {
            let n = name(tag);
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
        self.update();
        self.wrote = [false; 10];
        let kept = if fi.vis.invisible { fi.vis.invis_shown.as_ref() } else { fi.vis.always_visible.as_ref() };
        for (t, tag) in TAGS.iter().enumerate() {
            let Some(p) = self.part_of[t] else { continue };
            let on = if view.parts_only {
                kept.is_some_and(|s| name(tag).is_some_and(|n| s.contains(&n))) && !self.cut[t]
            } else {
                self.own[t].unwrap_or(self.sim_vis[t] == Some(true))
            };
            view.part_on[p] = on;
        }
        let anim = w.names.str(fi.anim.anim);
        CURVES.fill(clips, anim, f64::from(fi.anim.anim_time), &mut self.tgt);
        if let Some(s) = velma_of(w, f).filter(|_| !self.stand_in) {
            for x in &s.pawn.slot_mix.slot_mix {
                let clip = clips.get(w.names.str(x.anim));
                for k in 0..NC {
                    self.tgt[k] += (CURVES.value_of(clips, clip, k, f64::from(x.time)) - self.tgt[k]) * f64::from(x.w);
                }
            }
        }
    }

    /// `init()`: a new pawn, the AnimBP's state anew.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        // (the crossfade's count needs no reset: cur and from are both 0)
        self.tgt = [0.0; NC];
        self.cur = [0.0; NC];
        self.from = [0.0; NC];
        self.state = [false; 10];
        self.active = 0;
        self.inited = false;
        self.own = [None; 10];
        self.sim_vis = [None; 10];
        self.sim_av = [false; 10];
        self.cut = [false; 10];
    }

    /// Whether the mesh of a tag (lower case) is shown: the AnimBP's last write while the sim has not changed the
    /// mesh since, else the sim's.
    fn shown(&self, tag: &str, sim: bool) -> bool {
        match TAGS.iter().position(|t| *t == tag) {
            Some(t) if self.live => self.own[t].unwrap_or(sim),
            _ => sim,
        }
    }
}

/// The props by the skin's SubMeshes key (the pawn's component `<key>_GEN_VARIABLE`).
const PROPS: [&str; 9] =
    ["Megaphone", "Flashlight", "MagnifyingGlass", "Vial", "Camera", "Lightbulb", "Evidence", "Tablet", "Calculator"];

/// One pawn prop: its tag, its view and its attach.
#[derive(Debug)]
struct PawnProp {
    tag: String,
    view: StaticView,
    socket: String,
    rel: Xform,
    /// Not tagged NoOutline (chars::outlined).
    outlined: bool,
}

/// `pawnProps`: fighter `f`'s props, with the default skin's mesh and materials ('Velma.<key>').
fn pawn_props(host: &ActorHost<'_>, w: &World, f: FighterId) -> Vec<Box<dyn PawnActor>> {
    let d = &w.char_data(f).fighter;
    let mut out: Vec<Box<dyn PawnActor>> = Vec::new();
    for key in PROPS {
        let c = d.components.pawn.get(&format!("{key}_GEN_VARIABLE"));
        // (propLook: the default skin's own entry, another skin's static entry of the component's tag, else the
        // template's mesh in its own materials)
        let st = if host.on_default_skin(w, f) {
            Some((format!("Velma.{key}"), host.selected_variant(w, f)))
        } else {
            c.and_then(|c| c.p.as_obj()).and_then(|p| host.skin_static(w, f, p))
        };
        let view = st.and_then(|(k, variant)| StaticView::new(host, w, f, &k, variant));
        let (Some(c), Some(view)) = (c, view) else {
            eprintln!("warning: Velma: the prop {key} is not drawn (its component, mesh or materials are not loaded)");
            continue;
        };
        let p = c.p.as_obj();
        let rel = Xform {
            loc: xyz(p.and_then(|p| p.get("RelativeLocation")), 0.0),
            rot: rel_rows(p.and_then(|p| p.get("RelativeRotation"))),
            scale: xyz(p.and_then(|p| p.get("RelativeScale3D")), 1.0),
        };
        let socket = c.socket.clone().unwrap_or_else(|| "None".to_owned());
        out.push(Box::new(PawnProp { tag: key.to_lowercase(), view, socket, rel, outlined: super::outlined(c) }));
    }
    out
}

impl PawnActor for PawnProp {
    /// `placePawnProps`: shown by the AnimBP's write while the sim has not changed the mesh since, else the sim's.
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, _alpha: f64, out: &mut Vec<ActorPose>) {
        if !cx.shown(w, &self.tag) {
            return;
        }
        let Some(at) = host.socket(w, cx.fid, &self.socket, &self.rel) else { return };
        self.view.set(&at);
        let mut meshes = Vec::new();
        self.view.draw(&mut meshes);
        cx.outline(self.outlined, &mut meshes);
        out.push(ActorPose { meshes, ..ActorPose::default() });
    }
}

// ---- the evidence ----
//
// VelmaEvidence_C / VelmaEvidenceNoGrav_C (mvs-sim chars/velma/evidence.rs): EvidenceStaticMesh (SM_VelmaClue at Z 1,
// yaw 180, scale 150 on the Capsule) whose material Velma_Clues_3x3_MESH_MAT_Inst gets Team = SelectFloat(0.6, the
// owner's colour index, IsLocalOnline) @1468 and Frame = RandomFloatInRange(0, 0.5) @1560 (Init Visuals). The actor's
// yaw is 90 (the spawns' MakeTransform), the Capsule's roll the sim's spin (rotation about the world X: the body's
// Y / Z rotations are locked). Its PS_TeamGlow is the effects' (fx/chars/velma.rs).
// Deviation: Frame is a random draw of the build's stream at BeginPlay; drawn here from the actor's id (render side,
// no sim stream).
// Placed on the tick's place, once a tick.

const CLUE_MESH: &str = "SM_VelmaClue";

/// Frame: a value in [0, 0.5) from the actor's id (the Deviation above).
fn frame_of(id: i32) -> f64 {
    let h = (id.wrapping_add(1) as u32).wrapping_mul(2_654_435_761);
    f64::from(h % 1000) / 1000.0 * 0.5
}

/// The evidence's place, spin and root scale.
fn evidence_of(w: &World, id: ActorId) -> Option<(f64, f64, f64, f64)> {
    let a = w.actors.actors.get(id)?;
    match &a.state {
        WorldActorState::Evidence(e) => {
            Some((f64::from(e.x), f64::from(e.y), f64::from(e.spin), f64::from(a.size_scale.unwrap_or(1.0))))
        }
        _ => None,
    }
}

#[derive(Debug)]
struct ClueMesh {
    id: ActorId,
    view: StaticView,
    loc: V3,
    rot: Rows,
    scale: V3,
}

fn clue_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let c = cs.get("EvidenceStaticMesh")?;
    // (the class data and meshes: the owner's character, a Velma's)
    let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
    let mut view = StaticView::new(host, w, from, CLUE_MESH, host.selected_variant(w, from))?;
    view.set_scalar("Team", host.colour(w, x.owner).team);
    view.set_scalar("Frame", frame_of(x.seq));
    Some(Box::new(ClueMesh { id, view, loc: c.loc, rot: c.rot, scale: c.scale }))
}

impl ActorMesh for ClueMesh {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        _alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((x, y, spin, g)) = evidence_of(w, self.id) else { return };
        self.view.visible = shown;
        if !shown {
            return;
        }
        let xf = clue_place(x, y, spin, g, self.loc, &self.rot, self.scale);
        self.view.set(&xf);
        self.view.draw(out);
    }
}

/// The mesh's place: the root at yaw 90, rolled about the world X by the Capsule's spin (degrees), at the root's
/// scale `g`; the mesh's relative place, rotation and scale under it.
pub(crate) fn clue_place(x: f64, y: f64, spin: f64, g: f64, rel: V3, rel_rot: &Rows, scale: V3) -> Xform {
    let yaw = rotation_rows(0.0, std::f64::consts::FRAC_PI_2, 0.0);
    let mut root = compose_rows(&yaw, &rotation_rows(0.0, 0.0, spin.to_radians()));
    if g != 1.0 {
        for v in &mut root {
            *v *= g;
        }
    }
    let mut loc = [0.0, -x, y];
    for (k, l) in loc.iter_mut().enumerate() {
        *l += rel[0] * root[k] + rel[1] * root[3 + k] + rel[2] * root[6 + k];
    }
    Xform { loc, rot: compose_rows(rel_rot, &root), scale }
}

// ---- the book ----
//
// VelmaBookProjectile_C SkeletalMesh: Velma_Book_SkelMesh at relative scale 2, AnimationSingleNode with AnimToPlay
// Book_Flying_Loop (looping and playing from the spawn: the AnimationData defaults). ReceiveBeginPlay @1848-2069: the
// owner facing left (BPI_IsFacingLeft) negates the mesh's relative Y scale. The asset is the owner's skin sub-mesh of
// that name, its local material set: SetProperTeamColor's Red / Blue / LocalPlayer SetMaterial @469 runs only with the
// world's useTeamColoredSkins on (@3769), which no port match sets. The rotation is the shared path's for a skeletal
// actor mesh (look_yaw, look_pitch); the clip's position counts the actor's ticks from its first (1 / 60 s each).
// Unverified: the facing read at BeginPlay taken as the projectile's spawn facing (the owner's at the spawn); the
// clip's start at the actor's first tick, not read.

#[derive(Debug)]
struct BookMesh {
    id: ProjectileId,
    from: FighterId,
    view: SkinnedActor,
    clip: Option<usize>,
    base: V3,
}

fn book_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let c = cs.components.iter().find(|c| matches!(c.kind, ComponentKind::SkeletalMesh { .. }))?;
    let ComponentKind::SkeletalMesh { sub_mesh, anim, .. } = &c.kind else { return None };
    let from = mvs_sim::fighter::common::actor_def_owner(w, pr.owner?, pr.cls);
    let pick =
        usize::try_from(w.fighter(from).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let skin = crate::skins::skin_of(&w.char_data(from).fighter, pick).ok()?;
    let part = skin.parts.iter().find(|p| sub_mesh.as_ref().is_some_and(|k| p.key.eq_ignore_ascii_case(k)))?;
    let info = host.mesh(part.path.as_deref()?)?;
    let keys = host.draws_of(w, from).and_then(|d| {
        if part.own {
            d.actor(&part.mesh, None).map(|a| a.1)
        } else {
            d.set(&skin.name, &part.key, Variant::Local).ok()
        }
    })?;
    let anim = anim.as_deref().unwrap_or("None");
    let clip = host.clips_of(w, from).and_then(|cl| cl.get(anim));
    if clip.is_none() {
        eprintln!("warning: Velma: clip missing for the book: {anim}");
    }
    let flip = if pr.facing < 0.0 { -1.0 } else { 1.0 };
    Some(Box::new(BookMesh {
        id,
        from,
        view: SkinnedActor::new(info, &keys),
        clip,
        base: [c.scale[0], c.scale[1] * flip, c.scale[2]],
    }))
}

impl ActorMesh for BookMesh {
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
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        if let (Some(c), Some(clips)) = (self.clip, host.clips_of(w, self.from)) {
            self.view.play(clips, c, f64::from((pr.age - 1).max(0)) / 60.0, true);
        }
        let (x, y) = at.at(alpha);
        let scale = self.base.map(|v| v * f64::from(pr.scale));
        let rot = rotation_rows(f64::from(pr.look_pitch.to_radians()), f64::from(pr.look_yaw.to_radians()), 0.0);
        self.view.set(&Xform { loc: [0.0, -x, y], rot, scale });
        self.view.draw(out);
    }
}

// ---- the police car ----
//
// The car (Pawn_VelmaAIBotPolice, a World minion of the data-only key velmapolice): one view per Velma, made with the
// match (MinionView), shown while a car she called lives.
//   - Mesh: CharacterMesh0 (Velma_MysteryMachine_SkelMesh, RelativeLocation Z -180, scale 0.9: the data's boxes).
//   - Materials: StartupVisuals @5891 -> @5598 / @4578 (the siren instances, below) -> @4216 ForcedColorIndex =
//     FTrunc(SelectFloat(2.0, the owner's BPI_GetColorValue, its IsLocalOnline)) -> @1830 loop, Mesh.SetMaterial(0,
//     SelectMaterialColor(AppliedSkin.Materials[i])): the skin VelmaAIBotPolice has one entry (MysteryMachine Red /
//     Blue / Local), so slot 0 is the owner's red / blue, or the Local MIC when the owner is the local online player
//     (MinionView.local_set); slot 1 keeps its own Velma_PoliceCarWindow_MAT.
//   - The siren instances: CreateDynamicMaterialInstance(2, Velma_CopCar_Siren_Blue_MAT_Inst) @5598 and (3,
//     Velma_CopCar_Siren_INV_MAT_Inst) @4578 with their Team scalars: the mesh has two material slots, so both name
//     no slot and nothing draws them.
//   - Outline: SetOutline @15-314: ForcedColorIndex = FTrunc(ColorValue), SetStencilValue switch 0 EnemyOutline (3) /
//     1 Ally Outline (2), BaseFighter CDO values (the minion view's stencil).
//   - Damage: OnTakeDamage @3340 Mesh.SetMorphTarget(Damage_Adj, EaseOutCurve(CurrentDamage / DamageToKill)) @3467
//     and SetScalarParameterValueOnMaterials(DamageAmount, the same) @3649 (CRV_EaseOut, DamageToKill 18: the pawn's
//     defaults). The value is a function of the minion's damage: set from its damage each tick (car_place).
//   - DestroyVisuals @3959-4142 and the car's hit effects and dust: the effects' (mvs-render fx/chars/velma_police.rs).
//   - PS_VelmaCopCar / PS_VelmaCopCar02 (fml_un_R / L_flasher): bVisible false in the SCS and no graph of the class
//     shows them (Polymorph @5763 hides them again): not drawn.
//   - Deviation: Silhouette (Plane, VelmaPoliceCarPerson_MAT on fml_un_C_Body) is not drawn: no material of the
//     port's lists builds it.
//   - Deviation: Damage_Adj and DamageAmount are set from the minion's damage on every tick it is shown,
//     so a new car shows EaseOut(0) = 0.666 of the morph; the build sets both in OnTakeDamage alone, so a car not yet
//     hit has the morph off and the materials' own DamageAmount.
//   - Its montages (the Capture montage @1907) on the view's own montage path.
// The velma-police match spawns the car and pins Shaggy: its pose tape (ticks 11300, 11400, 11450) checks the car's
// Navigation machine, the Capture montage and the pin below; its damage stays 0, so no damage but EaseOut(0) is drawn.
//
// Velma_PoliceCar_AnimBP (its AnimGraph as decoded): Slot DefaultSlot (bAlwaysUpdateSourcePose)
// over the state machine Navigation (MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, InitialState 0 Stop),
// every crossfade HermiteCubic. Its inputs: SetHorizontalSpeed @0-270 (|Velocity.Y|, the CMC's IsFalling: the sim's
// |vx| and !grounded) and OnIsJumping (the Fighter's broadcast, cmd_jumped). The machine runs on the view's skeleton
// and stands in for the view's clip (MinionView.nav, `sample`); the montage over it is the view's slot blend.

const STEP: f64 = 1.0 / 60.0;
const ZERO_W: f64 = 1e-5;
/// Pawn_VelmaAIBotPolice DamageToKill.
const DAMAGE_TO_KILL: f64 = 18.0;
/// CRV_EaseOut (Characters/Velma/Cars): (-1, 0) cubic, user tangents 0.6636736; (1, 1.0000001) auto, tangents 0.
const EASE_OUT: [CurveKey; 2] = [[-1.0, 0.0, 0.6636736, 0.6636736, 1.0], [1.0, 1.0000001, 0.0, 0.0, 1.0]];
/// The clip the view samples while the machine draws (its key constant).
const CAR_IDLE: &str = "Velma_PoliceCar_Idle";

const POLICE_CAR: MinionView = MinionView {
    key: POLICE_DATA,
    skin: car_skin,
    mesh_scale: None,
    local_set: true,
    renew: true,
    head_scale: false,
    pose: Some(car_pose),
    nav: Some(CAR_IDLE),
    place: Some(car_place),
};

/// The companion's own skin (VelmaAIBotPolice: a held character draws its default skin; no player picks one for it).
fn car_skin(d: &FighterData) -> crate::Result<Skin> {
    crate::skins::skin_of(d, None)
}

/// @3340-3649: Mesh.SetMorphTarget(Damage_Adj, v, bRemoveZeroWeight true) @3467 and DamageAmount on every material of
/// the view @3649 (the Deviation above).
fn car_place(r: &mut Rec, w: &World, f: FighterId) {
    let v = rich_curve(&EASE_OUT, f64::from(w.fighter(f).stats.damage) / DAMAGE_TO_KILL);
    r.set_morph_target("Damage_Adj", v, true);
    r.set_scalar_all("DamageAmount", v);
}

/// Navigation's inputs.
#[derive(Debug, Default)]
pub(crate) struct NavInputs {
    /// HorizontalSpeed.
    pub(crate) speed: f64,
    /// IsFalling.
    pub(crate) falling: bool,
    /// IsJumping.
    pub(crate) jumping: bool,
}

fn nav_state(name: &'static str, seq: &'static str, looped: bool, always_reset: bool) -> StateDef<NavInputs> {
    StateDef { name, pose: Some(PoseDef::Seq(SeqDef::new(seq, looped))), entry_rule: None, always_reset }
}

type NavRule = fn(&NavInputs, &dyn MachineQuery) -> bool;

fn nav_edge(from: &'static str, to: &'static str, xfade: f64, rule: NavRule) -> TransitionDef<NavInputs> {
    TransitionDef { from, to, xfade, option: BlendOption::HermiteCubic, rule: Some(rule), auto: false }
}

/// Navigation, the states and each one's transitions in their baked order (abp_sm.js; the rules: the
/// TransitionResult functions and the copied ones of the event graph).
pub(crate) fn navigation() -> MachineDef<NavInputs> {
    MachineDef {
        entry: "Stop",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            nav_state("Stop", "Velma_PoliceCar_Stop", false, true),
            nav_state("Idle", CAR_IDLE, true, false),
            nav_state("Jump", "Velma_PoliceCar_Jump", false, true),
            nav_state("Drive", CAR_IDLE, true, false),
            nav_state("Go", "Velma_PoliceCar_Go", false, true),
            nav_state("Fall", "Velma_PoliceCar_Fall", true, false),
            nav_state("Land", "Velma_PoliceCar_Landing", true, false),
        ],
        transitions: vec![
            nav_edge("Stop", "Go", 0.03, |i, q| i.speed > 0.0 && q.remaining("Stop") <= 0.03), // T7 @1057
            nav_edge("Stop", "Idle", 0.03, |_, q| q.remaining("Stop") <= 0.03),                // T5 @1234
            nav_edge("Idle", "Go", 0.03, |i, _| i.speed > 0.0),                                // T2 @990
            nav_edge("Idle", "Jump", 0.03, |i, _| i.jumping),                                  // T1 (copied: IsJumping)
            nav_edge("Jump", "Idle", 0.03, |i, q| i.speed == 0.0 && q.remaining("Jump") <= 0.05), // T8 @813
            nav_edge("Jump", "Fall", 0.2, |i, q| i.falling && q.remaining("Jump") < 0.03),     // T10 @665
            nav_edge("Jump", "Drive", 0.0, |_, q| q.remaining("Jump") < 0.03),                 // T6 @560
            nav_edge("Drive", "Stop", 0.03, |i, _| i.speed <= 0.01),                           // T4 @488
            nav_edge("Drive", "Fall", 0.2, |i, _| i.falling),                                  // T11 (copied)
            nav_edge("Drive", "Jump", 0.0, |i, _| i.jumping),                                  // T0 (copied)
            nav_edge("Go", "Jump", 0.2, |i, _| i.jumping),                                     // T9 (copied)
            nav_edge("Go", "Drive", 0.0, |_, q| q.remaining("Go") <= 0.03),                    // T3 @353
            nav_edge("Fall", "Land", 0.0, |i, _| !i.falling),                                  // T13 (copied)
            nav_edge("Fall", "Jump", 0.2, |i, _| i.jumping),                                   // T12 (copied)
            nav_edge("Land", "Idle", 0.2, |i, _| !i.falling && i.speed == 0.0),                // T15 @194
            nav_edge("Land", "Drive", 0.03, |_, q| q.remaining("Land") < 0.03),                // T14 @74
        ],
    }
}

/// The car's AnimBP on its view: Navigation, updated once a tick before the view's pose (the AnimBP's update: the
/// source under the Slot is updated whatever the montage's weight), its pose the view's sample with no montage.
/// No slaved gate: the car has no copy of a Disguise (the car views have none).
#[derive(Debug)]
struct PoliceCarPose {
    machine: ActorMachine<NavInputs>,
    inp: NavInputs,
    nav: Pose,
    /// The view of a stand-in (no state read: the machine is not updated).
    stand_in: bool,
}

fn car_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let n = view.rest.n;
    Box::new(PoliceCarPose {
        machine: ActorMachine::new(navigation(), n, clips),
        inp: NavInputs::default(),
        nav: Pose::new(n),
        stand_in: of.stand_in,
    })
}

impl PoseRules for PoliceCarPose {
    fn sample(
        &mut self,
        cx: &mut PoseCx<'_>,
        _clip: usize,
        _time: f64,
        montage: Option<Name>,
        _dt: f64,
        out: &mut Pose,
        _again: bool,
    ) -> bool {
        // (no montage: the machine's pose in place of the clip; a montage's own sample is the view's)
        if montage.is_some() {
            return false;
        }
        let root = cx.parents.iter().position(|&p| p == -1).map_or(-1, |i| i as i32);
        let sk = ActorSkeleton { rig: cx.rig, rest: cx.rest, root, parents: cx.parents };
        self.machine.evaluate(cx.clips, &sk, &mut self.nav);
        out.copy(&self.nav);
        true
    }
}

impl PoseHook for PoliceCarPose {
    fn set(&mut self, w: &World, f: FighterId, _view: &mut CharView, clips: &Clips) {
        if self.stand_in {
            return;
        }
        let fi = w.fighter(f);
        self.inp.speed = f64::from(fi.body.vx.abs());
        self.inp.falling = !mvs_sim::fighter::grounded(w, f);
        self.inp.jumping = fi.moves.cmd_jumped;
        self.machine.update(&self.inp, STEP, clips);
    }

    /// A new car is a new pawn: a new anim instance.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.machine.reinit();
    }
}

// ---- the Jenkins pin ----
//
// VelmaJenkinsPin, its Mesh (Velma_JenkinsPin_SkelMesh, relative (0, 0, -50), scale 10, on the root Capsule,
// bAbsoluteScale) with Velma_JenkinsPin_AnimBP: Slot DefaultSlot over Velma_JenkinsPin_Loop, then ModifyBone Root
// (rotation BMM_Replace in BCS_WorldSpace, rotator 0): the pin's Root faces the world's +X (the camera) whatever the
// actor's turn.
//   - Place: AttachToTarget @3577: the TargetedEnemy's mesh socket fml_un_C_head_end (SnapToTarget location) while
//     not on the car (the sim's x / y are the target's actor location: the view reads the target's drawn socket);
//     Captured @2782 on the car (the sim's x / y: the car's Root + (0, 130, 400), KeepWorld).
//   - ReceiveTick @2349-2566: the root's SetWorldScale3D(CameraCenterPoint location X / 6500); the camera's centre is
//     its actor's root less the lens' 100 along its up (the root = centre + 100 x up, no shake), this frame's
//     (ActorHost.camera). A present with no camera given (render-frame's per-tick presents, which draw nothing)
//     draws no pin.
//   - Mesh visible from @676 (the sim's visible); its montages (Enter / Captured / Exit) on the actor's ActorAnim.
//   - Deviation: SetProfilePic @1829 (slot 1 gets a dynamic instance of Velma_JenkinsProfile_Mat with the target
//     skin's RewardThumbnailMaterial's texture) is not ported: the material's own texture shows.
//   - Unverified: the mesh's (0, 0, -50) is taken in world axes (the capsule's rotation after AttachToTarget's
//     KeepRelative is the head socket's; the Root's own rotation is replaced either way).
//   - On the velma-police pose tape: on Shaggy's head at t11400, on the car at t11450.

const PIN_MESH: &str = "Velma_JenkinsPin_SkelMesh";
const PIN_LOOP: &str = "Velma_JenkinsPin_Loop";
/// Mesh RelativeLocation Z.
const PIN_MESH_Z: f64 = -50.0;
/// Mesh RelativeScale3D.
const PIN_MESH_SCALE: f64 = 10.0;
const HEAD_SOCKET: &str = "fml_un_C_head_end";
/// ReceiveTick @2487.
const CAMERA_SCALE: f64 = 6500.0;
/// The camera's root above CameraCenterPoint along its up.
const LENS_UP: f64 = 100.0;

/// The pin's AnimBP source: its one sequence player.
fn pin_base() -> MachineDef<()> {
    MachineDef {
        entry: "Loop",
        reinit_on_becoming_relevant: false,
        max_transitions_per_frame: 1,
        skip_first_update_transition: true,
        states: vec![StateDef {
            name: "Loop",
            pose: Some(PoseDef::Seq(SeqDef::new(PIN_LOOP, true))),
            entry_rule: None,
            always_reset: false,
        }],
        transitions: Vec::new(),
    }
}

fn pin_of(w: &World, id: ActorId) -> Option<&JenkinsPin> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JenkinsPin(p) => Some(p),
        _ => None,
    }
}

#[derive(Debug)]
struct PinView {
    id: ActorId,
    sk: SkinnedActor,
    machine: ActorMachine<()>,
    slot: ActorSlot,
    base: Pose,
    out: Pose,
    /// The car-attached place of the last two ticks (the sim's x / y), for the frames between.
    px: f64,
    py: f64,
    x: f64,
    y: f64,
    on_car: bool,
    /// The head it follows off the car (a player).
    head: Option<FighterId>,
}

fn pin_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    pin_of(w, id)?;
    let (path, keys) = host.draws.get(POLICE_DATA)?.actor(PIN_MESH, None)?;
    let info = host.mesh(&path)?;
    let mut sk = SkinnedActor::new(info, &keys);
    sk.visible = false;
    let clips = host.clips.get(POLICE_DATA)?;
    if clips.get(PIN_LOOP).is_none() {
        eprintln!("warning: Velma: the Jenkins pin clip is not loaded: {PIN_LOOP}");
    }
    let n = sk.rest.n;
    Some(Box::new(PinView {
        id,
        machine: ActorMachine::new(pin_base(), n, clips),
        slot: ActorSlot::new("DefaultSlot", n),
        base: Pose::new(n),
        out: Pose::new(n),
        sk,
        px: 0.0,
        py: 0.0,
        x: 0.0,
        y: 0.0,
        on_car: false,
        head: None,
    }))
}

impl ActorMesh for PinView {
    /// The tick's pose and place (once a tick).
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(pin) = pin_of(w, self.id) else { return };
        self.sk.visible = pin.visible;
        let Some(clips) = host.clips.get_mut(POLICE_DATA) else { return };
        // the AnimBP: the Slot's source updated only while it passes it on (bAlwaysUpdateSourcePose false)
        let a = ActorRef::Actor(self.id);
        let inst =
            pin.anim.m.as_ref().and_then(|st| Some((st, montage_of(&w.data, w, a, w.names.str(st.montage.name))?)));
        let wt = inst.map_or(0.0, |(st, m)| {
            let v = st.shown_weight(m, &|s| eprintln!("warning: {s}"));
            if v > ZERO_W && st.pose_in(m, "DefaultSlot").is_some() { v } else { 0.0 }
        });
        if 1.0 - wt > ZERO_W {
            self.machine.update(&(), STEP, clips);
        }
        let shown = {
            let sk = self.sk.skeleton(clips);
            self.machine.evaluate(clips, &sk, &mut self.base);
            let posed = self.slot.apply(clips, &sk, inst, &self.base, &mut self.out);
            // ModifyBone Root: rotation replaced by 0 in world space (the component is drawn unturned: the local
            // rotation is it)
            let p = if posed { &mut self.out } else { &mut self.base };
            if let Ok(r) = usize::try_from(sk.root) {
                p.r[r * 4..r * 4 + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            }
            posed
        };
        self.sk.show(if shown { &self.out } else { &self.base });
        if pin.attached_to_car {
            let carried = self.on_car;
            self.px = if carried { self.x } else { f64::from(pin.x) };
            self.py = if carried { self.y } else { f64::from(pin.y) };
            self.x = f64::from(pin.x);
            self.y = f64::from(pin.y);
            self.on_car = true;
            self.head = None;
        } else {
            self.on_car = false;
            self.head = pin.target.filter(|t| w.players.contains(t));
        }
    }

    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        if !self.sk.visible {
            return;
        }
        // ReceiveTick: the root's world scale = CameraCenterPoint X / 6500
        let Some((root, up)) = host.camera else { return };
        let s = (root[0] - LENS_UP * up[0]) / CAMERA_SCALE;
        let mut loc = if self.on_car {
            [0.0, -(self.px + (self.x - self.px) * alpha), self.py + (self.y - self.py) * alpha]
        } else if let Some(t) = self.head {
            let ident = Xform { loc: [0.0; 3], rot: IDENT, scale: [1.0; 3] };
            match host.socket(w, t, HEAD_SOCKET, &ident) {
                Some(at) => at.loc,
                None => {
                    self.sk.draw(out);
                    return;
                }
            }
        } else {
            // (no place this tick: the last one's)
            self.sk.draw(out);
            return;
        };
        loc[2] += PIN_MESH_Z * s;
        let ms = PIN_MESH_SCALE * s;
        self.sk.set(&Xform { loc, rot: IDENT, scale: [ms; 3] });
        self.sk.draw(out);
    }
}
