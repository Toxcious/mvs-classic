//! Jerry_SpawnedActor on the drawing side (the sim: mvs-sim
//! chars/tom_and_jerry/jerry.rs, a World actor, and its follower half follower.rs); offsets are each class's
//! ubergraph's. Its effects (Trail_Effect, PS_TomJerry_ChampagnePop, PS_AttackChargeUP) are the effects side's
//! (mvs-render fx/chars/tom_and_jerry.rs), on the bones the view enters ([`TRAIL`], [`CHARGE`], [`NECK`]).
//!
//! JerryAnimBlueprint (crate::actor_anim runs it, once a sim tick): Root <- WAnimationMirror (#8, FacingLeft: the
//! reflection above) <- Slot DefaultSlot (#41: the sim's montage, the actor's ActorAnim) <- BlendListByBool #1
//! (IsFlying: no graph sets it true (@26983 writes false), so its false pose) <- BlendListByBool_1 #7 (HitReact:
//! BlendListByInt #6 of Only_Jerry_HR_01..04 (looping; blend times 0, 0, 0.1, 0.1) by HitReactInt, else Movement #9;
//! blend times 0).
//! Unverified: the BlendListByInt is run as nested BlendListByBools, each with the blend time of the child it picks.
//! Movement (#9): entry Intro, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, bReinitializeOnBecomingRelevant;
//! every crossfade HermiteCubic; each state's transitions in their baked order. WallSlide
//! has no pose and no transition into it: left out.
//! BlueprintUpdateAnimation @646-2099: with the follower (UpdateFollower @2349, called by the attach @3142 and never
//! cleared): IsFalling, IsCrouching, IsMovingOnGround and HorizontalSpeed / VerticalSpeed of its CMC; without:
//! HorizontalSpeed / VerticalSpeed of the actor's ProjectileMovement and IsFalling = VerticalSpeed != 0 @454.
//! UpdateFollower binds OnIsWallSliding, OnJumpNumberChange, OnIsJumping and OnIsFastFalling (@2368-2583): the
//! JerryActor's wall_sliding / jump_number / jumping / fast_fall (the sim copies them).
//! Deviation: the view's AnimBP runs once a presented tick: a rollback's jump of several ticks advances
//! it one tick.
//!
//! The mesh: JerryMesh (Jerry_SkelMesh, the skin's JerryMesh sub-mesh asset; (0, 0, -80) on the Capsule, yaw -90,
//! scale 1.2). Its material: SetMaterial(0) @12958 from the owner's skin SubMeshes[JerryMesh].Materials[0] by the
//! colour (Red / Blue / LocalPlayer override): the skin's set for the sub-mesh in the owner's variant.
//! Its place: the actor at the sim's x / y; while following, the depth UE X = Follow_Y_Offset = the follower's scaled
//! capsule radius (SetFollowingVariables @356; the actor's relative Y SelectFloat(-R, R, FacingLeft) @17898-18101
//! against the pawn's turned Y axis: +R world X both ways).
//! Its rotation: RotateMulticast @16866 sets JerryMesh's relative yaw +90 facing left / -90 facing right and the
//! AnimBP's FacingLeft (@17098: WAnimationMirrorGraphNode on Tom_and_Jerry_MirrorTable) under the actor's world yaw 0
//! (BeginPlay @1306, @22896, @23981, @31059; while following the actor's relative yaw -90 / +90 under the pawn's
//! +90 / -90 @18131).
//! Unverified: drawn as the fighters are: yaw -90 facing right, its reflection across world UE Y facing left (the
//! rows' Y components negated).
//! Deviation: the actor's own rotation at the spawn (@2653: relative yaw -90 / +90 by the owner's facing, mesh world
//! yaw 0) and in AimDownProjectileMovement's flight (@18516-18797: pitch from the velocity) are not drawn; the world
//! yaw 0 of the other states is taken throughout.
//! Visibility: JerryActor::mesh_vis (AdjustVisibility @29145-29345): none hides it.
//! Its material (JerryMaterialDynamic, CreateDynamicMaterialInstance(0) @14345): the view's first slot. The hit flash:
//! ColorMap 1 at a hit @22644, 0 after Delay(0.15) @72 (JerryActor::color_map).
//! UpdateLocality (@19803-20986; BeginPlay @13927 with the Instigator, the attach to an ally @2185 with the ally):
//! JerryMesh, Champagne and Tenis_Rachet_Mesh take that fighter's Comp_Pawn_MeshManager StencilValue and
//! SetRenderCustomDepth(GetOutlinesOn) (the meshes' custom stencil, as the fighters'); with a StencilValue other than 4
//! (the local player's own) DepthOffset 0, OpacityToggle 0 and LocalPlayerSceneDepth = PFGCamera's
//! SceneCaptureComponent2D_Priority1 target, so Jerry is cut out where the local player covers him. At BeginPlay the
//! call runs before the material is made (the pushes @12319-12359 run 13907 before 14294): its writes to
//! JerryMaterialDynamic find none, so only the attach's land.
//! Unverified: GetOutlinesOn as the fighter's OutlinesOn and the outline preference at the call (what SetOutline left
//! on its mesh manager); the call is not run again when either changes later (nothing in the class calls it again).
//! Deviation: the outlined meshes' stencil is not cut by the priority capture (a fighter's is).
//! The death: DeathVisuals (AlsoRespawn) @28530 PS_TomJerryPoof_Ghost at the actor, scale 1: the sim's emitter.
//!
//! The props (the begin switch @25231-25609 shows them, the end switch @24740-24999 hides them: the JerryActor's
//! booleans):
//!   - Champagne (Champagne_Mesh): RotateMulticast @17599-17836 attaches it at jerry_un_L_wrist facing right (R facing
//!     left) at (-35, 60, 0), rot (0, 180, 90) (the mirrored values facing left: drawn as the reflection).
//!     PlayAnimation(Champagne_Idle) when shown @26625 and PlayAnimation(Champagne_Shoot) at the BottleShoot notify
//!     @26227 (neither looping), each from the sim's tick of its call (the champagne state's bottle_anim /
//!     bottle_tick).
//!   - PS_AttackChargeUP: on Champagne at Z 21, scale 0.4 (the bone [`CHARGE`]).
//!   - Tenis_Rachet_Mesh (static, scale 0.6): at jerry_un_R_wrist facing right @17220 at (5, 0, -5), yaw 90 (@17331 /
//!     @17394).
//!   - TennisBall (static, Z 66.67 on JerryMesh, yaw 90), Rocket (JerryRocket_NoRope, scale 0.75 on JerryMesh).
//!     Deviation: the Rocket's slot 0 MID of TomJerryFishLine_MAT with Team (@14438-14724) is drawn with
//!     the mesh's own material set.

