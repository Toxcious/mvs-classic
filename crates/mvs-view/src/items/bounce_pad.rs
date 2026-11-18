//! The pad Item_BouncePad lays, BouncePad_SpawnedActor_C (the sim: mvs-sim
//! items/bounce_pad.rs, a World actor), from its component templates and BouncePad_01_Skeleton_AnimBlueprint.
//!
//! SkeletalMesh (BouncePad_01, AnimClass BouncePad_01_Skeleton_AnimBlueprint) under the root Box1 at its relative
//! transform (Z -30, scale 7, yaw 90); the actor's rotation is 0 (the deferred spawn's) and its rotations are locked. A
//! plain AActor: no SetTeamColor, the mesh's own slots (the items part's draw).
//! The AnimBP: Root <- Slot DefaultSlot (bAlwaysUpdateSourcePose false) <- SequencePlayer BouncePad_01_Idle (looping,
//! rate 1). PlayAnimation @274 plays BouncePad_01_Bounce_Montage (one DefaultSlot segment BouncePad_01_Bounce, 0 to its
//! length, blend in and out 0) from its start at each bounce (the sim's bounce_at): while it plays the slot shows it
//! alone and the idle player under it is not updated (it goes on from where it stopped).
//! Deviation: the idle player's time is counted by the view over the ticks it presents (a rollback does not take it
//! back).

use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::consts::DELTA_SECONDS;

use crate::actor_view::{Comp, IDENT, SkinnedActor, place_under};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule};
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;

const PAD: &str = "BouncePad_SpawnedActor_C";
const MESH: &str = "SkeletalMesh";
const IDLE: &str = "BouncePad_01_Idle";
const BOUNCE: &str = "BouncePad_01_Bounce";

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[(PAD, MeshRule { rot: None, mesh: Some(pad_mesh) })];

#[derive(Debug)]
struct PadView {
    id: ActorId,
    key: String,
    sk: SkinnedActor,
    comp: Comp,
    has_idle: bool,
    has_bounce: bool,
    /// The idle player's time, in ticks.
    idle_ticks: i32,
    /// The last tick counted.
    tick: i32,
}

fn pad_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let c = cs.get(MESH)?;
    let key = format!("{PAD}.{MESH}");
    let Some(sk) = SkinnedActor::item(host, &key) else {
        warn_once("bounce pad: not drawn (its mesh or materials are not loaded)".into());
        return None;
    };
    let has_idle = host.item_clip(&key, Some(IDLE)).is_some();
    let has_bounce = host.item_clip(&key, Some(BOUNCE)).is_some();
    if !has_idle || !has_bounce {
        warn_once(format!("bounce pad: {} is not loaded", if has_idle { BOUNCE } else { IDLE }));
    }
    let comp = Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale };
    Some(Box::new(PadView { id, key, sk, comp, has_idle, has_bounce, idle_ticks: 0, tick: w.flow.tick }))
}

impl PadView {
    /// The montage's time at tick `t` (None: not playing), `len` its length.
    fn bounce_time(&self, at: i32, len: f64, t: i32) -> Option<f64> {
        if at < 0 || !self.has_bounce || t < at {
            return None;
        }
        let s = f64::from(t - at) * f64::from(DELTA_SECONDS);
        (s < len).then_some(s)
    }
}

impl ActorMesh for PadView {
    /// padFx's actors, once a World step: the idle player's ticks counted, the pose of the tick and the place.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::BouncePad(s) = &x.state else { return };
        let (at, px, py) = (s.bounce_at, s.x, s.y);
        let len = host.item_clip(&self.key, Some(BOUNCE)).map_or(0.0, |(c, k)| c.clips[k].anim.length);
        let now = w.flow.tick;
        for t in self.tick + 1..=now {
            if self.bounce_time(at, len, t).is_none() {
                self.idle_ticks += 1;
            }
        }
        self.tick = self.tick.max(now);
        match self.bounce_time(at, len, now) {
            Some(b) => {
                if let Some((clips, c)) = host.item_clip(&self.key, Some(BOUNCE)) {
                    self.sk.play(clips, c, b, false);
                }
            }
            None if self.has_idle => {
                if let Some((clips, c)) = host.item_clip(&self.key, Some(IDLE)) {
                    self.sk.play(clips, c, f64::from(self.idle_ticks) * f64::from(DELTA_SECONDS), true);
                }
            }
            None => {
                let rest = self.sk.rest.clone();
                self.sk.show(&rest);
            }
        }
        self.sk.set(&place_under(&self.comp, &IDENT, 1.0, f64::from(px), f64::from(py)));
    }

    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        _w: &World,
        _at: &ActorPlace,
        _alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.sk.draw(out);
    }
}
