//! What a stage's scenery blocks see of the scenery: its instances and
//! skeletal meshes to show, hide and move, its random streams, the camera centre, the level's sounds and effects.

use std::sync::Arc;

use mvs_sim::World;
use mvs_sim::world::stage_actors::StageActor;
use mvs_view::M4;
use mvs_view::anim::{Clips, Pose, Rig, sample_clip};
use mvs_view::math::{self, Mat4};
use mvs_view::mesh::MeshInfo;
use serde_json::Value;

use super::{Instance, Skeletal};
use crate::Result;
use crate::stage::{bad, transform};
use crate::view::mul;

/// An instance of the scenery entry: its index in the entry's `instances`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(pub u32);

/// The match a scenery's level runs in. `me`: the fighter of this machine's player
/// (PlayerDataManager GetMyPlayerData); `players`: the fighters in GetAllPlayers' order; `seed`: what
/// `SceneryHost::seeded` draws from, the same on every peer of an online match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneryMatch {
    pub me: usize,
    pub players: Vec<usize>,
    pub seed: u32,
}

/// A skeletal mesh's animation as it plays: FSingleAnimationPlayData's animation from its
/// SavedPosition, looping or holding its last pose, at its play rate (0: not playing). A block may change any of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneryAnim {
    /// The animation sequence by build object path; None: the reference pose.
    pub clip: Option<Arc<str>>,
    pub looping: bool,
    /// Seconds into the clip.
    pub time: f64,
    pub rate: f64,
}

/// Where the level's sounds go (set by the scenery's owner). The
/// audio crate implements it; `NullSound` takes them nowhere.
pub trait SoundSink {
    /// PF_Post_Event_At_Location: `event` at the UE location `at`. key: an actor's own game object, whose Stop
    /// events stop that key's voices alone; None: the level's ambience key.
    fn post_at(&mut self, event: &str, at: [f64; 3], key: Option<&str>);
    /// PF_Post_Event_On_AkComponent on an actor's own AkComponent: `event` on the game object `key`, placed at `at`.
    fn post_on(&mut self, event: &str, key: &str, at: [f64; 3]);
    /// The game object `key` moved to `at`, its voices with it.
    fn place(&mut self, key: &str, at: [f64; 3]);
    /// A game parameter's global value (AkGameplayStatics GetRTPCValue), None while nothing has set it.
    fn rtpc(&self, name: &str) -> Option<f64>;
}

/// The sink of a scenery with no audio (render-frame, a test).
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSound;

impl SoundSink for NullSound {
    fn post_at(&mut self, _event: &str, _at: [f64; 3], _key: Option<&str>) {}
    fn post_on(&mut self, _event: &str, _key: &str, _at: [f64; 3]) {}
    fn place(&mut self, _key: &str, _at: [f64; 3]) {}
    fn rtpc(&self, _name: &str) -> Option<f64> {
        None
    }
}

/// A skeletal mesh as it is posed (bones, rest and pose): its mesh's reference skeleton,
/// its bones bound to the scenery's clips, the pose its clip or its block left.
#[derive(Debug)]
pub(crate) struct Skin {
    pub(crate) info: Arc<MeshInfo>,
    rig: Rig,
    pub(crate) pose: Pose,
    /// Posed by its block (SceneryAnim.drive): its clip no longer plays.
    driven: bool,
    /// The clip (an index in the scenery's clips; None the reference pose) and time `pose` holds; None: not known
    /// (a block had the pose, or nothing was sampled yet).
    sampled: Option<(Option<usize>, f64)>,
}

/// The scenery's state the blocks change through the host, read by the renderer when it draws the scenery.
#[derive(Debug, Default)]
pub struct SceneryState {
    pub(crate) instances: Vec<Instance>,
    pub(crate) skeletal: Vec<Skeletal>,
    /// Each instance shown (by InstanceId): its `hidden` field negated at the start.
    pub(crate) shown: Vec<bool>,
    /// Each instance's UE transform a block moved it to; None: as placed.
    pub(crate) placed: Vec<Option<M4>>,
    /// Each instance's material scalars a block set, by (slot, name).
    pub(crate) scalars: Vec<Vec<(usize, Arc<str>, f32)>>,
    /// Each skeletal mesh's animation, its show flag and the UE transform a block moved it to.
    pub(crate) anims: Vec<SceneryAnim>,
    pub(crate) skeletal_shown: Vec<bool>,
    pub(crate) skeletal_placed: Vec<Option<M4>>,
    /// The unseeded stream (`rand`) and the match's (`seeded`, on from a begin with a match).
    pub(crate) rand_state: u32,
    pub(crate) seed_state: u32,
    pub(crate) seeded_on: bool,
    /// The match the level runs in, as its latest begin or `Renderer::scenery_set_match` had it; None outside one.
    pub(crate) matched: Option<SceneryMatch>,
    /// The CameraCenter actor's location (UE) as the frame draws it, None with no camera.
    pub(crate) camera: Option<[f64; 3]>,
    /// Each skeletal mesh's skeleton and pose.
    pub(crate) skins: Vec<Skin>,
    /// The animation sequences of the scenery (each by its build object path): the skeletal meshes' own and those
    /// the blocks' data names.
    pub(crate) clips: Clips,
    /// Each instance's and skeletal mesh's component transform as the file places it (UE, row-vector; relative to
    /// its bone when attached), and the skeletal mesh and bone it is attached to.
    inst_base: Vec<M4>,
    inst_attach: Vec<Option<(usize, usize)>>,
    skel_base: Vec<M4>,
    skel_attach: Vec<Option<(usize, usize)>>,
    /// The blocks' world-space widgets (widget.rs).
    pub(crate) widgets: Vec<super::widget::SceneryWidget>,
    /// skin_bones's component-space transforms, kept between frames.
    cs_scratch: std::cell::RefCell<Vec<Mat4>>,
    /// Each skeletal mesh's sections whose material a block swapped or set parameters on.
    pub(crate) skel_slots: Vec<Vec<SlotDraw>>,
    /// The blocks' own skeletal meshes (SceneryHost::skinned), by SkinnedId.
    pub(crate) own: Vec<OwnSkin>,
    /// The pop texts the blocks made since the renderer's owner last took them (Renderer::scenery_pops).
    pub(crate) pops: Vec<PopSpec>,
}

