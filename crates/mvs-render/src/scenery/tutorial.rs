//! The tutorial level (Map_TrophyRoom_Tutorial): its block `tutorialSolids`.
//!
//! TutorialWall, TutorialWallShort and TutorialPlatform are the run-time solids (`World::runtime_solids`): the scenery
//! import writes each class's StaticMeshComponent once as a hidden instance at its transform relative to the actor's
//! root (`tutorialSolids`: class -> instance name), and this block draws one per live solid at the solid's place each
//! tick: the actor's root is the collision box's centre (sim x = -UE Y, y = UE Z; UE X as spawned) and its spawn
//! rotation. A solid gone from the list is no longer drawn.
//!
//! The block also pops Tutorial_Meeseeks's "+1": its On Hit's PopText when it
//! was destroyed by its own hit (MeeseeksEnd::Hit), lifetime 1, text colour (0.038204, 0.83077, 0.309469, 1), at the
//! actor + (0, 0, 200), scale 3, justification 1.
// Unverified: the scenery's statics are made once at load, so the
// import's instance of a class is cloned there into SOLID_COPIES instances (`<name>#<k>`), each placed or hidden here.
// No tutorial holds more than two solids of a class at once; one past the copies is not drawn (warned).
// Unverified: the World's RuntimeSolid keeps
// its spec's number only, so the spec is the held solid (geometry `Held`) of the same number among the tutorial's
// demos and minigames.
// Unverified: the pop is the scenery's (SceneryHost::pop, which the match's HUD reads), in the same queue and the same
// tick as the Meeseeks view's pop.
// Unverified: space 0 taken as the world place projected each frame, as the pop texts with an attach actor.

use std::collections::{BTreeMap, BTreeSet};

use mvs_sim::modes::tutorial::advanced::geometry::solids::Held;
use mvs_sim::modes::tutorial::advanced::geometry::{
    FastFallPlatform, KbiSideMinigame, KbiUpDemo, KbiUpMinigame, SpawnPlatform, SpawnWall, WallSlide,
};
use mvs_sim::modes::tutorial::advanced::meeseeks::MeeseeksEnd;
use mvs_sim::modes::tutorial::demo::DemoState;
use mvs_sim::modes::tutorial::minigame::MinigameState;
use mvs_sim::world::World;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_view::M4;
use serde_json::Value;

use super::Instance;
use super::host::{InstanceId, PopSpec, SceneryHost};
use crate::Result;
use crate::stage::{bad, transform};
use crate::view::mul;

/// The instances drawn per solid class: the import's and its clones.
pub(crate) const SOLID_COPIES: usize = 4;

const POP_COLOR: [f64; 4] = [0.038204, 0.83077, 0.309469, 1.0];

/// The entry's instances with SOLID_COPIES - 1 clones of each instance the `tutorialSolids` block names, each named
/// `<name>#<k>` (scenery's load, before the statics are made).
pub(crate) fn clone_solids(instances: &mut Vec<Instance>, block: Option<&Value>) {
    let Some(block) = block.and_then(Value::as_object) else { return };
    for name in block.values().filter_map(Value::as_str) {
        let Some(i) = instances.iter().find(|i| i.name == name).cloned() else { continue };
        for k in 1..SOLID_COPIES {
            let mut c = i.clone();
            c.name = format!("{name}#{k}");
            instances.push(c);
        }
    }
}

/// A class's instances and the transform of its mesh relative to the actor's root.
#[derive(Debug)]
struct Class {
    copies: Vec<InstanceId>,
    rel: M4,
}

/// The block `tutorialSolids`.
#[derive(Debug)]
pub struct Tutorial {
    classes: BTreeMap<String, Class>,
    /// The Meeseeks popped (stage actor index, spawn tick).
    popped: BTreeSet<(u32, i32)>,
    /// The solids no held spec names, warned.
    unknown: BTreeSet<i32>,
    full: bool,
}

/// The held solids of the tutorial's demos and minigames, by spec number (their class, centre and rotation).
fn held_solids(w: &World) -> BTreeMap<i32, &Held> {
    let mut out = BTreeMap::new();
    let Some(t) = w.tutorial.as_deref() else { return out };
    let mut held: Vec<&Option<Held>> = Vec::new();
    for d in &t.demos {
        match &d.state {
            DemoState::WallSlide(WallSlide { wall, .. }) | DemoState::KbiUp(KbiUpDemo { wall, .. }) => held.push(wall),
            DemoState::FastFallPlatform(FastFallPlatform { platform, .. }) => held.push(platform),
            _ => {}
        }
    }
    for m in t.actor.iter().flat_map(|a| &a.minigames) {
        match &m.state {
            MinigameState::SpawnWall(SpawnWall { wall }) => held.push(wall),
            MinigameState::SpawnPlatform(SpawnPlatform { platform }) => held.push(platform),
            MinigameState::KbiSide(KbiSideMinigame { k, .. }) | MinigameState::KbiUp(KbiUpMinigame { k }) => {
                held.push(&k.wall)
            }
            _ => {}
        }
    }
    for h in held.into_iter().flatten().filter(|h| h.valid()) {
        out.insert(h.spec, h);
    }
    out
}

