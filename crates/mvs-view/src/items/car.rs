//! The FFA cars (mvs-sim items/car.rs: Parent_Car_SpawnedActor_C's children BatMobile_SpawnedActor_Child_C and
//! MysteryMachine_SpawnedCar_C, World.projectiles) as a match draws them: their
//! SkeletalMesh with its AnimBP.
//!
//! The components (the classes' SCS): Capsule (the root) > CarMesh (an empty StaticMeshComponent, bVisible false, at
//! (38.85, -0.95, -55.10), yaw -90, scale 2) > SkeletalMesh (BatMobile: Batmobile_Car_SkelMesh at yaw 180, scale 0.5,
//! AnimClass Batmobile_Car_SkelMesh_Skeleton_AnimBlueprint; MysteryMachine: Velma_MysteryMachine_SkelMesh at (0, -19.05,
//! 0), scale 0.5, AnimClass MysteryMachine_AnimBP).
//! The mesh is placed as crate::items::class_view places a component (the actor's place and rotation, its attach
//! parents, the actor's scale) in the items part's template set (class_view's Unverified on SetTeamColor, and no
//! outline). HitShake moves CarMesh (Gizmo's car_mesh_loc; Parent_Car_SpawnedActor ReceiveAnyDamage @11989 HitShake,
//! its Update @17515), the sim's shake_age. Parent_Car's HitShake sets CarAnimBP's IsInHit / HitInt (@12397), members of
//! neither car's AnimBP: no hit pose.
//! Each AnimBP runs once per tick of the car's age (several after a jump of the age, at most 30; a rollback's earlier
//! age runs one), from what the sim holds of the car, as Gizmo's car.
//! Deviation: the AnimBP runs once a presented tick (a rollback's jump of several ticks runs them with the latest
//! place).
//! Not drawn: TurnVisuals' PS_RunTrail / TurnTireTracks (Parent_Car_SpawnedActor @14478 / @14809) and BatMobile's
//! BlobShadow decal. The particle components (the overheat PS, BatMobile's ThrusterEffect on the bone Body this
//! view's `bones` gives) are mvs-render fx/items.rs's.

use mvs_sim::World;
use mvs_sim::actors::{ActorRef, ProjectileExt, ProjectileId};
use mvs_sim::chars::gizmo::car::{CarActor, car_of};
use mvs_sim::items::car::{BATMOBILE, MYSTERY_MACHINE};

use super::{ClipSet, component};
use crate::actor_anim::{
    ActorMachine, BlendOption, BranchFilter, MachineDef, MachineQuery, PoseDef, SeqDef, StateDef, TransitionDef,
    branch_mask, layered_blend_in_place,
};
use crate::actor_view::{Comp, SkinnedActor, place_under, xyz};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, class_def};
use crate::anim::{Pose, accumulate_held_of, blend_pose, sample_clip};
use crate::chars::gizmo::car_mesh_loc;
use crate::chars::velma::{NavInputs, navigation};
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;

const MESH: &str = "SkeletalMesh";
const STEP: f64 = 1.0 / 60.0;
const ZERO_W: f64 = 1e-5;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (BATMOBILE, MeshRule { rot: None, mesh: Some(car_mesh) }),
    (MYSTERY_MACHINE, MeshRule { rot: None, mesh: Some(car_mesh) }),
];

// ---- Batmobile_Car_SkelMesh_Skeleton_AnimBlueprint ------------------------------------------------------------------
// AnimGraph (as decoded): Root <- LayeredBoneBlend (local space, weight 1; base: Slot
// DefaultSlot (bAlwaysUpdateSourcePose false) <- ApplyAdditive (alpha 1; base: the machine Movement, its one state Idle =
// Batmobile_Idle looped; additive: the machine AdditiveState); layer: Batmobile_Idle looped at PlayRate RPS, on the
// branches R_BackWheel, L_BackWheel, L_FrontWheel, R_FrontWheel, depth 0).
// BlueprintUpdateAnimation (@599-788, then @10-234): Speed = |StoredLocation - the mesh component's location| /
// DeltaSeconds, StoredLocation = that location (the CDO's (0, 0, 0) before the first update), RPS = Speed <= 40 ? 0 :
// 2600 / Speed. TransitionSpeed 1500 (the CDO).
// The Slot updates its source only while the montage leaves it a weight (FAnimNode_Slot::Update), so the two machines
// miss the ticks of a montage at full weight and start over at their entry when it blends out
// (bReinitializeOnBecomingRelevant).
const TRANSITION_SPEED: f64 = 1500.0;
const WHEELS: [BranchFilter; 4] = [
    BranchFilter { bone: "R_BackWheel", depth: 0 },
    BranchFilter { bone: "L_BackWheel", depth: 0 },
    BranchFilter { bone: "L_FrontWheel", depth: 0 },
    BranchFilter { bone: "R_FrontWheel", depth: 0 },
];
const ADDITIVE_SEQS: [&str; 3] = ["Batmobile_Idle_Additive", "Batmobile_Start", "Batmobile_Stop"];

