//! Marvin's render module: the entry, the AnimBP's pose hook, the pawn's tool
//! meshes and the classes' mesh rules (the cannon blast's rocket and the flag; the
//! UFO). His effects are mvs-render fx/chars/marvin.rs. Not drawn: the heart skin (left off). His trails are
//! AnimNotify_PlayVFX / AnimNotifyState_TrailVFX of his montages (the generic notify VFX). FindMirrorSocket: no graph of
//! C018 calls it; the L_ / R_ sockets of his notifies are the shared attach's. Emote_Intro's SubMeshSpawn
//! (C018_Spaceship_SkelMesh with C018_Emote_Intro_Spaceship) is the shared montage-mesh draw.
//!
//! What the pose rule draws (C018_AnimBP):
//! - The tool meshes' toggles: BlueprintUpdateAnimation's SetSubmeshVIsibility (@3216 -> @2366-3191) runs
//!   ToggleSubmeshVisibility(curve, tag, state) for ten sub-meshes, each with its own state variable (class default
//!   false): when (GetCurveValue(curve) >= 0.5) XOR state, VisibilityManager.SetSubMeshVisibility(tag, !state, true)
//!   and the state flips (@43-307). The pairs (TOGGLES) in the call order: Body, Head, RayGun, Dynamite, Flag, Remote,
//!   TimeSpaceGun, UltimatumAnswerer, Trumpet, BlasterGun. Nothing resets the states (no ResetSubmeshVisibility in the
//!   class): a respawn's SetupInitialVisibility hides the non-primary parts in the sim and the states stand. Body /
//!   Head / Flag / RayGun are skin sub-meshes (the view's parts); the other six are the pawn's own SkeletalMesh
//!   components with a mesh of their own, each given the main mesh as its master pose by Pawn_C018's
//!   UserConstructionScript (@0-420): `tools_make` adds them to the view as master-posed component meshes, this rule
//!   shows them (PoseHook::shown). The port's own state of a part over the sim's (sub_hidden), as Steven's: the AnimBP's
//!   write stands until the sim changes the part. The curves: the clips' own by the sequence the tick shows (the
//!   fighter's anim and anim time) and the view's crossfade.
//! - The aim slots: AimUp / AimDown (the TwoWayBlends' alphas from AimValue @1867-2040), DefaultSlot and JumpSlot are
//!   the sim's slot graph walk (MarvinPawnState.slot_mix), which the view mixes into the drawn pose.
//! - HeadScale: the entry's head_scale (ModifyBone on fml_un_C_Face_att, ModifyBone_3 on fml_un_C_head).
//!
//! Deviations and unverified models:
//! - Unverified: a curve's value in a crossfade and the one tick the update lags the evaluation: as Steven's pose. The
//!   mix's tracks (the aim slots) do not add their curves here: the sequence of the fighter's anim alone.
//! - Unverified: a sequence without the curve reads 0 (UE's GetCurveValue of a curve the pose lacks):
//!   C018_Air_Attack_Normal_Down_Fall / _Land and C018_Unique_Idle have no showBody, so the body hides in them (the
//!   crouch clips key it to 0 too).
//! - On Facing Changed's SetRelativeScale3D((1, -1, 1)) on the VisMesh components (HeadMesh) facing left is the view's
//!   reflection (CharRender.turns false).
//! - The Prop1List loop of the initialisation (@158-505: SetSubMeshVisibility(item, false) for each) runs before
//!   VisibilityManager is set (@510-695) and the class default list is empty: nothing.
//! - Deviation: the view places no dead fighter, so the respawn is seen as a gap in the ticks set() runs at (the
//!   effects are told of every dead tick).
//! - Not ported: a check of the data's slot graph against the AnimBP's Slot nodes (a warning of the data
//!   only).

use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::actor_montage::{mesh_instances, montage_of};
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::actors::{ActorRef, ProjectileExt};
use mvs_sim::chars::marvin::ufo::Ufo;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_vfx::math::{compose_rows, rotation_rows};

use super::{CharRender, PawnComponents, PoseHook, PoseOf, PreviewOf, PreviewStep};
use crate::actor_anim::ActorSlot;
use crate::actor_view::{Comp, SkinnedActor, Xform, place_at};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule, Rows};
use crate::anim::{
    Clips, HeadScaleNode, Pose, accumulate_additive, add_weighted_pose, normalize_rotations, sample_clip, weight_pose,
};
use crate::anim_nodes::hypot;
use crate::curves::CurveTable;
use crate::frame::MeshPose;
use crate::pose::{MeshMap, Rec, material, set_scalar};
use crate::skins::FighterDraws;
use crate::view::{CharView, PoseCx, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // C018_AnimBP's head scale nodes.
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone",
            bone: "fml_un_C_Face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_3",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    pose: Some(marvin_pose),
    components: Some(PawnComponents { make: tools_make, place: tools_place, reset: None }),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (CANNON_BLAST, MeshRule { rot: None, mesh: Some(rocket_view) }),
    (PERK_CANNON_BLAST, MeshRule { rot: None, mesh: Some(rocket_view) }),
    (FLAG, MeshRule { rot: None, mesh: Some(flag_view) }),
    (AIR_FLAG, MeshRule { rot: None, mesh: Some(flag_view) }),
    (UFO, MeshRule { rot: None, mesh: Some(ufo_view) }),
    (PERK_UFO, MeshRule { rot: None, mesh: Some(ufo_view) }),
];

/// The float curves its AnimBP reads by name (GetCurveValue), in the order the rule indexes them.
const CURVES: CurveTable = CurveTable::new(&[
    "showBody",
    "showHead",
    "show_rayGun",
    "show_dynamite_stick",
    "show_mars_flag",
    "show_remoteButton",
    "show_timeSpaceGun",
    "show_ultimatumAnswerer",
    "show_trumpet",
    "show_blaster_gun",
]);

