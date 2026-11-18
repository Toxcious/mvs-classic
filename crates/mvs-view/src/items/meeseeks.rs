//! The Meeseek box and its runner as a match draws them (the sim: mvs-sim
//! items/meeseeks.rs). The class rules of crate::actors, asked for by the class.
//!
//! Item_Meeseek (World.items): MeeseekBox = SM_MeeseeksBox under the Capsule (the root: bAbsoluteScale, template scale
//! 1.25, the spawn transform's 1 in play), relative scale 1.35, no relative location or rotation, OverrideMaterials
//! [null] (the mesh's 0_MeeseeksProp_MIC; no graph sets one). Thrown @1010 and ReceiveHit @1630:
//! MeeseekBox.SetWorldScale3D(1.75), never set back (the sim's big_box). In a hand the root is on the holder's hold
//! socket (as the batarang's and the C017 items'); out of it the actor's rotation is the item's (yaw by facing, its
//! pitch), as the shared item path draws Geo.
//!
//! SelfActivate_Meeseeks_Run (World.projectiles): Meeseeks_SkelMesh under the Capsule at (0, 0, -153), scale 1.155,
//! AnimClass Meeseeks_AnimBP, CastShadow false. The actor's rotation is yaw 90 while FacingLeft, -90 else (Run
//! SetFacing @3622-3685 / Base @180-265: SelectRotator((0, 90, 0), (0, -90, 0), FacingLeft)).
//! Material: Base SetTeamColor @3094-3420 sets slot 0 by FTrunc(GetOwner's BPI_GetColorValue): 0 Meeseeks_Red_MIC,
//! 1 Meeseeks_Blue_MIC, else the select's default (None: the slot's own Meeseeks_MAT) (the fighter entry's actor
//! Meeseeks_SkelMesh, variants red / blue). SAP MulticastReflect calls SetTeamColor again with the new owner: the view
//! is made anew when the owner changes.
//! Animation: Meeseeks_Run BeginPlay plays Meeseeks_Attack_Run_Montage (blend in and out 0, never completing); under
//! its full weight in both slots the AnimBP shows only the montage: the DefaultSlot track, or the AirSlot track while
//! the actor's velocity Z < -200 (IsFalling @1180), BlendListByBool_1 with a .1 s linear blend, mirrored by
//! Meeseeks_MirrorTable while FacingLeft (WAnimationMirrorNode). The state machine and the hit-react blend under the
//! slots are never visible on it, and are not drawn. The montage is the sim's (mvs-sim actors/actor_montage.rs: the
//! actor's MontageState, its sections, AdvanceMontage's jump and its notifies); the view draws its position. Its
//! PlayVFX notifies attach to this view's bones.
//! A match with random items whose fighters have no data of the class (the box is the items part's: its runner is the
//! user's) draws the items part's mesh in the template's set, and the montage of the first fighter whose data has it,
//! else the items part's (the sim's montage_of: the owner's, else a peer's, else the items part's), with the items
//! draw's clips.
//! Deviation: the blend's weight steps per tick. The mirror is drawn as the reflection of the right-facing view across
//! UE Y, as the fighters': the same picture only where the mesh is symmetric. No outline.
//! Deviation: with no fighter's data of the class the runner keeps its slot's Meeseeks_MAT (the team instances
//! SetTeamColor picks are not in the items part's draws); with no montage data (no fighter's and no items part) it is
//! drawn in its reference pose (the sim plays no montage then).

use std::sync::Arc;

use mvs_sim::World;
use mvs_sim::actors::actor_montage::montage_of;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::actors::{ActorRef, ItemId, ProjectileExt, ProjectileId};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_sim::fighter::match_side::hold_socket;
use mvs_vfx::math::rotation_rows;

use super::{ClipSet, component, data_fighter};
use crate::actor_anim::ActorSkeleton;
use crate::actor_view::{SkinnedActor, StaticView, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows};
use crate::anim::{Clips, Pose, blend_pose, sample_clip};
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;

const STEP: f64 = 1.0 / 60.0;
const BOX: &str = "Item_Meeseek_C";
const BOX_COMP: &str = "MeeseekBox";
const BIG_BOX: f64 = 1.75;
const RUN: &str = "SelfActivate_Meeseeks_Run_C";
const MESH: &str = "Meeseeks_SkelMesh";
const MONTAGE: &str = "Meeseeks_Attack_Run_Montage";
/// IsFalling: velocity Z < -200 (Meeseeks_AnimBP @1180).
const FALL_VZ: f64 = -200.0;
/// BlendListByBool_1's blend time, linear.
const BLEND: f64 = 0.1;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] =
    &[(BOX, MeshRule { rot: None, mesh: Some(box_mesh) }), (RUN, MeshRule { rot: None, mesh: Some(run_mesh) })];

