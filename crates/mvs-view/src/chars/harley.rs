//! Harley Quinn's render module: the entry, the AnimBP's pose hook, the pawn's own
//! mesh components and the classes' mesh rules. Her effects (the bat bomb's aura, the bomb's particle components, the
//! balloons) are mvs-render fx/chars/harley.rs.

use std::sync::Arc;

use mvs_data::Value;
use mvs_data::fighter::Montage;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::harley::actors::JackBox;
use mvs_sim::chars::harley::bomb::{Bomb, attached_to};
use mvs_sim::fighter::{FighterId, in_hitstun};
use mvs_sim::names::Name;
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::{CharRender, PawnComponents, PoseHook, PoseOf, PreviewOf};
use crate::actor_view::{IDENT, SkinnedActor, StaticView, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows};
use crate::anim::{Clips, HeadScaleNode, Pose, blend_pose};
use crate::anim_nodes::{AnimNodeSkel, RespawnFlag, SkinNodes, UeXf, mesh_xf};
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::pose::{MeshMap, Rec, material, set_scalar};
use crate::skins::FighterDraws;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    head_scale: &HARLEY_HEAD,
    pose: Some(harley_pose),
    components: Some(PawnComponents { make: props_make, place: props_place, reset: None }),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    ("HarleyDownAirProjectile_C", MeshRule { rot: None, mesh: Some(glove_view) }),
    ("HarleySideAirProjectile2_C", MeshRule { rot: None, mesh: Some(glove_view) }),
    (JACK, MeshRule { rot: None, mesh: Some(box_view) }),
    (BOMB, MeshRule { rot: None, mesh: Some(bomb_view) }),
];

/// The big head: HarleyQuinn_AnimBP's ModifyBone (scale Additive, bone space, from HeadScale; Pawn_Harley @1538).
const HARLEY_HEAD: [HeadScaleNode; 1] = [HeadScaleNode {
    node: "ModifyBone",
    bone: "fml_un_C_head",
    factor: 1.0,
    gated: false,
    moves: [0.0; 3],
    scale: true,
}];

const STEP: f64 = 1.0 / 60.0;

// ---- the pose ---------------------------------------------------------------------
//
// The graph, from the root back (42 nodes): Root <- ComponentToLocal <- 9 SpringBone and 7 Trail nodes (the bun, the
// pigtails, the jacket's chest pieces: SKIN_NODES) <- ModifyBone_1 (VB R_LowLeg, scale Replace) <- ModifyBone_2 (VB L_LowLeg,
// rotation Replace in world space 0, scale Replace) <- ModifyBone_3 (VB Spine, rotation and scale Replace, world) <-
// ModifyBone (fml_un_C_head, scale BMM_Additive in bone space = HeadScale) <- LocalToComponent <- MirroredAnim
// (WAnimationMirrorNode, bEnabled <- FacingLeft) <- AfterAim = TwoWayBlend (A = Slot AimGloveLeft over BeforeAim, B =
// Slot AimGloveRight over BeforeAim, Alpha <- AimDirection) ; BeforeAim = Slot DefaultSlot <- Slot JumpSlot <-
// Navigation = the linked layer BaseMovementMachine (Base_AnimBP_HarleyQuinn).
//
// What the hook draws:
//   - The aim blend. A montage with tracks on both aim slots (HarleyQuinn_Attack_Air_Bullseye_Montage alone) is the
//     two slots' poses at the montage's position, blended by AimDirection (0 = A, the left slot); any other montage
//     has no track on them and the blend is the shared pose. AimDirection (BlueprintUpdateAnimation @7164..7602) =
//     clamp((BPI_GetProjectileAimDirection().Y x (FacingLeft ? 1 : -1) + 0.75) / 1.5, 0, 1): the sim's
//     Items::proj_aim. The montage is behind HarleyHasBullsEyePerk.
//     Unverified: the AnimBP's update is taken to read the vector the step left (one frame early at most).
//   - The idle while she holds the bat bomb: SetIdleAnim sets the linked layer's Anim_Idle to
//     HarleyQuinn_Nav_Idle_HoldItem while BPI_IsHoldingSatchel (the sim's HarleyState::hold_idle). Where the shared
//     pose samples HarleyQuinn_Nav_Idle under no montage, the hold clip is sampled in its place.
//     Unverified: the sequence player keeps its position through the swap: the hold clip at the idle's time, wrapped
//     to its own length.
//   - C008_S02's layer (Base_AnimBP_HarleyQuinn_S02, linked @15-281): its Anim_Idle is
//     HarleyQuinn_Nav_Idle_NoHairSwirl, and SetIdleAnim's lookup by the exact class (exe 14283d1d0) finds no layer
//     there, so S02 keeps that idle whether she holds an item or not. Unverified: sampled at the idle's time, wrapped.
//   - The skin nodes (SKIN_NODES), made from the fighter's data at the first set (crate::anim_nodes::SkinNodes; a node
//     whose bones the skin's mesh lacks is left out). Each alpha (BlueprintUpdateAnimation @1432-7102) =
//     float(!Is Respawning && !Is Hit Stunned) x (curve < 0.1 ? k : 0), the curve and k per node (SKIN_NODES). They
//     run on the unmirrored pose with the component's transform at the actor's facing (mesh_xf), as Reindog's
//     (chars/creature/skin_controls.rs, its Deviation), with the hitpause dilation 0.005. Not run in a preview.
//     Unverified: the curves are the clip's the sim names at the tick before (no crossfade, no slot mix).
//     Deviation: the head-scale step (ModifyBone, before them) is the view's after the rule, so the nodes read the
//     pose before the big head's scale.
// The head's ModifyBone is the shared head-scale step (HARLEY_HEAD).
// Not ported: ModifyBone_1 / _2 / _3 on the virtual bones (the view has none: the bat bomb's leg sockets read them, bomb_view). A preview actor
// (no fighter, PoseHook::preview) holds no item and plays no aim montage: the shared pose.