/// A section of a skeletal mesh whose material a block changed (a skinned mesh's `material` set to a
/// block's CharMaterial, its setScalar / setVector): the material it draws and each dynamic instance's parameters.
#[derive(Clone, Debug)]
pub(crate) struct SlotDraw {
    /// The section (LOD 0, as the entry's per-section `materials`).
    pub(crate) section: usize,
    /// The section's own material (its object path, the draw context dropped).
    own: Arc<str>,
    /// The material swapped in (SceneryHost::skeletal_material); None: its own.
    swap: Option<Arc<str>>,
    /// Each dynamic instance's parameters a block set, by its material: one instance per material, so what a block
    /// set on one stays through a swap away and back (a character material is one object a block holds).
    sets: Vec<(Arc<str>, Params)>,
}

/// A dynamic material instance's scalar and vector parameters, in the order first set.
#[derive(Clone, Debug, Default)]
pub(crate) struct Params {
    scalars: Vec<(Arc<str>, f32)>,
    vectors: Vec<(Arc<str>, [f32; 4])>,
}

impl SlotDraw {
    fn params(&mut self) -> &mut Params {
        let at = self.swap.as_ref().unwrap_or(&self.own);
        let k = match self.sets.iter().position(|(p, _)| p == at) {
            Some(k) => k,
            None => {
                self.sets.push((at.clone(), Params::default()));
                self.sets.len() - 1
            }
        };
        &mut self.sets[k].1
    }

    /// The section's material slot of `materials` (a MeshPose's, by material slot) as the block left it.
    // Unverified: a draw changes a material slot, which every section of that slot draws with.
    // No section a block changes shares its slot in the build's data the blocks name.
    pub(crate) fn apply(&self, info: &MeshInfo, materials: &mut [mvs_view::MaterialRef]) {
        let Some(m) = info.sections.get(self.section).and_then(|x| materials.get_mut(x.slot)) else { return };
        let path = self.swap.as_ref().unwrap_or(&self.own);
        m.path = path.clone();
        m.scalars.clear();
        m.vectors.clear();
        if let Some((_, p)) = self.sets.iter().find(|(x, _)| x == path) {
            m.scalars.extend(p.scalars.iter().cloned());
            m.vectors.extend(p.vectors.iter().cloned());
        }
    }
}

/// A skeletal mesh of a block's own (SceneryHost::skinned): its mesh, dressed and placed as the entry's skeletal mesh
/// `like` is, posed by its block in `like`'s bone order.
#[derive(Debug)]
pub(crate) struct OwnSkin {
    pub(crate) like: usize,
    pub(crate) mesh: Arc<str>,
    /// Its materials by section, as the block names them.
    pub(crate) materials: Vec<Option<String>>,
    pub(crate) shown: bool,
    /// Its pose in `like`'s bone order, the block's to write.
    pub(crate) pose: Pose,
    /// Its mesh's reference skeleton, and per bone of it the bone of `like` that poses it (None: its reference
    /// pose); read after the blocks are made (SceneryState::resolve_own).
    pub(crate) info: Option<Arc<MeshInfo>>,
    map: Vec<Option<usize>>,
    pub(crate) slots: Vec<SlotDraw>,
}

/// A component's `attach` (the skeletal mesh's name and the bone's) as (skeletal index, bone index).
fn attach_of(
    attach: &Option<Value>,
    skeletal: &[Skeletal],
    infos: &[Arc<MeshInfo>],
    what: &str,
) -> Result<Option<(usize, usize)>> {
    let Some(a) = attach else { return Ok(None) };
    let pair = a.as_array().map(|v| v.iter().filter_map(Value::as_str).collect::<Vec<_>>());
    let Some([parent, bone]) = pair.as_deref() else {
        return Err(bad(what, format!("attach {a}: not a skeletal mesh and a bone")));
    };
    let k = skeletal
        .iter()
        .position(|s| s.name == *parent)
        .ok_or_else(|| bad(what, format!("attach: no skeletal mesh {parent}")))?;
    // (three's bones by their exact name: Object3D.name)
    let b = infos[k]
        .bones()
        .iter()
        .position(|n| n == bone)
        .ok_or_else(|| bad(what, format!("attach bone {bone} of {parent}")))?;
    Ok(Some((k, b)))
}

