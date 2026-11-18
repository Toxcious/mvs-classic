//! ItemHelper_Nepter as a match draws it (the sim: mvs-sim items/nepter.rs, a World
//! actor), from its graph, Neptr_AnimBlueprint and the class's component templates.
//!
//! The mesh: Neptr_Skelmesh (one slot, Neptr_MAT) under the Capsule (the root, bAbsoluteScale) at (0, 0, -80), yaw
//! -40, scale 1; AnimClass Neptr_AnimBlueprint. The actor's rotation is the spawn transform's (Comp_Spawner
//! HandleActorPostSpawn @3326: WorldTransforms true takes SetTransforms, Item_Helper's location alone): none. Its
//! facing is the Capsule's world scale (1, 1, 1) / (1, -1, 1) (BeginPlay @1955 / @2427, Facing change @5025 /
//! @5086): the sim's scale_y. BeginPlay's dynamic instance of slot 0 @2068 takes TredAlpha (0 at BeginPlay @765, the
//! tick's @3109) and Light (SetThrowTimer @5741): the sim's tred_alpha and oven_light.
//!
//! Neptr_AnimBlueprint (crate::actor_anim runs it, once a sim tick):
//!   BlueprintUpdateAnimation @1902: d = the actor's location - LastLocation (LastLocation starts at 0, so the
//!   first update sees the whole location), Running = |d.Y| > 10 @2135, Falling = !NearlyEqual(d.Z, 0, 1) @2384,
//!   LastLocation @2662. IsJumping / JumpCount (the Jumping event @2694 -> @1351: true toggles JumpCount) and
//!   WallSlide (WallSlideSet @2718) come from the Nepter's multicasts: the sim keeps them (is_jumping, jump_count,
//!   wall_slide).
//!   AnimGraph: Root <- LayeredBoneBlend (base: the cached pose AfterMovement = machine Movement; layer 0: Slot
//!   DefaultSlot over the same cached pose; weight 1; branch filters Arm and Mouth, BlendDepth 0, local space):
//!   the throw montage moves the Arm and Mouth subtrees (bones 11-20 of 25, the CDO's PerBoneBlendWeights) and the
//!   machine goes on under it. Movement: entry Idle, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition and
//!   bReinitializeOnBecomingRelevant, every crossfade HermiteCubic (the table below, checked against the
//!   AnimGraph as decoded: states, players, crossfades and rule handlers agree).
//! ThrowPieMontageMulticast @5283: Neptr_Throw_Montage at rate 1 from 0 (BlendIn 0, BlendOut 0.05, auto blend-out):
//!   the sim's (mvs-sim actors/actor_montage.rs: the Nepter's MontageState, advanced by its mesh's tick, its
//!   Ak_OneShot run there); the Slot node draws its position and weight.
//! The fighter whose character data has the class (the user of the TV need not be it) has its mesh, clips, montage and
//! materials; a match with random items whose fighters lack it draws the items part's draw of the component and the
//! sequences of its folder (ItemsData's class data and montages).
//! Deviation: the view's AnimBP runs once a presented tick: a rollback's jump of several ticks advances it one tick.
//! Not drawn here: the spawn and tread sounds and the turn RTPC (the sound's).

use std::sync::Arc;

use mvs_sim::World;
use mvs_sim::actors::actor_montage::montage_of;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};

use super::{ClipSet, component, data_fighter};
use crate::actor_anim::{
    ActorMachine, ActorSlot, BlendOption, BoolListDef, BranchFilter, MachineDef, MachineQuery, PoseDef, SeqDef,
    StateDef, TransitionDef, branch_mask, layered_blend_in_place,
};
use crate::actor_view::{SkinnedActor, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows};
use crate::anim::Pose;
use crate::frame::MeshPose;
use crate::pose::set_scalar;
use crate::skin_intro::warn_once;

const CLS: &str = "ItemHelper_Nepter_C";
const MESH_COMP: &str = "Neptr_Skelmesh";
const STEP: f64 = 1.0 / 60.0;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[(CLS, MeshRule { rot: None, mesh: Some(neptr_mesh) })];

/// The AnimBP's variables the rules read.
#[derive(Debug, Default)]
struct NeptrInputs {
    running: bool,
    falling: bool,
    is_jumping: bool,
    jump_count: i32,
    wall_slide: bool,
}

type Rule = fn(&NeptrInputs, &dyn MachineQuery) -> bool;

fn seq(name: &'static str, looped: bool) -> Option<PoseDef<NeptrInputs>> {
    Some(PoseDef::Seq(SeqDef::new(name, looped)))
}