const IDLE: &str = "HarleyQuinn_Nav_Idle";
const IDLE_HOLD: &str = "HarleyQuinn_Nav_Idle_HoldItem";
const IDLE_S02: &str = "HarleyQuinn_Nav_Idle_NoHairSwirl";
const SKIN_S02: &str = "C008_S02";
const AIM_LEFT: &str = "AimGloveLeft";
const AIM_RIGHT: &str = "AimGloveRight";

/// The curves the skin nodes' alphas read (GetCurveValue).
const CURVES: CurveTable = CurveTable::new(&[
    "TSSDynamicsDisable",
    "MadLoveChestDynamicsDisable",
    "MadlovePigtailDisable",
    "qinzelDynamicsDisable",
]);
const NC: usize = 4;
const TSS: usize = 0;
const CHEST: usize = 1;
const PIGTAIL: usize = 2;
const QUINZEL: usize = 3;
/// Less_FloatFloat's bound on each curve.
const CURVE_MAX: f64 = 0.1;
/// The skin nodes in the graph's evaluation order (the pose links from ComponentToLocalSpace back: input first), each
/// with its curve and the SelectFloat's value (@offset of its Alpha write).
const SKIN_NODES: [(&str, usize, f64); 16] = [
    ("AnimGraphNode_Trail_5", PIGTAIL, 1.0),      // @3404
    ("AnimGraphNode_Trail_4", PIGTAIL, 1.0),      // @3075
    ("AnimGraphNode_Trail_3", CHEST, 0.35),       // @2747
    ("AnimGraphNode_Trail_2", CHEST, 0.35),       // @2409
    ("AnimGraphNode_Trail_1", CHEST, 0.35),       // @2071
    ("AnimGraphNode_SpringBone", TSS, 1.0),       // @4398
    ("AnimGraphNode_SpringBone_1", TSS, 1.0),     // @6426
    ("AnimGraphNode_Trail", TSS, 1.0),            // @1733
    ("AnimGraphNode_SpringBone_2", TSS, 1.0),     // @6764
    ("AnimGraphNode_SpringBone_3", TSS, 1.0),     // @7102
    ("AnimGraphNode_Trail_6", TSS, 1.0),          // @4060
    ("AnimGraphNode_SpringBone_4", QUINZEL, 1.0), // @4736
    ("AnimGraphNode_SpringBone_5", QUINZEL, 1.0), // @5074
    ("AnimGraphNode_SpringBone_7", QUINZEL, 1.0), // @5412
    ("AnimGraphNode_SpringBone_6", QUINZEL, 1.0), // @5750
    ("AnimGraphNode_SpringBone_8", QUINZEL, 1.0), // @6088
];
const SKIN_NODE_NAMES: [&str; 16] = {
    let mut n = [""; 16];
    let mut i = 0;
    while i < 16 {
        n[i] = SKIN_NODES[i].0;
        i += 1;
    }
    n
};

/// `harleyPose`'s state on one view.
#[derive(Debug)]
struct HarleyPose {
    hold: Option<usize>,
    s02: Option<usize>,
    hold_idle: bool,
    /// The shown montage when it has both aim slots, by name.
    aim_montage: Option<(Name, Montage)>,
    aim: f64,
    a: Pose,
    b: Pose,
    /// The skin nodes, the curves of the tick before, the alpha's other factor, the component's transform, the
    /// update's dilation; whether a set ran (a fighter's view).
    nodes: SkinNodes,
    curves: [f64; NC],
    alpha_curves: [f64; NC],
    on: f64,
    comp: UeXf,
    dilation: f64,
    respawning: RespawnFlag,
    live: bool,
}

impl HarleyPose {
    /// A skin node's alpha (SKIN_NODES' index).
    fn alpha(on: f64, curves: &[f64; NC], i: usize) -> f64 {
        let (_, c, k) = SKIN_NODES[i];
        if curves[c] < CURVE_MAX { k * on } else { 0.0 }
    }
}

