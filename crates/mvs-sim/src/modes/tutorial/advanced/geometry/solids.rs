//! The advanced tutorials' solid actor classes, spawned at run time (`world/stage.rs` `RuntimeSolid`;
//! the solids of the geometry family):
//! - TutorialWall: a StaticMeshComponent root of 1M_Cube (a centred box of half extent 50) at scale (1.5, 4.025, 30),
//!   collision profile Floor (object type WorldStatic), DEV_Grid_02_MI, CastShadow off.
//! - TutorialWallShort: the same at scale (1.5, 1.5, 4) (its KeepOnCamera tag: no camera reads it in the port).
//! - TutorialPlatform (a BasePlatformBP child): a Platform-profile BoxComponent of extent (50, 53.5, 50) at scale
//!   (2, 5.5, 0.2) under the root, its StaticMeshComponent TUT_Platform_A_01 beside it at yaw 270 and scale 1.25,
//!   SwordBlade_Mat.
//!
//! Their places are the actor's, the boxes centred on it (no relative location on either collision component).
//! A class holds a solid as a [`Held`]: the actor's reference, its spawn record (the `RuntimeSolid` object
//! whole,
//! as the trace writes it) and, once destroyed, the solid as it was then (the object stays readable).

use crate::Real;
use crate::State;
use crate::fighter::StagePlatform;
use crate::math::narrow;
use crate::name;
use crate::names::Name;
use crate::world::World;
use crate::world::stage::{
    RuntimeBox, RuntimeSolid, RuntimeSolidSpec, add_runtime_solid, remove_runtime_solid, runtime_solid_at,
};

/// `SolidClass`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum SolidClass {
    Wall,
    WallShort,
    Platform,
}

impl SolidClass {
    /// The class name (`TutorialWall_C` ...).
    pub fn cls(self) -> &'static str {
        match self {
            SolidClass::Wall => "TutorialWall_C",
            SolidClass::WallShort => "TutorialWallShort_C",
            SolidClass::Platform => "TutorialPlatform_C",
        }
    }

    /// The collision profile is Floor (else Platform).
    pub fn floor(self) -> bool {
        self != SolidClass::Platform
    }

    /// `half`: the scaled half extent of the collision box (UE cm).
    pub fn half(self) -> [Real; 3] {
        match self {
            SolidClass::Wall => [50.0 * 1.5, 50.0 * 4.025, 50.0 * 30.0],
            SolidClass::WallShort => [50.0 * 1.5, 50.0 * 1.5, 50.0 * 4.0],
            SolidClass::Platform => [50.0 * 2.0, 53.5 * 5.5, 50.0 * 0.2],
        }
    }

    /// The collision component's name.
    pub fn component(self) -> Name {
        match self {
            SolidClass::Platform => name!("Box"),
            _ => name!("StaticMesh"),
        }
    }

    /// The static mesh the drawing side draws (a store path).
    pub fn mesh(self) -> &'static str {
        match self {
            SolidClass::Platform => "/Game/Panda_Main/Maps/Tutorials/Meshes/TUT_Platform_A_01",
            _ => "/Game/Geometry/Meshes/1M_Cube",
        }
    }

    /// Its material.
    pub fn material(self) -> &'static str {
        match self {
            SolidClass::Platform => "/Game/Panda_Main/Maps/Map01_LionWatch/SwordBlade_Mat",
            _ => "/Game/Panda_Main/Maps/Test_Gym/Materials/DEV_Grid_02_MI",
        }
    }

    /// The mesh component's rotation (pitch, yaw, roll) relative to the collision box.
    pub fn mesh_rot(self) -> [Real; 3] {
        match self {
            SolidClass::Platform => [0.0, 270.0, 0.0],
            _ => [0.0; 3],
        }
    }

    /// The mesh component's scale.
    pub fn mesh_scale(self) -> [Real; 3] {
        match self {
            SolidClass::Wall => [1.5, 4.025, 30.0],
            SolidClass::WallShort => [1.5, 1.5, 4.0],
            SolidClass::Platform => [1.25, 1.25, 1.25],
        }
    }
}

/// A solid as it was when its actor was destroyed (K2_DestroyActor): the record and, for a platform, its box.
#[derive(State, Debug)]
pub struct Gone {
    pub solid: RuntimeSolid,
    pub platform: Option<StagePlatform>,
}

/// A held `RuntimeSolid | null`: the actor a class stored (its field is `None` before the spawn and after a class
/// clears it), by the spawner's own number (`RuntimeSolid::spec`), with the spec it was made from.
#[derive(State, Debug)]
pub struct Held {
    pub class: SolidClass,
    /// `spec.center` (UE cm).
    pub center: [Real; 3],
    /// `spec.rot`: written only for a non-zero rotation.
    pub rot: Option<[Real; 3]>,
    /// `RuntimeSolid::spec`, unique among the live solids (`spec_of`).
    pub spec: i32,
    /// `destroyed`: the solid as it stood at its destroy.
    pub gone: Option<Box<Gone>>,
}

/// The number of the solid a tutorial object spawns in its `slot`: one per object and slot.
pub fn spec_of(owner: u32, slot: i32) -> i32 {
    i32::try_from(owner).expect("a tutorial object id") * 4 + slot
}

impl Held {
    /// `spawnSolid`: SpawnActor(class, location UE cm, rotation pitch / yaw / roll): `None` where the section is
    /// refused (warned).
    pub fn spawn(w: &mut World, class: SolidClass, spec: i32, loc: &[f64], rot: &[f64]) -> Option<Held> {
        let center = [narrow(loc[0]), narrow(loc[1]), narrow(loc[2])];
        let rot =
            if rot.iter().any(|&v| v != 0.0) { Some([narrow(rot[0]), narrow(rot[1]), narrow(rot[2])]) } else { None };
        let d = RuntimeSolidSpec {
            cls: class.cls().to_owned(),
            floor: class.floor(),
            center,
            half_extent: class.half(),
            rot,
            component: class.component(),
            flags: None,
        };
        add_runtime_solid(w, spec, &d).then_some(Held { class, center, rot, spec, gone: None })
    }

    /// `solidValid`: IsValid on the actor.
    pub fn valid(&self) -> bool {
        self.gone.is_none()
    }

    /// `x` / `y`: the root's place now (the box centre).
    pub fn at(&self, w: &World) -> (Real, Real) {
        match &self.gone {
            Some(g) => g.solid.at(),
            None => runtime_solid_at(w, self.spec).unwrap_or((0.0, 0.0)),
        }
    }

    /// `destroySolid`: K2_DestroyActor of a valid actor.
    pub fn destroy(&mut self, w: &mut World) {
        if !self.valid() {
            return;
        }
        let Some(list) = w.runtime_solids.as_ref() else { return };
        let Some(s) = list.iter().find(|s| s.spec == self.spec && !s.destroyed) else { return };
        let mut solid = s.clone();
        solid.destroyed = true;
        let platform = match solid.solid {
            RuntimeBox::Platform(i) => w.stage.stage_platforms.get(i as usize).cloned(),
            _ => None,
        };
        remove_runtime_solid(w, self.spec);
        self.gone = Some(Box::new(Gone { solid, platform }));
    }
}

/// `destroySolid` of a held reference that may be empty.
pub fn destroy(w: &mut World, h: &mut Option<Held>) {
    if let Some(h) = h {
        h.destroy(w);
    }
}

/// `solidValid` of a held reference that may be empty.
pub fn valid(h: &Option<Held>) -> bool {
    h.as_ref().is_some_and(Held::valid)
}
