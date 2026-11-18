//! The views a spawned actor's drawing is made of (a static view and a skinned actor) and the placement of a mesh
//! component under its actor: each view is placed by a UE world transform and gives the [`MeshPose`] it draws.

use std::sync::Arc;

use mvs_data::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::item::Item;
use mvs_sim::actors::projectile::{AttachParentOut, attach_parent};
use mvs_sim::fighter::FighterId;
use mvs_vfx::math::{V3, compose_rows, rotation_rows, rotator_rows};

use crate::actors::{ActorHost, Rows};
use crate::anim::{Clips, Pose, Rig, blend_pose, sample_clip};
use crate::frame::{MaterialRef, MeshPose};
use crate::math::{self, Mat4};
use crate::mesh::MeshInfo;

pub const IDENT: Rows = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// A UE world transform: location (cm), the rows of the world directions of the local X, Y and Z, the world Scale3D.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub loc: V3,
    pub rot: Rows,
    pub scale: V3,
}

/// `xyz`: a vector struct's X, Y and Z (`d` where unset).
pub fn xyz(v: Option<&Value>, d: f64) -> V3 {
    let o = v.and_then(Value::as_obj);
    let g = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(d);
    [g("X"), g("Y"), g("Z")]
}

/// `relRows`: a rotator struct's rows (Pitch, Yaw, Roll in degrees).
pub fn rel_rows(v: Option<&Value>) -> Rows {
    let o = v.and_then(Value::as_obj);
    let g = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
    rotator_rows([g("Pitch"), g("Yaw"), g("Roll")], None)
}

/// `itemRows`: an item's rotation rows, its facing's yaw and its pitch.
pub fn item_rows(it: &Item) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    rotation_rows(f64::from(it.pitch).to_radians(), if it.facing > 0.0 { -h } else { h }, 0.0)
}

/// UE's row-vector transform as the frame's FMatrix3x4 (StaticView.set's matrix).
fn to_world(x: &Xform) -> [f32; 12] {
    let (r, s, l) = (&x.rot, &x.scale, &x.loc);
    let mut o = [0.0f32; 12];
    for k in 0..3 {
        o[k * 4] = (r[k] * s[0]) as f32;
        o[k * 4 + 1] = (r[3 + k] * s[1]) as f32;
        o[k * 4 + 2] = (r[6 + k] * s[2]) as f32;
        o[k * 4 + 3] = l[k] as f32;
    }
    o
}

// ---- comp-place -------------------------------------------------------------------------------------------------

/// A mesh component at its relative transform under its attach parents.
#[derive(Clone, Debug)]
pub struct Comp {
    pub par: AttachParentOut,
    /// RelativeLocation, RelativeRotation's rows, RelativeScale3D.
    pub rl: V3,
    pub rot: Rows,
    pub rs: V3,
}

/// `compOf`: component `name` of the class's components `cs`.
pub fn comp_of(cs: &Props, name: &str) -> Option<Comp> {
    let c = cs.get(name)?.as_obj()?;
    Some(Comp {
        par: attach_parent(cs, name),
        rl: xyz(c.get("RelativeLocation"), 0.0),
        rot: rel_rows(c.get("RelativeRotation")),
        rs: xyz(c.get("RelativeScale3D"), 1.0),
    })
}

/// `placeAt`: the component under an actor at the UE world location `at` with the rows `rot` and root scale `s`
/// (FTransform composition); `own` / `rs`: its relative rotation rows and scale (a class's graph may set its own).
pub fn place_at(c: &Comp, own: &Rows, rs: V3, rot: &Rows, s: f64, mut at: V3) -> Xform {
    let (par, rl) = (&c.par, c.rl);
    let pr: Rows = std::array::from_fn(|k| par.rot[k]).map(f64::from);
    let cl = [f64::from(par.scale[0]) * rl[0], f64::from(par.scale[1]) * rl[1], f64::from(par.scale[2]) * rl[2]];
    let rel: V3 =
        std::array::from_fn(|k| f64::from(par.loc[k]) + cl[0] * pr[k] + cl[1] * pr[3 + k] + cl[2] * pr[6 + k]);
    for k in 0..3 {
        at[k] += s * (rel[0] * rot[k] + rel[1] * rot[3 + k] + rel[2] * rot[6 + k]);
    }
    Xform {
        loc: at,
        rot: compose_rows(&compose_rows(own, &pr), rot),
        scale: std::array::from_fn(|k| f64::from(par.scale[k]) * rs[k] * s),
    }
}