fn harley_pose(view: &mut CharView, clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    let hold = clips.get(IDLE_HOLD);
    if hold.is_none() {
        eprintln!("warning: Harley pose: clip missing: {IDLE_HOLD}");
    }
    let s02 = (of.skin.name == SKIN_S02).then(|| clips.get(IDLE_S02)).flatten();
    if of.skin.name == SKIN_S02 && s02.is_none() {
        eprintln!("warning: Harley pose: clip missing: {IDLE_S02}");
    }
    let n = view.rest.n;
    let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: None };
    Box::new(HarleyPose {
        hold,
        s02,
        hold_idle: false,
        aim_montage: None,
        aim: 0.5,
        a: Pose::new(n),
        b: Pose::new(n),
        nodes: SkinNodes::new(&SKIN_NODE_NAMES, &skel),
        curves: [0.0; NC],
        alpha_curves: [0.0; NC],
        on: 0.0,
        comp: UeXf::default(),
        dilation: 1.0,
        respawning: RespawnFlag::default(),
        live: false,
    })
}

/// A slot's clip and clip time at the montage position `pos` (`slotAt`; None: no segment there).
fn slot_at(m: &Montage, slot: &str, pos: f64, clips: &Clips) -> Option<(usize, f64)> {
    for sl in m.slots.iter().filter(|s| s.slot == slot) {
        for g in &sl.segments {
            let len = if g.rate > 0.0 { (g.a1 - g.a0) / g.rate } else { 0.0 };
            if pos < g.start - 1e-6 || pos > g.start + len * g.loops + 1e-6 {
                continue;
            }
            let clip = clips.get(&g.anim)?;
            let into = (pos - g.start) * g.rate;
            let span = g.a1 - g.a0;
            return Some((clip, g.a0 + if span > 0.0 && into > span { (into - 1e-6) % span } else { into }));
        }
    }
    None
}

impl PoseRules for HarleyPose {
    fn sample(
        &mut self,
        cx: &mut PoseCx<'_>,
        clip: usize,
        time: f64,
        montage: Option<Name>,
        _dt: f64,
        out: &mut Pose,
        _again: bool,
    ) -> bool {
        if montage.is_none() && cx.clips.clips[clip].name == IDLE {
            // S02's layer idle, held item or not; else the hold idle while she holds one
            let Some(c) = self.s02.or(self.hold.filter(|_| self.hold_idle)) else { return false };
            let len = cx.clips.clips[c].anim.length;
            cx.sample(c, if len > 0.0 { time % len } else { 0.0 }, out);
            return true;
        }
        let (Some(name), Some((aim_name, m))) = (montage, &self.aim_montage) else { return false };
        if name != *aim_name {
            return false;
        }
        // the montage position of the clip time the sim names (the left slot's track is the montage's first)
        let anim = &cx.clips.clips[clip].name;
        let pos = m.slots.iter().flat_map(|s| &s.segments).find_map(|g| {
            (g.anim == *anim && time >= g.a0 - 1e-6 && time <= g.a1 + 1e-6 && g.rate > 0.0)
                .then(|| g.start + (time - g.a0) / g.rate)
        });
        let Some(pos) = pos else { return false };
        let (Some(l), Some(r)) = (slot_at(m, AIM_LEFT, pos, cx.clips), slot_at(m, AIM_RIGHT, pos, cx.clips)) else {
            return false;
        };
        cx.sample(l.0, l.1, &mut self.a);
        cx.sample(r.0, r.1, &mut self.b);
        blend_pose(&self.a, &self.b, self.aim, out);
        true
    }

    /// The skin nodes on the tick's local pose.
    fn rule(&mut self, _cx: &mut PoseCx<'_>, p: &mut Pose, dt: f64) {
        if !self.live {
            return;
        }
        let (on, curves) = (self.on, self.alpha_curves);
        self.nodes.update(dt * self.dilation, |i| Self::alpha(on, &curves, i));
        self.nodes.evaluate(p, &self.comp);
    }
}

