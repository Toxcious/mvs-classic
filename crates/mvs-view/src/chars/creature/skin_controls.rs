//! Reindog's per-skin skeletal controls: what each skin turns on in
//! Creature_AnimBP (and Pawn_Reindog's OnSkinApplied), and the part of it the view draws.
//!
//! The skin's flags. Creature_AnimBP OnSkinApplied (@22095-23019) switches on SkinManager.AppliedSkin.Slug
//! (NotEqual_StriStri: any case) and Pawn_Reindog OnSkinApplied (@3534-4671) writes the same flags on its
//! Creature_AnimBP instance (it adds nothing the AnimBP's own switch does not set, and leaves out S05 / S07):
//!   skin_c009_s01, skin_c009_s10   Chef_Dynamics (@22043; the pawn @4429)
//!   skin_c009_s02                  ChineseDragon_Dynamics (@22055; @4463)
//!   skin_c009_s03                  Armor_Dynamics, AdjustBeard (@21710 -> @21721; @4497)
//!   skin_c009_s09                  the ElectricEye_L / _R particle components shown (@21733 loops @20934-21705; the
//!                                  pawn @4531-4565, not this file), then Armor_Dynamics, AdjustBeard (@21710)
//!   skin_c009_s04                  Elf_Dynamics, AdjustBeard (@22067 -> @21721; @4604)
//!   skin_c009_s08                  Mummy_Dynamics (@22083; @4638)
//!   skin_c009_s05                  TeaTime_Dynamics (@23019)
//!   skin_c009_s07                  AdjustBeard (@23014 -> @21721)
//!   any other slug (the default skin_creature_default): none (the switch has no default branch).
//! Chef / Mummy / TeaTime / Armor_Dynamics gate the alphas of their nodes in the exposed-input handlers of the
//! ubergraph; ChineseDragon_Dynamics and Elf_Dynamics are read by no function (xref): the ChineseDragon and elf nodes
//! run on every skin, at constant alphas (AnimDynamics_12 / _26 0.8, the others 1) or behind their curves alone. The
//! *DynamicsDisable curves (GetCurveValue < 0.1 in every gate) are carried by no sequence of the build (none under
//! Content names one), so they read 0 and every curve gate is open.
//!
//! The nodes of the graph, in its order (Root <- ComponentToLocal <- ModifyBone_3 <- ... <- ModifyBone_2 <-
//! ModifyBone_1 <- LocalToComponent <- the mirror node), with the bones each skin's mesh weights (checked against the
//! eight skin meshes):
//!   elf (S04's bones; every skin): Trail_24, AnimDynamics_26 / _12 (chains), AnimDynamics_25 / _22 / _24 / _23,
//!     Trail_15 / _14 / _13 / _12;
//!   ModifyBone (fml_un_beard: Rotation (0, 0, 30) BMM_Additive in BCS_ComponentSpace, translation and scale
//!     BMM_Ignore; alpha float(AdjustBeard) @14237-14274): S03 / S04 / S07 / S09 (every skin weights the beard);
//!   Trail_9 / _8 / _7 (hairLower8 / hairMiddle7 / hairTop7) and Trail_23 / _22 (L / R scarf4): Armor_Dynamics (S03,
//!     S09) x !IsHitStunned x !IsRespawning (@14699-15274);
//!   ChineseDragon (S02; every skin): Trail_21 / _20 / _19 / _11 / _10 / _18 / _17 / _16, AnimDynamics_14 / _13 / _21
//!     / _30 / _15 / _28 / _31 / _20 / _29 / _27;
//!   Trail_6 then SpringBone_3 (fml_un_rdg_chef_C_hat_end): SelectFloat(0.5, 0, Chef_Dynamics) x !IsHitStunned x
//!     !IsRespawning (@13861-14200, @14323-14662): S01;
//!   mummy (S08): AnimDynamics_19 / _18 / _17 / _16, Trail_5 / _4, SpringBone_2 (mummyWrap_Leg_R), SpringBone_1
//!     (mummyWrap_Leg_L1): Mummy_Dynamics x !IsHitStunned x !IsRespawning x !COM_Spin (@10240-10636, @11106-11502);
//!   tea party (S05): SpringBone (fml_un_tparty_C_hat), AnimDynamics_9 / _10 / _7 / _6, Trail_2 / _1,
//!     AnimDynamics_3 / _2 / _1 / _5 / _4, Trail, AnimDynamics_11 / AnimDynamics / _8, Trail_3: TeaTime_Dynamics x
//!     !IsHitStunned x !IsRespawning x !COM_Spin (@2878-10203; SpringBone @6343-6739).
//!
//! Drawn here: the beard ModifyBone and the skin's Trail, AnimDynamics and SpringBone nodes (crate::anim_nodes, made
//! from the fighter data's Creature_AnimBP nodes at the first set), in the graph's order (NODES), after ModifyBone_1
//! (the pose rule's spin). A node whose bones the view's mesh lacks (another skin's) is left out, as the build's node
//! is invalid on it; so the elf and ChineseDragon nodes run on S04 and S02 alone.
//! - Not run in a preview (the menus, the podium): the nodes are made from the fighter's data, which a preview has not.
//! - The beard: the bone's component-space rotation premultiplied by the node's quaternion (additive rotation = node
//!   quat x bone quat, component space a no-op conversion), written outright (alpha 1: LocalBlendCSBoneTransforms).
//!   FRotator(0, 0, 30)::Quaternion as crate::rotator::rotator_quat (stock 4.26: not read, as every user of it).
//!   Unverified: the node sits after the mirror node; the view mirrors the right-facing pose (Yaw(+90) M_Y), and
//!   M_Y Rx(a) M_Y = Rx(-a), so facing left the roll is taken as -30 on the unmirrored pose (the build's +30 on its
//!   mirrored pose), as the pose rule takes the spin's pitch (about Y, which M_Y keeps) as it is.
//! - The nodes: on the local pose after the spin, with the mesh component's world transform at the actor's facing
//!   (mesh_xf, as Bugs' pose). The pose is the unmirrored one under a yaw of +90 facing left, which is the build's
//!   mirrored world with its depth (UE X) reversed; the view's reflection turns the result back, so the bones' sideways
//!   and vertical lag are the build's.
//!   Deviation: SpringBoneNode's rotation step mixes a world target with a component parent position (the exe's),
//!   which this mapping does not keep for the depth axis (S01's hat end and S08's leg wraps rotate; S05's hat
//!   translates on Z alone). The same mapping turns the AnimDynamics nodes' gravity and the component's motion the
//!   build's way (both are in the screen plane or along the vertical), so they are kept.
//!   Unverified: the two Actor-space AnimDynamics nodes take the actor as the component at scale 1
//!   (AnimDynamicsNode::evaluate).
//!   The update's DeltaTime carries the mesh's CustomTimeDilation (0.005 in hitpause, the pose rule's). IsHitStunned
//!   is Comp_Pawn_HitstunManager.CheckHitstunActive in BlueprintUpdateAnimation (@645-712): the fighter's in_hitstun
//!   (Unverified: the same reading as the sim's). IsRespawning and COM_Spin are the sim's (ReindogState).