/// ToggleSubmeshVisibility @80.
const THRESHOLD: f64 = 0.5;
/// The calls in their order: the curve (its index in CURVES) and the sub-mesh tag (the skin's key or the component's
/// tag).
const TOGGLES: [&str; 10] = [
    "BodyMesh",
    "HeadMesh",
    "RayGun",
    "Dynamite",
    "Flag",
    "Remote",
    "TimeSpaceGun",
    "UltimatumAnswerer",
    "Trumpet",
    "BlasterGun",
];

/// `marvinPose`'s state on one view.
#[derive(Debug)]
struct MarvinPose {
    /// Per toggle: its index in the view's parts (None: a component mesh, shown by its tag).
    part: [Option<usize>; 10],
    tags: [String; 10],
    checked: bool,
    /// The last tick set() ran at (a gap: the fighter was dead between).
    last_tick: Option<i32>,
    /// The curves: the sequence's (tgt), the evaluation's (cur) and the pose the running fade started from (from).
    tgt: [f64; 10],
    cur: [f64; 10],
    from: [f64; 10],
    fades: u32,
    /// BodyVisible, HeadVisible, ...
    state: [bool; 10],
    /// The AnimBP's last write (-1 none, 0 hidden, 1 shown) and the sim's visibility at the last update.
    own: [i8; 10],
    sim_vis: [i8; 10],
    /// PoseOf::slaved: no AnimBP of its own (set returns after the respawn's check).
    slaved: bool,
}

fn marvin_pose(view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    Box::new(MarvinPose {
        part: TOGGLES.map(|t| of.part_keys.iter().position(|k| k == t)),
        tags: TOGGLES.map(str::to_lowercase),
        checked: false,
        last_tick: None,
        tgt: [0.0; 10],
        cur: [0.0; 10],
        from: [0.0; 10],
        fades: view.fade_count(),
        state: [false; 10],
        own: [-1; 10],
        sim_vis: [-1; 10],
        slaved: of.slaved,
    })
}

impl MarvinPose {
    /// ToggleSubmeshVisibility of toggle `k` on the curves of the evaluation before: its write (None: none).
    fn toggle(&mut self, k: usize) -> Option<bool> {
        let on = self.cur[k] >= THRESHOLD;
        if on != self.state[k] {
            self.state[k] = !self.state[k];
            return Some(self.state[k]);
        }
        None
    }
}

impl PoseRules for MarvinPose {
    fn rule(&mut self, cx: &mut PoseCx<'_>, _p: &mut Pose, _dt: f64) {
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

impl PoseHook for MarvinPose {
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        // the respawn: SetupInitialVisibility's write is the later one (the states are kept)
        let tick = w.flow.tick;
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.own = [-1; 10];
        }
        self.last_tick = Some(tick);
        if self.slaved {
            return;
        }
        let fi = w.fighter(f);
        let sub_meshes = &w.char_data(f).def.sub_meshes;
        if !self.checked {
            self.checked = true;
            for k in 0..TOGGLES.len() {
                if self.part[k].is_none() && !sub_meshes.contains(&self.tags[k]) {
                    eprintln!("warning: Marvin pose: the sim's mesh list has no {}: it stays hidden", self.tags[k]);
                }
            }
        }
        // BlueprintUpdateAnimation on the curves of the evaluation before
        for k in 0..TOGGLES.len() {
            if let Some(on) = self.toggle(k) {
                self.own[k] = i8::from(on);
            }
            // the AnimBP's write until the sim changes the part's visibility (the later write)
            let hidden = w.names.get(&self.tags[k]).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            let vis = i8::from((self.part[k].is_some() || sub_meshes.contains(&self.tags[k])) && !hidden);
            if self.sim_vis[k] >= 0 && self.sim_vis[k] != vis {
                self.own[k] = -1;
            }
            self.sim_vis[k] = vis;
            let shown = if self.own[k] >= 0 { self.own[k] == 1 } else { vis == 1 };
            if let Some(p) = self.part[k] {
                view.set_part_visible(p, shown);
            }
        }
        // the sequence this tick's pose is sampled from
        CURVES.fill(clips, w.names.str(fi.anim.anim), f64::from(fi.anim.anim_time), &mut self.tgt);
    }

    /// A tool component's shown state: the AnimBP's write, else the sim's.
    fn shown(&self, tag: &str, sim: bool) -> bool {
        match self.tags.iter().position(|t| t == tag) {
            Some(k) if self.part[k].is_none() && self.own[k] >= 0 => self.own[k] == 1,
            _ => sim,
        }
    }

    /// A view with no fighter (a preview actor, a podium fighter): the AnimBP anew; its toggles run in preview_step.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {
        self.reset(&mut |_, _, _| {});
    }

    /// The preview's update (as gizmo.rs's): on BP_CharacterPreview the update's SetSubmeshVIsibility (@3216 ->
    /// @2366-3191) tests only the owning actor's VisibilityManager (@510-695; the Fighter test @813 ends only its own
    /// branch): the toggles on the curves of the evaluation before, a write put on its part (the AnimBP is the parts'
    /// only writer here; the six tool meshes are Pawn_C018's components, which a preview has none of), then the
    /// curves of the frame's sequence.
    fn preview_step(&mut self, view: &mut CharView, clips: &Clips, s: &PreviewStep<'_>) {
        for k in 0..TOGGLES.len() {
            if let Some(on) = self.toggle(k)
                && let Some(p) = self.part[k]
            {
                view.set_part_visible(p, on);
            }
        }
        CURVES.fill(clips, s.anim, s.time, &mut self.tgt);
    }

    /// `reset()`: a new pawn: the AnimBP's state anew.
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {
        self.tgt = [0.0; 10];
        self.cur = [0.0; 10];
        self.from = [0.0; 10];
        self.state = [false; 10];
        self.own = [-1; 10];
        self.sim_vis = [-1; 10];
        self.last_tick = None;
    }
}