impl PoseHook for HarleyPose {
    /// `preview()`: a view with no fighter: no held item's idle, no aim montage (the shared pose).
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.hold_idle = false;
        self.aim_montage = None;
        self.live = false;
    }

    /// A new pawn: the nodes made anew at the next set.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.nodes.clear();
        self.respawning.reset();
        self.curves = [0.0; NC];
    }

    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        self.hold_idle = mvs_sim::chars::harley::harley_of(w, f).is_some_and(|h| h.hold_idle);
        let fi = w.fighter(f);
        // the skin nodes' inputs: the update's, on the curves of the tick before
        let respawning = self.respawning.step(w.flow.tick);
        self.on = f64::from(u8::from(!respawning && !in_hitstun(w, f)));
        if !self.nodes.is_made() {
            let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
            let (on, curves) = (self.on, self.curves);
            let data = w.char_data(f).fighter.anim_nodes.as_ref();
            self.nodes.make(data, &skel, "Harley pose", |i| Self::alpha(on, &curves, i));
        }
        self.live = self.nodes.any();
        self.alpha_curves = self.curves;
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.curves);
        self.dilation = if fi.hit.hitpause > 0.0 { f64::from(mvs_sim::consts::HITPAUSE_DILATION) } else { 1.0 };
        self.comp = mesh_xf(
            f64::from(-mvs_sim::fighter::mesh_wx(w, f)),
            f64::from(mvs_sim::fighter::mesh_wy(w, f)) + view.mesh_z * view.size_scale,
            f64::from(fi.body.facing_dir),
            view.mesh_scale * view.size_scale,
        );
        let shown = mvs_sim::fighter::shown_montage(w, f).and_then(|dash| {
            let s = if dash { &fi.montages.dash_montage } else { &fi.montages.montage };
            s.as_ref().map(|s| s.montage.name)
        });
        match shown {
            Some(n) if self.aim_montage.as_ref().is_some_and(|(k, _)| *k == n) => {}
            Some(n) => {
                let m = mvs_sim::fighter::common::data_of(w, f).fighter.montages.get(w.names.str(n));
                self.aim_montage = m
                    .filter(|m| {
                        m.slots.iter().any(|s| s.slot == AIM_LEFT) && m.slots.iter().any(|s| s.slot == AIM_RIGHT)
                    })
                    .map(|m| (n, m.clone()));
            }
            None => self.aim_montage = None,
        }
        // AimDirection (@7286-7602): FacingLeft is the pawn's facing.
        let left = if fi.body.facing_dir < 0.0 { 1.0 } else { -1.0 };
        self.aim = f64::from(((fi.items.proj_aim.y * left + 0.75) / 1.5).clamp(0.0, 1.0));
    }
}

// ---- the pawn's own mesh components ----------------------------------------------
//
// Children of CharacterMesh0, every one with bVisible false and filed by Comp_Pawn_MeshManager under its tag:
//   Pistol (tag Pistol): HarleyQuinn_Pistol_SkelMesh on fml_un_C_prop2, its own skeleton, no animation.
//   Glove (tag Glove): static HarleyQuinn_Glove on fml_un_C_prop2 at (52, 0, 25), scale 1.3.
//   ConfettiGun, Detonator (tags of the same names): static, on fml_un_C_prop2.
//   BatBomb (tag SatchelCharge): HarleyQuinn_BatBomb_SkelMesh on fml_un_R_PropOffset, scale 1.25, with the child
//     BombAuraVFX (the effects').
//   Bat, Mallet, BodyA / B / C: the skin's sub-meshes (the view's parts). JackInTheBox, BubbleGum, Flag, Bomb: no
//     notify of her montages names their tags and no graph shows them. Not made.
// The data file keeps each template with its node's socket (components.pawn.<name>_GEN_VARIABLE: p, socket).
//
// Who shows them: AnimNotify_Equip and AnimNotify_SetSubmeshVisibility through Comp_FighterVisibilityManager; the sim
// keeps the state (sub_hidden, the tags of the character's sub_meshes).
// Deviation: the sim's list holds the skin's sub-meshes and the pawn's static mesh components; a tag it
// does not hold (pistol, satchelcharge: skeletal components with a mesh of their own) is never shown, and its mesh
// stays hidden here with a warning.
//
// The outline: Comp_Pawn_MeshManager AddMesh (@1291-1332) gives each of them the fighter's stencil and custom depth,
// the static ones too (Rec::add_component_mesh).
// Materials: the meshes' own (Glove's is HarleyQuinnProps_Yellow_MIC), but BatBomb: OnMeshActivated @705 and
// SpawnInvulnerability @1213 run BatBomb.CreateDynamicMaterialInstance(0, HarleyQuinnProps_MIC) @715 and
// SetScalarParameterValue(Team, SelectFloat(0.6, ColorIndex, IsLocalOnlinePlayer)) @944.
//
// The facing: the components ride the mirrored pose: the build's bone is the view's mirror partner under the view's
// reflection with the bone's flip in front (Rec::component_attach, both facings made here). OnFacingChange
// @1228..1309 also re-attaches BatBomb: K2_AttachToComponent(Mesh, FacingLeft ? fml_un_L_PropOffset :
// fml_un_R_PropOffset, SnapToTarget, SnapToTarget, KeepRelative).
// Unverified: FacingChanged is taken to have run for the facing she has; the socket is the last rendered pose's.

/// The props' material (the skin context's: the pistol's skeletal mesh names it).
const PROPS_MIC: &str = "/Game/Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_MIC.HarleyQuinnProps_MIC";
/// The components drawn here, with OnFacingChange's literal socket facing left (None: the node's).
const COMPONENTS: [(&str, Option<&str>); 5] = [
    ("Pistol", None),
    ("Glove", None),
    ("ConfettiGun", None),
    ("Detonator", None),
    ("BatBomb", Some("fml_un_L_PropOffset")),
];
const BAT_BOMB: &str = "BatBomb";