use mvs_sim::World;
use mvs_sim::fighter::FighterId;

use crate::anim::Pose;
use crate::anim_nodes::{AnimNode, AnimNodeSkel, BoneXfs, CsPose, UeXf, mesh_xf, nodes_of_data, qmul};
use crate::rotator::rotator_quat;
use crate::view::CharView;

/// A flag OnSkinApplied's switch sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    Chef,
    ChineseDragon,
    Armor,
    Elf,
    Mummy,
    TeaTime,
    AdjustBeard,
}

/// Creature_AnimBP OnSkinApplied's switch, by the slug in lower case (`SKIN_FLAGS`).
const SKIN_FLAGS: &[(&str, &[Flag])] = &[
    ("skin_c009_s01", &[Flag::Chef]),
    ("skin_c009_s02", &[Flag::ChineseDragon]),
    ("skin_c009_s03", &[Flag::Armor, Flag::AdjustBeard]),
    ("skin_c009_s09", &[Flag::Armor, Flag::AdjustBeard]),
    ("skin_c009_s04", &[Flag::Elf, Flag::AdjustBeard]),
    ("skin_c009_s08", &[Flag::Mummy]),
    ("skin_c009_s10", &[Flag::Chef]),
    ("skin_c009_s05", &[Flag::TeaTime]),
    ("skin_c009_s07", &[Flag::AdjustBeard]),
];