use std::sync::Arc;

use mvs_sim::World;
use mvs_sim::actors::actor_montage::montage_of;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::chars::tom_and_jerry::jerry::{JerryActor, jerry_of};
use mvs_sim::chars::tom_and_jerry::state::tj_of;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_sim::fighter::{FighterId, cap_radius, grounded, outlines_on};
use mvs_vfx::math::{V3, compose_rows, rotation_rows};

use super::JERRY_PART;
use super::actors::{STEP, bone_ue, selected, skin_pick, warn_once};
use crate::actor_anim::{
    ActorMachine, ActorSlot, BlendOption, BoolListDef, MachineDef, MachineQuery, PoseDef, SeqDef, StateDef,
    TransitionDef,
};
use crate::actor_view::{IDENT, SkinnedActor, StaticView, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, Rows};
use crate::anim::Pose;
use crate::frame::MeshPose;
use crate::pose::{PRIORITY_CAPTURES, set_scalar, set_texture};

const DEG: f64 = std::f64::consts::PI / 180.0;
/// JerryMesh's template.
const MESH_Z: f64 = -80.0;
const MESH_SCALE: f64 = 1.2;
const CHAMPAGNE: &str = "Champagne_Mesh";
const ROCKET_MESH: &str = "JerryRocket_NoRope";
const RACKET: &str = "Tennis_Rachet_Mesh";
const BALL: &str = "TennisBall";
const STENCIL_MINE: u8 = 4;