// ---- the pawn's tool meshes ----
//
// DynamiteMesh, RemoteMesh, TimeSpaceGunMesh, UltimatumAnswererMesh, TrumpetMesh and BlasterGunMesh, SkeletalMeshComponents
// with a mesh of their own (C018_Dynamite_stick_SkelMesh, ...), bVisible false, one tag each (Dynamite, Remote, ...),
// children of CharacterMesh0 with no socket or relative transform. The UserConstructionScript gives each the main mesh
// as its master pose (@126-420): the meshes share Marvin's skeleton and are weighted to his tool bones (blaster*,
// stick* / wick*, remote*, tsGun*, trumpe*, ua*), which his clips animate. Drawn as master-posed component meshes of
// the view, shown by the AnimBP's toggles and the sim's sub-mesh state. FlagMesh and RayGunMesh are the skin's
// sub-meshes (the view's parts).
// Materials: the meshes' own (no override in the templates, no graph sets one): the draws' actor entry keyed by the
// mesh (C018_Props_MIC_Local; the remote's second section C018_Props_Transparent_MIC).

const TOOL_COMPONENTS: [&str; 6] =
    ["DynamiteMesh", "RemoteMesh", "TimeSpaceGunMesh", "UltimatumAnswererMesh", "TrumpetMesh", "BlasterGunMesh"];

/// The tag of a tool component of the data (lower case), None where its template is not loaded.
fn tool_tag(w: &World, f: FighterId, name: &str) -> Option<(String, String)> {
    let c = w.char_data(f).fighter.components.pawn.get(&format!("{name}_GEN_VARIABLE"))?;
    let p = c.p.as_obj()?;
    let mesh = p.get("SkeletalMesh").and_then(Value::as_str)?;
    let tag = p.get("ComponentTags").and_then(Value::as_arr)?.first()?.as_str()?;
    Some((tag.to_lowercase(), mesh.to_owned()))
}

fn tools_make(r: &mut Rec, w: &World, clips: &Clips, draws: &FighterDraws, meshes: &MeshMap) {
    let f = r.fighter();
    for name in TOOL_COMPONENTS {
        let Some((tag, mesh)) = tool_tag(w, f, name) else {
            eprintln!(
                "warning: Marvin: pawn component not drawn: {name} (its template, mesh or materials are not loaded)"
            );
            continue;
        };
        let actor = draws.actor(&mesh, None);
        let info = actor.as_ref().and_then(|(path, _)| meshes.get(path)).cloned();
        let (Some((_, keys)), Some(info)) = (actor, info) else {
            eprintln!(
                "warning: Marvin: pawn component not drawn: {name} (its template, mesh or materials are not loaded)"
            );
            continue;
        };
        let at =
            [false, true].map(|left| r.component_attach(w, "None", [0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3], left));
        let mats = keys.iter().map(|m| m.as_deref().map(material)).collect();
        r.add_component_mesh(clips, &tag, at, info, mats, true);
    }
}

/// `setComponentMesh` of the pose rule's set(), once a presentation tick: each tool shown by the AnimBP's write, else
/// the sim's (the mesh list has its tag and sub_hidden does not).
fn tools_place(r: &mut Rec, w: &World) {
    let f = r.fighter();
    for name in TOOL_COMPONENTS {
        let Some((tag, _)) = tool_tag(w, f, name) else { continue };
        let on = r.pose_shown(&tag, r.sub_mesh_shown(w, &tag));
        r.set_component_mesh(&tag, on);
    }
}

// ---- the cannon blast and the flag ----

const CANNON_BLAST: &str = "C018CannonBlast_C";
const PERK_CANNON_BLAST: &str = "C018PerkCannonBlast_C";
const FLAG: &str = "C018Flag_C";
const AIR_FLAG: &str = "C018AirFlag_C";
const ROCKET_MESH: &str = "C018_SpaceshipRocket_SkelMesh";
const STEP: f64 = 1.0 / 60.0;

/// `skinned`: the ship's or the rocket's view of the fighter `f`'s character in the instigator's pick (None: not
/// loaded, warned).
fn skinned(host: &ActorHost<'_>, w: &World, f: FighterId, mesh: &str, variant: &str) -> Option<SkinnedActor> {
    let drawn = host
        .draws_of(w, f)
        .and_then(|d| d.actor(mesh, (variant != "local").then_some(variant)))
        .and_then(|(path, keys)| Some((host.mesh(&path)?, keys)));
    let Some((info, keys)) = drawn else {
        eprintln!("warning: Marvin: the UFO mesh {mesh} is not drawn (its model or materials are not loaded)");
        return None;
    };
    Some(SkinnedActor::new(info, &keys))
}

/// `Comp` of a component of the class's components.
fn comp(c: &crate::actors::Component) -> Comp {
    Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale }
}

/// C018CannonBlast_C, C018PerkCannonBlast_C (the perk class: the same components, no PS Armor Break template):
/// C018_SpaceshipRocket_SkelMesh at (-1035, 0, 0), pitch 90 under Visuals, its slot 0 C018_Props_MIC_Local / _Red /
/// _Blue by SetTeamColor (@1877: the instigator's pick); no AnimClass: the reference pose. Visuals' RelativeScale3D =
/// VLerp(0.4, 1, RocketVisualGrowTimeline) (@2089-2152; the timeline 0 -> 1 linear over 0.4 s, PlayFromStart at
/// BeginPlay @535): the mesh and the three particle components under it (Trail PS_C018_MissileExhaust, PS Rocket Head
/// PS_C018_RocketHead, PS Armor Break PS_C018_HeavyProjectile_ArmorBreak) grow with it.
/// Unverified: the timeline counts the projectile's age (its ticks).
pub(crate) const GROW_FROM: f64 = 0.4;
pub(crate) const GROW_TIME: f64 = 0.4;