/// A node's alpha: a constant (the elf and ChineseDragon nodes, which no flag gates), or its flag's (x 0.5 for the
/// chef's SelectFloat) times !IsHitStunned x !IsRespawning, and x !COM_Spin where `spin`.
#[derive(Clone, Copy, Debug)]
enum Gate {
    Const(f64),
    /// No flag: !IsHitStunned x !IsRespawning (x !COM_Spin where `spin`); the *DynamicsDisable curve reads 0.
    Open {
        spin: bool,
    },
    Flag {
        flag: Flag,
        k: f64,
        spin: bool,
    },
}

const fn on(flag: Flag, k: f64, spin: bool) -> Gate {
    Gate::Flag { flag, k, spin }
}

const ARMOR: Gate = on(Flag::Armor, 1.0, false);
const CHEF: Gate = on(Flag::Chef, 0.5, false);
const MUMMY: Gate = on(Flag::Mummy, 1.0, true);
const TEA: Gate = on(Flag::TeaTime, 1.0, true);
const ONE: Gate = Gate::Const(1.0);
const OPEN: Gate = Gate::Open { spin: false };
const OPEN_SPIN: Gate = Gate::Open { spin: true };
const ARMOR_SPIN: Gate = on(Flag::Armor, 1.0, true);

/// The skin nodes of Creature_AnimBP after ModifyBone_1 (Trail, AnimDynamics, SpringBone), in the graph's order (the
/// header), each with its gate. Unverified: the beard's ModifyBone sits between the elf nodes and the armour's Trails; the beard
/// bone is under none of the elf nodes' bones, so it is turned first here.
const NODES: &[(&str, Gate)] = &[
    ("AnimGraphNode_Trail_24", OPEN),
    ("AnimGraphNode_AnimDynamics_26", Gate::Const(0.8)),
    ("AnimGraphNode_AnimDynamics_12", Gate::Const(0.8)),
    ("AnimGraphNode_AnimDynamics_25", ONE),
    ("AnimGraphNode_AnimDynamics_22", ONE),
    ("AnimGraphNode_AnimDynamics_24", ONE),
    ("AnimGraphNode_AnimDynamics_23", ONE),
    ("AnimGraphNode_Trail_15", ONE),
    ("AnimGraphNode_Trail_14", ONE),
    ("AnimGraphNode_Trail_13", ONE),
    ("AnimGraphNode_Trail_12", ONE),
    ("AnimGraphNode_Trail_9", ARMOR),
    ("AnimGraphNode_Trail_8", ARMOR),
    ("AnimGraphNode_Trail_7", ARMOR),
    ("AnimGraphNode_Trail_23", ARMOR_SPIN),
    ("AnimGraphNode_Trail_22", ARMOR_SPIN),
    ("AnimGraphNode_Trail_21", OPEN_SPIN),
    ("AnimGraphNode_Trail_20", OPEN_SPIN),
    ("AnimGraphNode_Trail_19", OPEN_SPIN),
    ("AnimGraphNode_Trail_11", OPEN_SPIN),
    ("AnimGraphNode_Trail_10", OPEN_SPIN),
    ("AnimGraphNode_Trail_18", OPEN_SPIN),
    ("AnimGraphNode_Trail_17", OPEN_SPIN),
    ("AnimGraphNode_Trail_16", OPEN_SPIN),
    ("AnimGraphNode_AnimDynamics_14", ONE),
    ("AnimGraphNode_AnimDynamics_13", ONE),
    ("AnimGraphNode_AnimDynamics_21", ONE),
    ("AnimGraphNode_AnimDynamics_30", ONE),
    ("AnimGraphNode_AnimDynamics_15", ONE),
    ("AnimGraphNode_AnimDynamics_28", ONE),
    ("AnimGraphNode_AnimDynamics_31", ONE),
    ("AnimGraphNode_AnimDynamics_20", ONE),
    ("AnimGraphNode_AnimDynamics_29", ONE),
    ("AnimGraphNode_AnimDynamics_27", ONE),
    ("AnimGraphNode_Trail_6", CHEF),
    ("AnimGraphNode_SpringBone_3", CHEF),
    ("AnimGraphNode_AnimDynamics_19", MUMMY),
    ("AnimGraphNode_AnimDynamics_18", MUMMY),
    ("AnimGraphNode_AnimDynamics_17", MUMMY),
    ("AnimGraphNode_AnimDynamics_16", MUMMY),
    ("AnimGraphNode_Trail_5", MUMMY),
    ("AnimGraphNode_Trail_4", MUMMY),
    ("AnimGraphNode_SpringBone_2", MUMMY),
    ("AnimGraphNode_SpringBone_1", MUMMY),
    ("AnimGraphNode_SpringBone", TEA),
    ("AnimGraphNode_AnimDynamics_9", TEA),
    ("AnimGraphNode_AnimDynamics_10", TEA),
    ("AnimGraphNode_AnimDynamics_7", TEA),
    ("AnimGraphNode_AnimDynamics_6", TEA),
    ("AnimGraphNode_Trail_2", TEA),
    ("AnimGraphNode_Trail_1", TEA),
    ("AnimGraphNode_AnimDynamics_3", TEA),
    ("AnimGraphNode_AnimDynamics_2", TEA),
    ("AnimGraphNode_AnimDynamics_1", TEA),
    ("AnimGraphNode_AnimDynamics_5", TEA),
    ("AnimGraphNode_AnimDynamics_4", TEA),
    ("AnimGraphNode_Trail", TEA),
    ("AnimGraphNode_AnimDynamics_11", TEA),
    ("AnimGraphNode_AnimDynamics", TEA),
    ("AnimGraphNode_AnimDynamics_8", TEA),
    ("AnimGraphNode_Trail_3", TEA),
];

