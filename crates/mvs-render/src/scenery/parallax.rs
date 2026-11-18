//! Comp_Parallax_C, a component of a level's actor: the scenery entry's
//! `parallax: [ParalaxValueY, ParalaxValueZ]` on the actor's instances, or a stage module's record of a Blueprint
//! actor. Stage blocks use it: a ParallaxCamera per block, a ParallaxActor per owner.
//!
//! BeginPlay (@465..733) keeps the CameraCenter_C actor's location (CameraStartLocation) and its owner's
//! (ActorStartLocation); every tick off a dedicated server (@15..402) it moves the owner, with no sweep, to
//! ActorStartLocation + (0, dY x ParalaxValueY, dZ x ParalaxValueZ), (dX, dY, dZ) = CameraStartLocation - the camera
//! centre's location now. A level script may move ActorStartLocation (Tree Fort's cloud groups). What is attached to
//! the owner moves with it. Drawn at the frame, with the camera centre as the frame draws it (SceneryHost::camera; the
//! camera's place between two ticks), not at the tick: the owner keeps its place on screen as the camera eases.
// Unverified: the order of the component's tick and PFGCamera's (which moves the CameraCenter) is not read; this
// takes the camera centre of the same frame.

use mvs_view::M4;

use super::host::{InstanceId, SceneryHost};

/// The camera centre's start (at begin) and its offset this frame.
#[derive(Clone, Debug, Default)]
pub struct ParallaxCamera {
    start: [f64; 3],
    known: bool,
    /// CameraStartLocation - the camera centre, Y and Z.
    pub dy: f64,
    pub dz: f64,
}

impl ParallaxCamera {
    /// BeginPlay: CameraStartLocation (with no camera yet, the first frame that has one).
    pub fn begin(&mut self, host: &SceneryHost) {
        self.known = match host.camera() {
            Some(c) => {
                self.start = c;
                true
            }
            None => false,
        };
        self.dy = 0.0;
        self.dz = 0.0;
    }

    /// The camera centre's offset from its start this frame.
    pub fn frame(&mut self, host: &SceneryHost) {
        let Some(now) = host.camera() else { return };
        if !self.known {
            self.start = now;
            self.known = true;
        }
        self.dy = self.start[1] - now[1];
        self.dz = self.start[2] - now[2];
    }
}

/// One Comp_Parallax's owner: its placed location, ActorStartLocation (which a level script may move: `start`), and
/// what moves with it, each at its UE transform as placed (at the placed location): a translation of the owner moves
/// each by the same translation.
#[derive(Clone, Debug)]
pub struct ParallaxActor {
    pub name: String,
    pub placed: [f64; 3],
    /// ParalaxValueY, ParalaxValueZ.
    pub factor: [f64; 2],
    pub start: [f64; 3],
    /// Its location this frame - its placed one (UE).
    pub offset: [f64; 3],
    drawn: [f64; 3],
    instances: Vec<(InstanceId, M4)>,
    skeletals: Vec<(String, M4)>,
}

impl ParallaxActor {
    pub fn new(name: &str, placed: [f64; 3], factor: [f64; 2]) -> ParallaxActor {
        ParallaxActor {
            name: name.to_owned(),
            placed,
            factor,
            start: placed,
            offset: [0.0; 3],
            drawn: [f64::NAN; 3],
            instances: Vec::new(),
            skeletals: Vec::new(),
        }
    }

    /// An instance that moves with the owner, at its UE transform as placed.
    pub fn add(&mut self, id: InstanceId, base: M4) {
        self.instances.push((id, base));
    }

    /// A skeletal mesh that moves with the owner, at its component's UE transform as placed.
    pub fn add_skeletal(&mut self, name: &str, base: M4) {
        self.skeletals.push((name.to_owned(), base));
    }

    /// BeginPlay: ActorStartLocation back at the placed location.
    pub fn begin(&mut self) {
        self.start = self.placed;
    }

    /// Its location for the camera's offset, and what moves with it there (nothing done where it is drawn already).
    pub fn frame(&mut self, cam: &ParallaxCamera, host: &mut SceneryHost) {
        self.offset = [
            self.start[0] - self.placed[0],
            self.start[1] - self.placed[1] + cam.dy * self.factor[0],
            self.start[2] - self.placed[2] + cam.dz * self.factor[1],
        ];
        if self.offset == self.drawn {
            return;
        }
        self.drawn = self.offset;
        let moved = |base: &M4| {
            let mut m = *base;
            for (t, o) in m[3].iter_mut().zip(self.offset) {
                *t += o;
            }
            m
        };
        for (id, base) in &self.instances {
            host.place(*id, Some(moved(base)));
        }
        for (name, base) in &self.skeletals {
            host.place_skeletal(name, Some(moved(base)));
        }
    }
}