/// A skeletal mesh's single-node animation as the file has it (`anim`, `rate`, `loop`
/// absent or true, `start` its SavedPosition).
fn anim_of(k: &Skeletal) -> Result<SceneryAnim> {
    let f = &k.fields;
    let rate = f.get("rate").and_then(Value::as_f64).ok_or_else(|| bad(&k.name, "a skeletal mesh without its rate"))?;
    Ok(SceneryAnim {
        clip: f.get("anim").and_then(Value::as_str).map(Arc::from),
        looping: f.get("loop").and_then(Value::as_bool) != Some(false),
        time: f.get("start").and_then(Value::as_f64).unwrap_or(0.0),
        rate,
    })
}

impl SceneryState {
    /// The state of an entry's instances and skeletal meshes as placed: `infos` each skeletal mesh's mesh, `clips`
    /// the scenery's animation sequences. A skeletal mesh is in its reference pose until its first tick.
    pub(crate) fn new(
        instances: Vec<Instance>,
        skeletal: Vec<Skeletal>,
        infos: Vec<Arc<MeshInfo>>,
        clips: Clips,
    ) -> Result<SceneryState> {
        let (n, k) = (instances.len(), skeletal.len());
        let inst_attach =
            instances.iter().map(|i| attach_of(&i.attach, &skeletal, &infos, &i.name)).collect::<Result<Vec<_>>>()?;
        let skel_attach =
            skeletal.iter().map(|s| attach_of(&s.attach, &skeletal, &infos, &s.name)).collect::<Result<Vec<_>>>()?;
        // (a chain of attachments ends at a skeletal mesh that is not attached)
        for (s, mut at) in skel_attach.iter().enumerate() {
            for _ in 0..=k {
                match at {
                    Some((p, _)) => at = &skel_attach[*p],
                    None => break,
                }
            }
            if at.is_some() {
                return Err(bad(&skeletal[s].name, "attached in a loop"));
            }
        }
        let anims = skeletal.iter().map(anim_of).collect::<Result<Vec<_>>>()?;
        for (s, a) in skeletal.iter().zip(&anims) {
            if let Some(c) = &a.clip
                && clips.get(c).is_none()
            {
                return Err(bad(&s.name, format!("its animation {c} is not loaded")));
            }
        }
        let skins = infos
            .iter()
            .map(|info| Skin {
                info: info.clone(),
                rig: Rig::bind(&clips, info.bones().to_vec()),
                pose: info.rest.clone(),
                driven: false,
                sampled: None,
            })
            .collect();
        Ok(SceneryState {
            shown: instances.iter().map(|i| !i.hidden).collect(),
            placed: vec![None; n],
            scalars: vec![Vec::new(); n],
            skel_slots: vec![Vec::new(); k],
            anims,
            skeletal_shown: skeletal.iter().map(|s| !s.hidden).collect(),
            skeletal_placed: vec![None; k],
            inst_base: instances.iter().map(|i| transform(i.loc, i.rot, i.scale)).collect(),
            skel_base: skeletal.iter().map(|s| transform(s.loc, s.rot, s.scale)).collect(),
            inst_attach,
            skel_attach,
            instances,
            skeletal,
            skins,
            clips,
            rand_state: RAND_SEED,
            ..Default::default()
        })
    }

    /// The level begins: each skeletal mesh's animation back at its SavedPosition (its clip, rate
    /// and loop as a block left them).
    pub(crate) fn begin(&mut self) {
        for (a, k) in self.anims.iter_mut().zip(&self.skeletal) {
            a.time = k.fields.get("start").and_then(Value::as_f64).unwrap_or(0.0);
        }
    }

    /// The skeletal meshes' animations one tick of `dt` seconds on, after the blocks':
    /// FSingleAnimationPlayData's time on by the rate and the sequence's RateScale, wrapped by a looping one, held
    /// at the end by another; with no clip the reference pose. One its block poses keeps the block's pose.
    pub(crate) fn tick_skeletal(&mut self, dt: f64) {
        let SceneryState { anims, skins, clips, skeletal, .. } = self;
        for ((a, skin), k) in anims.iter_mut().zip(skins.iter_mut()).zip(skeletal.iter()) {
            if skin.driven {
                continue;
            }
            let c = a.clip.as_deref().map(|p| clip_index(clips, p, &k.name));
            if let Some(c) = c {
                let clip = &clips.clips[c].anim;
                a.time += dt * a.rate * clip.rate_scale;
            }
            let t = clip_time(clips, c, a);
            pose_at(clips, skin, c, t);
        }
    }