/// `growth`: Visuals' scale.
pub fn growth(age: i32) -> f64 {
    GROW_FROM + (1.0 - GROW_FROM) * (f64::from(age) * STEP / GROW_TIME).min(1.0)
}

#[derive(Debug)]
struct RocketView {
    id: mvs_sim::actors::ProjectileId,
    view: SkinnedActor,
    comp: Comp,
}

fn rocket_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let c = cs.get(ROCKET_MESH)?;
    let from = actor_def_owner(w, owner, pr.cls);
    let view = skinned(host, w, from, ROCKET_MESH, host.variant(w, owner))?;
    Some(Box::new(RocketView { id, view, comp: comp(c) }))
}

impl ActorMesh for RocketView {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
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
        let age = w.actors.projectiles.get(self.id).map_or(0, |p| p.age);
        let (x, y) = at.at(alpha);
        // under Visuals (at the Capsule, no turn): its own place, turn and scale; Visuals' scale on top
        let s = at.scale * growth(age);
        let c = &self.comp;
        self.view.set(&place_at(c, &c.rot, c.rs, &at.rot, s, [0.0, -x, y]));
        self.view.draw(out);
    }

    fn bones(&self, out: &mut Vec<(std::sync::Arc<str>, Xform)>) {
        self.view.bones_world(out);
    }

    fn end(&mut self) {}
}

/// C018Flag_C, C018AirFlag_C: FlagMesh (C018_flag_SkelMesh: the skin's Flag sub-mesh's model, no AnimClass: the
/// reference pose) at yaw 270 under the Capsule, hidden until MulticastActivate (@5228: the sim's flag_visible), slot 0
/// C018_Props_MIC_Local / _Red / _Blue by SetTeamColor (@7599).
#[derive(Debug)]
struct FlagView {
    id: mvs_sim::actors::ProjectileId,
    view: SkinnedActor,
    rot: Rows,
}

const FLAG_COMP: &str = "FlagMesh";
/// The skin's sub-mesh with the same model.
const FLAG_KEY: &str = "Flag";

fn flag_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let c = cs.get(FLAG_COMP);
    let from = actor_def_owner(w, owner, pr.cls);
    let variant = host.variant(w, owner);
    let pick =
        usize::try_from(w.fighter(from).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let skin = crate::skins::skin_of(&w.char_data(from).fighter, pick).ok()?;
    let part = skin.parts.iter().find(|p| p.key.eq_ignore_ascii_case(FLAG_KEY));
    let draws = host.draws_of(w, from)?;
    // (the owner's applied skin's Flag entry; the variant of its set)
    let props = draws
        .actor(ROCKET_MESH, (variant != "local").then_some(variant))
        .and_then(|(_, k)| k.first().cloned().flatten());
    let (Some(c), Some(part), Some(props)) = (c, part, props) else {
        eprintln!(
            "warning: Marvin: the flag mesh is not drawn (no FlagMesh component, Flag sub-mesh or C018_Props_MIC set)"
        );
        return None;
    };
    let set = if part.own {
        draws.actor(&part.mesh, None).map(|a| a.1)
    } else {
        let v = match host.colour(w, owner).variant {
            crate::skins::Variant::Menu => crate::skins::Variant::Local,
            v => v,
        };
        draws.set(&skin.name, &part.key, v).ok()
    }?;
    // SetMaterial(0, the pick of C018_Props_MIC); the other slots the sub-mesh's own
    let keys: Vec<Option<String>> =
        set.into_iter().enumerate().map(|(n, m)| if n == 0 { Some(props.clone()) } else { m }).collect();
    let info = part.path.as_deref().and_then(|m| host.mesh(m))?;
    Some(Box::new(FlagView { id, view: SkinnedActor::new(info, &keys), rot: c.rot }))
}

impl ActorMesh for FlagView {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let visible = w.actors.projectiles.get(self.id).is_some_and(|p| match &p.ext {
            ProjectileExt::C018Flag(f) => f.flag_visible,
            _ => false,
        });
        self.view.visible = shown && visible;
        if !self.view.visible {
            return;
        }
        let (x, y) = at.at(alpha);
        let s = at.scale;
        self.view.set(&Xform { loc: [0.0, -x, y], rot: compose_rows(&self.rot, &at.rot), scale: [s; 3] });
        self.view.draw(out);
    }

    fn end(&mut self) {}
}