/// The bones the view enters for the effects side: Trail_Effect's place (JerryMesh's, at the root's scale),
/// PS_AttackChargeUP's (Z 21 on the bottle) and the bottle's bone Neck (PS_TomJerry_ChampagnePop's).
pub(crate) const TRAIL: &str = "tj:Trail";
pub(crate) const CHARGE: &str = "tj:Charge";
pub(crate) const NECK: &str = "tj:Neck";

/// The AnimBP's variables the rules and blend lists read.
#[derive(Clone, Copy, Debug, Default)]
struct JerryInputs {
    is_falling: bool,
    /// IsMovingOnGround.
    on_ground: bool,
    /// HorizontalSpeed.
    h_speed: f64,
    is_jumping: bool,
    jump_number: i32,
    fast_fall: bool,
    /// HitReactInt while HitReact, else -1.
    hit_react: i32,
}

type I = JerryInputs;

fn seq(s: &'static str, looped: bool) -> PoseDef<I> {
    PoseDef::Seq(SeqDef::new(s, looped))
}

fn list(test: fn(&I) -> bool, t: PoseDef<I>, f: PoseDef<I>, blend_time: [f64; 2]) -> PoseDef<I> {
    PoseDef::BoolList(BoolListDef { test, t: Box::new(t), f: Box::new(f), blend_time, option: BlendOption::Linear })
}

fn state(name: &'static str, pose: PoseDef<I>) -> StateDef<I> {
    StateDef { name, pose: Some(pose), entry_rule: None, always_reset: false }
}

fn tr(from: &'static str, to: &'static str, xfade: f64, rule: fn(&I, &dyn MachineQuery) -> bool) -> TransitionDef<I> {
    TransitionDef { from, to, xfade, option: BlendOption::HermiteCubic, rule: Some(rule), auto: false }
}

/// Movement (#9). Rule offsets in JerryAnimBlueprint.txt (GetRelevantAnimTimeRemaining(9, state)).
fn movement() -> MachineDef<I> {
    MachineDef {
        entry: "Intro",
        reinit_on_becoming_relevant: true,
        max_transitions_per_frame: 3,
        skip_first_update_transition: true,
        states: vec![
            state("Intro", seq("Tom_and_Jerry_Jerry_StopFlying", false)),
            // BlendListByBool_2 (HorizontalSpeed >= 10 @579): true Jerry_Run, false Only_Jerry_Idle, 0.1 s each
            state(
                "Ground",
                list(
                    |i| i.h_speed >= 10.0,
                    seq("Tom_and_Jerry_Jerry_Run", true),
                    seq("Tom_and_Jerry_Only_Jerry_Idle", true),
                    [0.1, 0.1],
                ),
            ),
            // BlendListByBool_3 (FastFall): true Jerry_Fast_Fall, false Jerry_Fall, 0.1 s each
            state(
                "Fall",
                list(
                    |i| i.fast_fall,
                    seq("Tom_and_Jerry_Jerry_Fast_Fall", true),
                    seq("Tom_and_Jerry_Jerry_Fall", true),
                    [0.1, 0.1],
                ),
            ),
            state("Jump", seq("Tom_and_Jerry_Jerry_Jump_01", false)),
            // BlendListByBool_4 (HorizontalSpeed >= 10 @512): true Jerry_Land_Run, false Jerry_Land, 0.1 s each
            state(
                "Land",
                list(
                    |i| i.h_speed >= 10.0,
                    seq("Tom_and_Jerry_Jerry_Land_Run", false),
                    seq("Tom_and_Jerry_Jerry_Land", false),
                    [0.1, 0.1],
                ),
            ),
            state("Jump2", seq("Tom_and_Jerry_Jerry_Jump_02", false)),
        ],
        transitions: vec![
            tr("Intro", "Ground", 0.0, |_, q| q.remaining("Intro") == 0.0), // @244 (9, 0)
            tr("Ground", "Jump", 0.0, |i, _| i.is_jumping),
            tr("Ground", "Fall", 0.2, |i, _| i.is_falling),
            tr("Fall", "Jump2", 0.0, |i, _| i.is_jumping && i.jump_number == 2), // @139
            tr("Fall", "Land", 0.0, |i, _| i.on_ground),
            tr("Jump", "Land", 0.0, |i, _| i.on_ground),
            tr("Jump", "Jump2", 0.0, |i, _| i.is_jumping && i.jump_number == 2), // @2119
            tr("Jump", "Fall", 0.2, |_, q| q.remaining("Jump") == 0.0),          // @2224 (9, 3)
            tr("Land", "Jump", 0.0, |i, _| i.is_jumping),
            tr("Land", "Ground", 0.1, |_, q| q.remaining("Land") <= 0.1), // @34 (9, 5)
            tr("Jump2", "Land", 0.0, |i, _| i.on_ground),
            // @349 reads GetRelevantAnimTimeRemaining(9, 3): the Jump state's, not Jump2's (kept as the build has it)
            tr("Jump2", "Fall", 0.2, |_, q| q.remaining("Jump") == 0.0),
        ],
    }
}