#[derive(Debug, Default)]
struct BatInputs {
    speed: f64,
}

type BatRule = fn(&BatInputs, &dyn MachineQuery) -> bool;

fn bat_state(name: &'static str, seq: &'static str, looped: bool) -> StateDef<BatInputs> {
    StateDef { name, pose: Some(PoseDef::Seq(SeqDef::new(seq, looped))), entry_rule: None, always_reset: false }
}

fn bat_edge(from: &'static str, to: &'static str, xfade: f64, rule: BatRule) -> TransitionDef<BatInputs> {
    TransitionDef { from, to, xfade, option: BlendOption::HermiteCubic, rule: Some(rule), auto: false }
}

/// AdditiveState (MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, entry Idle; every crossfade HermiteCubic).
fn additive_state() -> MachineDef<BatInputs> {
    MachineDef {
        entry: "Idle",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            bat_state("Idle", "Batmobile_Idle_Additive", true),
            bat_state("Run", "Batmobile_Start", false),
            bat_state("Stop", "Batmobile_Stop", false),
        ],
        transitions: vec![
            bat_edge("Idle", "Run", 0.0, |i, _| i.speed >= TRANSITION_SPEED), // T2 @266
            bat_edge("Run", "Stop", 0.0, |i, _| i.speed < TRANSITION_SPEED),  // T1 @518
            bat_edge("Stop", "Run", 0.05, |i, _| i.speed >= TRANSITION_SPEED), // T3 @337
            bat_edge("Stop", "Idle", 0.0, |_, q| q.remaining("Stop") <= 0.0), // T0 @413
        ],
    }
}

/// Movement: its one state.
fn movement() -> MachineDef<BatInputs> {
    MachineDef {
        entry: "Idle",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![bat_state("Idle", "Batmobile_Idle", true)],
        transitions: vec![],
    }
}

// TurnVisuals (BatMobile_SpawnedActor_Child @1594-1670: the parent's, then CreateProxyObjectForPlayMontage(SkeletalMesh,
// FacingLeft ? Batmobile_Turn_02_Montage : Batmobile_Turn_Montage, 1.0, 0.0)), at each DriverTurnEvent (the sim's turns,
// the driver's facing then). Both montages: one section, a DefaultSlot segment of their sequence (Batmobile_Turn /
// Batmobile_Turn_02) from 0 to 0.25 s at rate 1, BlendIn 0, BlendOut 0.1 (linear), auto blend-out, no notifies; their
// proxy's events lead nowhere (@29-184).
// Deviation: the montage is the view's (its position from the tick of the turn), not a MontagePlayer of the sim: it
// has no notify and no event the graph reads.
// Unverified: its first tick's position (one tick in, as the Batcave stage's car montage is taken).
const TURN_LEN: f64 = 0.25;
const TURN_BLEND_OUT: f64 = 0.1;

#[derive(Debug)]
struct BatAnim {
    movement: ActorMachine<BatInputs>,
    additive: ActorMachine<BatInputs>,
    inp: BatInputs,
    wheel_clip: Option<usize>,
    /// Right, left.
    turn: [Option<usize>; 2],
    add_clip: Option<usize>,
    all: Vec<usize>,
    mask: Vec<f32>,
    base: Pose,
    delta: Pose,
    tmp: Pose,
    /// StoredLocation.
    stored: [f64; 3],
    rps: f64,
    wheel_t: f64,
    /// The Slot's source was updated in the last tick.
    source_updated: bool,
    turns: i32,
    /// The car's age at the last turn's Montage_Play, -1 none.
    turn_at: i32,
    turn_left: bool,
}

impl BatAnim {
    fn new(sk: &SkinnedActor, clips: &super::Clips, turns: i32) -> BatAnim {
        let clip = |n: &str| {
            let c = clips.get(n);
            if c.is_none() {
                warn_once(format!("BatMobile AnimBP: the clip is not loaded: {n}"));
            }
            c
        };
        let n = sk.names.len();
        for s in ADDITIVE_SEQS {
            if let Some(c) = clip(s)
                && !clips.clips[c].additive
            {
                warn_once(format!("BatMobile AnimBP: {s} is not an additive sequence: the additive is not drawn"));
            }
        }
        BatAnim {
            movement: ActorMachine::new(movement(), n, clips),
            additive: ActorMachine::new(additive_state(), n, clips),
            inp: BatInputs::default(),
            wheel_clip: clip("Batmobile_Idle"),
            turn: [clip("Batmobile_Turn"), clip("Batmobile_Turn_02")],
            add_clip: clip("Batmobile_Idle_Additive"),
            all: (0..n).collect(),
            mask: branch_mask(&sk.names, &sk.parents, &WHEELS),
            base: Pose::new(n),
            delta: Pose::new(n),
            tmp: Pose::new(n),
            stored: [0.0; 3],
            rps: 0.0,
            wheel_t: 0.0,
            source_updated: true,
            turns,
            turn_at: -1,
            turn_left: false,
        }
    }

