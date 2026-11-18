//! The scenery as it runs (tick, frame, dispose): its blocks and the state they
//! change, stepped by the renderer's owner at the level's begin, each world tick and each drawn frame.

use std::sync::Arc;

use indexmap::IndexSet;
use mvs_sim::World;
use mvs_view::M4;
use mvs_view::anim::Clips;
use mvs_view::mesh::MeshInfo;
use serde_json::Value;

use super::blocks::{GENERIC_KEYS, SceneryBlock};
use super::host::{NullSound, PopSpec, SceneryHost, SceneryMatch, SceneryState, SoundSink};
use super::platform_blend::PlatformBlend;
use super::{Instance, Skeletal};
use crate::Result;
use crate::renderer::{Renderer, StaticMesh};
use crate::stage::{to_world, transform};

/// What a static mesh of the scenery draws of its state: an instance (one of its static meshes, by the light of its
/// slots) or a skeletal mesh, with the sections it draws when shown.
#[derive(Clone, Debug)]
pub enum SceneryLink {
    Instance {
        id: usize,
        shown: Vec<bool>,
    },
    Skeletal {
        k: usize,
        shown: Vec<bool>,
    },
    /// A block's own skeletal mesh (SceneryHost::skinned), by its SkinnedId.
    Own {
        id: usize,
        shown: Vec<bool>,
    },
}

/// A pass-through platform's instance: its index in StageData.platforms, the offsets it
/// is drawn at between the ticks and the one last drawn (UE Y, Z), checked against the stage's data once.
#[derive(Clone, Debug)]
struct PlatformMesh {
    index: u32,
    id: usize,
    blend: PlatformBlend,
    drawn: (f64, f64),
    named: bool,
}

/// A stage's scenery blocks and their state.
pub struct SceneryRun {
    pub(crate) state: SceneryState,
    pub(crate) blocks: Vec<SceneryBlock>,
    pub(crate) sound: Box<dyn SoundSink>,
    /// The renderer's statics the scenery's start at, one a link.
    base: usize,
    links: Vec<SceneryLink>,
    platforms: Vec<PlatformMesh>,
    /// The strings of each block's data in the file's order, then the events the blocks' code posts by name
    /// (Scenery.sounds).
    sound_events: Vec<String>,
}

impl std::fmt::Debug for SceneryRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneryRun").field("state", &self.state).field("blocks", &self.blocks).finish_non_exhaustive()
    }
}