/// BlendListByInt #6 (HitReactInt: HR_01..04, blend times 0, 0, 0.1, 0.1) as nested bools.
fn hit_react() -> PoseDef<I> {
    let hr = |n: u8| {
        seq(
            match n {
                1 => "Tom_and_Jerry_Only_Jerry_HR_01",
                2 => "Tom_and_Jerry_Only_Jerry_HR_02",
                3 => "Tom_and_Jerry_Only_Jerry_HR_03",
                _ => "Tom_and_Jerry_Only_Jerry_HR_04",
            },
            true,
        )
    };
    list(
        |i| i.hit_react <= 0,
        hr(1),
        list(|i| i.hit_react == 1, hr(2), list(|i| i.hit_react == 2, hr(3), hr(4), [0.1, 0.1]), [0.0, 0.1]),
        [0.0, 0.0],
    )
}

/// The graph above the slot: BlendListByBool_1 #7 (HitReact) over BlendListByInt #6 and Movement, as one machine's
/// one state (no transition: the state's pose alone).
fn top() -> MachineDef<I> {
    MachineDef {
        entry: "Top",
        reinit_on_becoming_relevant: false,
        max_transitions_per_frame: 1,
        skip_first_update_transition: false,
        states: vec![state("Top", list(|i| i.hit_react >= 0, hit_react(), PoseDef::Machine(movement()), [0.0, 0.0]))],
        transitions: Vec::new(),
    }
}

/// A prop on JerryMesh: its socket facing right (None: the mesh's own transform), its relative transform.
#[derive(Debug)]
struct Prop<V> {
    v: V,
    bone: Option<&'static str>,
    loc: V3,
    rot: Rows,
    scale: V3,
}

#[derive(Debug)]
pub(super) struct JerryView {
    id: ActorId,
    /// The fighter whose character data has the class (its mesh, clips and materials come with it).
    from: FighterId,
    owner: FighterId,
    sk: SkinnedActor,
    machine: ActorMachine<I>,
    slot: ActorSlot,
    base: Pose,
    out: Pose,
    inp: I,
    /// UpdateFollower's, never cleared.
    follower: Option<FighterId>,
    prev: V3,
    cur: V3,
    facing_left: bool,
    stepped: bool,
    champagne: Option<Prop<SkinnedActor>>,
    idle: Option<usize>,
    shoot: Option<usize>,
    racket: Option<Prop<StaticView>>,
    ball: Option<Prop<StaticView>>,
    rocket: Option<Prop<SkinnedActor>>,
    /// ColorMap as last written.
    color_map: f64,
    /// UpdateLocality's fighter, as last called, and the stencil it gave the outlined meshes (0: none).
    locality: Option<FighterId>,
    stencil: u8,
    trail: Option<Xform>,
    charge: Option<Xform>,
    neck: Option<Xform>,
}

const fn rel_rows(p: f64, y: f64, r: f64) -> (f64, f64, f64) {
    (p * DEG, y * DEG, r * DEG)
}

fn rows_of((p, y, r): (f64, f64, f64)) -> Rows {
    rotation_rows(p, y, r)
}