/// A pawn component's template (its data file entry's `p`), its SCS socket and its tag in lower case.
fn component<'a>(w: &'a World, f: FighterId, name: &str) -> Option<(&'a mvs_data::Props, Option<&'a str>, String)> {
    let c = w.char_data(f).fighter.components.pawn.get(&format!("{name}_GEN_VARIABLE"))?;
    let p = c.p.as_obj()?;
    let tag = p.get("ComponentTags").and_then(Value::as_arr).and_then(|t| t.first()).and_then(Value::as_str)?;
    Some((p, c.socket.as_deref(), tag.to_lowercase()))
}

/// `pawnComps`: the components on the view when it is made.
fn props_make(r: &mut Rec, w: &World, clips: &Clips, draws: &FighterDraws, meshes: &MeshMap) {
    let f = r.fighter();
    for (name, left_socket) in COMPONENTS {
        let Some((p, socket, tag)) = component(w, f, name) else {
            eprintln!("warning: Harley: pawn component not drawn: {name} (its template or tag is not loaded)");
            continue;
        };
        let Some(socket) = socket else {
            eprintln!("warning: Harley: pawn component not drawn: {name} (the data file has no socket for it)");
            continue;
        };
        let mesh = p.get("SkeletalMesh").or_else(|| p.get("StaticMesh")).and_then(Value::as_str);
        // (BatBomb's mesh is no entry of the draws' actor table: a spawned actor's mesh, loaded by its path)
        let actor = mesh.and_then(|m| {
            let path = || w.char_data(f).fighter.paths.meshes.get(m).cloned();
            draws.actor(m, None).or_else(|| (name == BAT_BOMB).then(|| Some((path()?, Vec::new()))).flatten())
        });
        let info = actor.as_ref().and_then(|(path, _)| meshes.get(path)).cloned();
        let (Some((_, keys)), Some(info)) = (actor, info) else {
            eprintln!("warning: Harley: pawn component not drawn: {name} (its mesh or materials are not loaded)");
            continue;
        };
        if p.contains_key("RelativeRotation") {
            eprintln!("warning: Harley: pawn component with a rotation: {name}");
        }
        let v3 = |k: &str, d: f64| {
            let o = p.get(k).and_then(Value::as_obj);
            ["X", "Y", "Z"].map(|a| o.and_then(|o| o.get(a)).and_then(Value::as_f64).unwrap_or(d))
        };
        let (loc, scale) = (v3("RelativeLocation", 0.0), v3("RelativeScale3D", 1.0));
        let q = [0.0, 0.0, 0.0, 1.0];
        let at = [false, true].map(|left| {
            let s = if left { left_socket.unwrap_or(socket) } else { socket };
            r.component_attach(w, s, loc, q, scale, left)
        });
        let mats = if name == BAT_BOMB {
            let mut m = material(PROPS_MIC);
            set_scalar(&mut m, "Team", r.colour.team);
            vec![Some(m)]
        } else {
            keys.iter().map(|m| m.as_deref().map(material)).collect()
        };
        r.add_component_mesh(clips, &tag, at, info, mats, false);
        if !w.char_data(f).def.sub_meshes.contains(&tag) {
            eprintln!("warning: Harley: the sim's mesh list has no {tag}: the component stays hidden");
        }
    }
}

/// `placePawnComps`: once a presentation tick, each shown while the sim's mesh list has its tag and sub_hidden does
/// not (the facing's socket is the view's pick of the two made).
fn props_place(r: &mut Rec, w: &World) {
    let f = r.fighter();
    for (name, _) in COMPONENTS {
        let Some((_, _, tag)) = component(w, f, name) else { continue };
        let on = r.sub_mesh_shown(w, &tag);
        r.set_component_mesh(&tag, on);
    }
}

// ---- the glove shots ----
//
// HarleyDownAirProjectile / HarleySideAirProjectile2 (the second a child with no bytecode): StaticMesh =
// HarleyQuinn_Glove under the Capsule at scale 2; SetTeamColor (@849) puts the instigator's pick of the class's
// Materials set on slot 0 (taken once with the view: selected_variant). The class default's ApplyLocalUserColor is
// false (ActorHost::apply_local_user_colour reads it all the same). The ParticleSystem component
// (PS_HarleyNADAIR_Ribbon) is the shared projectile path's. The actor turns with its velocity
// (bRotationFollowsVelocity: the place's rows).
// Deviation: RotatingMovement (yaw 2000 deg/s on the root) is left out, as the C019 shots'.

#[derive(Debug)]
struct GloveView {
    view: StaticView,
    rs: V3,
}