impl SceneryRun {
    /// The entry's instances and skeletal meshes as placed, and its blocks made (the blocks
    /// of the keys besides the entry's own, in the file's order), with no sound. `statics`: the renderer's statics
    /// from `base` on are the scenery's, one per link. The animation sequences loaded are the skeletal meshes' and
    /// every one the blocks' data names (a block's SceneryBlockRule.load).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: &mvs_store::Store,
        base: usize,
        links: Vec<SceneryLink>,
        instances: Vec<Instance>,
        skeletal: Vec<Skeletal>,
        meshes: Vec<Arc<MeshInfo>>,
        blocks: &indexmap::IndexMap<String, Value>,
    ) -> Result<SceneryRun> {
        let anims = store.kind("animation")?;
        let mut paths: IndexSet<String> = IndexSet::new();
        for k in &skeletal {
            if let Some(p) = k.fields.get("anim").and_then(Value::as_str) {
                paths.insert(p.to_owned());
            }
        }
        for (_, data) in blocks.iter().filter(|(k, _)| !GENERIC_KEYS.contains(&k.as_str())) {
            // Unverified: every
            // string of a block's data that is an object path in the animation kind is loaded, and one not in it is
            // taken for another asset's path (a mesh, a material: not told apart from a clip path by its text) and
            // skipped. A clip a block needs and the store lacks fails at its first use, not at load.
            strings(data, &mut |p| {
                if p.starts_with('/') && anims.has(p) {
                    paths.insert(p.to_owned());
                }
            });
        }
        let clips = Clips::load(store, paths.iter().map(|p| (p.as_str(), p.as_str())))?;
        // (a platform's mesh is moved by the sim alone, never a Blueprint actor's component)
        let mut platforms = Vec::new();
        for (id, i) in instances.iter().enumerate() {
            let Some(index) = i.platform else { continue };
            if i.attach.is_some() || i.hidden {
                return Err(crate::stage::bad(&i.name, "a platform's mesh is attached or hidden"));
            }
            platforms.push(PlatformMesh {
                index,
                id,
                blend: PlatformBlend::default(),
                drawn: (0.0, 0.0),
                named: false,
            });
        }
        let mut run = SceneryRun {
            state: SceneryState::new(instances, skeletal, meshes, clips)?,
            blocks: Vec::new(),
            sound: Box::new(NullSound),
            base,
            links,
            platforms,
            sound_events: Vec::new(),
        };
        run.sound_events = block_sound_events(blocks);
        let mut made = Vec::new();
        {
            let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut *run.sound, world: None };
            for (key, data) in blocks.iter().filter(|(k, _)| !GENERIC_KEYS.contains(&k.as_str())) {
                made.push(SceneryBlock::make(key, data, &mut host)?);
            }
        }
        run.blocks = made;
        run.state.resolve_own(store)?;
        Ok(run)
    }

    /// The scenery's statics as its state is now (its shown parts, places and skinned bones): each one's shown
    /// sections (none while hidden), its world transform (under its bone when attached), a skeletal mesh's skinning
    /// matrices and an instance's material scalars.
    pub(crate) fn apply(&self, statics: &mut [StaticMesh]) {
        let st = &self.state;
        for (j, link) in self.links.iter().enumerate() {
            let s = &mut statics[self.base + j];
            let (on, shown) = match link {
                SceneryLink::Instance { id, shown } => (st.shown[*id], shown),
                SceneryLink::Skeletal { k, shown } => (st.skeletal_shown[*k], shown),
                SceneryLink::Own { id, shown } => (st.own[*id].shown, shown),
            };
            for (d, b) in s.pose.shown.iter_mut().zip(shown) {
                *d = on && *b;
            }
            if !on {
                continue;
            }
            match link {
                SceneryLink::Instance { id, .. } => {
                    s.pose.to_world = to_world(&st.instance_world(*id));
                    for (slot, name, v) in &st.scalars[*id] {
                        let Some(m) = s.pose.materials.get_mut(*slot) else { continue };
                        match m.scalars.iter_mut().find(|(n, _)| n == name) {
                            Some(x) => x.1 = *v,
                            None => m.scalars.push((name.clone(), *v)),
                        }
                    }
                }
                SceneryLink::Skeletal { k, .. } => {
                    s.pose.to_world = to_world(&st.skeletal_world(*k));
                    st.skin_bones(*k, &mut s.pose.bones);
                    for d in &st.skel_slots[*k] {
                        d.apply(&st.skins[*k].info, &mut s.pose.materials);
                    }
                }
                SceneryLink::Own { id, .. } => {
                    // (its holder a copy of like's, beside it; no block moves a like it made
                    // its own meshes for, so here it is where like is)
                    let o = &st.own[*id];
                    s.pose.to_world = to_world(&st.skeletal_world(o.like));
                    st.own_bones(*id, &mut s.pose.bones);
                    let info = o.info.as_ref().expect("an own skeletal mesh resolved at the scenery's load");
                    for d in &o.slots {
                        d.apply(info, &mut s.pose.materials);
                    }
                }
            }
        }
    }

    /// The pass-through platforms as the World's tick left them: after every world tick, of
    /// a World whose stage is the scenery's alone. Each platform's place (World.stage_platforms,
    /// sim axes) against its data place (StageData.platforms) is its offset (UE Y, Z), recorded with where the tick
    /// took it from: where it was when the tick's own move (its run's mx / my, StageMoved)
    /// carried it from there, else (a reset, a rollback that put it elsewhere, a platform with no run) where it is
    /// now: the fighters' rule. `platforms_at` draws it between the two.
    pub(crate) fn platforms(&mut self, w: &World) {
        let rest = w.data.stage.platforms.as_deref().unwrap_or(&[]);
        for p in &mut self.platforms {
            let i = p.index as usize;
            let r = rest.get(i);
            let name = &self.state.instances[p.id].name;
            if !p.named {
                if r.is_none_or(|r| format!("{}.StaticMesh", r.name) != *name) {
                    panic!("scenery: {name} is not the stage's platform {i}");
                }
                p.named = true;
            }
            let (Some(n), Some(r)) = (w.stage.stage_platforms.get(i), r) else { continue };
            let y1 = *r.y.get(1).unwrap_or_else(|| panic!("the stage's platform {}: its y without a right", r.name));
            let run = w.stage_run.as_ref().and_then(|sr| sr.platforms.iter().find(|x| x.index == p.index));
            // (sim x = -UE Y, sim y = UE Z; the data place's left is -y[1])
            p.blend.tick(
                -(f64::from(n.left) + y1),
                f64::from(n.top) - r.top,
                run.map(|x| (f64::from(x.m.mx), f64::from(x.m.my))),
            );
        }
    }

    /// The platforms' meshes at the drawn frame's alpha (0 the previous tick's place, 1 the
    /// latest), the fighters' own, so a fighter standing on a moving deck is drawn on it. Nothing is done for a
    /// platform drawn where it was.
    pub(crate) fn platforms_at(&mut self, alpha: f64) {
        for p in &mut self.platforms {
            let at = p.blend.at(alpha);
            if at == p.drawn {
                continue;
            }
            p.drawn = at;
            // (the instance's UE transform at the data place, translated by the offset in the world)
            let i = &self.state.instances[p.id];
            let mut m = transform(i.loc, i.rot, i.scale);
            m[3][1] += at.0;
            m[3][2] += at.1;
            self.state.placed[p.id] = Some(m);
        }
    }

    /// The statics of the blocks' own skeletal meshes, after the scenery's others: each one
    /// dressed as its like (its lighting channels; its materials by section, a section with none or disabled not
    /// drawn), linked to its state. Hidden until its block shows it.
    pub(crate) fn own_statics(&mut self, r: &Renderer) -> Result<Vec<StaticMesh>> {
        let mut out = Vec::new();
        for (id, o) in self.state.own.iter().enumerate() {
            let info = o.info.as_ref().expect("an own skeletal mesh resolved at the scenery's load");
            let mesh = r.assets().mesh(&o.mesh)?;
            let sections = &mesh.lods.first().ok_or_else(|| crate::stage::bad(&o.mesh, "no LOD"))?.sections;
            if info.sections.len() != sections.len() || info.inv_ref.len() != mesh.bones as usize {
                return Err(crate::stage::bad(&o.mesh, "its header's sections or bones are not its LOD 0's"));
            }
            let like = &self.state.skeletal[o.like];
            let shown = super::skeletal_shown(info, &o.materials);
            self.links.push(SceneryLink::Own { id, shown: shown.clone() });
            out.push(StaticMesh {
                pose: mvs_view::MeshPose {
                    mesh: o.mesh.clone(),
                    to_world: to_world(&self.state.skeletal_world(o.like)),
                    bones: vec![super::IDENTITY_3X4; mesh.bones as usize],
                    materials: super::skeletal_slot_materials(info, &o.materials),
                    shown: vec![false; shown.len()],
                    ..Default::default()
                },
                lightmap: None,
                channels: crate::stage::channel_mask(&like.channels),
                instances: None,
                receives_decals: true,
                sort: 0.0,
            });
        }
        Ok(out)
    }

    /// The blocks' own skeletal meshes as the warm-up takes them (drawn once hidden, for its
    /// programs): each mesh with its sections' materials by slot.
    pub(crate) fn own_warm(&self) -> Vec<mvs_view::WarmMesh> {
        let st = &self.state;
        st.own
            .iter()
            .filter_map(|o| {
                let info = o.info.as_ref()?;
                let materials = super::skeletal_slot_materials(info, &o.materials)
                    .into_iter()
                    .map(|m| Some(m.path).filter(|p| !p.is_empty()))
                    .collect();
                Some(mvs_view::WarmMesh { mesh: Some(o.mesh.clone()), materials, ..Default::default() })
            })
            .collect()
    }

    /// The match the level runs in (Renderer::scenery_set_match), the seeded stream left as it is.
    fn set_match(&mut self, matched: Option<&SceneryMatch>) {
        self.state.matched = matched.cloned();
    }

    fn each(
        &mut self,
        vfx: Option<&mut mvs_vfx::Player>,
        world: Option<&World>,
        mut call: impl FnMut(&mut SceneryBlock, &mut SceneryHost),
    ) {
        let mut host = SceneryHost { state: &mut self.state, vfx, sound: &mut *self.sound, world };
        for b in &mut self.blocks {
            call(b, &mut host);
        }
    }
}