const BEARD: &str = "fml_un_beard";
/// ModifyBone.Rotation.Roll.
const BEARD_ROLL: f64 = 30.0;

/// The skin's nodes on one view (`skinDynamics`' object).
#[derive(Debug)]
pub(super) struct SkinDynamics {
    flags: &'static [Flag],
    beard: Option<usize>,
    /// The nodes the view's mesh holds the bones of (made from the fighter's data at the first set: None before),
    /// and the gate of each.
    nodes: Option<Vec<Box<dyn AnimNode>>>,
    cs: CsPose,
    gates: Vec<Gate>,
    alphas: Vec<f64>,
    comp: UeXf,
    q_r: [f64; 4],
    q_l: [f64; 4],
    out: BoneXfs,
    /// A fighter's view this tick (set ran, not slaved).
    live: bool,
    slaved: bool,
    left: bool,
    dilation: f64,
}

impl SkinDynamics {
    /// The skin's nodes on a view. `slaved`: the nodes are left to the master (the main mesh's pose).
    pub(super) fn new(view: &CharView, slug: &str, skin: &str, slaved: bool) -> Option<SkinDynamics> {
        let flags: &[Flag] = SKIN_FLAGS.iter().find(|(s, _)| s.eq_ignore_ascii_case(slug)).map_or(&[], |(_, f)| f);
        let mut beard = None;
        if flags.contains(&Flag::AdjustBeard) {
            beard = view.rig.index(BEARD);
            if beard.is_none() {
                eprintln!("warning: Reindog pose: skin {skin}: bone {BEARD} missing: the beard is not turned");
            }
        }
        if beard.is_none() && slaved {
            return None;
        }
        Some(SkinDynamics {
            flags,
            beard,
            nodes: None,
            cs: CsPose::new(&view.rig.names, &view.parents),
            gates: Vec::new(),
            alphas: Vec::new(),
            comp: UeXf::default(),
            q_r: rotator_quat(0.0, 0.0, BEARD_ROLL),
            q_l: rotator_quat(0.0, 0.0, -BEARD_ROLL),
            out: BoneXfs::default(),
            live: false,
            slaved,
            left: false,
            dilation: 1.0,
        })
    }