/// `makeView`: Jerry_SkelMesh as the skin's JerryMesh sub-mesh, in the owner's variant, with its props.
pub(super) fn jerry_mesh(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    _cs: &ActorComponents,
) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let WorldActorState::JerryActor(_) = &x.state else { return None };
    let owner = x.owner;
    let from = actor_def_owner(w, owner, x.cls);
    let variant = selected(host, w, owner);
    let d = host.draws_of(w, from)?;
    // (the owner's applied skin's JerryMesh part)
    let skin = crate::skins::skin_of(&w.char_data(from).fighter, skin_pick(w, from)).ok()?;
    let p = skin.parts.iter().find(|p| p.key.eq_ignore_ascii_case(JERRY_PART))?;
    let keys = if p.own { d.actor(&p.mesh, None).map(|a| a.1) } else { d.set(&skin.name, &p.key, variant).ok() }?;
    let mut sk = SkinnedActor::new(p.path.as_deref().and_then(|x| host.mesh(x))?, &keys);
    let vk = variant.key();
    let skinned = |host: &ActorHost<'_>, mesh: &str| -> Option<SkinnedActor> {
        let d = host.draws_of(w, from)?;
        let got = d.actor(mesh, (vk != "local").then_some(vk)).or_else(|| d.actor(mesh, None));
        let v = got.and_then(|(path, keys)| Some(SkinnedActor::new(host.mesh(&path)?, &keys)));
        if v.is_none() {
            eprintln!("warning: Tom and Jerry: {mesh} is not drawn (its mesh or materials are not loaded)");
        }
        v.map(|mut v| {
            v.visible = false;
            v
        })
    };
    let stat = |host: &ActorHost<'_>, mesh: &str| -> Option<StaticView> {
        let v = StaticView::new(host, w, from, mesh, vk);
        if v.is_none() {
            eprintln!("warning: Tom and Jerry: {mesh} is not drawn (its mesh or materials are not loaded)");
        }
        v.map(|mut v| {
            v.visible = false;
            v
        })
    };
    fn prop<V>(v: V, bone: Option<&'static str>, loc: V3, rot: Rows, s: f64) -> Prop<V> {
        Prop { v, bone, loc, rot, scale: [s; 3] }
    }
    let champagne = skinned(host, CHAMPAGNE)
        .map(|v| prop(v, Some("jerry_un_L_wrist"), [-35.0, 60.0, 0.0], rows_of(rel_rows(0.0, 180.0, 90.0)), 1.0));
    let racket = stat(host, RACKET)
        .map(|v| prop(v, Some("jerry_un_R_wrist"), [5.0, 0.0, -5.0], rows_of(rel_rows(0.0, 90.0, 0.0)), 0.6));
    let ball = stat(host, BALL).map(|v| prop(v, None, [0.0, 0.0, 66.666664], rows_of(rel_rows(0.0, 90.0, 0.0)), 1.0));
    let rocket = skinned(host, ROCKET_MESH).map(|v| prop(v, None, [0.0; 3], IDENT, 0.75));
    let team = host.colour(w, owner).team;
    for m in sk.mats.iter_mut().flatten() {
        set_scalar(m, "Team", team);
    }
    let clips = host.clips_of(w, from)?;
    let clip = |n: &str| {
        let c = clips.get(n);
        if c.is_none() {
            eprintln!("warning: Tom and Jerry: Jerry clip missing: {n}");
        }
        c
    };
    let (idle, shoot) = (clip("Champagne_Idle"), clip("Champagne_Shoot"));
    let n = sk.names.len();
    let machine = ActorMachine::new(top(), n, clips);
    Some(Box::new(JerryView {
        id,
        from,
        owner,
        sk,
        machine,
        slot: ActorSlot::new("DefaultSlot", n),
        base: Pose::new(n),
        out: Pose::new(n),
        inp: I { hit_react: -1, ..I::default() },
        follower: None,
        prev: [0.0; 3],
        cur: [0.0; 3],
        facing_left: false,
        stepped: false,
        champagne,
        idle,
        shoot,
        racket,
        ball,
        rocket,
        color_map: 0.0,
        locality: None,
        stencil: 0,
        trail: None,
        charge: None,
        neck: None,
    }))
}