fn glove_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let c = cs.get("StaticMesh")?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    let from = mvs_sim::fighter::common::actor_def_owner(w, owner, pr.cls);
    let variant = host.selected_variant(w, owner);
    let mut view = StaticView::new(host, w, from, mesh, variant)?;
    // (the class default's flag is false)
    if let Some(def) = mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(owner), pr.cls) {
        host.apply_local_user_colour(w, &mut view, &def.p, owner);
    }
    Some(Box::new(GloveView { view, rs: c.scale }))
}

impl ActorMesh for GloveView {
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
        let (x, y) = at.at(alpha);
        let scale = self.rs.map(|v| v * at.scale);
        self.view.set(&Xform { loc: [0.0, -x, y], rot: at.rot, scale });
        self.view.draw(out);
    }
}

// ---- the box ----
//
// HarleyJackInTheBox: SkeletalMesh = HarleyQuinn_JackInTheBox_SkelMesh under the Capsule at scale 1.45, CastShadow
// false. ReceiveBeginPlay: CreateDynamicMaterialInstance(0, local ? HarleyQuinnProps_Yellow_MIC : colour index 0 ?
// HarleyQuinnProps_MIC : HarleyQuinnProps_Blue_MIC) @3837 (the skin's JackInTheBox sets of her fighter entry hold the
// same three); an instigator facing left: SetWorldScale3D((1.45, -1.45, 1.45)) @4123; PlayAnimation(the ground probe
// hit ? JackInTheBox_Special_Idle : JackInTheBox_Special_Air_Idle, looping) @4839. ExplodeMulticast @5486:
// PlayAnimation(JackInTheBox_Special_Explode, not looping). BalloonSpawn's emitter is the effects'.
// The actor's transform is the spawner's: its depth is her own; rotation yaw 90 for an instigator facing left, -90
// facing right @1385.
// Unverified: the animations' positions are the sim's ticks of 1 / 60 s from the spawn and from the explosion.
// Deviation: no outline. The jump input label and the "Jumps Reset!" pop text are the HUD's.

const JACK: &str = "HarleyJackInTheBox_C";
const JACK_KEY: &str = "JackInTheBox";

#[derive(Debug)]
struct BoxView {
    id: mvs_sim::actors::ActorId,
    view: SkinnedActor,
    /// The character whose draws and clips the box takes.
    ch: String,
    idle: Option<usize>,
    explode: Option<usize>,
    rot: Rows,
    scale: V3,
    /// The age the last tick's place was made at.
    age: Option<i32>,
}

fn jack_of(w: &World, id: mvs_sim::actors::ActorId) -> Option<(&JackBox, f64)> {
    let a = w.actors.actors.get(id)?;
    match &a.state {
        WorldActorState::JackBox(j) => Some((j, f64::from(a.size_scale.unwrap_or(1.0)))),
        _ => None,
    }
}

fn box_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let (j, _) = jack_of(w, id)?;
    // (the class's character: the owner, else a peer after a reflect; a fighter whose character holds the class
    // takes its skin, else, a tutorial's box with no Harley pawn, the first character holding it on its default skin,
    // as the sim's actor data)
    let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
    let key = w.names.str(x.cls);
    let holds = |d: &mvs_data::CharData| d.fighter.actors.as_ref().is_some_and(|m| m.contains_key(key));
    let (ch, pick) = if holds(w.char_data(from)) {
        let pick = usize::try_from(w.fighter(from).id)
            .ok()
            .and_then(|i| w.setup.players.get(i))
            .and_then(|p| p.skin.as_deref());
        (w.names.str(w.fighter(from).ch).to_owned(), pick)
    } else {
        (w.data.chars.iter().find(|(_, d)| holds(d))?.0.clone(), None)
    };
    let c = cs.get("SkeletalMesh")?;
    let ComponentKind::SkeletalMesh { mesh, .. } = &c.kind else { return None };
    let d = host.draws.get(&ch)?;
    let (path, _) = d.actor(mesh, None)?;
    // (the skin's prop mesh entry of that key is what the material sets are built from)
    let skin = crate::skins::skin_of(&w.data.chars[&ch].fighter, pick).ok()?;
    let variant = host.colour(w, x.owner).variant;
    let keys = d.set(&skin.name, JACK_KEY, variant).ok()?;
    let info = host.mesh(&path)?;
    let mut view = SkinnedActor::new(info, &keys);
    view.blend_ticks();
    let idle_name = if j.grounded { "JackInTheBox_Special_Idle" } else { "JackInTheBox_Special_Air_Idle" };
    let clips = host.clips.get_mut(&ch)?;
    let (idle, explode) = (clips.get(idle_name), clips.get("JackInTheBox_Special_Explode"));
    if idle.is_none() {
        eprintln!("warning: Harley: clip missing: {idle_name}");
    }
    if explode.is_none() {
        eprintln!("warning: Harley: clip missing: JackInTheBox_Special_Explode");
    }
    let h = std::f64::consts::FRAC_PI_2;
    let left = j.facing < 0.0;
    let s = c.scale;
    Some(Box::new(BoxView {
        id,
        view,
        ch,
        idle,
        explode,
        rot: rotation_rows(0.0, if left { h } else { -h }, 0.0),
        scale: [s[0], if left { -s[1] } else { s[1] }, s[2]],
        age: None,
    }))
}

