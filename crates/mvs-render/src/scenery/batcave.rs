//! Batcave (Map_Batcave): the blocks its scenery entry has of its own Blueprint actors and level Blueprints
//! The blocks are `batcave` and `batcaveMotion`.
//! - The centre floor (BCAVE_ST_main_A_01_BP_C) and the two fans' blades (Batcave_FanBlades_C) are the file's
//!   instances; the blades turn by `batcaveMotion` (vents.rs).
//! - The two Batmobiles (BatcaveWallDestructible_C: car.rs), the two bat swarms (BatcaveBatsBP_C: bats.rs), the two
//!   fogs (FogOcclusionBP_C) and the six warning screens (vents.rs) are drawn from their sim state, the World's stage
//!   run read each tick (SceneryHost's World). The cars' splash (PS_BatcaveWaterfallForeGround_Splashing:
//!   PS_Batcave_Batmobile_SplashLoop on the mesh's socket VFX_Root, bone Root, at the component's RelativeLocation)
//!   is spawned at BeginPlay (bAutoActivate) and follows the bone. The component saves WarmupTime 2, never read:
//!   InitParticles 142bf0750 and SetTemplate copy the template's (0) over it before ActivateSystem warms up.
//! - The six warning screens (`screens`, in the sim's SCREEN_* order: mvs-sim stages/batcave/vents.rs) start hidden:
//!   BeginPlay's SetBatcaveWarningState(Idle) hides all six; they are shown from VentsState.hidden.

mod bats;
mod car;
mod vents;

use mvs_sim::World;
use mvs_sim::world::stage::StageOwn;
use mvs_sim::world::stage_actors::StageActorOwn;
use serde::Deserialize;
use serde_json::Value;

use super::host::{InstanceId, SceneryHost, stage_actor};
use crate::Result;
use crate::stage::bad;

pub use vents::BatcaveMotion;

#[derive(Debug, Deserialize)]
struct FanRef {
    instance: String,
}

#[derive(Debug, Deserialize)]
struct BlockData {
    cars: Vec<car::CarData>,
    bats: Vec<bats::BatsData>,
    fogs: Vec<vents::FogData>,
    screens: Vec<String>,
    fans: Vec<FanRef>,
}

/// The `batcave` block: the cars, the swarms, the fogs and the screens.
#[derive(Debug)]
pub struct Batcave {
    cars: Vec<car::Car>,
    bats: Vec<bats::Bats>,
    fogs: Vec<vents::Fog>,
    /// Each screen's instances, and whether it is shown now.
    screens: Vec<Vec<InstanceId>>,
    shown: Vec<bool>,
    /// The StageData the indices below were matched in (its address), matched once per StageData.
    placed_of: Option<usize>,
    /// The cars', swarms' and fogs' places in StageData.placed (by their actor names).
    idx_cars: Vec<Option<u32>>,
    idx_bats: Vec<Option<u32>>,
    idx_fogs: Vec<Option<u32>>,
}

impl Batcave {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<Batcave> {
        let what = format!("scenery batcave: {key}");
        let d: BlockData = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        let field = |i: &super::Instance, f: &str| i.fields.get(f).and_then(Value::as_u64);
        let mut screens = vec![Vec::new(); d.screens.len()];
        let mut fans = vec![0usize; d.fans.len()];
        for (k, i) in host.instances().iter().enumerate() {
            if let Some(s) = field(i, "batcaveScreen") {
                let s = s as usize;
                if d.screens.get(s) != Some(&i.name) || !i.hidden {
                    return Err(bad(&what, format!("batcave screen {}", i.name)));
                }
                screens[s].push(InstanceId(k as u32));
            }
            if let Some(f) = field(i, "batcaveFan") {
                let f = f as usize;
                if d.fans.get(f).map(|x| &x.instance) != Some(&i.name) {
                    return Err(bad(&what, format!("batcave fan {}", i.name)));
                }
                fans[f] += 1;
            }
        }
        if screens.iter().any(Vec::is_empty) || fans.contains(&0) {
            return Err(bad(&what, "batcave screens or fans not in the file"));
        }
        let cars = d.cars.into_iter().map(|c| car::Car::make(c, host)).collect::<Result<Vec<_>>>()?;
        let n = d.screens.len();
        Ok(Batcave {
            idx_cars: vec![None; cars.len()],
            idx_bats: vec![None; d.bats.len()],
            idx_fogs: vec![None; d.fogs.len()],
            cars,
            bats: d.bats.into_iter().map(bats::Bats::new).collect(),
            fogs: d.fogs.into_iter().map(vents::Fog::new).collect(),
            screens,
            shown: vec![false; n],
            placed_of: None,
        })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        // (the previous level's systems were ended before: VfxPlayer.endAll)
        for c in &mut self.cars {
            c.begin(host);
        }
        for b in &mut self.bats {
            b.begin();
        }
        for f in &mut self.fogs {
            f.begin();
        }
        for (k, ids) in self.screens.iter().enumerate() {
            if self.shown[k] {
                host.show(ids, false);
                self.shown[k] = false;
            }
        }
    }