impl JerryView {
    /// BlueprintUpdateAnimation and the four bound events.
    fn inputs(&mut self, w: &World, a: &JerryActor) {
        if a.following.is_some() {
            self.follower = a.following;
        }
        let i = &mut self.inp;
        match self.follower {
            Some(f) => {
                let g = grounded(w, f);
                i.is_falling = !g;
                i.on_ground = g;
                i.h_speed = f64::from(w.fighter(f).body.vx.abs());
            }
            None => {
                i.h_speed = f64::from(a.vx.abs());
                i.is_falling = a.vy.abs() != 0.0; // @454
            }
        }
        i.is_jumping = a.jumping;
        i.jump_number = a.jump_number;
        i.fast_fall = a.fast_fall;
        i.hit_react = a.hit_react;
    }

    /// UpdateLocality(f): the outlined meshes' stencil, and the opacity mask (`material`: false at BeginPlay).
    fn locality(&mut self, host: &ActorHost<'_>, w: &World, f: FighterId, material: bool) {
        self.locality = Some(f);
        let colour = host.colour(w, f);
        let stencil = colour.stencil;
        self.stencil = if colour.outline && outlines_on(w, f) { stencil } else { 0 };
        if !material || stencil == STENCIL_MINE {
            return;
        }
        let Some(Some(m)) = self.sk.mats.get_mut(0) else { return };
        set_scalar(m, "DepthOffset", 0.0);
        set_scalar(m, "OpacityToggle", 0.0);
        set_texture(m, "LocalPlayerSceneDepth", PRIORITY_CAPTURES[0]);
    }

    /// `placeProp`: a prop's world transform under JerryMesh (its bone's world transform, else the mesh's).
    fn prop_at<V>(sk: &SkinnedActor, pr: &Prop<V>, at: V3, rows: &Rows, ms: f64) -> (V3, Rows) {
        let b = pr.bone.and_then(|n| {
            let b = sk.bone(n);
            if b.is_none() {
                warn_once(format!("Tom and Jerry: Jerry has no bone {n}"));
            }
            b
        });
        let (loc, l) = match b.and_then(|b| bone_ue(sk, b)) {
            Some(x) => x,
            None => (at, rows.map(|v| v * ms)),
        };
        // loc + rel L; rows rel rot x L (L carries the parent's scale)
        let p = pr.loc;
        let o = std::array::from_fn(|k| loc[k] + p[0] * l[k] + p[1] * l[3 + k] + p[2] * l[6 + k]);
        (o, compose_rows(&pr.rot, &l))
    }
}

const RIGHT: (f64, f64, f64) = rel_rows(0.0, -90.0, 0.0);

fn mesh_rows(left: bool) -> Rows {
    let r = rows_of(RIGHT);
    if !left {
        return r;
    }
    std::array::from_fn(|k| if k % 3 == 1 { -r[k] } else { r[k] })
}

fn stencil_all(out: &mut [MeshPose], s: u8) {
    for m in out {
        m.custom_stencil = s;
    }
}