// ---- the UFO ----
//
// C018_Platform_C and its child C018_PerkPlatform_C (the same components; the child's graph adds nothing drawn), a
// World actor of the sim (mvs-sim chars/marvin/ufo.rs Ufo, perks.rs). Offsets are C018_Platform's ubergraph's and
// C018_Ship_AnimBP's; the components are the class data's.
// - The actor's rotation is yaw -90 whatever the facing (the spawn's, @1327). ShipMesh (C018_Spaceship_SkelMesh) is
//   at relative (0, -210, -240) under the Capsule: UE world (-210, 0, -240) from the actor (depth -210). Its relative
//   yaw is the SCS's 0 until the first aim; Set Ship Aim Direction (ReceiveTick with AimOn, @2713-2949) sets it to
//   RLerp(Start, End, RotationDeltaTime, shortest) with Start = Facing Left ? 0 : -180, End = Facing Left ? -180 : 0:
//   a quaternion slerp from one to the other, which turns through -90 either way (yaw = Start + (End - Start) x
//   RotationDeltaTime). Without AimOn it keeps the last yaw.
// - RocketMesh (C018_SpaceshipRocket_SkelMesh) under ShipMesh, master-posed on it (the same skeleton): drawn as its own
//   view with the ship's pose copied bone for bone. Hidden by the destroy (@19694) and the expiry (@20404): the sim's
//   rocket_hidden. ShipMesh hidden by the exit / destroy montages' end and shown by the intro: the sim's ship_hidden,
//   which hides its children too (SetVisibility(false, true)).
// - Materials: SetTeamColor (@10475-10761) puts C018_Props_MIC_Local / _Red / _Blue on slot 0 of ShipMesh and
//   RocketMesh (the SCS override C018_Ship_MIC_Local is replaced): the draws' actor entry by the mesh, the instigator's
//   pick. The ship's slot 1 keeps C018_Props_Transparent_MIC_Local_MIC.
// - C018_Ship_AnimBP: Root <- ComponentToLocal <- LookAt(shipRocketMount) <- ModifyBone(shipRocketMount1, scale x 0.5
//   additive in component space, alpha 1) <- LocalToComponent <- ApplyAdditive(alpha 1; Base = TwoWayBlend, Additive =
//   Slot AdditiveSlot on an unlinked source: the additive identity) <- TwoWayBlend(Alpha = ArmAngle; A = Slot
//   DefaultSlot(C018_Ship_Idle, looped), B = Slot AngledSlot(Slot DefaultSlot(C018_Ship_Idle))). The montages: the
//   sim's (Ufo.anim): Idle, Enter, Fire, turnAround and Reload have DefaultSlot and AngledSlot (_30U) tracks, Destroy
//   and Exit DefaultSlot, Bounce / HR_F / HR_B the AdditiveSlot (AAT_LocalSpaceBase over C018_Ship_Idle frame 0).
//   BlueprintUpdateAnimation (@114): AimOn -> Aim Weight += 2 x delta (clamped), LookAt Location = the Target's actor
//   location (@490-583, kept when the Target is not valid), Arm Angle = MapRangeClamped(Dot((0, 0, 1),
//   Normal(LookAt Location - (the actor's location + (0, 0, -500)))), 0, 0.5, 0, 1) (@730-1045); else Aim Weight -=
//   2 x delta. AimOn and Target come from BPI_SetAimTargetWithBool(target, true) alone (@18132, @21831): once aimed,
//   the AnimBP keeps aiming at its last target (nothing sends false).
//   LookAt: axis (0, -1, 0) and look-up axis (0, 0, 1) in the bone's space, no clamp, no interpolation; alpha Aim
//   Weight. FAnimNode_LookAt / SolveAim as the stage motion reads them (unverified).
// - Timeline_0 (@20921, the expiry): ShipMesh.SetScalarParameterValueOnMaterials(Collapse, 0 -> 1 linear over 0.8 s)
//   (@21682). Only 0_C018_Props_MAT's GPU-skin VS reads it, and ExitMontage hides ShipMesh before a vertex moves: the
//   port leaves the VS out (the scalar is set).
// - The particle components (arrival, death, depart, the beam, the hit impact) are the effects' (mvs-render
//   fx/chars/marvin.rs), the beam's socket from this view's bones.
// Deviation: the AnimBP runs at a drawn frame that sees a new World tick (as Jake's house's rule; each tick at
//   60 drawn frames a second or more); the Collapse timeline counts those ticks from the one that first shows the
//   expiry.
// Unverified: the AnimBP's AimOn latch reads the sim's AimOn edge (its Target the AimTarget while the platform aims).
// Unverified: two weighted instances in one slot are not blended; the newest stands for both.

const UFO: &str = "C018_Platform_C";
const PERK_UFO: &str = "C018_PerkPlatform_C";
const SHIP_MESH: &str = "C018_Spaceship_SkelMesh";
const SHIP_IDLE: &str = "C018_Ship_Idle";
const ZERO_W: f64 = 1e-5;
const DEG: f64 = std::f64::consts::PI / 180.0;
/// ShipMesh's place from the actor, UE world (the actor's yaw -90).
const SHIP_OFF: [f64; 3] = [-210.0, 0.0, -240.0];
/// Aim Weight's rate (@128 / @323).
const AIM_RATE: f64 = 2.0;
/// Set Arm Angle's (0, 0, -500) (@800).
const ARM_BELOW: f64 = 500.0;
/// Timeline_0's curve: 0 at 0, 1 at 0.8 (linear).
const COLLAPSE_TIME: f64 = 0.8;
/// The LookAt's bone and BeamVFXParent's socket.
pub const MOUNT: &str = "shipRocketMount";
/// The ModifyBone's.
const MOUNT1: &str = "shipRocketMount1";
const MOUNT1_SCALE: f32 = 0.5;
/// The LookAt's axes in the bone's space, in the mesh's content space (UE (x, y, z) -> (x, z, y)).
const AIM_AXIS: [f64; 3] = [0.0, 0.0, -1.0];
const UP_AXIS: [f64; 3] = [0.0, 1.0, 0.0];

/// `UfoView`'s mesh side.
#[derive(Debug)]
struct UfoView {
    id: mvs_sim::actors::ActorId,
    /// The fighter whose clips and draws it uses.
    from: FighterId,
    ship: SkinnedActor,
    rocket: Option<SkinnedActor>,
    idle: Option<usize>,
    dfl: ActorSlot,
    angled: ActorSlot,
    p_idle: Pose,
    p_a: Pose,
    p_b: Pose,
    out: Pose,
    ta: Pose,
    rocket_pose: Pose,
    /// Per rocket bone: the ship's (None: at rest).
    rocket_map: Vec<Option<usize>>,
    /// The bones from the root to the mount's parent.
    chain: Vec<usize>,
    mount: Option<usize>,
    mount1: Option<usize>,
    last_tick: Option<i32>,
    /// C018_Ship_Idle's player.
    clock: f64,
    aim_weight: f64,
    arm_angle: f64,
    /// The AnimBP's AimOn and Target.
    anim_aim: bool,
    anim_target: Option<FighterId>,
    /// LookAt Location (UE world).
    look_at: [f64; 3],
    /// A Set Ship Aim Direction has run.
    aimed: bool,
    /// ShipMesh's relative yaw (deg).
    rel_yaw: f64,
    expired_seen: bool,
    /// Timeline_0's position (s), None before the expiry.
    collapse: Option<f64>,
    /// The actor's place this tick (UE world) and the ship component's.
    at: [f64; 3],
    ship_loc: [f64; 3],
    ship_rot: Rows,
    /// The root's scale (the sim's size_scale), on every component.
    k: f64,
}