fn state(name: &'static str, pose: Option<PoseDef<NeptrInputs>>) -> StateDef<NeptrInputs> {
    StateDef { name, pose, entry_rule: None, always_reset: false }
}

fn edge(from: &'static str, to: &'static str, xfade: f64, rule: Rule) -> TransitionDef<NeptrInputs> {
    TransitionDef { from, to, xfade, option: BlendOption::HermiteCubic, rule: Some(rule), auto: false }
}

/// Movement (Neptr_AnimBlueprint BakedStateMachines[0]; rule offsets in Neptr_AnimBlueprint.txt).
fn movement() -> MachineDef<NeptrInputs> {
    // BlendListByBool(WallSlide): true Neptr_Wall, false Neptr_Fall, 0.1 s Linear each
    let fall = PoseDef::BoolList(BoolListDef {
        test: |i: &NeptrInputs| i.wall_slide,
        t: Box::new(PoseDef::Seq(SeqDef::new("Neptr_Wall", true))),
        f: Box::new(PoseDef::Seq(SeqDef::new("Neptr_Fall", true))),
        blend_time: [0.1, 0.1],
        option: BlendOption::Linear,
    });
    MachineDef {
        entry: "Idle",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            state("Idle", seq("Neptr_Idle", true)),
            state("Run", seq("Neptr_Run", true)),
            state("Jump", seq("Neptr_Jump", false)),
            state("Fall", Some(fall)),
            state("Land", seq("Neptr_Land", false)),
            state("Jump_1", seq("Neptr_Jump", false)),
            StateDef { name: "JumpSelect", pose: None, entry_rule: Some(|_| true), always_reset: false },
        ],
        transitions: vec![
            edge("Idle", "Fall", 0.2, |i, _| i.wall_slide || (i.falling && !i.is_jumping)), // @1475
            edge("Idle", "JumpSelect", 0.2, |i, _| i.is_jumping),
            edge("Idle", "Run", 0.1, |i, _| i.running),
            edge("Run", "Fall", 0.2, |i, _| i.falling && !i.is_jumping), // @1575
            edge("Run", "JumpSelect", 0.2, |i, _| i.is_jumping),
            edge("Run", "Idle", 0.1, |i, _| !i.running),
            edge("Jump", "Land", 0.0, |i, q| !i.falling && !i.is_jumping && q.time("Jump") > 0.1), // @1251
            edge("Jump", "Fall", 0.05, |i, _| i.wall_slide),
            edge("Jump", "Jump_1", 0.0, |i, _| i.is_jumping && i.jump_count == 1), // @945
            edge("Jump", "Fall", 0.0, |_, q| q.remaining("Jump") == 0.0),          // @840
            edge("Fall", "JumpSelect", 0.2, |i, _| i.is_jumping),
            edge("Fall", "Land", 0.0, |i, _| !i.wall_slide && !i.falling), // @630
            edge("Land", "Run", 0.05, |i, q| q.remaining("Land") <= 0.05 && i.running), // @501
            edge("Land", "Idle", 0.05, |i, q| q.remaining("Land") <= 0.05 && !i.running), // @253
            edge("Jump_1", "Fall", 0.05, |i, _| i.wall_slide),
            edge("Jump_1", "Land", 0.2, |i, q| !i.falling && !i.is_jumping && q.time("Jump_1") > 0.1), // @1814
            edge("Jump_1", "Jump", 0.0, |i, _| i.is_jumping && i.jump_count == 0),                     // @358
            edge("Jump_1", "Fall", 0.0, |_, q| q.remaining("Jump_1") == 0.0),                          // @735
            edge("JumpSelect", "Jump", 0.0, |i, _| i.jump_count == 0),                                 // @1012
            edge("JumpSelect", "Jump_1", 0.0, |i, _| i.jump_count == 1),                               // @1318
        ],
    }
}

const BRANCHES: [BranchFilter; 2] = [BranchFilter { bone: "Arm", depth: 0 }, BranchFilter { bone: "Mouth", depth: 0 }];

#[derive(Debug)]
struct NeptrView {
    id: ActorId,
    sk: SkinnedActor,
    set: ClipSet,
    machine: ActorMachine<NeptrInputs>,
    slot: ActorSlot,
    mask: Vec<f32>,
    base: Pose,
    layer: Pose,
    inp: NeptrInputs,
    /// LastLocation.
    last: [f64; 3],
    /// The mesh component's relative location and rotation rows.
    rel: [f64; 3],
    rel_rot: Rows,
    /// TredAlpha and Light as last set (-1: none yet).
    tred: f64,
    light: f64,
}