/// An object path's or a reference's asset name (`/Game/A/B.B` -> `B`).
fn asset_name(s: &str) -> &str {
    let s = s.rsplit('/').next().unwrap_or(s);
    s.split('.').next_back().unwrap_or(s).trim_end_matches('\'')
}

// ---- the box -----------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct BoxView {
    id: ItemId,
    view: StaticView,
    /// MeeseekBox's RelativeScale3D X.
    rs: f64,
}

fn box_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let c = cs.get(BOX_COMP)?;
    let ComponentKind::StaticMesh { mesh, .. } = &c.kind else { return None };
    // (a random item has no spawner, and a Mystery box's item one whose data may not have the class: its mesh from
    // the items part, crate::items::class_view)
    let item = |host: &ActorHost<'_>| StaticView::item(host, &format!("{BOX}.{BOX_COMP}"), "template");
    let view = match it.spawner {
        Some(s) => {
            StaticView::new(host, w, actor_def_owner(w, s, it.cls), asset_name(mesh), "local").or_else(|| item(host))
        }
        None => item(host),
    }?;
    Some(Box::new(BoxView { id, view, rs: c.scale[0] }))
}

impl ActorMesh for BoxView {
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
        let ItemExt::MeeseekItem(b) = &it.ext else { return };
        self.view.visible = shown;
        if !shown {
            return;
        }
        let s = if b.big_box { BIG_BOX } else { self.rs * f64::from(it.scale) };
        // (held: the holder's hold socket, else its mesh origin: socketFollow's `missing`)
        let hold = at.holder.and_then(|h| host.hold_place(w, h, &hold_socket(w, h)));
        let (loc, rot) = match hold {
            Some(x) => (x.loc, x.rot),
            None => {
                let (x, y) = at.at(alpha);
                ([0.0, -x, y], at.rot)
            }
        };
        self.view.set(&Xform { loc, rot, scale: [s; 3] });
        self.view.draw(out);
    }
}

// ---- the runner --------------------------------------------------------------------------------------------------

/// The actor's rotation facing right: yaw -90.
fn right() -> Rows {
    rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0)
}

#[derive(Debug)]
struct RunnerView {
    id: ProjectileId,
    /// The owner the view was made for (a reflect gives another: SetTeamColor again).
    owner: FighterId,
    /// The fighter whose data has the class (a character's mesh and materials), None: the items part's.
    own: Option<FighterId>,
    sk: SkinnedActor,
    /// The clips the montage's segments play from.
    clips: ClipSet,
    mesh_z: f64,
    mesh_scale: f64,
    /// BlendListByBool_1's AirSlot weight (-1: before the first pose) and the age it was last stepped at.
    air_w: f64,
    last_age: i32,
    ground: Pose,
    air: Pose,
    out: Pose,
}

/// The runner's mesh in the materials SetTeamColor picks for `owner`.
fn make_sk(host: &ActorHost<'_>, w: &World, own: Option<FighterId>, owner: FighterId) -> Option<SkinnedActor> {
    match own {
        Some(f) => {
            let variant = match host.colour(w, owner).color_index {
                0 => Some("red"),
                1 => Some("blue"),
                _ => None,
            };
            let (path, keys) = host.draws_of(w, f)?.actor(MESH, variant)?;
            Some(SkinnedActor::new(host.mesh(&path)?, &keys))
        }
        None => SkinnedActor::item(host, &format!("{RUN}.{MESH}")),
    }
}