    /// The turn montage's weight and position at the car's age (0: none).
    fn turn_at0(&self, age: i32) -> (f64, f64) {
        if self.turn_at < 0 || age < self.turn_at {
            return (0.0, 0.0);
        }
        let t = f64::from(age - self.turn_at + 1) * STEP;
        let w = if t <= TURN_LEN - TURN_BLEND_OUT { 1.0 } else { ((TURN_LEN - t) / TURN_BLEND_OUT).max(0.0) };
        (w, t.min(TURN_LEN))
    }

    /// `n` ticks: BlueprintUpdateAnimation and the graph's update. `loc`: the mesh component's world location (UE).
    fn update(&mut self, clips: &super::Clips, car: &CarActor, driver_left: bool, loc: [f64; 3], age: i32, n: i32) {
        // DriverTurnEvent -> TurnVisuals: a new montage (bStopAllMontages) at the driver's facing of now
        if car.turns > self.turns {
            self.turn_at = age;
            self.turn_left = driver_left;
        }
        self.turns = car.turns;
        let d = (0..3).map(|k| (loc[k] - self.stored[k]).powi(2)).sum::<f64>().sqrt();
        self.stored = loc;
        let speed = d / (f64::from(n) * STEP);
        self.inp.speed = speed;
        self.rps = if speed <= 40.0 { 0.0 } else { 2600.0 / speed };
        let relevant = 1.0 - self.turn_at0(age).0 > ZERO_W;
        for _ in 0..n {
            if relevant {
                if !self.source_updated {
                    self.movement.reinit();
                    self.additive.reinit();
                }
                self.movement.update(&self.inp, STEP, clips);
                self.additive.update(&self.inp, STEP, clips);
            }
            self.source_updated = relevant;
            let l = self.wheel_clip.map_or(0.0, |c| clips.clips[c].anim.length);
            if l > 0.0 {
                self.wheel_t = (((self.wheel_t + STEP * self.rps) % l) + l) % l;
            }
        }
    }

    /// The graph's pose at the car's age, shown on the mesh.
    fn evaluate(&mut self, sk: &mut SkinnedActor, clips: &mut super::Clips, age: i32) {
        let (w, t) = self.turn_at0(age);
        let turn = self.turn[usize::from(self.turn_left)];
        let sks = sk.skeleton(clips);
        let mut on_tmp = false;
        if w < 1.0 - ZERO_W || turn.is_none() {
            self.movement.evaluate(clips, &sks, &mut self.base);
            if let Some(c) = self.add_clip.filter(|&c| clips.clips[c].additive) {
                self.additive.evaluate(clips, &sks, &mut self.delta);
                accumulate_held_of(clips, c, sks.rig, &self.delta, &self.all, 1.0, true, &mut self.base);
            }
        }
        if let Some(c) = turn.filter(|_| w > ZERO_W) {
            sample_clip(clips, c, sks.rig, sks.rest, t, &mut self.tmp, sks.root);
            if w >= 1.0 - ZERO_W {
                on_tmp = true;
            } else {
                let base = self.base.clone();
                blend_pose(&base, &self.tmp, w, &mut self.base);
            }
        }
        let p = if on_tmp { &mut self.tmp } else { &mut self.base };
        if let Some(c) = self.wheel_clip {
            sample_clip(clips, c, sks.rig, sks.rest, self.wheel_t, &mut self.delta, sks.root);
            layered_blend_in_place(p, &self.delta, &self.mask);
        }
        sk.show(p);
    }
}

// ---- MysteryMachine_AnimBP ------------------------------------------------------------------------------------------
// AnimGraph: Root <- Slot DefaultSlot (bAlwaysUpdateSourcePose; no graph plays a montage on the item car's mesh) <- the
// machine Navigation: Velma's police car's (crate::chars::velma::navigation; the AnimBP's conduit "To fall", its entry
// rule false with no handler, is never entered and is left out). Its inputs: BlueprintInitializeAnimation (@15-257)
// Fighter = the owning car's Instigator (the picker the car was spawned for: the sim's owner) and IsJumping by his
// OnIsJumping (the sim's cmd_jumped); SetHorizontalSpeed (each BlueprintUpdateAnimation @1672) HorizontalSpeed = |his
// velocity's Y| (|vx|) and IsFalling = his CMC's IsFalling (!grounded).