impl Renderer {
    /// Where the scenery's sounds go from now on (soundOn, place, rtpc).
    pub fn set_scenery_sound(&mut self, sink: Box<dyn SoundSink>) {
        if let Some(s) = &mut self.scenery {
            s.sound = sink;
        }
    }

    /// The sound events the open scenery's blocks name, as given, for the stage's sound scope (first each block's data,
    /// its string values in order; then what the blocks' code posts by name, SceneryBlockRule.sounds). Empty without a
    /// scenery.
    pub fn scenery_sound_events(&self) -> Vec<String> {
        self.scenery.as_ref().map_or_else(Vec::new, |s| s.sound_events.clone())
    }

    /// The level begins: the match it runs in (None outside one), the camera centre (UE) as the
    /// first frame draws it. The caller spawns the level's Emitter actors first (`Fx::begin_level`).
    pub fn scenery_begin(
        &mut self,
        vfx: Option<&mut mvs_vfx::Player>,
        matched: Option<&SceneryMatch>,
        camera: Option<[f64; 3]>,
    ) {
        let Some(s) = &mut self.scenery else { return };
        s.state.begin();
        s.state.camera = camera;
        s.state.seeded_on = matched.is_some();
        s.state.seed_state = matched.map_or(0, |m| m.seed);
        s.state.matched = matched.cloned();
        s.each(vfx, None, |b, host| b.begin(host));
    }