fn run_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    // (the class data, clips and mesh: the character that has the class, after a reflect too)
    let own = data_fighter(w, owner, pr.cls);
    let c = component(w, cs, pr.cls, MESH)?;
    if !matches!(c.kind, ComponentKind::SkeletalMesh { .. }) {
        return None;
    }
    let montage_key = |f: FighterId| {
        w.char_data(f).fighter.actor_montages.as_ref().is_some_and(|m| m.contains_key(MONTAGE)).then_some(f)
    };
    let from = own
        .and_then(montage_key)
        .or_else(|| w.all_ids().into_iter().find_map(montage_key))
        .map_or(ClipSet::Items, ClipSet::Fighter);
    let Some(sk) = make_sk(host, w, own, owner) else {
        warn_once(format!("{RUN}: not drawn (its mesh or materials are not loaded)"));
        return None;
    };
    let m = montage_of(&w.data, w, a, MONTAGE);
    if m.is_none() {
        warn_once(format!(
            "Meeseeks runner: neither a fighter nor the items part has {MONTAGE}: drawn in its reference pose"
        ));
    }
    if let Some(m) = m
        && ["DefaultSlot", "AirSlot"].iter().any(|s| !m.slots.iter().any(|x| x.slot == *s))
    {
        warn_once(format!("{MONTAGE} has no DefaultSlot and AirSlot tracks: the runner is not drawn"));
        return None;
    }
    let n = sk.names.len();
    Some(Box::new(RunnerView {
        id,
        owner,
        own,
        sk,
        clips: from,
        mesh_z: c.loc[2],
        mesh_scale: c.scale[0],
        air_w: -1.0,
        last_age: -1,
        ground: Pose::new(n),
        air: Pose::new(n),
        out: Pose::new(n),
    }))
}

/// A slot's track at the sim's montage position (none playing: the rest pose; the montage never ends on it).
fn sample_track<K>(
    sk: &ActorSkeleton<'_>,
    clips: &mut Clips,
    inst: Option<(&mvs_anim::montage::MontageState<K>, &mvs_anim::montage::Montage)>,
    slot: &str,
    out: &mut Pose,
) {
    let seg = inst.and_then(|(st, m)| {
        let (si, gi, t) = st.pose_in(m, slot)?;
        Some((clips.get(&m.slots[si].segments[gi].anim)?, t))
    });
    match seg {
        Some((c, t)) => sample_clip(clips, c, sk.rig, sk.rest, t, out, sk.root),
        None => out.copy(sk.rest),
    }
}

impl ActorMesh for RunnerView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        let ProjectileExt::MeeseeksRun(s) = &pr.ext else { return };
        // (a reflect gives it a new owner: SAP MulticastReflect's SetTeamColor; made anew, once a reflect)
        if let Some(o) = pr.owner.filter(|&o| o != self.owner) {
            if let Some(sk) = make_sk(host, w, self.own, o) {
                self.sk = sk;
            }
            self.owner = o;
            self.air_w = -1.0;
        }
        self.sk.visible = shown;
        if !shown {
            return;
        }
        // BlendListByBool_1: the active child's weight moves to 1 over .1 s, per tick of the actor's age
        let target = if s.bvy < mvs_sim::math::narrow(FALL_VZ) { 1.0 } else { 0.0 };
        if self.air_w < 0.0 || pr.age < self.last_age {
            self.air_w = target;
        } else if pr.age > self.last_age {
            let d = f64::from(pr.age - self.last_age) * STEP / BLEND;
            self.air_w = if target > self.air_w { target.min(self.air_w + d) } else { target.max(self.air_w - d) };
        }
        self.last_age = pr.age;
        let inst = s.anim.m.as_ref().and_then(|st| {
            let m = montage_of(&w.data, w, ActorRef::Projectile(self.id), w.names.str(st.montage.name))?;
            Some((st, m))
        });
        if let Some(clips) = self.clips.clips(host, w) {
            let sks = self.sk.skeleton(clips);
            if self.air_w <= 0.0 {
                sample_track(&sks, clips, inst, "DefaultSlot", &mut self.out);
            } else if self.air_w >= 1.0 {
                sample_track(&sks, clips, inst, "AirSlot", &mut self.out);
            } else {
                sample_track(&sks, clips, inst, "DefaultSlot", &mut self.ground);
                sample_track(&sks, clips, inst, "AirSlot", &mut self.air);
                blend_pose(&self.ground, &self.air, self.air_w, &mut self.out);
            }
            self.sk.show(&self.out);
        }
        // The actor at UE (0, -x, y), yaw -90 facing right; facing left, the right-facing view reflected across UE Y.
        let r = right();
        let rot = std::array::from_fn(|j| r[j] * if s.facing_left && j % 3 == 1 { -1.0 } else { 1.0 });
        let (x, y) = at.at(alpha);
        let k = pr.scale;
        self.sk.set(&Xform {
            loc: [0.0, -x, y + self.mesh_z * f64::from(k)],
            rot,
            scale: [self.mesh_scale * f64::from(k); 3],
        });
        self.sk.draw(out);
    }

    /// The bones for the montage's PlayVFX notifies.
    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        self.sk.bones_world(out);
    }
}