impl ActorMesh for BoxView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((j, k)) = jack_of(w, self.id) else { return };
        if self.age != Some(j.age) {
            self.age = Some(j.age);
            // (the root's scale: the sim's sizeScale)
            let scale = self.scale.map(|v| v * k);
            self.view.set(&Xform { loc: [0.0, f64::from(-j.x), f64::from(j.y)], rot: self.rot, scale });
            if let Some(clips) = host.clips.get_mut(&self.ch) {
                match (j.exploded, self.explode, self.idle) {
                    (true, Some(c), _) => self.view.play(clips, c, f64::from(j.age - j.explode_age) * STEP, false),
                    (_, _, Some(c)) => self.view.play(clips, c, f64::from(j.age - 1) * STEP, true),
                    _ => {}
                }
            }
        }
        self.view.present(alpha);
        self.view.visible = shown;
        self.view.draw(out);
    }

    /// The bones for the balloons (BalloonSpawn's socket Box: the bone "box").
    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        self.view.bones_world(out);
    }
}

// ---- the bomb ----
//
// HarleyBomb: Capsule (bAbsoluteScale) > AttachPoint > HarleyQuinn_BatBomb_SkelMesh at (0, 0, 13.49), scale 2.25,
// AnimToPlay BatBomb_Item_Tumble, with the particle components PS_Explosion_SatchelAura (PS_HarleyBatDollAura) and
// PS_Explosion_SatchelAura1 (PS_Harley_BatbombTimer) under the mesh (the effects'). ReceiveBeginPlay: the mesh's
// K2_SetWorldRotation(yaw -180, either facing) @2528, CreateDynamicMaterialInstance(0, HarleyQuinnProps_MIC) @2580 with
// Team = SelectFloat(0.6, the colour index, local) @2817. The attach's PlayAnimation(BatBomb_Item_Attach, looping)
// @1048 / @5368. MulticastAttachVisuals @6423: AttachPoint.K2_AttachToComponent(the carrier's Mesh, R_LoLeg / L_LoLeg,
// SnapToTarget location and rotation, KeepWorld scale) @6837, relative rotation yaw 180 when FlipOnAttach else 0
// @7033, relative location (50, -30, 0) @7085; the mesh visible while the carrier's InvisibilityCount is 0 @7338 /
// @7999.
// Deviation: in flight the mesh's bodies simulate (SetAllBodiesBelowSimulatePhysics @2145: a rag doll on a rigid
// capsule that may turn): drawn as the Tumble animation, looping, on an actor that does not turn. The actor's rotation
// at the spawn is taken as zero, so the mesh's relative rotation under AttachPoint is yaw -180. The animations'
// positions are ticks of 1 / 60 s. The carrier's sockets R_LoLeg / L_LoLeg sit on virtual bones (VB R_LowLeg /
// VB L_LowLeg), followed through the carrier's skeleton's virtual_bones (leg_follow); a skeleton without either
// socket attaches to the component itself (ActorHost::socket's mesh origin). No outline.

const BOMB: &str = "HarleyBomb_C";
/// The mesh component: its place is also entered under this name in bones() for the effects' particle components.
pub const BOMB_MESH: &str = "HarleyQuinn_BatBomb_SkelMesh";
/// AttachPoint's relative location on the leg @7085.
const ATTACH_LOC: V3 = [50.0, -30.0, 0.0];

#[derive(Debug)]
struct BombView {
    id: mvs_sim::actors::ActorId,
    view: SkinnedActor,
    from: FighterId,
    tumble: Option<usize>,
    attach: Option<usize>,
    /// The mesh under AttachPoint.
    rel: V3,
    scale: V3,
    /// The mesh's world transform of the latest tick.
    at: Xform,
    /// The bomb's age at the attach seen first.
    attached_at: Option<i32>,
    age: Option<i32>,
    shown: bool,
}

fn bomb_of(w: &World, id: mvs_sim::actors::ActorId) -> Option<(&mvs_sim::actors::world_actor::WorldActor, &Bomb)> {
    let a = w.actors.actors.get(id)?;
    match &a.state {
        WorldActorState::HarleyBomb(b) => Some((a, b)),
        _ => None,
    }
}

/// The bomb's leg socket on its carrier (`Bomb.legSocket`), with the carrier.
pub fn leg_socket(w: &World, id: mvs_sim::actors::ActorId) -> Option<(FighterId, &'static str)> {
    let (a, b) = bomb_of(w, id)?;
    let c = attached_to(w, a, b)?;
    let left = w.fighter(c).body.facing_dir < 0.0;
    Some((c, if left == b.front { "R_LoLeg" } else { "L_LoLeg" }))
}

