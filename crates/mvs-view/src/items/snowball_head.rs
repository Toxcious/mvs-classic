//! Buff_SnowballHead's snowman head on its wearer (the sim: mvs-sim
//! items/snowball_head.rs, a buff of the fighter's list or of a spawned actor's).
//!
//! SnowmanHead (StaticMeshComponent, SnowManHead, RelativeScale3D 1.3, no material override) is attached by BeginPlay's
//! K2_AttachToComponent(the owner's BPI_GetSkeletalMesh, 'Head ATT', SnapToTarget, SnapToTarget, KeepRelative) @217:
//! at the socket with the socket's rotation, its own scale under the mesh's. It goes with the buff actor (its end: the
//! sim's entry leaving the list). The mesh and its slots are those of Item_SuperSnowball's SnowballMesh (the same
//! SnowManHead, no override either), drawn from that items-part draw.
//! Deviation: not drawn is Pawn_C017's head (the cast @2388 takes the BodyMesh's SkeletalMesh component,
//! snowmanHead_SkelMesh @2836, which the items part does not draw; SnowmanHead stays unattached there);
//! AdjustScalePerStack is empty.
//! Deviation: Comp_Pawn_MeshManager.AddMesh(SnowmanHead) @1056 is not followed: the head has no outline, hit flash or
//! invisibility of the pawn's meshes; it is hidden with the pawn's view root alone.
//! On a spawned actor the attach to a None parent does nothing (USceneComponent::AttachToComponent 142988650), so the
//! head is drawn at the buff actor's own transform: the hit actor's place and root scale at the buff's spawn (the
//! sim's ext), x 1.3, for the buff's life, not following the actor and not hidden with it.
//! Unverified: the rotation is the actor's at the first frame drawn with the buff (an item's facing yaw and pitch, a
//! projectile's actorRot, another actor's facing yaw), not the one at the spawn tick.

use std::collections::HashMap;

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::elem::ElemExt;
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::rotation_rows;

use crate::actor_view::{IDENT, StaticView, Xform, item_rows};
use crate::actors::{ActorHost, Rows, actor_rot};
use crate::frame::ActorPose;
use crate::skin_intro::warn_once;

const DRAW: &str = "Item_SuperSnowball_C.SnowballMesh";
const SOCKET: &str = "Head ATT";
/// SnowmanHead RelativeScale3D.
const SCALE: f64 = 1.3;
/// Pawn_C017's key.
const GIANT: &str = "irongiant";

/// The heads drawn: per fighter (its id) and per spawned actor's buff (the actor, the entry's serial); None where
/// the draw is not loaded (warned once).
#[derive(Debug, Default)]
pub(crate) struct Heads {
    fighters: HashMap<i32, Option<StaticView>>,
    placed: HashMap<(ActorRef, u32), Option<StaticView>>,
    /// The frame's fighters and buffs with a head (kept between frames).
    seen_f: Vec<i32>,
    seen_a: Vec<(ActorRef, u32)>,
}

/// The rotation rows of a spawned actor that wears the head (`actorRows`).
fn actor_rows(w: &World, a: ActorRef) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    let by = |f: f64| rotation_rows(0.0, if f > 0.0 { -h } else { h }, 0.0);
    match a {
        ActorRef::Item(id) => w.actors.items.get(id).map_or(IDENT, item_rows),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(IDENT, |pr| actor_rot(w, pr)),
        ActorRef::Marker(id) => w.actors.markers.get(id).map_or(IDENT, |m| by(f64::from(m.facing))),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map_or(IDENT, |m| by(f64::from(m.facing))),
        // Unverified: a World actor's facing is its class state's, which is not read here: taken as none (rotation 0).
        _ => IDENT,
    }
}

fn head_view(host: &ActorHost<'_>) -> Option<StaticView> {
    let v = StaticView::item(host, DRAW, "template");
    if v.is_none() {
        warn_once(format!("snowman head not drawn: {DRAW} is not loaded"));
    }
    v
}

