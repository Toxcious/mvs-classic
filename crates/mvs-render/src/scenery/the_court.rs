//! The Court (Map_M010): the block its scenery entry has of its own Blueprint actors and level Blueprints
//! Block `court`. The entry draws every placed actor still, at its place and in
//! its saved or reference pose, with its placed emitters; this block adds what BeginPlay spawns and what moves with
//! the match clock.
//! - M010_ShotClock_C's loop over the hoops (GetAllActorsOfClass @816): PS_M010_Clock spawned attached to its
//!   PS_Spacejam_Clock once per hoop (SpawnEmitterAttached @357: offset 0, rot 0, scale 1, KeepRelativeOffset, auto
//!   destroyed), at the placed clock emitter's level transform (`shotClock.at`). Spawned again at each begin, the
//!   last ones killed first.
//! - The ship's montages, their notifies' effects and sounds (ship.rs), the clock hands (hands.rs), the balloon
//!   boat's level sequence (sequence.rs), the walls' barrier material (below).
//! - The spotlights stay in their reference pose: their animation is of another skeleton.
//! - The parts that read the World (the block's `tick`): the hoops' plays, explosion and score pop text (hoops.rs), the
//!   walls' floor glow, barrier place, Damage / Dissolve, effects and ambience (walls.rs), the ship's take-off at the
//!   match's end (ship.rs) and the clock hands' turn on a score change (hands.rs). The basketball's mesh is the
//!   scenery's three hidden instances, which `Renderer::scenery_place` shows (mvs-view court_ball.rs); the pickups' text is
//!   pickups.rs and their particles are
//!   the match's effects (fx/shared/m010_pickups.rs).
//! - The shot clock's Finished and the JumboTron's scoreboard (scoreboard.rs).
// Not ported: LevelKnockoutFX's `knockout` is not drawn.

mod hands;
mod hoops;
mod pickups;
mod scoreboard;
mod sequence;
mod ship;
mod walls;

use mvs_vfx::{Attachment, SystemId};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use self::hands::{Hands, HandsBlock};
use self::hoops::{HoopBlock, Hoops};
use self::pickups::Pickups;
use self::sequence::{Sequence, SequenceBlock};
use self::ship::{Ship, ShipBlock};
use self::walls::{WallBlock, Walls};
use mvs_view::M4;

use super::host::{InstanceId, SceneryHost};
use crate::Result;
use crate::stage::bad;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CourtBlock {
    shot_clock: ShotClock,
    hoops: Vec<HoopBlock>,
    ball: BallBlock,
    ship: ShipBlock,
    clock_hands: HandsBlock,
    sequence: SequenceBlock,
    walls: Vec<WallBlock>,
}

#[derive(Debug, Deserialize)]
struct Place {
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct ShotClock {
    at: Place,
    begin: Vec<String>,
}

/// The ball's three instances by material variant.
#[derive(Debug, Deserialize)]
struct BallBlock {
    gray: String,
    red: String,
    blue: String,
}

/// FRotationMatrix rows of a rotator in degrees, as an Attachment's.
pub(crate) fn rows_of(rot: [f64; 3]) -> [[f64; 3]; 3] {
    let r = mvs_vfx::math::rotation_rows(rot[0].to_radians(), rot[1].to_radians(), rot[2].to_radians());
    [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]]
}

/// The stage's block of its key `court`.
#[derive(Debug)]
pub struct TheCourt(Box<Court>);