fn bomb_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let (x, _) = bomb_of(w, id)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
    let c = cs.get(BOMB_MESH)?;
    let ComponentKind::SkeletalMesh { mesh, anim, .. } = &c.kind else { return None };
    // (no entry of the draws' actor table: the class's mesh by its path, loaded with the view)
    let path = w.char_data(from).fighter.paths.meshes.get(mesh.as_str())?;
    let info = host.mesh(path)?;
    let mut m = material(PROPS_MIC);
    set_scalar(&mut m, "Team", host.colour(w, x.owner).team);
    let mut view = SkinnedActor::new(info, &[]);
    view.mats = vec![Some(m)];
    // (drawn between the sim's ticks; on the carrier's leg, between the leg's two tick places)
    view.blend_ticks();
    let clips = host.clips_of(w, from)?;
    let tumble_name = anim.as_deref().unwrap_or("None");
    let (tumble, attach) = (clips.get(tumble_name), clips.get("BatBomb_Item_Attach"));
    if tumble.is_none() {
        eprintln!("warning: Harley: clip missing: {tumble_name}");
    }
    if attach.is_none() {
        eprintln!("warning: Harley: clip missing: BatBomb_Item_Attach");
    }
    Some(Box::new(BombView {
        id,
        view,
        from,
        tumble,
        attach,
        rel: c.loc,
        scale: c.scale,
        at: Xform { loc: [0.0; 3], rot: mesh_yaw(), scale: c.scale },
        attached_at: None,
        age: None,
        shown: true,
    }))
}

/// K2_SetWorldRotation(yaw -180) @2528.
fn mesh_yaw() -> Rows {
    rotation_rows(0.0, -std::f64::consts::PI, 0.0)
}

impl BombView {
    /// `placeBomb`: the tick's place and pose.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World, b: &Bomb, r: f64) {
        let mut shown = true;
        let mut play = None;
        match leg_socket(w, self.id) {
            Some((c, socket)) => {
                let since = *self.attached_at.get_or_insert(b.age);
                let rot = if b.flip_on_attach { rotation_rows(0.0, std::f64::consts::PI, 0.0) } else { IDENT };
                // AttachPoint (KeepWorld scale: 1)
                let p = leg_follow(host, w, c, socket, &rot);
                let mut loc = p.loc;
                for (k, v) in loc.iter_mut().enumerate() {
                    *v += r * (self.rel[0] * p.rot[k] + self.rel[1] * p.rot[3 + k] + self.rel[2] * p.rot[6 + k]);
                }
                self.at.loc = loc;
                self.at.rot = compose_rows(&mesh_yaw(), &p.rot);
                play = self.attach.map(|a| (a, f64::from(b.age - since) * STEP));
                shown = !w.fighter(c).vis.invisible;
            }
            None => {
                self.at.loc = [self.rel[0] * r, -f64::from(b.x) + self.rel[1] * r, f64::from(b.y) + self.rel[2] * r];
                self.at.rot = mesh_yaw();
                play = play.or(self.tumble.map(|t| (t, f64::from(b.age - 1) * STEP)));
            }
        }
        if let (Some((c, t)), Some(clips)) = (play, host.clips_of(w, self.from)) {
            self.view.play(clips, c, t, true);
        }
        self.at.scale = self.scale.map(|v| v * r);
        self.shown = shown;
        if shown {
            self.view.set(&self.at);
        } else {
            // (shown again: from where it is then)
            self.view.cut();
        }
    }
}

impl ActorMesh for BombView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((a, b)) = bomb_of(w, self.id) else { return };
        if self.age != Some(b.age) {
            self.age = Some(b.age);
            let r = a.size_scale.unwrap_or(1.0);
            self.tick(host, w, b, f64::from(r));
        }
        self.view.present(alpha);
        self.view.visible = self.shown;
        self.view.draw(out);
    }

    /// The mesh component's world transform of the latest tick, under its component name (the particle components
    /// under it ride it: mvs-render fx/chars/harley.rs).
    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        out.push((Arc::from(BOMB_MESH), self.at));
    }
}

/// `legFollow`: AttachPoint at ATTACH_LOC turned by `rot` on fighter `c`'s socket `socket` (the bomb's place on its
/// carrier): socketFollow, a virtual bone's included (crate::virtual_bone); the place itself where the carrier has no
/// such socket.
fn leg_follow(host: &ActorHost<'_>, w: &World, c: FighterId, socket: &str, rot: &Rows) -> Xform {
    let rel = Xform { loc: ATTACH_LOC, rot: *rot, scale: [1.0; 3] };
    host.socket_follow(w, c, socket, &rel).unwrap_or_else(|| {
        crate::skin_intro::warn_once(format!("Harley: the bomb's carrier has no socket {socket}: at the origin"));
        rel
    })
}
