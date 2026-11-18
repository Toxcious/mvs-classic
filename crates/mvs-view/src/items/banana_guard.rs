//! ItemHelper_BananaGuard as a match draws it (the sim: mvs-sim items/banana_guard.rs,
//! a World actor), from its graph, BananaGuard_AnimBp and the class's component templates.
//!
//! The mesh: CharacterMesh0 (BananaGuard_Skelmesh, AnimClass BananaGuard_AnimBp) under the CollisionCylinder at
//! (0, 0, -210), scale 0.8, no rotation of its own. The capsule's world rotation is yaw -90 (BeginPlay
//! K2_SetWorldRotation @1665, a constant: its X is UE -Y, the sim's +x). The facing is the mesh's X scale:
//! SetFacingMulticast @2358-2575 sets RelativeScale3D (FacingLeft ? -OriginalScale.X : OriginalScale.X, Y, Z),
//! OriginalScale = the template's (BeginPlay @1616): the sim's facing_left and mesh_scale.
//!
//! BananaGuard_AnimBp: its Movement machine is the sim's (items/banana_guard.rs: the SpearHitBox rides the pose, so the
//! sim runs it). The view draws the clip of the sim's state (GUARD_STATE_CLIP[anim]) at the sim's player time
//! (anim_time) each tick, and runs no machine of its own: what is drawn is the pose the hit box is taken from, a
//! rollback included. Every transition the guard can take has crossfade 0, so the pose is one clip alone.
//! Unverified: the pose is drawn at the presented tick's state; the mesh's tick within the frame (after the CMC: the
//! ACharacter mesh's prerequisite) is taken, not read in the exe for this class.
//! The death poof (PS_TomJerryPoof00, DeathEffectMulticast @2800) and the sounds are the sim's events.
//! The fighter whose character data has the class (the user of the TV need not be it) has its mesh, clips and
//! materials; a match with random items whose fighters lack it draws the items part's draw of the component and the
//! sequences of its folder.

use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::items::banana_guard::GUARD_STATE_CLIP;
use mvs_vfx::math::rotation_rows;

use super::{ClipSet, data_fighter};
use crate::actor_view::{SkinnedActor, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule, Rows};
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;

const CLS: &str = "ItemHelper_BananaGuard_C";
const MESH_COMP: &str = "CharacterMesh0";

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[(CLS, MeshRule { rot: None, mesh: Some(guard_mesh) })];

#[derive(Debug)]
struct GuardView {
    id: ActorId,
    sk: SkinnedActor,
    set: ClipSet,
    /// By the sim's state.
    clips: [Option<usize>; 5],
    rot: Rows,
}

fn guard_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let from = data_fighter(w, x.owner, x.cls);
    let key = format!("{CLS}.{MESH_COMP}");
    let (sk, set) = match from {
        Some(f) => {
            let mesh = cs.get(MESH_COMP).and_then(|c| match &c.kind {
                ComponentKind::SkeletalMesh { mesh, .. } => Some(mesh.as_str()),
                _ => None,
            });
            let drawn = mesh.and_then(|m| host.draws_of(w, f)?.actor(m, None));
            let sk = drawn.and_then(|(path, keys)| Some(SkinnedActor::new(host.mesh(&path)?, &keys)));
            (sk, ClipSet::Fighter(f))
        }
        None => (SkinnedActor::item(host, &key), ClipSet::Items),
    };
    let Some(mut sk) = sk else {
        warn_once("BananaGuard: not drawn (its mesh or materials are not loaded)".into());
        return None;
    };
    // (drawn between the sim's ticks: place's present)
    sk.blend_ticks();
    let clips = set.clips(host, w)?;
    let clips = GUARD_STATE_CLIP.map(|n| {
        let k = clips.get(n);
        if k.is_none() {
            warn_once(format!("BananaGuard: clip missing: {n}"));
        }
        k
    });
    Some(Box::new(GuardView { id, sk, set, clips, rot: rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0) }))
}

impl ActorMesh for GuardView {
    /// guardFx's actors, once a World step: the sim's pose of the tick, then the component's transform (the capsule:
    /// sim x = -UE Y, y = UE Z, the guard at UE X 0; the mesh's relative (0, 0, mesh_z)).
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::BananaGuard(g) = &x.state else { return };
        let clip = usize::try_from(g.anim).ok().and_then(|i| self.clips.get(i).copied().flatten());
        match (clip, self.set.clips(host, w)) {
            (Some(c), Some(clips)) => self.sk.play(clips, c, f64::from(g.anim_time), false),
            _ => {
                let rest = self.sk.rest.clone();
                self.sk.show(&rest);
            }
        }
        let s = g.mesh_scale;
        // (the template's 0.8 x 3)
        let scale = [if g.facing_left { -s } else { s }, s, s];
        self.sk.set(&Xform {
            loc: [0.0, f64::from(-g.x), f64::from(g.y + g.mesh_z)],
            rot: self.rot,
            scale: scale.map(f64::from),
        });
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
}