/// `placeUnder`: the component under an actor at sim (`x`, `y`), its template's relative rotation and scale.
pub fn place_under(c: &Comp, rot: &Rows, s: f64, x: f64, y: f64) -> Xform {
    place_at(c, &c.rot, c.rs, rot, s, [0.0, -x, y])
}

// ---- static view ------------------------------------------------------------------------------------------------

/// The draws a view's entry is of.
#[derive(Clone, Debug)]
enum DrawsOf {
    /// A character's (its key).
    Char(String),
    /// The items part's (crate::items).
    Items,
}

/// One static mesh component: the mesh of an actors entry of a character's draws, or of a
/// draw of the items part, with that entry's materials of a variant, placed by a UE world transform.
#[derive(Clone, Debug)]
pub struct StaticView {
    info: Arc<MeshInfo>,
    /// The draws that hold the entry, and the entry.
    of: DrawsOf,
    key: String,
    dressed: Option<String>,
    /// Per slot: the material (None: the slot is not drawn).
    mats: Vec<Option<MaterialRef>>,
    scalars: Vec<(String, f64)>,
    vectors: Vec<(String, [f64; 4])>,
    to_world: [f32; 12],
    tween: Option<Box<PlaceTween>>,
    pub visible: bool,
}

/// StaticView::ticked's state: the previous tick's place, the latest tick's transform.
#[derive(Clone, Copy, Debug)]
struct PlaceTween {
    at0: V3,
    last: Xform,
    placed: bool,
    tick: i32,
    /// A new tick began: its first set() moves the latest place to the previous.
    shift: bool,
}

impl StaticView {
    /// `staticView`: the actors entry `mesh` of fighter `f`'s character in `variant` (None: no such entry or mesh).
    pub fn new(host: &ActorHost<'_>, w: &World, f: FighterId, mesh: &str, variant: &str) -> Option<StaticView> {
        let char_key = w.names.str(w.fighter(f).ch).to_owned();
        let (path, _) = host.draws.get(&char_key)?.actor(mesh, None)?;
        let info = host.meshes.get(&path)?.clone();
        Some(StaticView::made(host, info, DrawsOf::Char(char_key), mesh, variant))
    }

    /// The actors entry `mesh` of character `key`'s draws with its own materials (an actor drawn with a character's
    /// assets that no fighter owns: the tutorial's Meeseeks); None: no such entry, or its mesh is not loaded.
    pub fn of_char(host: &ActorHost<'_>, key: &str, mesh: &str) -> Option<StaticView> {
        let (path, _) = host.draws.get(key)?.actor(mesh, None)?;
        let info = host.meshes.get(&path)?.clone();
        Some(StaticView::made(host, info, DrawsOf::Char(key.to_owned()), mesh, "template"))
    }

    /// `itemView`: the items part's static mesh component `key` ('<class>.<component>')
    /// in `variant` ('template': its own materials); None where the match has not loaded it or it is skeletal.
    pub fn item(host: &ActorHost<'_>, key: &str, variant: &str) -> Option<StaticView> {
        let asset = host.item_asset(key)?;
        if asset.info.skeleton.is_some() {
            return None;
        }
        host.item_materials(key, "template")?;
        Some(StaticView::made(host, asset.info, DrawsOf::Items, key, variant))
    }

    fn made(host: &ActorHost<'_>, info: Arc<MeshInfo>, of: DrawsOf, key: &str, variant: &str) -> StaticView {
        let mut v = StaticView {
            info,
            of,
            key: key.to_owned(),
            dressed: None,
            mats: Vec::new(),
            scalars: Vec::new(),
            vectors: Vec::new(),
            to_world: [0.0; 12],
            tween: None,
            visible: true,
        };
        v.dress(host, variant);
        v
    }