    /// The nodes of NODES whose bones the view's mesh holds, from the fighter's Creature_AnimBP data; then InitAnim's
    /// DeltaTime-0 tick (crate::anim_nodes header).
    fn make(&mut self, w: &World, f: FighterId, view: &CharView) {
        let skel = AnimNodeSkel { names: &view.rig.names, parents: &view.parents, rest: Some(&view.rest) };
        let Some(data) = w.char_data(f).fighter.anim_nodes.as_ref() else {
            eprintln!("warning: Reindog pose: the fighter data has no anim nodes: the skin's nodes are not run");
            self.nodes = Some(Vec::new());
            return;
        };
        let names: Vec<&str> = NODES.iter().map(|n| n.0).collect();
        let (chain, idx) = nodes_of_data(data, &names, &skel);
        let mut nodes = chain.nodes;
        self.gates = idx.iter().map(|&i| NODES[i].1).collect();
        self.alphas = vec![0.0; idx.len()];
        self.gate(false, false, false);
        for (n, &a) in nodes.iter_mut().zip(&self.alphas) {
            n.initialize();
            n.pre_update(1.0);
            n.update(0.0, a);
        }
        self.nodes = Some(nodes);
    }

    fn gate(&mut self, hit_stunned: bool, respawning: bool, com_spin: bool) {
        for (a, g) in self.alphas.iter_mut().zip(&self.gates) {
            *a = match *g {
                Gate::Const(k) => k,
                Gate::Open { spin } => f64::from(u8::from(!hit_stunned && !respawning && !(spin && com_spin))),
                Gate::Flag { flag, k, spin } => {
                    let ok = self.flags.contains(&flag) && !hit_stunned && !respawning && !(spin && com_spin);
                    if ok { k } else { 0.0 }
                }
            };
        }
    }

    /// The update's inputs of the tick.
    pub(super) fn set(
        &mut self,
        w: &World,
        f: FighterId,
        view: &CharView,
        dilation: f64,
        respawning: bool,
        com_spin: bool,
    ) {
        self.live = !self.slaved;
        if self.live && self.nodes.is_none() {
            self.make(w, f, view);
        }
        let facing = w.fighter(f).body.facing_dir;
        self.left = facing < 0.0;
        self.dilation = dilation;
        self.comp = mesh_xf(
            f64::from(-mvs_sim::fighter::mesh_wx(w, f)),
            f64::from(mvs_sim::fighter::mesh_wy(w, f)) + view.mesh_z * view.size_scale,
            f64::from(facing),
            view.mesh_scale * view.size_scale,
        );
        self.gate(mvs_sim::fighter::in_hitstun(w, f), respawning, com_spin);
    }

    /// `preview()`: a view with no fighter: no node runs, the beard turned as facing right.
    pub(super) fn preview(&mut self) {
        self.live = false;
        self.left = false;
    }

    /// The nodes on the local pose (the pose rule, after the spin).
    pub(super) fn apply(&mut self, p: &mut Pose, dt: f64) {
        let nodes = if self.live { self.nodes.as_deref_mut().unwrap_or_default() } else { &mut [] };
        for (n, &a) in nodes.iter_mut().zip(&self.alphas) {
            n.pre_update(1.0);
            n.update(dt * self.dilation, a);
        }
        let mut cs = self.cs.begin(p);
        if let Some(b) = self.beard {
            // ModifyBone: rotation additive in component space, alpha float(AdjustBeard) = 1
            let x = cs.get(b);
            let q = qmul(if self.left { self.q_l } else { self.q_r }, x.q);
            self.out.clear();
            self.out.push(b, UeXf { t: x.t, q, s: x.s });
            cs.local_blend(&self.out, 1.0);
        }
        for n in nodes {
            n.evaluate(&mut cs, &self.comp);
        }
    }
}