fn ufo_of(w: &World, id: mvs_sim::actors::ActorId) -> Option<(&WorldActor, &Ufo)> {
    let a = w.actors.actors.get(id).filter(|a| !a.dead)?;
    match &a.state {
        WorldActorState::Ufo(u) => Some((a, u)),
        _ => None,
    }
}

fn ufo_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, _cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let (x, _) = ufo_of(w, id)?;
    // (his actors can be a Marvin's when he is not in the match: the perk UFO; their data is the class owner's then)
    let from = actor_def_owner(w, x.owner, x.cls);
    let variant = host.variant(w, x.owner);
    let ship = skinned(host, w, from, SHIP_MESH, variant)?;
    let rocket = skinned(host, w, from, ROCKET_MESH, variant);
    let clips = host.clips_of(w, from)?;
    let idle = clips.get(SHIP_IDLE);
    if idle.is_none() {
        eprintln!("warning: Marvin: the UFO clip is not loaded: {SHIP_IDLE}");
    }
    let n = ship.names.len();
    let mount = ship.bone(MOUNT);
    let mount1 = ship.bone(MOUNT1);
    if mount.is_none() || mount1.is_none() {
        eprintln!("warning: Marvin: the UFO skeleton lacks {MOUNT} or {MOUNT1}");
    }
    let mut chain = Vec::new();
    let mut b = mount.map_or(-1, |m| ship.parents[m]);
    while b >= 0 {
        chain.insert(0, b as usize);
        b = ship.parents[b as usize];
    }
    let (rocket_map, rocket_pose) = match &rocket {
        Some(r) => {
            let map: Vec<Option<usize>> = r.names.iter().map(|s| ship.bone(s)).collect();
            if map.iter().any(Option::is_none) {
                eprintln!(
                    "warning: Marvin: the UFO rocket has bones the ship lacks (its master pose leaves them at rest)"
                );
            }
            let mut p = Pose::new(r.names.len());
            p.copy(&r.rest);
            (map, p)
        }
        None => (Vec::new(), Pose::new(0)),
    };
    Some(Box::new(UfoView {
        id,
        from,
        rocket,
        idle,
        dfl: ActorSlot::new("DefaultSlot", n),
        angled: ActorSlot::new("AngledSlot", n),
        p_idle: Pose::new(n),
        p_a: Pose::new(n),
        p_b: Pose::new(n),
        out: Pose::new(n),
        ta: Pose::new(n),
        rocket_pose,
        rocket_map,
        chain,
        mount,
        mount1,
        last_tick: None,
        clock: 0.0,
        aim_weight: 0.0,
        arm_angle: 0.0,
        anim_aim: false,
        anim_target: None,
        look_at: [0.0; 3],
        aimed: false,
        rel_yaw: 0.0,
        expired_seen: false,
        collapse: None,
        at: [0.0; 3],
        ship_loc: [0.0; 3],
        ship_rot: rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0),
        k: 1.0,
        ship,
    }))
}

/// `slotInstance`: the newest of ShipMesh's instances with a track in the slot and a weight.
fn slot_instance<'a>(
    w: &'a World,
    a: ActorRef,
    slot: &str,
) -> Option<(&'a mvs_sim::fighter::MontageState, &'a mvs_anim::montage::Montage)> {
    let all = mesh_instances(w, a);
    all.into_iter().rev().find_map(|st| {
        let m = montage_of(&w.data, w, a, w.names.str(st.montage.name))?;
        (st.shown_weight(m, &|s| eprintln!("warning: {s}")) > ZERO_W && st.pose_in(m, slot).is_some())
            .then_some((st, m))
    })
}

/// The ship's place and turn at a World place (UE: 0, -x, y), the platform's RotationDeltaTime turn and the root's
/// scale: (ShipMesh's location, its rows). The effects' particle components ride it too.
pub fn ship_place(at: [f64; 3], rel_yaw: f64, k: f64) -> ([f64; 3], Rows) {
    (std::array::from_fn(|i| at[i] + SHIP_OFF[i] * k), rotation_rows(0.0, (-90.0 + rel_yaw) * DEG, 0.0))
}

/// Set Ship Aim Direction's yaw (deg).
pub fn aim_yaw(u: &Ufo) -> f64 {
    let (start, end) = if u.facing_left { (0.0, -180.0) } else { (-180.0, 0.0) };
    f64::from(start + (end - start) * u.rdt)
}

impl UfoView {
    /// BlueprintUpdateAnimation, one tick.
    fn update(&mut self, w: &World, u: &Ufo) {
        self.clock += STEP;
        if u.aim_on {
            self.anim_aim = true;
            self.anim_target = u.aim_target;
        }
        if !self.anim_aim {
            self.aim_weight = (self.aim_weight - STEP * AIM_RATE).clamp(0.0, 1.0);
            return;
        }
        self.aim_weight = (self.aim_weight + STEP * AIM_RATE).clamp(0.0, 1.0);
        if let Some(t) = self.anim_target {
            self.look_at = [0.0, f64::from(-w.fighter(t).body.x), f64::from(mvs_sim::fighter::actor_y(w, t))];
        }
        // Set Arm Angle: the owning actor's location + (0, 0, -500)
        let d =
            [self.look_at[0] - self.at[0], self.look_at[1] - self.at[1], self.look_at[2] - (self.at[2] - ARM_BELOW)];
        let l = hypot(&d);
        let dot = if l > 1e-4 { d[2] / l } else { 0.0 };
        self.arm_angle = (dot / 0.5).clamp(0.0, 1.0);
    }