    /// The match a level that goes on runs in from now on (it is set for every match; a
    /// Lab fighter change skips the begin): `SceneryHost::scenery_match` reads it. The seeded stream goes on from the
    /// begin's.
    pub fn scenery_set_match(&mut self, matched: Option<&SceneryMatch>) {
        if let Some(s) = &mut self.scenery {
            s.set_match(matched);
        }
    }

    /// One world tick of `dt` seconds: the blocks, then the skeletal meshes' animations. Before
    /// the particle player's tick. `world`: the World as the tick left it, given only when its stage is the
    /// scenery's (Scenery.world is null for a World of another stage), the blocks' to read
    /// (SceneryHost::world).
    pub fn scenery_tick(&mut self, dt: f64, vfx: Option<&mut mvs_vfx::Player>, world: Option<&World>) {
        // (MPC TutorialEnvironmentDesat from the TutorialActor, 0 in a World with none)
        if let Some(w) = world {
            self.set_tutorial_desaturation(mvs_sim::modes::tutorial::desaturation(w) as f32);
        }
        let Some(s) = &mut self.scenery else { return };
        s.each(vfx, world, |b, host| b.tick(host, dt));
        s.state.tick_skeletal(dt);
    }

    /// A drawn frame, after the camera is placed: `alpha` between the previous tick and the
    /// latest, the camera centre (UE) as this frame draws it.
    pub fn scenery_frame(&mut self, alpha: f64, camera: Option<[f64; 3]>, vfx: Option<&mut mvs_vfx::Player>) {
        let Some(s) = &mut self.scenery else { return };
        s.state.camera = camera;
        s.each(vfx, None, |b, host| b.frame(host, alpha));
    }

    /// The level ends: what the blocks handed out goes back.
    pub fn scenery_end(&mut self, vfx: Option<&mut mvs_vfx::Player>) {
        self.set_tutorial_desaturation(0.0);
        let Some(s) = &mut self.scenery else { return };
        s.each(vfx, None, |b, host| b.end(host));
    }

    /// The pass-through platforms as the World's tick left them (SceneryRun::platforms): after
    /// every world tick, of a World whose stage is the scenery's alone.
    pub fn scenery_platforms(&mut self, w: &World) {
        if let Some(s) = &mut self.scenery {
            s.platforms(w);
        }
    }

    /// The platforms' meshes at the drawn frame's alpha (SceneryRun::platforms_at), the
    /// fighters' own (0 the previous tick's place, 1 the latest).
    pub fn platforms_at(&mut self, alpha: f64) {
        if let Some(s) = &mut self.scenery {
            s.platforms_at(alpha);
        }
    }

    /// A stage item's meshes, which are the scenery's, as its view places them this frame (mvs-view
    /// `SceneryPlace`): the scenery block `block`'s part `part` (The Court's ball: "court", "ball") shows `shown`'s
    /// variant (a key of the part's data: "gray", "red", "blue") at its UE transform, the others hidden; None: all
    /// hidden.
    pub fn scenery_place(&mut self, block: &str, part: &str, shown: Option<(&str, M4)>) {
        let Some(s) = &mut self.scenery else { return };
        // (a scenery of another stage has no such part: the call is the caller's every frame)
        let SceneryRun { blocks, state, sound, .. } = s;
        let mut host = SceneryHost { state, vfx: None, sound: &mut **sound, world: None };
        for b in blocks {
            if let (SceneryBlock::TheCourt(c), "court") = (b, block) {
                c.place_part(&mut host, part, shown);
            }
        }
    }

    /// The pop texts the blocks made since the last call (SceneryHost::pop), for the match's HUD.
    pub fn scenery_pops(&mut self) -> Vec<PopSpec> {
        self.scenery.as_mut().map_or_else(Vec::new, |s| std::mem::take(&mut s.state.pops))
    }
}