    /// `dress`: the materials of a variant (SetMaterial(0, ..) of SetTeamColor); the entry's own where it has no set of
    /// that name.
    pub fn dress(&mut self, host: &ActorHost<'_>, variant: &str) {
        if self.dressed.as_deref() == Some(variant) {
            return;
        }
        self.dressed = Some(variant.to_owned());
        let keys = match &self.of {
            DrawsOf::Char(k) => host.draws.get(k).and_then(|d| d.actor(&self.key, Some(variant))).map(|d| d.1),
            DrawsOf::Items => host.item_materials(&self.key, variant),
        };
        let keys = keys.unwrap_or_default();
        self.mats = (0..self.info.slots.len())
            .map(|k| {
                let mut m = crate::pose::material(keys.get(k).cloned().flatten()?.as_str());
                for (n, v) in &self.scalars {
                    crate::pose::set_scalar(&mut m, n, *v);
                }
                for (n, v) in &self.vectors {
                    crate::pose::set_vector(&mut m, n, *v);
                }
                Some(m)
            })
            .collect();
    }

    /// loc: UE world, cm; rot: the rows of the world directions of the component's X, Y and Z; scale: its world
    /// Scale3D.
    pub fn set(&mut self, x: &Xform) {
        if let Some(tw) = &mut self.tween {
            if tw.shift {
                tw.at0 = if tw.placed { tw.last.loc } else { x.loc };
                tw.shift = false;
            }
            tw.last = *x;
            tw.placed = true;
        }
        self.to_world = to_world(x);
    }

    /// SkinnedActor::ticked for a static mesh: the frame is of sim tick `tick` (a tick other than the last one's starts
    /// the next, a rollback's included); set() then keeps the tick's last place, and present(a) draws the mesh at the
    /// frame `a` of the way from the previous tick's place to it (the rotation and scale the latest tick's).
    // Deviation: (port-only) these meshes are placed once a tick, a moving one stepping at 60 Hz on a faster
    // display; no build reference (the build ticks at the display's rate).
    pub fn ticked(&mut self, tick: i32) {
        let tw = self.tween.get_or_insert_with(|| {
            Box::new(PlaceTween {
                at0: [0.0; 3],
                last: Xform { loc: [0.0; 3], rot: IDENT, scale: [1.0; 3] },
                placed: false,
                tick,
                shift: true,
            })
        });
        if tw.tick != tick {
            tw.tick = tick;
            tw.shift = true;
        }
    }

    /// The frame `a` of the way from the previous tick's place to the latest (1 the latest; with ticked()).
    pub fn present(&mut self, a: f64) {
        let Some(tw) = &self.tween else { return };
        if tw.placed {
            let loc = std::array::from_fn(|k| tw.at0[k] + (tw.last.loc[k] - tw.at0[k]) * a);
            self.to_world = to_world(&Xform { loc, ..tw.last });
        }
    }

    /// SetScalarParameterValue on every slot's dynamic instance (kept for a later dress).
    pub fn set_scalar(&mut self, name: &str, v: f64) {
        match self.scalars.iter_mut().find(|s| s.0 == name) {
            Some(s) => s.1 = v,
            None => self.scalars.push((name.to_owned(), v)),
        }
        for m in self.mats.iter_mut().flatten() {
            crate::pose::set_scalar(m, name, v);
        }
    }

    /// SetVectorParameterValueOnMaterials.
    pub fn set_vector(&mut self, name: &str, v: [f64; 4]) {
        match self.vectors.iter_mut().find(|s| s.0 == name) {
            Some(s) => s.1 = v,
            None => self.vectors.push((name.to_owned(), v)),
        }
        for m in self.mats.iter_mut().flatten() {
            crate::pose::set_vector(m, name, v);
        }
    }

    /// What it draws now, into `out` (nothing while hidden).
    pub fn draw(&self, out: &mut Vec<MeshPose>) {
        if self.visible {
            out.push(mesh_pose(&self.info, &self.mats, self.to_world, Vec::new()));
        }
    }
}