    /// The component-space transform (column vectors) of bone `b` of skeletal mesh `k` as posed.
    fn bone_cs(&self, k: usize, b: usize) -> Mat4 {
        let skin = &self.skins[k];
        let parents = &skin.info.skeleton.as_ref().expect("a skeletal mesh's skeleton").parents;
        let mut m = bone_local(&skin.pose, b);
        let mut p = parents[b];
        while p >= 0 {
            m = math::mul(&bone_local(&skin.pose, p as usize), &m);
            p = parents[p as usize];
        }
        m
    }

    /// Skeletal mesh `k`'s component world transform (UE, row-vector): as placed or moved, under its bone when
    /// attached.
    pub(crate) fn skeletal_world(&self, k: usize) -> M4 {
        let local = self.skeletal_placed[k].unwrap_or(self.skel_base[k]);
        match self.skel_attach[k] {
            Some((p, b)) => mul(&local, &self.bone_world_at(p, b)),
            None => local,
        }
    }

    /// Bone `b` of skeletal mesh `k`: its world transform (UE, row-vector) as posed.
    pub(crate) fn bone_world_at(&self, k: usize, b: usize) -> M4 {
        mul(&row_form(&math::to_ue(&self.bone_cs(k, b))), &self.skeletal_world(k))
    }

    /// Instance `i`'s world transform (UE, row-vector): as placed or moved, under its bone when attached.
    pub(crate) fn instance_world(&self, i: usize) -> M4 {
        let local = self.placed[i].unwrap_or(self.inst_base[i]);
        match self.inst_attach[i] {
            Some((p, b)) => mul(&local, &self.bone_world_at(p, b)),
            None => local,
        }
    }

    /// Skeletal mesh `k`'s skinning matrices as posed (MeshPose.bones: each bone's component-space transform in UE
    /// times its inverse reference matrix), into `out`.
    pub(crate) fn skin_bones(&self, k: usize, out: &mut Vec<[f32; 12]>) {
        let skin = &self.skins[k];
        self.skin_with(&skin.info, |b| bone_local(&skin.pose, b), out);
    }

    /// A block's own skeletal mesh's skinning matrices as posed: each of its bones as the bone of `like` that poses
    /// it (its `bones`, which the block writes), one `like` lacks in its reference pose.
    pub(crate) fn own_bones(&self, id: usize, out: &mut Vec<[f32; 12]>) {
        let o = &self.own[id];
        let info = o.info.as_ref().expect("an own skeletal mesh resolved at the scenery's load");
        self.skin_with(
            info,
            |b| match o.map[b] {
                Some(j) => bone_local(&o.pose, j),
                None => bone_local(&info.rest, b),
            },
            out,
        );
    }

    fn skin_with(&self, info: &MeshInfo, local: impl Fn(usize) -> Mat4, out: &mut Vec<[f32; 12]>) {
        let parents = &info.skeleton.as_ref().expect("a skeletal mesh's skeleton").parents;
        let mut cs = self.cs_scratch.borrow_mut();
        cs.clear();
        out.clear();
        for (b, &p) in parents.iter().enumerate() {
            let local = local(b);
            let m = if p >= 0 { math::mul(&cs[p as usize], &local) } else { local };
            out.push(math::rows3x4(&math::mul(&math::to_ue(&m), &info.inv_ref[b])));
            cs.push(m);
        }
    }

    /// The blocks' own skeletal meshes' meshes read, after the blocks are made : each
    /// one a skeletal mesh of a section a material, with every bone of its `like` (by name); its pose starts as its
    /// own reference pose.
    pub(crate) fn resolve_own(&mut self, store: &mvs_store::Store) -> Result<()> {
        for o in &mut self.own {
            let like = &self.skeletal[o.like].name;
            let info = MeshInfo::shared(store, &o.mesh)?;
            if info.skeleton.is_none() {
                return Err(bad(like, format!("{} is not a skeletal mesh", o.mesh)));
            }
            if info.sections.len() != o.materials.len() {
                return Err(bad(
                    like,
                    format!(
                        "a mesh of {} sections for {} materials ({})",
                        info.sections.len(),
                        o.materials.len(),
                        o.mesh
                    ),
                ));
            }
            let names = self.skins[o.like].info.bones();
            let own = info.bones();
            for n in names {
                if !own.contains(n) {
                    return Err(bad(like, format!("a mesh of other bones ({}: no {n})", o.mesh)));
                }
            }
            o.map = own.iter().map(|n| names.iter().position(|x| x == n)).collect();
            for (b, j) in o.map.iter().enumerate() {
                if let Some(j) = j {
                    copy_bone_to(&mut o.pose, *j, &info.rest, b);
                }
            }
            o.info = Some(info);
        }
        Ok(())
    }