#[derive(Debug)]
struct Court {
    clock_at: Attachment,
    clock_systems: Vec<String>,
    clocks: Vec<SystemId>,
    hoops: Hoops,
    hands: Hands,
    sequence: Sequence,
    ship: Ship,
    walls: Walls,
    pickups: Pickups,
    /// The ball's three hidden instances, by variant.
    ball: [(&'static str, InstanceId); 3],
    score: scoreboard::Scoreboard,
}

fn parse<T: DeserializeOwned>(key: &str, data: &Value) -> Result<T> {
    serde_json::from_value(data.clone()).map_err(|e| bad(&format!("scenery: {key}"), e.to_string()))
}

impl TheCourt {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<TheCourt> {
        let b: CourtBlock = parse(key, data)?;
        let what = "scenery: the court";
        let c = &b.shot_clock;
        let clock_at = Attachment { loc: c.at.loc, rot: rows_of(c.at.rot), scale: Some(c.at.scale) };
        let ball = [("gray", &b.ball.gray), ("red", &b.ball.red), ("blue", &b.ball.blue)].map(|(v, name)| {
            let id = host.instance(name);
            (v, id.ok_or_else(|| bad(what, format!("the ball's {v} instance {name} is not in the file"))))
        });
        let [(v0, i0), (v1, i1), (v2, i2)] = ball;
        let ball = [(v0, i0?), (v1, i1?), (v2, i2?)];
        let hoops = Hoops::make(b.hoops, host)?;
        let walls = Walls::make(b.walls, host)?;
        Ok(TheCourt(Box::new(Court {
            clock_at,
            clock_systems: c.begin.clone(),
            clocks: Vec::new(),
            hoops,
            hands: Hands::make(b.clock_hands, host)?,
            sequence: Sequence::make(b.sequence, host)?,
            ship: Ship::make(b.ship, host)?,
            walls,
            pickups: Pickups::default(),
            ball,
            score: scoreboard::Scoreboard::make(data)?,
        })))
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        self.0.begin(host);
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.0.tick(host, dt);
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        self.0.frame(host, alpha);
    }

    pub fn end(&mut self, host: &mut SceneryHost) {
        self.0.hide_ball(host);
        self.0.score.end(host);
    }

    /// A part of the block a stage item's view places (mvs-view `SceneryPlace`): the ball's `shown` variant at its UE
    /// transform, the others hidden; None: all hidden (the ball mesh's show and place).
    pub fn place_part(&mut self, host: &mut SceneryHost, part: &str, shown: Option<(&str, M4)>) {
        if part != "ball" {
            panic!("scenery: the court has no part {part}");
        }
        for (variant, id) in self.0.ball {
            match shown {
                Some((v, m)) if v == variant => {
                    host.place(id, Some(m));
                    host.show(&[id], true);
                }
                _ => host.show(&[id], false),
            }
        }
    }
}

impl Court {
    fn begin(&mut self, host: &mut SceneryHost) {
        if let Some(vfx) = host.vfx() {
            for id in self.clocks.drain(..) {
                vfx.kill(id);
            }
            for system in &self.clock_systems {
                self.clocks.extend(vfx.spawn_level(system, &self.clock_at, false, &[], 0, false));
            }
        }
        self.hands.begin(host);
        self.sequence.begin(host);
        self.ship.begin(host);
        self.walls.begin(host);
        self.hoops.begin(host);
        self.pickups.begin();
        self.hide_ball(host);
        self.score.begin(host);
    }

    /// The ball's meshes hidden (the Item_Basketball_C instances are the scenery's, shown while an item draws them).
    fn hide_ball(&self, host: &mut SceneryHost) {
        for (_, id) in self.ball {
            host.show(&[id], false);
        }
    }

    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        let knockouts: Option<Vec<f64>> =
            host.world().map(|w| w.flow.team_knockouts.iter().map(|&k| f64::from(k)).collect());
        self.hands.tick(dt, knockouts.as_deref());
        self.sequence.tick(host, dt);
        // (the ship's AnimBP pose, after the blocks' ticks as the skeletal meshes' animations)
        self.ship.tick(host, dt);
        self.walls.tick(host);
        self.hoops.tick(host);
        self.pickups.tick(host);
        self.score.tick(host);
    }

    fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        self.hands.frame(host, alpha);
        self.sequence.frame(host, alpha);
        self.walls.frame(host, alpha);
        self.score.frame(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Court's entry of the developer's store makes its block: the clock hands, the sequence's binding, the
    /// ship's montages and sockets and the walls' barriers are found; the hands, the boat and the ship move without a
    /// World.
    #[test]
    fn court_block_makes() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        let Ok(pack) = store.kind("scenery") else { return };
        let Some(path) = pack.paths().find(|p| p.to_ascii_lowercase().contains("m010")) else { return };
        let bytes = pack.bytes(path).unwrap();
        let bin = mvs_material::jsonbin::JsonBin::split(b"MVSC", &bytes, path).unwrap();
        let h: super::super::SceneryHeader = bin.parse(path).unwrap();
        let s = h.scenery;
        let meshes = s
            .skeletal
            .iter()
            .map(|k| std::sync::Arc::new(mvs_view::mesh::MeshInfo::load(&store, &k.mesh).unwrap()))
            .collect();
        let mut run =
            super::super::SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        let k = run.blocks.iter().position(|b| matches!(b, super::super::SceneryBlock::TheCourt(_))).unwrap();
        let mut court = std::mem::replace(&mut run.blocks[k], super::super::SceneryBlock::Pending(String::new()));
        let mut sound = super::super::NullSound;
        let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
        court.begin(&mut host);
        let before = host.rest("SJAM_MG_MarvinShip_2.Mesh").unwrap().clone();
        for _ in 0..90 {
            court.tick(&mut host, 1.0 / 60.0);
        }
        court.frame(&mut host, 0.5);
        // the landing montage has moved the ship's bones off the reference pose
        assert_ne!(host.pose("SJAM_MG_MarvinShip_2.Mesh").unwrap().t, before.t);
        let a = host.instance("SJAM_BombFactory_ClockHands_4.SJAM_BombFactory_ClockHand_A").unwrap();
        assert!(host.state.placed[a.0 as usize].is_some());
        let boat = host.skeletal_index("M010_BalloonBoat_01").unwrap();
        let m = host.state.skeletal_placed[boat].unwrap();
        // the translation channels at 1.5 s + 1/30: X -7520, Y and Z near their first keys
        assert_eq!(m[3][0], -7520.0);
        assert!((m[3][1] - -1730.0).abs() < 300.0 && (m[3][2] - 2012.7).abs() < 100.0, "{:?}", m[3]);
        assert_eq!(host.state.anims[boat].rate, 0.0);
    }
}