fn neptr_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let from = data_fighter(w, x.owner, x.cls);
    let c = component(w, cs, x.cls, MESH_COMP)?;
    let key = format!("{CLS}.{MESH_COMP}");
    let (sk, set) = match from {
        Some(f) => {
            let mesh = match &c.kind {
                ComponentKind::SkeletalMesh { mesh, .. } => Some(mesh.as_str()),
                _ => None,
            };
            let drawn = mesh.and_then(|m| host.draws_of(w, f)?.actor(m, None));
            let sk = drawn.and_then(|(path, keys)| Some(SkinnedActor::new(host.mesh(&path)?, &keys)));
            (sk, ClipSet::Fighter(f))
        }
        None => (SkinnedActor::item(host, &key), ClipSet::Items),
    };
    let Some(mut sk) = sk else {
        warn_once("Neptr: not drawn (its mesh or materials are not loaded)".into());
        return None;
    };
    // (drawn between the sim's ticks: place's present)
    sk.blend_ticks();
    let n = sk.names.len();
    let clips = set.clips(host, w)?;
    for name in ["Neptr_Idle", "Neptr_Run", "Neptr_Jump", "Neptr_Wall", "Neptr_Fall", "Neptr_Land"] {
        if clips.get(name).is_none() {
            warn_once(format!("Neptr: clip missing: {name}"));
        }
    }
    let _ = sk.skeleton(clips);
    let machine = ActorMachine::new(movement(), n, clips);
    let mask = branch_mask(&sk.names, &sk.parents, &BRANCHES);
    Some(Box::new(NeptrView {
        id,
        slot: ActorSlot::new("DefaultSlot", n),
        mask,
        base: Pose::new(n),
        layer: Pose::new(n),
        inp: NeptrInputs::default(),
        last: [0.0; 3],
        rel: c.loc,
        rel_rot: c.rot,
        tred: -1.0,
        light: -1.0,
        machine,
        sk,
        set,
    }))
}

impl ActorMesh for NeptrView {
    /// One sim tick of the view: the anim instance's update (BlueprintUpdateAnimation, the graph; the montage is the
    /// sim's), its pose, the component's transform and the material's two scalars.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::Nepter(a) = &x.state else { return };
        let loc = [a.depth, -a.x, a.y];
        let inp = &mut self.inp;
        inp.running = (f64::from(loc[1]) - self.last[1]).abs() > 10.0;
        inp.falling = !((f64::from(loc[2]) - self.last[2]).abs() <= 1.0);
        self.last = loc.map(f64::from);
        inp.is_jumping = a.is_jumping;
        inp.jump_count = a.jump_count;
        inp.wall_slide = a.wall_slide;
        let inst = a.anim.m.as_ref().and_then(|st| {
            let m = montage_of(&w.data, w, ActorRef::Actor(self.id), w.names.str(st.montage.name))?;
            Some((st, m))
        });
        if let Some(clips) = self.set.clips(host, w) {
            self.machine.update(&self.inp, STEP, clips);
            let sks = self.sk.skeleton(clips);
            self.machine.evaluate(clips, &sks, &mut self.base);
            if self.slot.apply(clips, &sks, inst, &self.base, &mut self.layer) {
                layered_blend_in_place(&mut self.base, &self.layer, &self.mask);
            }
        } else {
            self.base.copy(&self.sk.rest);
        }
        self.sk.show(&self.base);
        // the Capsule's world scale (1, scale_y, 1) over the mesh's relative transform
        let sy = if a.scale_y < 0.0 { -1.0 } else { 1.0 };
        let rot = std::array::from_fn(|j| self.rel_rot[j] * if j % 3 == 1 { sy } else { 1.0 });
        let at =
            [f64::from(loc[0]) + self.rel[0], f64::from(loc[1]) + self.rel[1] * sy, f64::from(loc[2]) + self.rel[2]];
        self.sk.set(&Xform { loc: at.map(f64::from), rot, scale: [1.0; 3] });
        let tred = a.tred_alpha;
        let light = if a.oven_light { 1.0 } else { 0.0 };
        if let Some(Some(m)) = self.sk.mats.first_mut() {
            if f64::from(tred) != self.tred {
                set_scalar(m, "TredAlpha", f64::from(tred));
            }
            if light != self.light {
                set_scalar(m, "Light", light);
            }
        }
        self.tred = f64::from(tred);
        self.light = light;
    }

    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        _w: &World,
        _at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.sk.present(alpha);
        self.sk.visible = shown;
        self.sk.draw(out);
    }

    /// The bones for a PlayVFX of the actor's own mesh.
    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        self.sk.bones_world(out);
    }
}