    /// The section `slot` of a skeletal mesh as a block changes it, made on first use.
    fn slot_draw(&mut self, mesh: SkinRef<'_>, slot: usize) -> &mut SlotDraw {
        // (`what`: the mesh's name and, for a block's own, its mesh; the panic's message alone builds the text)
        let (list, materials, what) = match mesh {
            SkinRef::File(name) => {
                let k = self
                    .skeletal
                    .iter()
                    .position(|s| s.name == name)
                    .unwrap_or_else(|| panic!("scenery: {name} is not a skeletal mesh of the file"));
                (&mut self.skel_slots[k], &self.skeletal[k].materials, (name, None))
            }
            SkinRef::Own(id) => {
                let o = &mut self.own[id.0 as usize];
                (&mut o.slots, &o.materials, (self.skeletal[o.like].name.as_str(), Some(&*o.mesh)))
            }
        };
        let k = match list.iter().position(|d| d.section == slot) {
            Some(k) => k,
            None => {
                let own = match materials.get(slot) {
                    Some(m) => super::material_ref(m).path,
                    None => match what {
                        (name, None) => panic!("scenery: {name}: no section {slot} of {}", materials.len()),
                        (name, Some(m)) => {
                            panic!("scenery: {name}#own {m}: no section {slot} of {}", materials.len())
                        }
                    },
                };
                list.push(SlotDraw { section: slot, own, swap: None, sets: Vec::new() });
                list.len() - 1
            }
        };
        &mut list[k]
    }
}

/// The scenery's clip `path`; a clip no one loaded is a bug of the scenery's load (SceneryRun::new loads every
/// sequence the entry and its blocks name).
fn clip_index(clips: &Clips, path: &str, skeletal: &str) -> usize {
    clips.get(path).unwrap_or_else(|| panic!("scenery: {skeletal}: the animation {path} is not one of the scenery's"))
}

/// The time of the clip `c` an animation shows: wrapped by a looping one, held at the end by another.
fn clip_time(clips: &Clips, c: Option<usize>, a: &SceneryAnim) -> f64 {
    let Some(c) = c else { return 0.0 };
    let len = clips.clips[c].anim.length;
    if a.looping { a.time % len } else { a.time.min(len) }
}

/// The skin's pose as clip `c` at `t` (the reference pose with no clip); not sampled again where it holds it.
fn pose_at(clips: &mut Clips, skin: &mut Skin, c: Option<usize>, t: f64) {
    if skin.sampled == Some((c, t)) {
        return;
    }
    match c {
        Some(c) => sample_clip(clips, c, &skin.rig, &skin.info.rest, t, &mut skin.pose, -1),
        None => skin.pose.copy(&skin.info.rest),
    }
    skin.sampled = Some((c, t));
}

/// Bone `b` of `src` into bone `j` of `dst`.
fn copy_bone_to(dst: &mut Pose, j: usize, src: &Pose, b: usize) {
    dst.t[j * 3..j * 3 + 3].copy_from_slice(&src.t[b * 3..b * 3 + 3]);
    dst.s[j * 3..j * 3 + 3].copy_from_slice(&src.s[b * 3..b * 3 + 3]);
    dst.r[j * 4..j * 4 + 4].copy_from_slice(&src.r[b * 4..b * 4 + 4]);
}

/// Bone `b`'s local transform of `pose` (column vectors).
fn bone_local(pose: &Pose, b: usize) -> Mat4 {
    let v = |a: &[f32], k: usize| [f64::from(a[k * 3]), f64::from(a[k * 3 + 1]), f64::from(a[k * 3 + 2])];
    math::compose(v(&pose.t, b), pose.quat(b), v(&pose.s, b))
}

/// A column-vector matrix (three's elements, column-major) as the row-vector M4: the same sixteen values in rows.
fn row_form(e: &Mat4) -> M4 {
    [[e[0], e[1], e[2], e[3]], [e[4], e[5], e[6], e[7]], [e[8], e[9], e[10], e[11]], [e[12], e[13], e[14], e[15]]]
}

/// The unseeded stream's start.
// Unverified: RandomFloatInRange draws from a
// mulberry32 stream of its own from a constant, not the build's engine random (not
// modelled), so a picture of a block driven by it is compared by eye.
const RAND_SEED: u32 = 1;

/// mulberry32's next draw in [0, 1), the state stepped.
fn mulberry32(state: &mut u32) -> f64 {
    *state = state.wrapping_add(0x6d2b_79f5);
    let mut t = *state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    f64::from(t ^ (t >> 14)) / 4_294_967_296.0
}

/// A block's view of the scenery for one call (make, begin, tick, frame, end).
pub struct SceneryHost<'a> {
    pub(crate) state: &'a mut SceneryState,
    pub(crate) vfx: Option<&'a mut mvs_vfx::Player>,
    pub(crate) sound: &'a mut dyn SoundSink,
    /// The World of the scenery's stage as its latest tick left it, in `tick` alone (Renderer::scenery_tick).
    pub(crate) world: Option<&'a World>,
}

/// A skeletal mesh of a block's own (SceneryHost.skinned): its index in the order the blocks made them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SkinnedId(pub u32);

/// A skeletal mesh a block changes the materials of: one of the entry's (by name) or one of the block's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinRef<'a> {
    File(&'a str),
    Own(SkinnedId),
}