/// The strings of each stage block's data in the file's order, then the events the blocks' code posts by name
/// (Scenery.sounds).
pub(crate) fn block_sound_events(blocks: &indexmap::IndexMap<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    for (_, data) in blocks.iter().filter(|(k, _)| !GENERIC_KEYS.contains(&k.as_str())) {
        strings(data, &mut |s| out.push(s.to_owned()));
    }
    for (key, _) in blocks.iter().filter(|(k, _)| !GENERIC_KEYS.contains(&k.as_str())) {
        out.extend(SceneryBlock::sounds(key).iter().map(|e| (*e).to_owned()));
    }
    out
}

/// Each string of a block's data, depth first.
fn strings(v: &Value, f: &mut impl FnMut(&str)) {
    match v {
        Value::String(s) => f(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, f)),
        Value::Object(o) => o.values().for_each(|x| strings(x, f)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use mvs_material::jsonbin::JsonBin;
    use mvs_view::M4;
    use mvs_view::math;

    use super::super::SceneryHeader;
    use super::*;
    use crate::stage::transform;
    use crate::view::mul;

    fn close(a: &M4, b: &M4, what: &str) {
        for r in 0..4 {
            for c in 0..4 {
                assert!((a[r][c] - b[r][c]).abs() < 1e-6 * (1.0 + b[r][c].abs()), "{what}: [{r}][{c}] {a:?} {b:?}");
            }
        }
    }

    /// Sky Arena's arm (M011_Arm_C): in its reference pose each bone's world transform is its reference
    /// component-space one under the component; its clip then plays; its donut follows the bone it is attached to;
    /// the block's own sequence is the scenery's; a driven pose stays as the block leaves it.
    #[test]
    fn sky_arena_arm_plays_and_carries_its_donut() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let pack = store.kind("scenery").unwrap();
        let Some(path) = pack.paths().find(|p| p.ends_with("/m011.m011")).map(str::to_owned) else {
            eprintln!("no m011 scenery: nothing to check");
            return;
        };
        let bytes = pack.bytes(&path).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let block_anim = s.rest["arm"]["anim"].as_str().unwrap().to_owned();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        let arm = "M011_Arm_2";
        let bone = "fml_un_C_prop1";
        let k = run.state.skeletal.iter().position(|x| x.name == arm).unwrap();
        let st = &run.state;
        let b = st.skins[k].info.bones().iter().position(|n| n == bone).unwrap();
        let reference = math::invert(&st.skins[k].info.inv_ref[b]);
        let as_rows = |e: &math::Mat4| std::array::from_fn::<[f64; 4], 4, _>(|r| std::array::from_fn(|c| e[r * 4 + c]));
        close(&st.bone_world_at(k, b), &mul(&as_rows(&reference), &st.skeletal_world(k)), "reference bone");

        let rest = st.skins[k].info.rest.clone();
        run.state.begin();
        for _ in 0..30 {
            run.state.tick_skeletal(1.0 / 60.0);
        }
        assert!((run.state.anims[k].time - 0.5).abs() < 1e-9 || run.state.anims[k].rate != 1.0);
        assert_ne!(run.state.skins[k].pose, rest, "the clip poses the arm");

        let d = run.state.instances.iter().position(|i| i.name == "M011_Arm_2.Donut").unwrap();
        let i = &run.state.instances[d];
        let base = transform(i.loc, i.rot, i.scale);
        let mut sound = NullSound;
        let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
        let bw = host.bone_world(arm, bone).unwrap();
        close(&host.state.instance_world(d), &mul(&base, &bw), "the donut on its bone");

        let mut out = rest.clone();
        assert!(host.sample(arm, &block_anim, 0.5, &mut out));
        assert!(!host.sample(arm, "/Game/None.None", 0.0, &mut out));
        assert!(host.drive(arm));
        let held = host.pose(arm).unwrap().clone();
        for _ in 0..10 {
            run.state.tick_skeletal(1.0 / 60.0);
        }
        assert_eq!(run.state.skins[k].pose, held, "a driven pose is the block's");
    }
}

#[cfg(test)]
mod block_clips {
    use mvs_material::jsonbin::JsonBin;

    use super::super::SceneryHeader;
    use super::*;