#[derive(Debug)]
struct MysteryAnim {
    nav: ActorMachine<NavInputs>,
    inp: NavInputs,
    out: Pose,
}

impl MysteryAnim {
    fn update(&mut self, clips: &super::Clips, w: &World, owner: Option<mvs_sim::fighter::FighterId>, n: i32) {
        self.inp = match owner {
            Some(f) => NavInputs {
                speed: f64::from(w.fighter(f).body.vx.abs()),
                falling: !mvs_sim::fighter::grounded(w, f),
                jumping: w.fighter(f).moves.cmd_jumped,
            },
            None => NavInputs::default(),
        };
        for _ in 0..n {
            self.nav.update(&self.inp, STEP, clips);
        }
    }
}

// ---- the view -------------------------------------------------------------------------------------------------------

#[derive(Debug)]
enum Anim {
    Bat(Box<BatAnim>),
    Mystery(Box<MysteryAnim>),
}

#[derive(Debug)]
struct CarView {
    id: ProjectileId,
    sk: SkinnedActor,
    anim: Anim,
    /// SkeletalMesh under its attach parents (CarMesh the shake moves).
    comp: Comp,
    /// CarMesh's template location.
    cm0: [f64; 3],
    last_age: i32,
}

fn car_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let key = format!("{}.{MESH}", w.names.str(pr.cls));
    let comp = component(w, cs, pr.cls, MESH);
    let (Some(sk), Some(c)) = (SkinnedActor::item(host, &key), comp) else {
        warn_once(format!(
            "{}: the car is not drawn ({key} not in the items part's draws or not loaded, or no {MESH} component)",
            w.names.str(pr.cls)
        ));
        return None;
    };
    let n = sk.names.len();
    let clips = ClipSet::Items.clips(host, w)?;
    let anim = match &pr.ext {
        ProjectileExt::BatMobileCar(b) => Anim::Bat(Box::new(BatAnim::new(&sk, clips, b.item.car.turns))),
        _ => Anim::Mystery(Box::new(MysteryAnim {
            nav: ActorMachine::new(navigation(), n, clips),
            inp: NavInputs::default(),
            out: Pose::new(n),
        })),
    };
    // CarMesh's template location: the mesh's place under a CarMesh the shake moves (Capsule, the root, is the actor's
    // own transform: a change of CarMesh's location moves the mesh by as much in the root's frame)
    let cm0 = class_def(w, a)
        .and_then(|d| d.components.get("CarMesh"))
        .and_then(|c| c.as_obj())
        .map_or([0.0; 3], |c| xyz(c.get("RelativeLocation"), 0.0));
    let comp = Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale };
    Some(Box::new(CarView { id, sk, anim, comp, cm0, last_age: -1 }))
}

impl ActorMesh for CarView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(pr) = w.actors.projectiles.get(self.id).filter(|pr| !pr.dead) else {
            self.sk.visible = false;
            return;
        };
        self.sk.visible = shown;
        let Some(car) = car_of(&pr.ext).filter(|_| shown) else { return };
        let cl = car_mesh_loc(pr.age, car.shake_age, self.cm0);
        let mut comp = self.comp.clone();
        for (k, l) in comp.par.loc.iter_mut().enumerate() {
            *l = mvs_sim::math::narrow(f64::from(*l) + cl[k] - self.cm0[k]);
        }
        let (x, y) = at.at(alpha);
        let xf = place_under(&comp, &at.rot, at.scale, x, y);
        self.sk.set(&xf);
        let age = pr.age;
        if age != self.last_age {
            let n = if self.last_age < 0 || age < self.last_age { 1 } else { (age - self.last_age).min(30) };
            self.last_age = age;
            if let Some(clips) = ClipSet::Items.clips(host, w) {
                match &mut self.anim {
                    Anim::Bat(b) => {
                        let who = car.driver.or(pr.owner);
                        let left = who.is_some_and(|f| mvs_sim::fighter::facing(w, f) < 0.0);
                        b.update(clips, car, left, xf.loc, age, n);
                        b.evaluate(&mut self.sk, clips, age);
                    }
                    Anim::Mystery(m) => {
                        m.update(clips, w, pr.owner, n);
                        let sks = self.sk.skeleton(clips);
                        m.nav.evaluate(clips, &sks, &mut m.out);
                        self.sk.show(&m.out);
                    }
                }
            }
        }
        self.sk.draw(out);
    }

    /// The bones for the montage's and the particle components' attach.
    fn bones(&self, out: &mut Vec<(std::sync::Arc<str>, crate::actor_view::Xform)>) {
        self.sk.bones_world(out);
    }
}