    /// The sim actors by their StageData.placed index, once per StageData.
    fn match_world(&mut self, w: &World) {
        let at = std::ptr::from_ref(&w.data.stage) as usize;
        if self.placed_of == Some(at) {
            return;
        }
        self.placed_of = Some(at);
        let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
        let by = |n: &str| placed.iter().position(|p| p.name == n).map(|k| k as u32);
        self.idx_cars = self.cars.iter().map(|c| by(c.actor())).collect();
        self.idx_bats = self.bats.iter().map(|b| by(&b.d.actor)).collect();
        self.idx_fogs = self.fogs.iter().map(|f| by(&f.d.actor)).collect();
    }

    pub fn tick(&mut self, host: &mut SceneryHost, _dt: f64) {
        // (the World's reference outlives the host's borrow: the host is changed below)
        let world = host.world;
        if let Some(w) = world
            && w.stage_run.is_some()
        {
            self.world_tick(host, w);
        }
        // (the AnimBP's pose as the drive writes it, then what follows the cars' bones, then
        // the VfxPlayer's follow)
        for c in &mut self.cars {
            c.pose(host);
            c.follow(host);
        }
    }

    fn world_tick(&mut self, host: &mut SceneryHost, w: &World) {
        self.match_world(w);
        let tick = w.flow.tick;
        let own = |i: Option<u32>| i.and_then(|i| stage_actor(w, i)).map(|a| &a.own);
        for (c, &i) in self.cars.iter_mut().zip(&self.idx_cars) {
            if let Some(StageActorOwn::BatcaveCar(a)) = own(i) {
                c.tick(host, a, tick);
            }
        }
        for (b, &i) in self.bats.iter_mut().zip(&self.idx_bats) {
            if let Some(StageActorOwn::BatcaveBats(a)) = own(i) {
                b.tick(host, a, tick);
            }
        }
        for (f, &i) in self.fogs.iter_mut().zip(&self.idx_fogs) {
            let a = match own(i) {
                Some(StageActorOwn::BatcaveFog(a)) => Some(a),
                _ => None,
            };
            f.tick(host, a);
        }
        if let Some(StageOwn::Batcave(s)) = w.stage_run.as_ref().map(|r| &r.s) {
            for (k, ids) in self.screens.iter().enumerate() {
                let on = !s.vents.hidden.get(k).copied().unwrap_or(true);
                if on != self.shown[k] {
                    self.shown[k] = on;
                    host.show(ids, on);
                }
            }
        }
    }

    pub fn frame(&mut self, _host: &mut SceneryHost, _alpha: f64) {}

    pub fn end(&mut self, _host: &mut SceneryHost) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mvs_material::jsonbin::JsonBin;
    use mvs_view::mesh::MeshInfo;

    use super::super::SceneryHeader;
    use super::super::host::{NullSound, SceneryHost};
    use super::super::run::SceneryRun;