/// A mesh's pose: a section is drawn when its slot has a material.
fn mesh_pose(info: &MeshInfo, mats: &[Option<MaterialRef>], to_world: [f32; 12], bones: Vec<[f32; 12]>) -> MeshPose {
    MeshPose {
        mesh: Arc::from(info.path.as_str()),
        lod: 0,
        to_world,
        bones,
        shown: info.sections.iter().map(|s| !s.disabled && mats.get(s.slot).is_some_and(Option::is_some)).collect(),
        // (a slot with no material built, Windshield_MAT's, is not drawn: it names the mesh slot's own material, as
        // the pose tape and the renderer's fallback do)
        materials: mats
            .iter()
            .enumerate()
            .map(|(k, m)| {
                m.clone()
                    .or_else(|| info.slots.get(k).and_then(|s| s.material.as_deref()).map(crate::pose::material))
                    .unwrap_or_default()
            })
            .collect(),
        custom_stencil: 0,
        capture_skip: false,
        no_decals: false,
        morphs: Vec::new(),
    }
}

// ---- skinned actor ----------------------------------------------------------------------------------------------

/// blendTicks' state: the previous and the latest tick's place, the latest rotation and scale, the poses.
#[derive(Clone, Debug)]
struct Tween {
    at0: V3,
    at1: V3,
    rot: Rows,
    scale: V3,
    placed: bool,
    before: Pose,
    shown: Pose,
    posed: bool,
    /// ticked()'s tick (None: every set() and show() is a tick's).
    tick: Option<i32>,
    /// A new tick began (ticked()): its first set() / show() moves the latest place / pose to the previous.
    shift_place: bool,
    shift_pose: bool,
}

/// A skeletal mesh component of a spawned actor with its own animation: the mesh's
/// sections with the given materials, placed by a UE world transform, its bones in the view's space.
#[derive(Debug)]
pub struct SkinnedActor {
    info: Arc<MeshInfo>,
    /// Per slot: the material (None: not drawn).
    pub mats: Vec<Option<MaterialRef>>,
    /// The skeleton as a pose of it is laid out: bone names, each one's parent (-1 for a top bone), the reference
    /// pose.
    pub names: Vec<String>,
    pub parents: Vec<i32>,
    pub rest: Pose,
    /// The skeleton's top bone: a sequence's root lock replaces it (sample_clip).
    pub root: i32,
    /// The bone names as the particle side keys a bone (bones_world), made once.
    keys: Vec<Arc<str>>,
    /// The bones bound to the clips the actor plays (None until the first play).
    rig: Option<Rig>,
    /// The pose on the bones.
    bones: Pose,
    pose: Pose,
    tween: Option<Tween>,
    xf: Xform,
    pub visible: bool,
}

impl SkinnedActor {
    /// `itemSkinned`: the items part's skeletal mesh component `key` in its template's set;
    /// None where the match has not loaded it or it is static. Its class's sequences: ActorHost::item_clip.
    pub fn item(host: &ActorHost<'_>, key: &str) -> Option<SkinnedActor> {
        let asset = host.item_asset(key)?;
        let keys = host.item_materials(key, "template")?;
        asset.info.skeleton.is_some().then(|| SkinnedActor::new(asset.info, &keys))
    }

    /// The mesh `info` with the materials `keys` (per slot; None: the slot is not drawn).
    pub fn new(info: Arc<MeshInfo>, keys: &[Option<String>]) -> SkinnedActor {
        let mats =
            (0..info.slots.len()).map(|k| keys.get(k).cloned().flatten().map(|p| crate::pose::material(&p))).collect();
        let sk = info.skeleton.as_ref();
        let names = info.bones().to_vec();
        let parents = sk.map_or_else(Vec::new, |s| s.parents.clone());
        let root = parents.iter().position(|&p| p < 0).map_or(-1, |i| i as i32);
        let rest = info.rest.clone();
        let keys = names.iter().map(|n| Arc::from(n.as_str())).collect();
        SkinnedActor {
            keys,
            mats,
            bones: rest.clone(),
            pose: rest.clone(),
            rest,
            names,
            parents,
            root,
            rig: None,
            tween: None,
            xf: Xform { loc: [0.0; 3], rot: IDENT, scale: [1.0; 3] },
            visible: true,
            info,
        }
    }