    /// The sequences only a block's data names are the scenery's (The Court's ship's landing cycle, Trophy's Edge's
    /// crate): `SceneryHost::sample` serves them.
    #[test]
    fn block_data_clips_are_loaded() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let pack = store.kind("scenery").unwrap();
        let mut seen = 0;
        for (end, block) in [("/m010.m010", "court"), ("/trophyroom_2platforms.trophyroom_2platforms", "crate")] {
            let Some(path) = pack.paths().find(|p| p.ends_with(end)).map(str::to_owned) else { continue };
            let bytes = pack.bytes(&path).unwrap();
            let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
            let mut named = Vec::new();
            strings(&s.rest[block], &mut |p| {
                if p.starts_with('/') && store.kind("animation").unwrap().has(p) {
                    named.push(p.to_owned());
                }
            });
            let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
            let run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
            eprintln!("{block}: {} sequences named, {} loaded", named.len(), run.state.clips.clips.len());
            assert!(!named.is_empty(), "{block} names no sequence");
            for p in &named {
                assert!(run.state.clips.get(p).is_some(), "{block}: {p}");
            }
            seen += 1;
        }
        eprintln!("{seen} blocks checked");
    }
}

#[cfg(test)]
mod platform_moves {
    use mvs_material::jsonbin::JsonBin;

    use super::super::SceneryHeader;
    use super::*;
    use crate::stage::stage_key;
    use crate::stagedraw::StageDraw;

    /// Trophy's Edge's moving platforms (the stage-trophy match): each platform instance's offset follows its place
    /// in the World, and a frame between two ticks of a carried move draws it between them.
    #[test]
    fn trophy_platforms_follow_the_world() {
        let Some(mut h) = mvs_sim_trace::harness::at("stage-trophy", 0) else { return };
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap();
        let entry = StageDraw::load(&store, &stage_key(&h.w.data.stage_key)).unwrap().scenery.entry;
        let bytes = store.kind("scenery").unwrap().bytes(&entry).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &entry).unwrap().parse::<SceneryHeader>(&entry).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        assert!(!run.platforms.is_empty(), "{entry}: no platform instance");
        let mut moving = None;
        for _ in 0..1200 {
            h.step_recorded();
            run.platforms(&h.w);
            moving = run.platforms.iter().position(|p| p.blend.py != p.blend.cy || p.blend.pz != p.blend.cz);
            if moving.is_some() {
                break;
            }
        }
        let k = moving.expect("a platform carried by its move within 20 s");
        eprintln!("{entry}: platform {} carried at tick {}", run.platforms[k].index, h.w.flow.tick);
        let p = run.platforms[k].clone();
        // (the World's place against the data's, in UE Y, Z)
        let r = &h.w.data.stage.platforms.as_ref().unwrap()[p.index as usize];
        let n = &h.w.stage.stage_platforms[p.index as usize];
        assert_eq!((p.blend.cy, p.blend.cz), (-(f64::from(n.left) + r.y[1]), f64::from(n.top) - r.top));
        run.platforms_at(0.5);
        let i = &run.state.instances[p.id];
        let base = transform(i.loc, i.rot, i.scale);
        let m = run.state.placed[p.id].expect("the platform moved");
        let half = |a: f64, b: f64| a + (b - a) * 0.5;
        assert_eq!(m[3][1] - base[3][1], half(p.blend.py, p.blend.cy));
        assert_eq!(m[3][2] - base[3][2], half(p.blend.pz, p.blend.cz));
        assert_eq!(&m[..3], &base[..3]);
    }
}

#[cfg(test)]
mod own_meshes {
    use mvs_material::jsonbin::JsonBin;
    use mvs_view::MaterialRef;
    use serde_json::Value;

    use super::super::SceneryHeader;
    use super::super::host::{PopSpec, SkinRef};
    use super::*;