/// UI_Functions.PopText with a level actor as the attach actor (SceneryHost.pop): what the
/// match's HUD pops (mvs-ui hud/pop.rs PopParams), as a block gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct PopSpec {
    pub text: String,
    pub lifetime: f64,
    pub text_color: [f64; 4],
    /// UE offset from the attach actor.
    pub location: [f64; 3],
    pub scale: f64,
    /// 0 left, 1 centre, 2 right.
    pub justification: u8,
    pub delay: f64,
    /// A material.
    pub icon_right: Option<String>,
    /// The attach fighter (its player index); None: the level actor at `at`.
    pub fighter: Option<usize>,
    /// The attach level actor's location (UE, PopSpec.at); None: the fighter's.
    pub at: Option<[f64; 3]>,
}

/// The stage actor of the World's stage run whose place in `StageData.placed` (or, for a run-time one, in the run's
/// list) is `index` (StageActor.index); None with no stage run or no such actor (a stage
/// actor lookup).
pub fn stage_actor(w: &World, index: u32) -> Option<&StageActor> {
    w.stage_run.as_ref()?.actors.iter().find(|a| a.index == index)
}

impl<'a> SceneryHost<'a> {
    /// The World of the scenery's stage as its latest tick left it (SceneryHost.world): in `tick` alone, None outside
    /// a match of the stage. A block copies the numbers it needs and holds nothing of it past the call; the borrow is
    /// the host's, so a block may hold it while it calls the host's other methods.
    pub fn world(&self) -> Option<&'a World> {
        self.world
    }
}