    /// A bone by its name, as an FName compares: without case.
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n.eq_ignore_ascii_case(name))
    }

    /// The skeleton an [`crate::actor_anim`] runner poses, its rig bound to `clips` (the actor plays from one clip
    /// set: the rig is bound to the first one given).
    pub fn skeleton(&mut self, clips: &Clips) -> crate::actor_anim::ActorSkeleton<'_> {
        let rig = self.rig.get_or_insert_with(|| Rig::bind(clips, self.names.clone()));
        crate::actor_anim::ActorSkeleton { rig, rest: &self.rest, root: self.root, parents: &self.parents }
    }

    /// The clip `c` of `clips` at `time` (looping, or held at its end). The actor plays from one clip set: the rig
    /// is bound to the first one played.
    pub fn play(&mut self, clips: &mut Clips, c: usize, time: f64, looped: bool) {
        let len = clips.clips[c].anim.length;
        let t = if looped && len > 0.0 { time % len } else { time.min(len) };
        let rig = self.rig.get_or_insert_with(|| Rig::bind(clips, self.names.clone()));
        let mut pose = std::mem::replace(&mut self.pose, Pose::new(0));
        sample_clip(clips, c, rig, &self.rest, t, &mut pose, self.root);
        self.show(&pose);
        self.pose = pose;
    }

    /// A pose of the skeleton (`names`' order) on the bones.
    pub fn show(&mut self, p: &Pose) {
        if let Some(tw) = &mut self.tween {
            if tw.tick.is_none() || tw.shift_pose {
                tw.before.copy(if tw.posed { &tw.shown } else { p });
                tw.shift_pose = false;
            }
            tw.shown.copy(p);
            tw.posed = true;
        }
        self.bones.copy(p);
    }

    /// loc: UE world, cm; rot: the rows of the world directions of the component's X, Y and Z; scale: its world
    /// Scale3D.
    pub fn set(&mut self, x: &Xform) {
        if let Some(tw) = &mut self.tween {
            if tw.tick.is_none() || tw.shift_place {
                tw.at0 = if tw.placed { tw.at1 } else { x.loc };
                tw.shift_place = false;
            }
            tw.at1 = x.loc;
            tw.scale = x.scale;
            tw.rot = x.rot;
            tw.placed = true;
        }
        self.xf = *x;
    }

    /// `blendTicks`: set() and show() (play()) once a sim tick keep that tick's place and pose; present(a) then draws
    /// each frame between the previous tick's and the latest (the rotation and scale the latest tick's).
    // Deviation: from a tick's start until its view's set(), the mesh holds the last drawn frame's blend,
    // not the previous tick's place.
    pub fn blend_ticks(&mut self) {
        let n = self.names.len();
        self.tween.get_or_insert_with(|| Tween {
            at0: [0.0; 3],
            at1: [0.0; 3],
            rot: [0.0; 9],
            scale: [1.0; 3],
            placed: false,
            before: Pose::new(n),
            shown: Pose::new(n),
            posed: false,
            tick: None,
            shift_place: false,
            shift_pose: false,
        });
    }

    /// blend_ticks for a view whose set() and show() (play()) run every frame: the frame is of sim tick `tick` (a
    /// tick other than the last one's starts the next, a rollback's included), whose last set() and show() are that
    /// tick's place and pose; present(a) blends from the previous tick's.
    // Deviation: (port-only) these meshes are placed and posed once a tick, a moving one stepping at 60 Hz on a
    // faster display; no build reference (the build ticks at the display's rate).
    pub fn ticked(&mut self, tick: i32) {
        self.blend_ticks();
        let Some(tw) = &mut self.tween else { return };
        if tw.tick != Some(tick) {
            tw.tick = Some(tick);
            tw.shift_place = true;
            tw.shift_pose = true;
        }
    }

    /// The frame `a` of the way from the previous tick's place and pose to the latest (1 the latest).
    pub fn present(&mut self, a: f64) {
        let Some(tw) = &self.tween else { return };
        if tw.placed {
            let loc = std::array::from_fn(|k| tw.at0[k] + (tw.at1[k] - tw.at0[k]) * a);
            self.xf = Xform { loc, rot: tw.rot, scale: tw.scale };
        }
        if !tw.posed {
            return;
        }
        if a >= 1.0 {
            self.bones.copy(&tw.shown);
            return;
        }
        let mut out = std::mem::replace(&mut self.bones, Pose::new(0));
        blend_pose(&tw.before, &tw.shown, a, &mut out);
        self.bones = out;
    }

    /// `cut`: the next tick's place and pose are drawn as they are, with nothing to blend from.
    pub fn cut(&mut self) {
        if let Some(tw) = &mut self.tween {
            tw.placed = false;
            tw.posed = false;
        }
    }

    /// The bones' component-space matrices in the view's space (three's matrixWorld under the model: the plain
    /// product down each chain).
    pub fn bones_cs(&self) -> Vec<Mat4> {
        let p = &self.bones;
        let mut out: Vec<Mat4> = Vec::with_capacity(p.n);
        for i in 0..p.n {
            let f = |v: &[f32], k: usize| f64::from(v[k]);
            let t = [f(&p.t, i * 3), f(&p.t, i * 3 + 1), f(&p.t, i * 3 + 2)];
            let s = [f(&p.s, i * 3), f(&p.s, i * 3 + 1), f(&p.s, i * 3 + 2)];
            let local = math::compose(t, p.quat(i), s);
            let pa = self.parents.get(i).copied().unwrap_or(-1);
            out.push(if pa < 0 { local } else { math::mul(&out[pa as usize], &local) });
        }
        out
    }

    /// Every bone's UE world transform now, by name (a bone follow's bone: a mirrored one with its Y
    /// row and scale turned).
    pub fn bones_world(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        let world = crate::actors::xform_mat(&self.xf);
        for (b, cs) in self.bones_cs().iter().enumerate() {
            let m = math::mul(&world, &math::to_ue(cs));
            out.push((self.keys[b].clone(), crate::actors::mat_xform(&m)));
        }
    }

    /// What it draws now, into `out` (nothing while hidden).
    pub fn draw(&self, out: &mut Vec<MeshPose>) {
        if !self.visible {
            return;
        }
        let cs = self.bones_cs();
        let bones = (0..self.info.inv_ref.len().min(cs.len()))
            .map(|k| math::rows3x4(&math::mul(&math::to_ue(&cs[k]), &self.info.inv_ref[k])))
            .collect();
        out.push(mesh_pose(&self.info, &self.mats, to_world(&self.xf), bones));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> Xform {
        Xform { loc: [x, 0.0, 0.0], rot: IDENT, scale: [1.0; 3] }
    }

    /// ticked(): every set() of a tick keeps the tick's place, present(a) blends from the previous tick's last one.
    #[test]
    fn a_ticked_static_view_blends_from_the_previous_ticks_place() {
        let info = Arc::new(MeshInfo {
            path: "m".into(),
            slots: Vec::new(),
            sections: Vec::new(),
            skeleton: None,
            inv_ref: Vec::new(),
            rest: Pose::new(0),
            used_joints: Vec::new(),
        });
        let mut v = StaticView {
            info,
            of: DrawsOf::Items,
            key: String::new(),
            dressed: None,
            mats: Vec::new(),
            scalars: Vec::new(),
            vectors: Vec::new(),
            to_world: [0.0; 12],
            tween: None,
            visible: true,
        };
        let frame = |v: &mut StaticView, tick: i32, x: f64, a: f64| {
            v.ticked(tick);
            v.set(&at(x));
            v.present(a);
            v.to_world[3]
        };
        // the first tick: nothing to blend from
        assert_eq!(frame(&mut v, 1, 4.0, 0.5), 4.0);
        assert_eq!(frame(&mut v, 1, 4.0, 0.9), 4.0);
        // the next tick: from 4 to 12, its second set() the latest
        assert_eq!(frame(&mut v, 2, 10.0, 0.25), 5.5);
        assert_eq!(frame(&mut v, 2, 12.0, 0.5), 8.0);
        // a rollback to an earlier tick starts the next from the last place
        assert_eq!(frame(&mut v, 1, 0.0, 0.5), 6.0);
        // without ticked(), set() places as it is
        let mut w = v.clone();
        w.tween = None;
        w.set(&at(3.0));
        w.present(0.5);
        assert_eq!(w.to_world[3], 3.0);
    }
}