    fn run_of(store: &mvs_store::Store, end: &str) -> Option<SceneryRun> {
        let pack = store.kind("scenery").unwrap();
        let path = pack.paths().find(|p| p.ends_with(end)).map(str::to_owned)?;
        let bytes = pack.bytes(&path).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(store, &k.mesh).unwrap())).collect();
        Some(SceneryRun::new(store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap())
    }

    fn host(run: &mut SceneryRun) -> SceneryHost<'_> {
        SceneryHost { state: &mut run.state, vfx: None, sound: &mut *run.sound, world: None }
    }

    /// Sky Arena's arm made again as a block's own skeletal mesh of its own mesh: hidden until shown, skinned as the
    /// arm when posed as it; a material swapped in and out keeps each instance's parameters; an effects material's
    /// scalars are the instance's; a pop is queued until taken.
    #[test]
    fn own_mesh_material_and_pop() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let Some(mut run) = run_of(&store, "/m011.m011") else {
            eprintln!("no m011 scenery: nothing to check");
            return;
        };
        let arm = "M011_Arm_2";
        let k = run.state.skeletal.iter().position(|x| x.name == arm).unwrap();
        let (mesh, mats) = (run.state.skeletal[k].mesh.clone(), run.state.skeletal[k].materials.clone());
        let refs: Vec<Option<&str>> = mats.iter().map(Option::as_deref).collect();
        let id = host(&mut run).skinned(arm, &mesh, &refs).unwrap();
        assert!(host(&mut run).skinned("no such mesh", &mesh, &refs).is_err());
        run.state.resolve_own(&store).unwrap();
        let o = &run.state.own[id.0 as usize];
        assert!(!o.shown, "an own mesh starts hidden");
        assert_eq!(o.pose, run.state.skins[k].info.rest, "its own reference pose, the arm's");
        host(&mut run).show_skinned(id, true);
        assert!(run.state.own[id.0 as usize].shown);

        // posed as the arm is, it is skinned as the arm
        run.state.begin();
        for _ in 0..20 {
            run.state.tick_skeletal(1.0 / 60.0);
        }
        let pose = run.state.skins[k].pose.clone();
        host(&mut run).pose_skinned(id).copy(&pose);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        run.state.skin_bones(k, &mut a);
        run.state.own_bones(id.0 as usize, &mut b);
        assert_eq!(a, b);

        // a swap and its parameters, the section's own instance's kept apart
        let info = run.state.skins[k].info.clone();
        let slot = info.sections[0].slot;
        let mut out = super::super::skeletal_slot_materials(&info, &mats);
        let own_path = out[slot].path.clone();
        let other = "/Game/Some/M_Other.M_Other?ctx=local";
        {
            let mut h = host(&mut run);
            h.skeletal_scalars(SkinRef::File(arm), 0, &[("Own", 2.0)]);
            h.skeletal_material(SkinRef::File(arm), 0, Some(other));
            h.skeletal_scalars(SkinRef::File(arm), 0, &[("Tint", 0.5), ("Break", 1.0)]);
            h.skeletal_scalars(SkinRef::File(arm), 0, &[("Tint", 0.25)]);
            h.skeletal_vectors(SkinRef::File(arm), 0, &[("Color", [1.0, 0.5, 0.0, 1.0])]);
            h.skeletal_material(SkinRef::Own(id), 0, Some(other));
        }
        let apply = |run: &SceneryRun, out: &mut Vec<MaterialRef>| {
            for d in &run.state.skel_slots[k] {
                d.apply(&info, out);
            }
        };
        apply(&run, &mut out);
        assert_eq!(&*out[slot].path, "/Game/Some/M_Other.M_Other");
        assert_eq!(out[slot].scalars, vec![(Arc::from("Tint"), 0.25), (Arc::from("Break"), 1.0)]);
        assert_eq!(out[slot].vectors, vec![(Arc::from("Color"), [1.0, 0.5, 0.0, 1.0])]);
        host(&mut run).skeletal_material(SkinRef::File(arm), 0, None);
        apply(&run, &mut out);
        assert_eq!(out[slot].path, own_path);
        assert_eq!(out[slot].scalars, vec![(Arc::from("Own"), 2.0)]);
        assert!(out[slot].vectors.is_empty());
        host(&mut run).skeletal_material(SkinRef::File(arm), 0, Some(other));
        apply(&run, &mut out);
        assert_eq!(out[slot].scalars, vec![(Arc::from("Tint"), 0.25), (Arc::from("Break"), 1.0)], "kept");
        assert_eq!(run.state.own[id.0 as usize].slots.len(), 1);
        assert_eq!(run.state.skel_slots[k].len(), 1);

        // an effects material's scalars, a pop
        let d = run.state.instances.iter().position(|i| i.name == "M011_Arm_2.Donut").unwrap();
        host(&mut run).vfx_material_scalars(super::super::InstanceId(d as u32), 0, &[("Scroll", 0.3)]);
        assert_eq!(run.state.scalars[d], vec![(0, Arc::from("Scroll"), 0.3)]);
        let p = PopSpec {
            text: "+2".into(),
            lifetime: 2.0,
            text_color: [1.0; 4],
            location: [0.0, 0.0, 1200.0],
            scale: 2.0,
            justification: 1,
            delay: 0.0,
            icon_right: None,
            fighter: None,
            at: Some([1.0, 2.0, 3.0]),
        };
        assert!(host(&mut run).scenery_match().is_none());
        run.state.matched = Some(SceneryMatch { me: 1, players: vec![0, 1], seed: 7 });
        assert_eq!(host(&mut run).scenery_match().map(|m| m.me), Some(1));
        // (a kept level's next match: the match is the new one, the seeded stream goes on)
        let (seed, on) = (run.state.seed_state, run.state.seeded_on);
        run.set_match(Some(&SceneryMatch { me: 0, players: vec![0, 1, 2], seed: 9 }));
        assert_eq!(host(&mut run).scenery_match().map(|m| (m.me, m.players.len())), Some((0, 3)));
        assert_eq!((run.state.seed_state, run.state.seeded_on), (seed, on));
        host(&mut run).pop(p.clone());
        assert_eq!(std::mem::take(&mut run.state.pops), vec![p]);
    }

    /// A block's own mesh: like, its mesh, its materials, the sections a block swaps on it and on like.
    type Made = (String, String, Vec<Option<String>>, Vec<usize>, Vec<usize>);

    /// The meshes the blocks make their own of (Tree Fort's pillars' damage meshes, Cromulons' speakers'): each has
    /// a material per section and every bone of the mesh it is dressed as, and no section whose material a block
    /// swaps shares its material slot with another drawn section (SlotDraw::apply's Unverified).
    #[test]
    fn block_own_meshes_fit() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let pack = store.kind("scenery").unwrap();
        let mut seen = 0;
        let paths: Vec<String> = pack.paths().map(str::to_owned).collect();
        let strs = |v: &Value| -> Vec<Option<String>> {
            v.as_array().unwrap().iter().map(|m| m.as_str().map(str::to_owned)).collect()
        };
        let nums = |v: &Value| -> Vec<usize> {
            v.as_array().map_or_else(Vec::new, |a| a.iter().map(|x| x.as_u64().unwrap() as usize).collect())
        };
        for path in paths {
            let bytes = pack.bytes(&path).unwrap();
            let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
            let mut made: Vec<Made> = Vec::new();
            if let Some(b) = s.rest.get("treefortActors") {
                for p in b["pillars"].as_array().unwrap() {
                    let death = nums(&p["deathMaterial"]["slots"]);
                    for d in p["damage"].as_array().unwrap().iter().skip(1) {
                        let like = p["mesh"].as_str().unwrap().to_owned();
                        let m = d["mesh"].as_str().unwrap().to_owned();
                        made.push((like, m, strs(&d["materials"]), death.clone(), death.clone()));
                    }
                }
            }
            if let Some(b) = s.rest.get("cromulons")
                && let Some(art) = b.get("speakerArt")
            {
                for sp in b["speakers"].as_array().unwrap() {
                    for d in art["damage"].as_array().unwrap() {
                        let like = sp["skeletal"].as_str().unwrap().to_owned();
                        let m = d["mesh"].as_str().unwrap().to_owned();
                        made.push((like, m, strs(&d["materials"]), nums(&art["damaged"]), nums(&art["base"])));
                    }
                }
            }
            if made.is_empty() {
                continue;
            }
            let meshes: Vec<Arc<MeshInfo>> =
                s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
            let skeletal = s.skeletal.clone();
            let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
            for (like, m, mats, _, _) in &made {
                let refs: Vec<Option<&str>> = mats.iter().map(Option::as_deref).collect();
                host(&mut run).skinned(like, m, &refs).unwrap();
            }
            run.state.resolve_own(&store).unwrap_or_else(|e| panic!("{path}: {e}"));
            let shares = |info: &MeshInfo, mats: &[Option<String>], swapped: &[usize], what: &str| {
                for &x in swapped {
                    let slot = info.sections[x].slot;
                    for (y, sec) in info.sections.iter().enumerate() {
                        let drawn = !sec.disabled && mats.get(y).is_some_and(Option::is_some);
                        assert!(y == x || sec.slot != slot || !drawn, "{path}: {what}: section {x} shares slot {slot}");
                    }
                }
            };
            for (o, (like, m, mats, on_own, on_like)) in run.state.own.iter().zip(&made) {
                shares(o.info.as_ref().unwrap(), mats, on_own, m);
                let k = skeletal.iter().position(|x| x.name == *like).unwrap();
                shares(&run.state.skins[k].info, &skeletal[k].materials, on_like, like);
            }
            eprintln!("{path}: {} own meshes", made.len());
            seen += 1;
        }
        eprintln!("{seen} entries with own meshes");
    }
}