impl SceneryHost<'_> {
    /// A skeletal mesh of the block's own, made at make (SceneryHost.skinned: a mesh a Blueprint swaps in,
    /// SetSkeletalMesh): the mesh kind's entry `mesh` (build object path), dressed as the entry's skeletal mesh `like`
    /// is (its place and parent, its lighting channels), its materials by section as the entry names them (a draw
    /// reference with its context; None: the slot is not drawn). It starts hidden; its pose (`pose_skinned`) is in
    /// `like`'s bone order.
    pub fn skinned(&mut self, like: &str, mesh: &str, materials: &[Option<&str>]) -> Result<SkinnedId> {
        let k = self.skeletal_index(like).ok_or_else(|| bad(like, "not a skeletal mesh of the file"))?;
        let st = &mut *self.state;
        // (its pose in like's bone order; its own reference pose once its mesh is read: SceneryState::resolve_own)
        let pose = st.skins[k].info.rest.clone();
        st.own.push(OwnSkin {
            like: k,
            mesh: Arc::from(mesh),
            materials: materials.iter().map(|m| m.map(str::to_owned)).collect(),
            shown: false,
            pose,
            info: None,
            map: Vec::new(),
            slots: Vec::new(),
        });
        Ok(SkinnedId((st.own.len() - 1) as u32))
    }

    /// A skeletal mesh of the block's own shown or hidden.
    pub fn show_skinned(&mut self, id: SkinnedId, on: bool) {
        self.state.own[id.0 as usize].shown = on;
    }

    /// The pose of a skeletal mesh of the block's own (its bones' local transforms in `like`'s bone order, mvs-view's
    /// space), which the block writes each tick.
    pub fn pose_skinned(&mut self, id: SkinnedId) -> &mut Pose {
        &mut self.state.own[id.0 as usize].pose
    }

    /// The section `slot` (LOD 0, as the entry's or `skinned`'s per-section materials) of a skeletal mesh draws
    /// `material` (a draw reference as the entry names one; its context is not read) from now on: its own dynamic
    /// instance (CreateDynamicMaterialInstance, set on the skinned mesh), which
    /// `skeletal_scalars` / `skeletal_vectors` then set. None: back to the section's own material.
    pub fn skeletal_material(&mut self, mesh: SkinRef<'_>, slot: usize, material: Option<&str>) {
        let d = self.state.slot_draw(mesh, slot);
        // (the draw context dropped, as material_ref; a new Arc only for another material than the one swapped in)
        let path = material.map(|m| m.split_once('?').map_or(m, |(p, _)| p));
        if d.swap.as_deref() != path {
            d.swap = path.map(Arc::from);
        }
    }

    /// Scalar parameters set on the dynamic material instance the section `slot` of a skeletal mesh draws
    /// (SceneryHost.material's CharMaterial.setScalar): the material `skeletal_material` swapped in, else its own.
    pub fn skeletal_scalars(&mut self, mesh: SkinRef<'_>, slot: usize, scalars: &[(&str, f32)]) {
        let p = self.state.slot_draw(mesh, slot).params();
        for &(name, v) in scalars {
            match p.scalars.iter_mut().find(|(n, _)| &**n == name) {
                Some(x) => x.1 = v,
                None => p.scalars.push((Arc::from(name), v)),
            }
        }
    }

    /// Vector parameters set on the same instance as `skeletal_scalars` (CharMaterial.setVector).
    pub fn skeletal_vectors(&mut self, mesh: SkinRef<'_>, slot: usize, vectors: &[(&str, [f32; 4])]) {
        let p = self.state.slot_draw(mesh, slot).params();
        for &(name, v) in vectors {
            match p.vectors.iter_mut().find(|(n, _)| &**n == name) {
                Some(x) => x.1 = v,
                None => p.vectors.push((Arc::from(name), v)),
            }
        }
    }

    /// Scalar parameters set on the dynamic instance of an instance's effects material in `slot` (SceneryHost
    /// vfxMaterial / vfxMaterialSet: VfxPlayer.ownMaterial, its set). The renderer draws an effects material on a
    /// static mesh as any other, so this is `material_scalars`: the instance's slot keeps what was set last (the
    /// blocks share one instance among the meshes of a value and swap them between instances; here each
    /// instance's slot holds the values).
    pub fn vfx_material_scalars(&mut self, id: InstanceId, slot: usize, scalars: &[(&str, f32)]) {
        self.material_scalars(id, slot, scalars);
    }

    /// A pop text on a level actor (SceneryHost.pop): queued for the match's HUD (Renderer::scenery_pops).
    pub fn pop(&mut self, p: PopSpec) {
        self.state.pops.push(p);
    }

    /// The entry's instances, in the file's order (a block claims its own by their fields at make).
    pub fn instances(&self) -> &[Instance] {
        &self.state.instances
    }

    /// The first instance named `name`.
    pub fn instance(&self, name: &str) -> Option<InstanceId> {
        let k = self.state.instances.iter().position(|i| i.name == name)?;
        Some(InstanceId(k as u32))
    }

    /// Instances shown or hidden (SceneryHost.show).
    pub fn show(&mut self, ids: &[InstanceId], on: bool) {
        for id in ids {
            self.state.shown[id.0 as usize] = on;
        }
    }

    /// An instance moved to the UE transform `ue` (row-vector, `stage::transform`'s form; relative to its bone when
    /// it is attached to one); None: back as placed.
    pub fn place(&mut self, id: InstanceId, ue: Option<M4>) {
        self.state.placed[id.0 as usize] = ue;
    }

    /// Scalar parameters set on an instance's material in `slot` (its dynamic material instance's).
    pub fn material_scalars(&mut self, id: InstanceId, slot: usize, scalars: &[(&str, f32)]) {
        let held = &mut self.state.scalars[id.0 as usize];
        for &(name, v) in scalars {
            match held.iter_mut().find(|(s, n, _)| *s == slot && &**n == name) {
                Some(x) => x.2 = v,
                None => held.push((slot, Arc::from(name), v)),
            }
        }
    }

    /// The entry's skeletal meshes, in the file's order.
    pub fn skeletals(&self) -> &[Skeletal] {
        &self.state.skeletal
    }

    /// The skeletal mesh named `name`: its index in `skeletals`.
    pub fn skeletal_index(&self, name: &str) -> Option<usize> {
        self.state.skeletal.iter().position(|k| k.name == name)
    }

    /// The animation of the skeletal mesh named `name` (SceneryHost.skeletal).
    pub fn skeletal(&mut self, name: &str) -> Option<&mut SceneryAnim> {
        let k = self.skeletal_index(name)?;
        Some(&mut self.state.anims[k])
    }

    /// The skeletal mesh named `name` posed by its block from now on (its AnimBP): its
    /// clip no longer plays; the block writes `pose` each tick. The pose holds what the clip left in it (a drive may
    /// keep the pose it finds). False with no skeletal mesh of that name.
    pub fn drive(&mut self, name: &str) -> bool {
        let Some(k) = self.skeletal_index(name) else { return false };
        let SceneryState { anims, skins, clips, .. } = &mut *self.state;
        let (a, skin) = (&anims[k], &mut skins[k]);
        if !skin.driven {
            let c = a.clip.as_deref().map(|p| clip_index(clips, p, name));
            let t = clip_time(clips, c, a);
            pose_at(clips, skin, c, t);
            skin.driven = true;
        }
        true
    }

    /// The pose of the skeletal mesh `name` (its bones' local transforms, mvs-view's space), as its clip or its
    /// block left it.
    pub fn pose(&mut self, name: &str) -> Option<&mut Pose> {
        let k = self.skeletal_index(name)?;
        let skin = &mut self.state.skins[k];
        // (the block may write it: a clip's next tick samples it again)
        skin.sampled = None;
        Some(&mut skin.pose)
    }

    /// The reference pose of the skeletal mesh `name`.
    pub fn rest(&self, name: &str) -> Option<&Pose> {
        let k = self.skeletal_index(name)?;
        Some(&self.state.skins[k].info.rest)
    }

    /// The animation `clip` (build object path) at `time` seconds, sampled over the bones of the skeletal mesh
    /// `skeletal` into `out`. False when the clip is not one of the scenery's.
    pub fn sample(&mut self, skeletal: &str, clip: &str, time: f64, out: &mut Pose) -> bool {
        let Some(k) = self.skeletal_index(skeletal) else { return false };
        let SceneryState { skins, clips, .. } = &mut *self.state;
        let Some(c) = clips.get(clip) else { return false };
        let skin = &skins[k];
        sample_clip(clips, c, &skin.rig, &skin.info.rest, time, out, -1);
        true
    }

    /// The world transform (UE, row-vector) of bone `bone` of the skeletal mesh `skeletal` as it is posed now.
    pub fn bone_world(&self, skeletal: &str, bone: &str) -> Option<M4> {
        let k = self.skeletal_index(skeletal)?;
        let b = self.state.skins[k].info.bones().iter().position(|n| n == bone)?;
        Some(self.state.bone_world_at(k, b))
    }

    /// A skeletal mesh shown or hidden.
    pub fn show_skeletal(&mut self, name: &str, on: bool) {
        if let Some(k) = self.skeletal_index(name) {
            self.state.skeletal_shown[k] = on;
        }
    }

    /// A skeletal mesh's component moved to the UE transform `ue` (relative to its bone when it is attached to one);
    /// None: back as placed.
    pub fn place_skeletal(&mut self, name: &str, ue: Option<M4>) {
        if let Some(k) = self.skeletal_index(name) {
            self.state.skeletal_placed[k] = ue;
        }
    }

    /// RandomFloatInRange over [range[0], range[1]) (the unseeded stream: scenery is no part of the simulation).
    pub fn rand(&mut self, range: [f64; 2]) -> f64 {
        range[0] + mulberry32(&mut self.state.rand_state) * (range[1] - range[0])
    }

    /// A draw in [range[0], range[1]) for a value the build's authority draws and replicates (Map_TreeHouse's
    /// ServerSnail): (the port's own) a mulberry32 stream on the match's seed, started at each begin, so every peer
    /// draws the same; the unseeded stream outside a match.
    pub fn seeded(&mut self, range: [f64; 2]) -> f64 {
        if !self.state.seeded_on {
            return self.rand(range);
        }
        range[0] + mulberry32(&mut self.state.seed_state) * (range[1] - range[0])
    }

    /// The match this scenery's level runs in (SceneryHost.match), as `Renderer::scenery_begin` was given it; None
    /// outside a match.
    pub fn scenery_match(&self) -> Option<&SceneryMatch> {
        self.state.matched.as_ref()
    }

    /// The CameraCenter actor's location (UE) as the frame draws it, None with no camera.
    pub fn camera(&self) -> Option<[f64; 3]> {
        self.state.camera
    }

    /// PF_Post_Event_At_Location (SceneryHost.sound).
    pub fn sound(&mut self, event: &str, at: [f64; 3], key: Option<&str>) {
        self.sound.post_at(event, at, key);
    }

    /// PF_Post_Event_On_AkComponent on an actor's moving AkComponent (SceneryHost.soundOn).
    pub fn sound_on(&mut self, event: &str, key: &str, at: [f64; 3]) {
        self.sound.post_on(event, key, at);
    }

    /// The game object `key` moved (SceneryHost.place).
    pub fn sound_place(&mut self, key: &str, at: [f64; 3]) {
        self.sound.place(key, at);
    }

    /// A game parameter's global value (SceneryHost.rtpc).
    pub fn rtpc(&self, name: &str) -> Option<f64> {
        self.sound.rtpc(name)
    }

    /// SpawnEmitterAtLocation of a level actor (VfxPlayer.spawnLevel): `system` at `loc`, `rotator` (pitch, yaw,
    /// roll in degrees), `scale`, with its instance parameters. None with no particle player or nothing to play.
    pub fn vfx_spawn(
        &mut self,
        system: &str,
        loc: [f64; 3],
        rotator: [f64; 3],
        scale: [f64; 3],
        params: &[(&str, f64)],
    ) -> Option<mvs_vfx::SystemId> {
        self.vfx.as_deref_mut()?.spawn(system, loc, rotator, scale, params)
    }

    /// The particle player, for what a block does to a system it spawned (attach, deactivate, kill, parameters).
    pub fn vfx(&mut self) -> Option<&mut mvs_vfx::Player> {
        self.vfx.as_deref_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The World's state the stage blocks read (SceneryWorld) is reachable from here, by these paths;
    /// a block calls them, never a copy.
    #[allow(dead_code)]
    fn world_getters(w: &World, f: mvs_sim::fighter::FighterId) {
        use mvs_sim::fighter::body::{actor_y, cap_radius, capsule_half};
        let _: [f64; 3] = [f64::from(actor_y(w, f)), f64::from(cap_radius(w, f)), f64::from(capsule_half(w, f))];
        let _: (&Vec<mvs_sim::Real>, bool, bool) = (&w.flow.team_knockouts, w.flow.match_started, w.flow.match_over);
        let stage = &w.data.stage;
        let _ = (&stage.placed, &stage.platforms, &stage.audio);
        if let Some(r) = &w.stage_run {
            let _ = (&r.actors, &r.solids, &r.s, &r.platforms);
        }
        let _ = (&w.stage.stage_platforms, w.flow.tick, &w.fighters);
    }

    /// The seeded stream (JavaScript's 32-bit integer rules): its first draws from state 1, and from -5 (a seed
    /// past i32's range, `seed | 0`).
    #[test]
    fn mulberry32_draws_as_the_page() {
        let mut s = 1u32;
        assert_eq!(
            [mulberry32(&mut s), mulberry32(&mut s), mulberry32(&mut s)],
            [0.6270739405881613, 0.002735721180215478, 0.5274470399599522]
        );
        let mut s = (-5i32).cast_unsigned();
        assert_eq!(mulberry32(&mut s), 0.48384718922898173);
    }
}