impl Heads {
    /// headFx's frame: each drawn frame, after the views and the actors are placed: the spawned actors' heads at
    /// their buffs' transforms, the fighters' on their Head ATT sockets (`shown`: the fighter's view root is shown).
    pub(crate) fn present(
        &mut self,
        host: &ActorHost<'_>,
        w: &World,
        shown: impl Fn(FighterId) -> bool,
        out: &mut Vec<ActorPose>,
    ) {
        self.actors(host, w, out);
        self.seen_f.clear();
        for f in w.all_ids() {
            let fi = w.fighter(f);
            let on = fi.buffs.buffs.iter().any(|b| b.class == BuffClass::BuffSnowballHead && b.frames > 0.0)
                && w.names.str(fi.ch) != GIANT;
            if !on {
                continue;
            }
            self.seen_f.push(fi.id);
            let v = self.fighters.entry(fi.id).or_insert_with(|| head_view(host));
            let Some(v) = v else { continue };
            if !shown(f) {
                v.visible = false;
                continue;
            }
            let rel = Xform { loc: [0.0; 3], rot: IDENT, scale: [SCALE; 3] };
            let Some(at) = host.socket_follow(w, f, SOCKET, &rel).or_else(|| {
                // (socketFollow's `missing`: the mesh origin)
                host.mesh_origin(w, f).map(|o| Xform { scale: [SCALE; 3], ..o })
            }) else {
                continue;
            };
            v.visible = true;
            v.set(&at);
            push(v, out);
        }
        let seen = &self.seen_f;
        self.fighters.retain(|id, _| seen.contains(id));
    }

    /// The heads of the spawned actors (`actors`): made at a buff's first sight at its transform, gone with it.
    fn actors(&mut self, host: &ActorHost<'_>, w: &World, out: &mut Vec<ActorPose>) {
        let a = &w.actors;
        let live = a
            .projectiles
            .iter()
            .filter(|(_, x)| !x.dead)
            .map(|(id, x)| (ActorRef::Projectile(id), x.elem.as_ref()))
            .chain(a.items.iter().filter(|(_, x)| !x.dead).map(|(id, x)| (ActorRef::Item(id), x.elem.as_ref())))
            .chain(a.actors.iter().filter(|(_, x)| !x.dead).map(|(id, x)| (ActorRef::Actor(id), x.elem.as_ref())))
            .chain(a.markers.iter().filter(|(_, x)| !x.dead).map(|(id, x)| (ActorRef::Marker(id), x.elem.as_ref())))
            .chain(a.mortars.iter().filter(|(_, x)| !x.dead).map(|(id, x)| (ActorRef::Mortar(id), x.elem.as_ref())));
        self.seen_a.clear();
        for (r, elem) in live {
            let Some(elem) = elem else { continue };
            for b in elem.buffs.iter().filter(|b| b.class == BuffClass::BuffSnowballHead) {
                let k = (r, b.serial);
                self.seen_a.push(k);
                let v = self.placed.entry(k).or_insert_with(|| {
                    let mut v = head_view(host)?;
                    let (x, y, s) = match b.ext {
                        ElemExt::SnowballHead { x, y, scale } => (f64::from(x), f64::from(y), f64::from(scale)),
                        _ => (f64::NAN, f64::NAN, 1.0),
                    };
                    let s = s * SCALE;
                    v.set(&Xform { loc: [0.0, -x, y], rot: actor_rows(w, r), scale: [s; 3] });
                    Some(v)
                });
                if let Some(v) = v {
                    push(v, out);
                }
            }
        }
        let seen = &self.seen_a;
        self.placed.retain(|k, _| seen.contains(k));
    }

    /// The Lab's reset and the match's end: every head goes.
    pub(crate) fn clear(&mut self) {
        self.fighters.clear();
        self.placed.clear();
    }
}

fn push(v: &StaticView, out: &mut Vec<ActorPose>) {
    let mut meshes = Vec::new();
    v.draw(&mut meshes);
    if !meshes.is_empty() {
        out.push(ActorPose { meshes, ..ActorPose::default() });
    }
}