    /// The AnimGraph of the tick's state.
    fn evaluate(&mut self, clips: &mut Clips, w: &World) {
        let a = ActorRef::Actor(self.id);
        let mut p_idle = std::mem::take(&mut self.p_idle);
        let mut p_a = std::mem::take(&mut self.p_a);
        let mut p_b = std::mem::take(&mut self.p_b);
        let mut out = std::mem::take(&mut self.out);
        let mut ta = std::mem::take(&mut self.ta);
        let sk = self.ship.skeleton(clips);
        match self.idle {
            Some(c) => {
                let len = clips.clips[c].anim.length.max(1e-6);
                sample_clip(clips, c, sk.rig, sk.rest, self.clock % len, &mut p_idle, sk.root);
            }
            None => p_idle.copy(sk.rest),
        }
        let has_a = self.dfl.apply(clips, &sk, slot_instance(w, a, "DefaultSlot"), &p_idle, &mut p_a);
        let pa: &Pose = if has_a { &p_a } else { &p_idle };
        let has_b = self.angled.apply(clips, &sk, slot_instance(w, a, "AngledSlot"), pa, &mut p_b);
        let pb: &Pose = if has_b { &p_b } else { pa };
        let wt = self.arm_angle;
        if wt <= ZERO_W {
            out.copy(pa);
        } else if wt >= 1.0 - ZERO_W {
            out.copy(pb);
        } else {
            weight_pose(pa, 1.0 - wt, &mut out);
            add_weighted_pose(pb, wt, &mut out);
            normalize_rotations(&mut out);
        }
        // the AdditiveSlot on the additive identity, applied at alpha 1
        if let Some((st, m)) = slot_instance(w, a, "AdditiveSlot") {
            let sw = st.shown_weight(m, &|s| eprintln!("warning: {s}"));
            if sw > ZERO_W
                && let Some((si, gi, time)) = st.pose_in(m, "AdditiveSlot")
            {
                let anim = &m.slots[si].segments[gi].anim;
                match clips.get(anim).filter(|&c| clips.clips[c].additive) {
                    Some(c) => accumulate_additive(clips, c, sk.rig, sk.rest, time, sw, false, &mut out, &mut ta),
                    None => eprintln!("warning: Marvin: the UFO additive clip is not loaded: {anim}"),
                }
            }
        }
        // ModifyBone: shipRocketMount1's component space scale x 0.5 (its children follow)
        if let Some(m1) = self.mount1 {
            for k in 0..3 {
                out.s[m1 * 3 + k] *= MOUNT1_SCALE;
            }
        }
        self.look_at_mount(&mut out);
        self.ship.show(&out);
        if let Some(r) = &mut self.rocket {
            let rp = &mut self.rocket_pose;
            for (i, j) in self.rocket_map.iter().enumerate() {
                let Some(j) = *j else { continue };
                rp.t[i * 3..i * 3 + 3].copy_from_slice(&out.t[j * 3..j * 3 + 3]);
                rp.r[i * 4..i * 4 + 4].copy_from_slice(&out.r[j * 4..j * 4 + 4]);
                rp.s[i * 3..i * 3 + 3].copy_from_slice(&out.s[j * 3..j * 3 + 3]);
            }
            r.show(rp);
        }
        self.p_idle = p_idle;
        self.p_a = p_a;
        self.p_b = p_b;
        self.out = out;
        self.ta = ta;
    }

    /// FAnimNode_LookAt on shipRocketMount (content space: the mesh's model space, UE (x, y, z) / 100 as (x, z, y)),
    /// in three.js's math.
    fn look_at_mount(&self, out: &mut Pose) {
        let Some(h) = self.mount else { return };
        if !(self.aim_weight > ZERO_W) {
            return;
        }
        let f = |v: &[f32], i: usize| -> [f64; 3] { [f64::from(v[i]), f64::from(v[i + 1]), f64::from(v[i + 2])] };
        let mut parent_m = crate::math::IDENTITY;
        let mut parent_q = [0.0, 0.0, 0.0, 1.0];
        for &i in &self.chain {
            let q = out.quat(i);
            parent_m = crate::math::mul(&parent_m, &crate::math::compose(f(&out.t, i * 3), q, f(&out.s, i * 3)));
            parent_q = crate::math::qmul(parent_q, q);
        }
        let head_pos = apply_m4(&parent_m, f(&out.t, h * 3));
        let mut head_q = crate::math::qmul(parent_q, out.quat(h));
        // LookAt Location in the ship's content space
        // Unverified: the LookAt node's target is taken into component space through the ship's whole transform, its
        // scale (the root's size) included, so a grown ship aims its mount at the same world point.
        let r = &self.ship_rot;
        let g = 100.0 * self.k;
        let d = [
            self.look_at[0] - self.ship_loc[0],
            self.look_at[1] - self.ship_loc[1],
            self.look_at[2] - self.ship_loc[2],
        ];
        let cx = (r[0] * d[0] + r[1] * d[1] + r[2] * d[2]) / g;
        let cy = (r[3] * d[0] + r[4] * d[1] + r[5] * d[2]) / g;
        let cz = (r[6] * d[0] + r[7] * d[1] + r[8] * d[2]) / g;
        let mut to = [cx - head_pos[0], cz - head_pos[1], cy - head_pos[2]];
        if len_sq(to) < 1e-12 {
            return;
        }
        to = v_normalize(to);
        let up = apply_q(head_q, UP_AXIS);
        let mut aim = apply_q(head_q, AIM_AXIS);
        to = project_on_plane(to, up);
        aim = project_on_plane(aim, up);
        if len_sq(to) < 1e-12 || len_sq(aim) < 1e-12 {
            return;
        }
        to = v_normalize(to);
        aim = v_normalize(aim);
        let mut delta = from_unit_vectors(aim, to);
        if self.aim_weight < 1.0 - ZERO_W {
            delta = slerp([0.0, 0.0, 0.0, 1.0], delta, self.aim_weight);
        }
        head_q = crate::math::qmul(delta, head_q);
        let q = crate::math::qmul([-parent_q[0], -parent_q[1], -parent_q[2], parent_q[3]], head_q);
        for (d, v) in out.r[h * 4..h * 4 + 4].iter_mut().zip(q) {
            *d = v as f32;
        }
    }