impl Tutorial {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<Tutorial> {
        let block = data.as_object().ok_or_else(|| bad(key, "not an object"))?;
        let mut classes = BTreeMap::new();
        for (cls, name) in block {
            let name = name.as_str().ok_or_else(|| bad(key, format!("{cls}: not an instance name")))?;
            let mut copies = Vec::new();
            for k in 0..SOLID_COPIES {
                let n = if k == 0 { name.to_owned() } else { format!("{name}#{k}") };
                copies.push(host.instance(&n).ok_or_else(|| bad(key, format!("no instance {n}")))?);
            }
            let i = &host.instances()[copies[0].0 as usize];
            let rel = transform(i.loc, i.rot, i.scale);
            host.show(&copies, false);
            classes.insert(cls.clone(), Class { copies, rel });
        }
        Ok(Tutorial { classes, popped: BTreeSet::new(), unknown: BTreeSet::new(), full: false })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        self.hide(host);
        self.popped.clear();
    }

    pub fn tick(&mut self, host: &mut SceneryHost, _dt: f64) {
        let Some(w) = host.world() else {
            self.hide(host);
            return;
        };
        self.solids(host, w);
        self.pops(host, w);
    }

    pub fn frame(&mut self, _host: &mut SceneryHost, _alpha: f64) {}

    pub fn end(&mut self, host: &mut SceneryHost) {
        self.hide(host);
    }

    fn hide(&self, host: &mut SceneryHost) {
        for c in self.classes.values() {
            host.show(&c.copies, false);
            for &id in &c.copies {
                host.place(id, None);
            }
        }
    }

    /// A copy of its class's mesh per live solid at its root.
    fn solids(&mut self, host: &mut SceneryHost, w: &World) {
        let held = held_solids(w);
        let mut used: BTreeMap<&str, usize> = BTreeMap::new();
        for s in w.runtime_solids.iter().flatten().filter(|s| !s.destroyed) {
            let Some(h) = held.get(&s.spec) else {
                if self.unknown.insert(s.spec) {
                    eprintln!("tutorialSolids: the run-time solid {} is no tutorial's held solid: not drawn", s.spec);
                }
                continue;
            };
            let cls = h.class.cls();
            let Some(c) = self.classes.get(cls) else { continue };
            let k = used.entry(cls).or_insert(0);
            let Some(&id) = c.copies.get(*k) else {
                if !self.full {
                    self.full = true;
                    eprintln!("tutorialSolids: more than {SOLID_COPIES} live {cls}: the rest are not drawn");
                }
                continue;
            };
            *k += 1;
            let (x, y) = s.at();
            let r = h.rot.map_or([0.0; 3], |r| r.map(f64::from));
            let root = transform([f64::from(h.center[0]), -f64::from(x), f64::from(y)], r, [1.0; 3]);
            host.place(id, Some(mul(&c.rel, &root)));
            host.show(&[id], true);
        }
        for (cls, c) in &self.classes {
            let from = used.get(cls.as_str()).copied().unwrap_or(0);
            host.show(&c.copies[from..], false);
        }
    }

    /// The "+1" of a Meeseeks its own hit destroyed, once.
    fn pops(&mut self, host: &mut SceneryHost, w: &World) {
        let Some(run) = w.stage_run.as_ref() else { return };
        for a in run.actors.iter().filter(|a| a.dead) {
            let StageActorOwn::Meeseeks(m) = &a.own else { continue };
            if m.end != MeeseeksEnd::Hit || !self.popped.insert((a.index, m.spawned)) {
                continue;
            }
            host.pop(PopSpec {
                text: "+1".to_owned(),
                lifetime: 1.0,
                text_color: POP_COLOR,
                location: [0.0, 0.0, 200.0],
                scale: 3.0,
                justification: 1,
                delay: 0.0,
                icon_right: None,
                fighter: None,
                at: Some([0.0, -f64::from(a.x), f64::from(a.y)]),
            });
        }
    }
}