impl ActorMesh for JerryView {
    /// `step`: one sim tick: the anim instance's update and pose, the actor's place at this tick, the props.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(a) = jerry_of(w, self.id) else { return };
        if w.actors.actors.get(self.id).is_none_or(|x| x.dead) {
            return;
        }
        let first = !self.stepped;
        self.stepped = true;
        self.inputs(w, a);
        let Some(clips) = host.clips_of(w, self.from) else { return };
        self.machine.update(&self.inp, STEP, clips);
        {
            let skel = self.sk.skeleton(clips);
            self.machine.evaluate(clips, &skel, &mut self.base);
            let inst = a.anim.m.as_ref().and_then(|st| {
                let m = montage_of(&w.data, w, ActorRef::Actor(self.id), w.names.str(st.montage.name))?;
                Some((st, m))
            });
            let on = self.slot.apply(clips, &skel, inst, &self.base, &mut self.out);
            if !on {
                self.out.copy(&self.base);
            }
        }
        self.sk.show(&self.out);
        self.prev = self.cur;
        self.cur = [a.following.map_or(0.0, |f| f64::from(cap_radius(w, f))), f64::from(-a.x), f64::from(a.y)];
        if first {
            self.prev = self.cur;
        }
        self.facing_left = a.facing < 0.0;
        self.sk.visible = a.mesh_vis.is_some();
        // UpdateLocality: at BeginPlay with the Instigator, at each attach with the ally
        match (self.locality, a.following) {
            (None, _) => self.locality(host, w, self.owner, false),
            (Some(l), Some(f)) if f != l => self.locality(host, w, f, true),
            _ => {}
        }
        // the hit flash
        if f64::from(a.color_map) != self.color_map {
            self.color_map = f64::from(a.color_map);
            if let Some(Some(m)) = self.sk.mats.get_mut(0) {
                set_scalar(m, "ColorMap", f64::from(a.color_map));
            }
        }
        // the props' windows
        let vis = self.sk.visible;
        let Some(clips) = host.clips_of(w, self.from) else { return };
        if let Some(c) = &mut self.champagne {
            c.v.visible = a.champagne && vis;
            // the sim's bottle animation since its notify's tick
            let b = if a.champagne { tj_of(w, self.owner).map(|s| &s.champagne) } else { None };
            if let Some(b) = b {
                let shoot = w.names.str(b.bottle_anim) == "shoot";
                if let Some(k) = if shoot { self.shoot } else { self.idle } {
                    c.v.play(clips, k, f64::from((w.flow.tick - b.bottle_tick).max(0)) * STEP, false);
                }
            }
        }
        if let Some(r) = &mut self.racket {
            r.v.visible = a.racket && vis;
        }
        if let Some(r) = &mut self.ball {
            r.v.visible = a.tennis_ball && vis;
        }
        if let Some(r) = &mut self.rocket {
            r.v.visible = a.rocket && vis;
        }
    }

    /// `place`: the mesh between the last two ticks' places, its props and the effects' bones on it.
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        if !self.stepped {
            return;
        }
        let mut at: V3 = std::array::from_fn(|k| self.prev[k] + (self.cur[k] - self.prev[k]) * alpha);
        // the root's scale (the sim's size_scale)
        let g = w.actors.actors.get(self.id).and_then(|x| x.size_scale).unwrap_or(1.0);
        let ms = MESH_SCALE * f64::from(g);
        at[2] += MESH_Z * f64::from(g);
        let rows = mesh_rows(self.facing_left);
        self.sk.set(&Xform { loc: at, rot: rows, scale: [ms; 3] });
        self.trail = Some(Xform { loc: at, rot: rows, scale: [f64::from(g); 3] });
        let start = out.len();
        self.sk.draw(out);
        stencil_all(&mut out[start..], self.stencil);
        // (the bottle is placed while hidden too: the charge and the pop follow it)
        if let Some(c) = &mut self.champagne {
            let (p, r) = Self::prop_at(&self.sk, c, at, &rows, ms);
            c.v.set(&Xform { loc: p, rot: r, scale: c.scale });
            let s = out.len();
            c.v.draw(out);
            stencil_all(&mut out[s..], self.stencil);
            self.charge = Some(Xform { loc: std::array::from_fn(|k| p[k] + 21.0 * r[6 + k]), rot: r, scale: [1.0; 3] });
            let mut all = Vec::new();
            c.v.bones_world(&mut all);
            self.neck = all.into_iter().find(|(n, _)| n.eq_ignore_ascii_case("Neck")).map(|b| b.1);
        }
        if let Some(r) = &mut self.racket
            && r.v.visible
        {
            let (p, m) = Self::prop_at(&self.sk, r, at, &rows, ms);
            r.v.set(&Xform { loc: p, rot: m, scale: r.scale });
            let s = out.len();
            r.v.draw(out);
            stencil_all(&mut out[s..], self.stencil);
        }
        if let Some(r) = &mut self.ball
            && r.v.visible
        {
            let (p, m) = Self::prop_at(&self.sk, r, at, &rows, ms);
            r.v.set(&Xform { loc: p, rot: m, scale: r.scale });
            r.v.draw(out);
        }
        if let Some(r) = &mut self.rocket
            && r.v.visible
        {
            let (p, m) = Self::prop_at(&self.sk, r, at, &rows, ms);
            r.v.set(&Xform { loc: p, rot: m, scale: r.scale });
            r.v.draw(out);
        }
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        for (n, x) in [(TRAIL, self.trail), (CHARGE, self.charge), (NECK, self.neck)] {
            if let Some(x) = x {
                out.push((Arc::from(n), x));
            }
        }
    }

    fn split(&self) -> bool {
        true
    }
}