    /// The AnimBP and the actor's place, once per World tick a drawn frame sees.
    fn anim(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some((x, u)) = ufo_of(w, self.id) else { return };
        // the actor and the ship component (UE world)
        self.at = [0.0, f64::from(-u.x), f64::from(u.y)];
        if u.aim_on {
            self.aimed = true;
        }
        if self.aimed {
            self.rel_yaw = aim_yaw(u);
        }
        self.k = f64::from(x.size_scale.unwrap_or(1.0));
        (self.ship_loc, self.ship_rot) = ship_place(self.at, self.rel_yaw, self.k);
        if u.life_end_at != 0.0 && !self.expired_seen {
            self.expired_seen = true;
            self.collapse = Some(0.0);
        }
        let tick = w.flow.tick;
        if self.last_tick == Some(tick) {
            return;
        }
        let n = match self.last_tick {
            Some(t) if tick >= t => (tick - t).min(30),
            _ => 1,
        };
        self.last_tick = Some(tick);
        if let Some(c) = self.collapse.filter(|&c| c < COLLAPSE_TIME) {
            let c = (c + f64::from(n) * STEP).min(COLLAPSE_TIME);
            self.collapse = Some(c);
            for m in self.ship.mats.iter_mut().flatten() {
                set_scalar(m, "Collapse", c / COLLAPSE_TIME);
            }
        }
        for _ in 0..n {
            self.update(w, u);
        }
        let from = self.from;
        let Some(clips) = host.clips_of(w, from) else { return };
        self.evaluate(clips, w);
    }
}

impl ActorMesh for UfoView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        _alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some((_, u)) = ufo_of(w, self.id) else {
            self.ship.visible = false;
            if let Some(r) = &mut self.rocket {
                r.visible = false;
            }
            return;
        };
        let shown = !u.ship_hidden;
        let rocket_shown = shown && !u.rocket_hidden;
        self.anim(host, w);
        self.ship.visible = shown;
        let xf = Xform { loc: self.ship_loc, rot: self.ship_rot, scale: [self.k; 3] };
        self.ship.set(&xf);
        self.ship.draw(out);
        if let Some(r) = &mut self.rocket {
            r.visible = rocket_shown;
            r.set(&xf);
            r.draw(out);
        }
    }

    fn bones(&self, out: &mut Vec<(std::sync::Arc<str>, Xform)>) {
        self.ship.bones_world(out);
    }

    /// The ship and the rocket: two scene children.
    fn split(&self) -> bool {
        true
    }

    fn end(&mut self) {}
}

// three.js's Vector3 / Quaternion steps the LookAt takes (three 0.186), in doubles.

/// Vector3.applyMatrix4.
fn apply_m4(e: &crate::math::Mat4, [x, y, z]: [f64; 3]) -> [f64; 3] {
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    [
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    ]
}

/// Vector3.applyQuaternion.
fn apply_q([qx, qy, qz, qw]: [f64; 4], [vx, vy, vz]: [f64; 3]) -> [f64; 3] {
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

fn len_sq(v: [f64; 3]) -> f64 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Vector3.normalize: divideScalar(length || 1).
fn v_normalize(v: [f64; 3]) -> [f64; 3] {
    let l = len_sq(v).sqrt();
    let l = if l == 0.0 { 1.0 } else { l };
    v.map(|c| c / l)
}

/// Vector3.projectOnPlane(n): v - projectOnVector(n).
fn project_on_plane(v: [f64; 3], n: [f64; 3]) -> [f64; 3] {
    let den = len_sq(n);
    let p = if den == 0.0 { [0.0; 3] } else { n.map(|c| c * (dot3(n, v) / den)) };
    [v[0] - p[0], v[1] - p[1], v[2] - p[2]]
}

/// Quaternion.normalize.
fn q_normalize(q: [f64; 4]) -> [f64; 4] {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l == 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let l = 1.0 / l;
    q.map(|c| c * l)
}

/// Quaternion.setFromUnitVectors.
fn from_unit_vectors(a: [f64; 3], b: [f64; 3]) -> [f64; 4] {
    let r = dot3(a, b) + 1.0;
    let q = if r < 1e-8 {
        if a[0].abs() > a[2].abs() { [-a[1], a[0], 0.0, 0.0] } else { [0.0, -a[2], a[1], 0.0] }
    } else {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0], r]
    };
    q_normalize(q)
}

/// Quaternion.slerp(b, t) on `a`.
fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let mut b = b;
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        b = b.map(|c| -c);
        dot = -dot;
    }
    let s = 1.0 - t;
    if dot < 0.9995 {
        let theta = dot.acos();
        let sin = theta.sin();
        let (s, t) = ((s * theta).sin() / sin, (t * theta).sin() / sin);
        std::array::from_fn(|k| a[k] * s + b[k] * t)
    } else {
        q_normalize(std::array::from_fn(|k| a[k] * s + b[k] * t))
    }
}