    /// The Batcave's scenery entry makes its two blocks (the data as the importer writes it, the clips loaded, the
    /// sockets' bones on the cars' skeletons), and a tick with no World leaves the cars in their reference pose.
    #[test]
    fn batcave_blocks_make() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let pack = store.kind("scenery").unwrap();
        let Some(path) = pack.paths().find(|p| p.ends_with("/batcave.batcave")).map(str::to_owned) else {
            eprintln!("no batcave scenery: nothing to check");
            return;
        };
        let bytes = pack.bytes(&path).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        let names: Vec<String> = run.state.skeletal.iter().map(|k| k.name.clone()).collect();
        let mut sound = NullSound;
        let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
        for b in &mut run.blocks {
            b.begin(&mut host);
        }
        for b in &mut run.blocks {
            b.tick(&mut host, 1.0 / 60.0);
        }
        for (k, n) in names.iter().enumerate() {
            assert_eq!(run.state.skins[k].pose, run.state.skins[k].info.rest, "{n}: not in its reference pose");
        }
    }

    /// The stage-batcave match stepped with the blocks reading its World: at each tick the screens show as
    /// VentsState.hidden says, a car under a full-weight montage shows it, a swarm's stamps spawn its systems, and the
    /// fans turn at the rate their vents' state gives.
    #[test]
    fn batcave_follows_the_world() {
        use mvs_sim::world::stage::StageOwn;
        use mvs_sim::world::stage_actors::StageActorOwn;

        use super::super::blocks::SceneryBlock;
        use crate::stage::stage_key;
        use crate::stagedraw::StageDraw;

        let Some(mut h) = mvs_sim_trace::harness::at("stage-batcave", 0) else { return };
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
        let entry = StageDraw::load(&store, &stage_key(&h.w.data.stage_key)).unwrap().scenery.entry;
        let bytes = store.kind("scenery").unwrap().bytes(&entry).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &entry).unwrap().parse::<SceneryHeader>(&entry).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        let mut vfx = mvs_vfx::Player::new(&store, 1).unwrap();
        let mut sound = NullSound;
        let dt = 1.0 / 60.0;
        {
            let mut host = SceneryHost { state: &mut run.state, vfx: Some(&mut vfx), sound: &mut sound, world: None };
            for b in &mut run.blocks {
                b.begin(&mut host);
            }
        }
        let (mut screens_on, mut retract_seen, mut warn_seen, mut main_seen, mut fast) = (0, 0, 0, 0, 0);
        while h.w.flow.tick < 6800 {
            h.step_recorded();
            {
                let mut host =
                    SceneryHost { state: &mut run.state, vfx: Some(&mut vfx), sound: &mut sound, world: Some(&h.w) };
                for b in &mut run.blocks {
                    b.tick(&mut host, dt);
                }
            }
            run.state.tick_skeletal(dt);
            vfx.tick(dt);
            let w = &h.w;
            let tick = w.flow.tick;
            let Some(StageOwn::Batcave(own)) = w.stage_run.as_ref().map(|r| &r.s) else { panic!("no batcave state") };
            if [981, 4241, 5701, 6761].contains(&tick) {
                eprintln!(
                    "tick {tick}: vents warning {} hidden {:?} fans {:?}",
                    own.vents.warning, own.vents.hidden, own.vents.fans
                );
                for a in &w.stage_run.as_ref().unwrap().actors {
                    match &a.own {
                        StageActorOwn::BatcaveCar(c) => eprintln!(
                            "  car {}: destroyed {} stage {} weight {} montage {:?} at {} done {} top {} front {} splashOff {}",
                            a.index,
                            c.wall.destroyed,
                            c.wall.stage,
                            c.weight,
                            c.montage,
                            c.montage_at,
                            c.montage_done,
                            c.top_at,
                            c.front_at,
                            c.splash_off_at
                        ),
                        StageActorOwn::BatcaveBats(b) => {
                            eprintln!("  bats {}: on {} warn {} main {}", a.index, b.on, b.warn_at, b.main_at)
                        }
                        StageActorOwn::BatcaveFog(f) => eprintln!("  fog {}: particle {}", a.index, f.particle),
                        _ => {}
                    }
                }
            }
            for b in &run.blocks {
                match b {
                    SceneryBlock::Batcave(c) => {
                        for (k, ids) in c.screens.iter().enumerate() {
                            let on = !own.vents.hidden[k];
                            screens_on += usize::from(on);
                            for id in ids {
                                assert_eq!(run.state.shown[id.0 as usize], on, "screen {k} at tick {tick}");
                            }
                        }
                        for (car, i) in c.cars.iter().zip(&c.idx_cars) {
                            let a = i.and_then(|i| super::stage_actor(w, i)).map(|a| &a.own);
                            let Some(StageActorOwn::BatcaveCar(a)) = a else {
                                panic!("car {} not in the World", car.actor())
                            };
                            if a.montage == "BatMobile_Retract_Montage" && !a.montage_done {
                                retract_seen += 1;
                                assert!(car.montage_on(), "{} retract not drawn at {tick}", car.actor());
                            }
                        }
                        for (b, i) in c.bats.iter().zip(&c.idx_bats) {
                            let a = i.and_then(|i| super::stage_actor(w, i)).map(|a| &a.own);
                            let Some(StageActorOwn::BatcaveBats(a)) = a else { panic!("bats not in the World") };
                            if a.warn_at >= 0 && tick - a.warn_at == 10 {
                                warn_seen += 1;
                                assert!(b.warn_alive(&vfx), "warning swarm not alive at {tick}");
                            }
                            if a.main_at >= 0 && tick - a.main_at == 10 {
                                main_seen += 1;
                                assert!(b.main_alive(&vfx), "full swarm not alive at {tick}");
                            }
                        }
                    }
                    SceneryBlock::BatcaveMotion(m) => {
                        for (k, &on) in own.vents.fans.iter().enumerate() {
                            if on {
                                fast += 1;
                                assert!(m.rate(k) > 60.5, "fan {k} at {tick}: {}", m.rate(k));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        eprintln!(
            "screens on {screens_on}, retract ticks {retract_seen}, warn {warn_seen}, main {main_seen}, fast {fast}"
        );
        assert!(screens_on > 0 && retract_seen > 0 && warn_seen > 0 && main_seen > 0 && fast > 0);
    }
}
