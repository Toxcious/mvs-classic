//! The podium's and the pre-match level's banners:
//! BP_Banner_PM's pieces with the custom stencil they read, and a player's banner element.

pub mod classes;
pub mod entry;

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use mvs_vfx::{Attachment, SystemId};
use mvs_view::math::{compose as compose_matrix, qmul, rotator_quat, rows3x4};
use mvs_view::{MaterialRef, MeshPose};

use self::entry::{BannerEntry, ComponentKind};
use super::LevelCx;
use super::format::{BannerInstance, BannerPiece, PieceAsset, PreMatchScene, V3, material_ref};
use crate::Result;
use crate::assets::Assets;
use crate::fx::warn_once;

/// The engine Plane BP_Banner_PM's pieces draw (their StaticMesh).
pub const PLANE: &str = "/Engine/BasicShapes/Plane.Plane";

/// Entrance Timeline: SetPlayRate(10) (Start EOG Entrance @7897), VLerp from (0, 0, 500) above (@8258).
const BANNER_RATE: f64 = 10.0;
const BANNER_DROP: f64 = 500.0;

/// The nameplate material's profile icon parameter (Set Profile Icon).
const PROFILE_TEXTURE: &str = "ProfileTexture";

/// The indices of `BannerView::pieces`: background, filter, trim, then the nameplate when the asset has it; the mask's
/// custom depth copy is last (`BannerView::writer`).
pub const PIECE_BACKGROUND: usize = 0;
pub const PIECE_FILTER: usize = 1;
pub const PIECE_TRIM: usize = 2;
pub const PIECE_NAMEPLATE: usize = 3;

/// The banner index of a store, parsed once: keyed by the store's folder and the index entry's path (a new import's
/// index is a new path).
fn cached_index(store: &mvs_store::Store) -> Result<Arc<entry::BannerIndex>> {
    type Cache = Option<((std::path::PathBuf, String), Arc<entry::BannerIndex>)>;
    static INDEX: std::sync::Mutex<Cache> = std::sync::Mutex::new(None);
    let key = (store.dir().to_path_buf(), entry::index_path(store)?);
    let mut c = INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((k, index)) = c.as_ref()
        && *k == key
    {
        return Ok(index.clone());
    }
    let index = Arc::new(entry::load_index(store)?);
    *c = Some((key, index.clone()));
    Ok(index)
}

// ---------------------------------------------------------------- UE transforms

/// A vector turned by the quaternion (x, y, z, w).
fn qrot(q: [f64; 4], v: V3) -> V3 {
    let [qx, qy, qz, qw] = q;
    let [vx, vy, vz] = v;
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

/// The inverse of a unit quaternion (its conjugate).
fn qinv(q: [f64; 4]) -> [f64; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

fn mul3(a: V3, b: V3) -> V3 {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dist(a: V3, b: V3) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// A transform in UE space (cm): location, rotation, scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Xf {
    loc: V3,
    q: [f64; 4],
    s: V3,
}

impl Xf {
    const IDENTITY: Xf = Xf { loc: [0.0; 3], q: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3] };

    /// A location, rotator (pitch, yaw, roll degrees) and scale.
    fn of([loc, rot, s]: [V3; 3]) -> Xf {
        Xf { loc, q: rotator_quat(rot[0], rot[1], rot[2]), s }
    }

    /// The child `r` under this parent with FTransform's rule (no shear): world loc = parent.rot (parent.scale x
    /// rel.loc) + parent.loc, rot = parent.rot rel.rot, scale = parent.scale x rel.scale.
    fn compose(&self, r: &Xf) -> Xf {
        Xf { loc: add3(qrot(self.q, mul3(r.loc, self.s)), self.loc), q: qmul(self.q, r.q), s: mul3(r.s, self.s) }
    }

    /// The component's LocalToWorld as MeshPose's FMatrix3x4.
    fn to_world(self) -> [f32; 12] {
        rows3x4(&compose_matrix(self.loc, self.q, self.s))
    }
}

/// A static mesh's pose with its one material slot set.
fn mesh_pose(mesh: &str, to_world: [f32; 12], material: MaterialRef, custom_stencil: u8) -> MeshPose {
    MeshPose {
        mesh: Arc::from(mesh),
        lod: 0,
        to_world,
        bones: Vec::new(),
        shown: Vec::new(),
        materials: vec![material],
        custom_stencil,
        capture_skip: false,
        no_decals: false,
        morphs: Vec::new(),
    }
}

/// FQuat::FindBetweenNormals: the rotation taking the unit vector `a` to the unit vector `b`.
pub fn find_between_normals(a: V3, b: V3) -> [f64; 4] {
    let w = 1.0 + a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let q = if w >= 1e-6 {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0], w]
    } else if a[0].abs() > a[1].abs() {
        [-a[2], 0.0, a[0], 0.0]
    } else {
        [0.0, -a[2], a[1], 0.0]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l == 0.0 { [0.0, 0.0, 0.0, 1.0] } else { q.map(|x| x / l) }
}

// ---------------------------------------------------------------- BP_Banner_PM's pieces

/// A banner's pieces as its asset has them, Slant Angle and the Entrance Timeline's length
/// (BannerAsset).
#[derive(Clone, Debug, PartialEq)]
pub struct BannerParts {
    /// Banner Mask, its material the custom depth pass's (context `customdepth`): it draws into custom depth alone.
    pub mask: BannerPiece,
    pub background: BannerPiece,
    pub filter: BannerPiece,
    pub trim: BannerPiece,
    /// Banner Nameplate (the pre-match's; the podium hides it).
    pub nameplate: Option<BannerPiece>,
    pub slant_angle: f64,
    pub entrance_length: f64,
}

impl BannerParts {
    /// A pre-match banner's pieces from its instance and the
    /// level's banner materials; the mask with its custom depth pass's DrawRef (`customDepth.draw`), Slant Angle its
    /// material's Angle (the scene's value, else the material's own from `assets`), the Entrance Timeline 1 long.
    pub fn of_prematch(scene: &PreMatchScene, b: &BannerInstance, assets: &Assets) -> Result<BannerParts> {
        let bad = |why: String| crate::Error::Format { what: format!("prematch banner {}", b.name), why };
        let piece = |name: &str| -> Result<(BannerPiece, &PieceAsset)> {
            let q = b.pieces.get(name).ok_or_else(|| bad(format!("no {name}")))?;
            let key = q.material.as_ref().ok_or_else(|| bad(format!("{name} has no material")))?;
            let material = scene.banner_materials.get(key).ok_or_else(|| bad(format!("no material {key}")))?.clone();
            Ok((BannerPiece { loc: q.loc, rot: q.rot, scale: q.scale, material }, q))
        };
        let (mut mask, q) = piece("Banner Mask")?;
        let depth = q.custom_depth.as_ref().ok_or_else(|| bad("Banner Mask has no custom depth".into()))?;
        mask.material.draw = depth.draw.clone();
        let slant_angle = match mask.material.scalars.get("Angle") {
            Some(a) => *a,
            None => {
                let path = material_ref(&mask.material, None).path;
                let m = assets.material(&path)?;
                let a = m.uniforms.scalars.iter().find(|p| p.name == "Angle");
                a.ok_or_else(|| bad(format!("the mask's material {path} has no Angle")))?.value
            }
        };
        Ok(BannerParts {
            mask,
            background: piece("Banner Default Background")?.0,
            filter: piece("Banner Filter")?.0,
            trim: piece("Banner Trim Mask")?.0,
            nameplate: Some(piece("Banner Nameplate")?.0),
            slant_angle,
            entrance_length: 1.0,
        })
    }
}

/// One of a banner's planes: its material with the values the banner set, and its location from the mask's (world
/// space).
#[derive(Clone, Debug, PartialEq)]
pub struct BannerPieceView {
    pub material: MaterialRef,
    pub offset: V3,
}

/// A banner placed in a level.
#[derive(Debug)]
pub struct BannerView {
    pub parts: BannerParts,
    /// Far to near: background, filter, trim, nameplate (when the asset has it), then the mask's custom depth copy.
    pub pieces: Vec<BannerPieceView>,
    /// Per piece of `pieces`: drawn. All false until `start`; a level may hide a piece after (the pre-match's
    /// showPieces).
    pub shown: Vec<bool>,
    /// The Banner Mask's world location (Starting Location); the level moves it.
    pub start: V3,
    /// The mask's world rotation (x, y, z, w) and scale.
    pub quat: [f64; 4],
    pub scale: V3,
    /// Banner Index: the custom stencil value the pieces compare with.
    pub stencil: i32,
    /// ColorType.
    pub color: u8,
    /// The nameplate's ProfileTexture (a texture object path; None: its material's own).
    pub icon: Option<String>,
    /// The Entrance Timeline's seconds played.
    pub age: f64,
    /// Done after the Entrance Timeline's Finished event.
    pub done: bool,
    /// AnimBannerIn has run.
    pub started: bool,
    /// The mask's height above `start` (the Entrance Timeline's drop).
    pub lift: f64,
}

impl BannerView {
    /// The pieces hidden until `start`, the mask at the world location `loc` with the
    /// banner actor's scale `s`. Initialize Material Data and the entrance's parameters: StencilValue = Banner Index,
    /// ColorType on the background and the trim, Angle = Slant Angle on the mask and the trim, UseTrim on the trim,
    /// StencilValue on the nameplate.
    ///
    /// Custom stencil: the Banner Mask draws into custom depth alone (bRenderInMainPass off, CustomDepthStencilValue =
    /// Banner Index: `custom_depth`); the other pieces read that frame's stencil at their pixel and compare it with
    /// their StencilValue, so a piece shows only where its own mask is the nearest writer. The pieces take the mask's
    /// orientation and scale.
    // Unverified: SetHiddenInGame's propagation showing the Banner Filter is taken from the 4.26 source.
    pub fn new(parts: BannerParts, loc: V3, s: f64, stencil: i32, color: u8) -> BannerView {
        let m = &parts.mask;
        let quat = rotator_quat(m.rot[0], m.rot[1], m.rot[2]);
        let scale = m.scale.map(|x| x * s);
        let set = |pairs: &[(&str, f64)]| -> IndexMap<String, f64> {
            pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
        };
        let (sv, ct, angle) = (f64::from(stencil), f64::from(color), parts.slant_angle);
        let piece = |p: &BannerPiece, pairs: &[(&str, f64)]| BannerPieceView {
            material: material_ref(&p.material, Some(&set(pairs))),
            offset: qrot(quat, mul3(p.loc, scale)),
        };
        let mut pieces = vec![
            piece(&parts.background, &[("StencilValue", sv), ("ColorType", ct)]),
            piece(&parts.filter, &[("StencilValue", sv)]),
            piece(&parts.trim, &[("StencilValue", sv), ("ColorType", ct), ("Angle", angle), ("UseTrim", 1.0)]),
        ];
        if let Some(n) = &parts.nameplate {
            pieces.push(piece(n, &[("StencilValue", sv)]));
        }
        pieces.push(BannerPieceView {
            material: material_ref(&m.material, Some(&set(&[("Angle", angle)]))),
            offset: [0.0; 3],
        });
        let shown = vec![false; pieces.len()];
        BannerView {
            parts,
            pieces,
            shown,
            start: loc,
            quat,
            scale,
            stencil,
            color,
            icon: None,
            age: 0.0,
            done: false,
            started: false,
            lift: 0.0,
        }
    }

    /// The index of the mask's custom depth copy in `pieces`.
    pub fn writer(&self) -> usize {
        self.pieces.len() - 1
    }

    /// AnimBannerIn: the pieces shown and the Entrance Timeline started, once.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.shown.fill(true);
    }

    /// The pieces with the mask `lift` above `start`.
    pub fn place(&mut self, lift: f64) {
        self.lift = lift;
    }

    /// A started banner's Entrance Timeline (rate 10, its length the parts') and
    /// placement; true in the tick it lands (its Finished event: CameraShakeMedium_C and ui_postmatch_banner_show are
    /// the caller's).
    pub fn tick(&mut self, dt: f64) -> bool {
        if !self.started {
            return false;
        }
        let len = self.parts.entrance_length;
        let mut lift = 0.0;
        let mut landed = false;
        if !self.done {
            self.age += dt;
            let pos = (self.age * BANNER_RATE).min(len);
            lift = if len > 0.0 { BANNER_DROP * (1.0 - pos / len) } else { 0.0 };
            if pos >= len {
                self.done = true;
                landed = true;
            }
        }
        self.place(lift);
        landed
    }

    /// A piece's world transform now.
    fn piece_world(&self, k: usize) -> Xf {
        let mut loc = add3(self.start, self.pieces[k].offset);
        loc[2] += self.lift;
        Xf { loc, q: self.quat, s: self.scale }
    }

    /// The shown pieces but the mask's copy, each with TranslucencySortPriority 0
    /// (BP_Banner_PM's components set none).
    pub fn meshes(&self, out: &mut Vec<(MeshPose, i32)>) {
        for k in 0..self.writer() {
            if !self.shown[k] {
                continue;
            }
            let mut material = self.pieces[k].material.clone();
            if k == PIECE_NAMEPLATE
                && let Some(icon) = &self.icon
            {
                match material.textures.iter_mut().find(|(n, _)| &**n == PROFILE_TEXTURE) {
                    Some(t) => t.1 = icon.as_str().into(),
                    None => material.textures.push((PROFILE_TEXTURE.into(), icon.as_str().into())),
                }
            }
            out.push((mesh_pose(PLANE, self.piece_world(k).to_world(), material, 0), 0));
        }
    }

    /// The custom stencil writer, the mask's copy drawn into custom depth alone with
    /// CustomDepthStencilValue the Banner Index; nothing while it is hidden.
    pub fn custom_depth(&self, out: &mut Vec<MeshPose>) {
        let k = self.writer();
        if !self.shown[k] {
            return;
        }
        let stencil = u8::try_from(self.stencil).unwrap_or_else(|_| panic!("banner: Banner Index {}", self.stencil));
        out.push(mesh_pose(PLANE, self.piece_world(k).to_world(), self.pieces[k].material.clone(), stencil));
    }
}

// ---------------------------------------------------------------- the banner element

/// What a banner element is made with (BannerInit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BannerInit {
    pub banner_index: i32,
    pub color_type: u8,
    pub slant_angle: f64,
}

/// A component's attach parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parent {
    /// DefaultSceneRoot: the actor root, at the Base Element Root.
    Root,
    Pivot,
    /// Another component, by its place in `BannerElement::comps`.
    Comp(usize),
}

/// What draws a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Draw {
    None,
    /// A mesh component with its slot 0 material.
    Mesh,
    System(SystemId),
}

/// A component of the element: its record, transforms, instance values and what draws it.
#[derive(Clone, Debug)]
struct Comp {
    /// Its record in the entry's `components`.
    c: usize,
    parent: Parent,
    rel: Xf,
    world: Xf,
    unmasked: bool,
    /// SetHiddenInGame's state.
    hidden: bool,
    /// The dynamic instance's scalars (None: the slot holds its MIC).
    scalars: Option<IndexMap<String, f64>>,
    /// The slot material's scalar defaults, for K2_GetScalarParameterValue.
    defs: IndexMap<String, f64>,
    /// The slot material's base is a MaskedLayers one.
    layered: bool,
    draw: Draw,
}

/// A player's banner: the `banner` kind's entry for its BannerData slug, its components under the banner's Base Element
/// Root, its particle systems on the menus' player and its class's BeginPlay
/// (BannerElement).
///
/// Order: construction (the class's SCS components and UCS arrays: the entry's components / masked / unmasked /
/// mirrored); BeginPlay (the classes' own graphs, classes.rs); Initialize Banner Element, the old or the new path; Get
/// Parallax Parameters (Parallax Origin = Parallax Pivot, the strengths and LookAt Offset the class defaults).
///
/// Spaces: UE world (cm). The element's actor root is at the Base Element Root's world transform (spawned there and
/// attached with KeepWorld: its relative transform is identity) and follows it at each `place`.
#[derive(Debug)]
pub struct BannerElement {
    pub slug: String,
    pub init: BannerInit,
    /// Show / Hide Banner's last call.
    pub shown: bool,
    /// HideDefaultBorder: the owner's Banner Trim Mask is hidden (the level reads it after `new`).
    pub hide_trim: bool,
    data: BannerEntry,
    /// Parents first.
    comps: Vec<Comp>,
    by_name: IndexMap<String, usize>,
    root_w: Xf,
    /// Parallax Pivot (Parallax Origin): relative to the actor root, and its world transform.
    pivot_rel: Xf,
    pivot_w: Xf,
    /// The camera's location at the spawn (CounterPerspective's).
    camera_loc: V3,
    /// The parallax asked since the last `place`: the cursor and the camera's location.
    look: Option<([f64; 2], V3)>,
    released: bool,
}

impl BannerElement {
    /// The element of the banner
    /// `slug` with its actor root at `root` (the Base Element Root's world location, rotator and scale) and the camera
    /// at `camera_loc`. None (with a warning, the scene going on) for a banner kind, index
    /// or entry that is missing or unreadable, or a slug the index does not have. The entry is read at once, as the
    /// build's LoadAsset_Blocking (BAN @3575-3956, @10704); the index is parsed once per store (`cached_index`).
    pub fn new(
        cx: &mut LevelCx<'_>,
        slug: &str,
        init: BannerInit,
        root: [V3; 3],
        camera_loc: V3,
    ) -> Result<Option<BannerElement>> {
        let store = cx.assets.store();
        let index = match cached_index(store) {
            Ok(i) => i,
            Err(e) => {
                warn_once(format!("banners: no banner index ({e}): the default banners stay"));
                return Ok(None);
            }
        };
        let data = match entry::load_banner(store, &index, slug) {
            Ok(Some(d)) => d,
            Ok(None) => {
                warn_once(format!("banners: no banner {slug} in the banner index"));
                return Ok(None);
            }
            Err(e) => {
                warn_once(format!("banners: banner {slug} unreadable ({e}): the default banner stays"));
                return Ok(None);
            }
        };
        let assets = cx.assets;
        let mut mesh_ok = |path: &str| assets.mesh(path).is_ok();
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let mut rng = mvs_vfx::RandomStream::new(seed);
        Ok(Some(BannerElement::of_entry(data, init, root, camera_loc, cx.vfx.as_deref_mut(), &mut mesh_ok, &mut rng)))
    }

    /// The constructor on an entry already read: `mesh_ok` tells whether the mesh kind holds
    /// a mesh, `rng` is RandomFloat's.
    pub fn of_entry(
        data: BannerEntry,
        init: BannerInit,
        root: [V3; 3],
        camera_loc: V3,
        vfx: Option<&mut mvs_vfx::Player>,
        mesh_ok: &mut dyn FnMut(&str) -> bool,
        rng: &mut dyn mvs_vfx::RandomSource,
    ) -> BannerElement {
        let slug = data.slug.clone();
        let p = &data.pivot;
        let pivot_rel = Xf::of([p.loc, p.rot, p.scale]);
        let unmasked: IndexSet<&str> = data.unmasked.iter().map(String::as_str).collect();
        let mut names: IndexMap<&str, usize> = IndexMap::new();
        for (k, c) in data.components.iter().enumerate() {
            if names.insert(c.name.as_str(), k).is_some() {
                panic!("banner {slug}: component {} twice", c.name);
            }
        }
        // each record's parent record (None: the root or the pivot)
        let mut parent_rec: Vec<Option<usize>> = Vec::with_capacity(data.components.len());
        for c in &data.components {
            parent_rec.push(match c.parent.as_str() {
                "DefaultSceneRoot" | "Parallax Pivot" => None,
                pn => Some(
                    *names.get(pn).unwrap_or_else(|| panic!("banner {slug}: {}'s parent {pn} is no component", c.name)),
                ),
            });
        }
        // parents first
        let mut order: Vec<usize> = Vec::with_capacity(parent_rec.len());
        let mut placed = vec![false; parent_rec.len()];
        for k in 0..parent_rec.len() {
            let mut chain = vec![k];
            let mut at = k;
            while let Some(p) = parent_rec[at] {
                if placed[p] {
                    break;
                }
                if chain.contains(&p) {
                    panic!("banner {slug}: attachment cycle at {}", data.components[p].name);
                }
                chain.push(p);
                at = p;
            }
            for &r in chain.iter().rev() {
                if !placed[r] {
                    placed[r] = true;
                    order.push(r);
                }
            }
        }
        let at_of: IndexMap<usize, usize> = order.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let comps: Vec<Comp> = order
            .iter()
            .map(|&k| {
                let c = &data.components[k];
                let key = c.materials.first();
                let defs = key
                    .and_then(|r| data.materials.get(r))
                    .map(|m| m.scalars.iter().map(|s| (s.name.clone(), s.def)).collect())
                    .unwrap_or_default();
                let parent = match (parent_rec[k], c.parent.as_str()) {
                    (Some(p), _) => Parent::Comp(at_of[&p]),
                    (None, "Parallax Pivot") => Parent::Pivot,
                    (None, _) => Parent::Root,
                };
                Comp {
                    c: k,
                    parent,
                    rel: Xf::of([c.loc, c.rot, c.scale]),
                    world: Xf::IDENTITY,
                    unmasked: unmasked.contains(c.name.as_str()),
                    hidden: c.hidden_in_game,
                    scalars: None,
                    defs,
                    layered: key.is_some_and(|r| data.masked_layers.contains_key(r)),
                    draw: Draw::None,
                }
            })
            .collect();
        let by_name: IndexMap<String, usize> =
            order.iter().enumerate().map(|(i, &k)| (data.components[k].name.clone(), i)).collect();
        drop(names);
        drop(unmasked);
        let mut el = BannerElement {
            slug,
            init,
            shown: false,
            hide_trim: false,
            data,
            comps,
            by_name,
            root_w: Xf::of(root),
            pivot_rel,
            pivot_w: Xf::IDENTITY,
            camera_loc,
            look: None,
            released: false,
        };
        // the UCS arrays name components
        for n in el.data.masked.iter().chain(&el.data.unmasked).chain(&el.data.mirrored) {
            el.comp(n);
        }
        el.relayout();
        // BeginPlay: the classes' own graphs
        classes::begin_play(&mut el, rng);
        // Initialize Banner Element
        if el.data.defaults.old_initialize {
            el.init_old();
        } else {
            el.init_new();
        }
        el.relayout();
        el.build(vfx, mesh_ok);
        el
    }

    // ---- what the classes' graphs call

    /// SetScalarParameterValue on the component's dynamic instance (made when the slot has
    /// none). One instance per component: a mesh has one slot (the engine Plane's one section), a particle component
    /// one per emitter, all given the same values (its system's material scalars).
    pub fn set_scalar(&mut self, name: &str, param: &str, value: f64) {
        let k = self.comp(name);
        self.comps[k].scalars.get_or_insert_with(IndexMap::new).insert(param.to_string(), value);
        if let Draw::System(_) = self.comps[k].draw {
            warn_once(format!("banner {}: {name} is a running system; its scalars are set at spawn", self.slug));
        }
    }

    /// The component's slot 0 material (a DrawRef).
    pub fn material_of(&self, name: &str) -> Option<String> {
        self.data.components[self.comps[self.comp(name)].c].materials.first().cloned()
    }

    /// SetWorldScale3D, the relative scale that gives this world scale under the parent's.
    pub fn set_world_scale(&mut self, name: &str, s: V3) {
        let k = self.comp(name);
        let pw = self.world_of_parent(k);
        self.comps[k].rel.s = [s[0] / pw.s[0], s[1] / pw.s[1], s[2] / pw.s[2]];
        self.relayout();
    }

    // ---- the level's calls

    /// Show / Hide Banner: SetHiddenInGame on the Unmasked components only. A component
    /// draws when its SCS bVisible is set and it is not hidden in game.
    pub fn show(&mut self, on: bool) {
        self.shown = on;
        for comp in &mut self.comps {
            if comp.unmasked {
                comp.hidden = !on;
            }
        }
    }

    /// BAN's tick parallax, applied at the next `place` with that root. `cursor` is
    /// UI_Cursor's MousePosNormalized (0..1), `camera_loc` the target camera's location. c = cursor - 0.5; d =
    /// Parallax Origin - camera; target = (2000, c.x (-StrengthH) + d.Y LookAtOffset, c.y (-StrengthV)), a world point;
    /// K2_LookAt(origin, target, aim (1, 0, 0)) = FindBetweenNormals((1, 0, 0), normalize(target - location)), set as
    /// the origin's world rotation. Only Parallax Pivot turns; its children follow.
    pub fn parallax(&mut self, cursor: [f64; 2], camera_loc: V3) {
        if !self.released {
            self.look = Some((cursor, camera_loc));
        }
    }

    fn apply_look(&mut self, cursor: [f64; 2], camera_loc: V3) {
        let d = &self.data.defaults;
        self.pivot_w = self.root_w.compose(&self.pivot_rel);
        let (cx, cy) = (cursor[0] - 0.5, cursor[1] - 0.5);
        let dy = self.pivot_w.loc[1] - camera_loc[1];
        let l = self.pivot_w.loc;
        let dir = [2000.0 - l[0], cx * -d.strength_h + dy * d.horizontal_offset - l[1], cy * -d.strength_v - l[2]];
        let n = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if n > 0.0 {
            self.pivot_w.q = find_between_normals([1.0, 0.0, 0.0], dir.map(|x| x / n));
            // K2_SetWorldTransform: the rotation relative to the actor root
            self.pivot_rel.q = qmul(qinv(self.root_w.q), self.pivot_w.q);
        }
    }

    /// The actor root at `root` (the Base Element Root's
    /// world location, rotator and scale), the parallax asked for applied, every component's world transform; the
    /// shown meshes into `out` with their TranslucencySortPriority, the systems moved on the menus' player and shown
    /// or hidden.
    pub fn place(&mut self, cx: &mut LevelCx<'_>, root: [V3; 3], out: &mut Vec<(MeshPose, i32)>) {
        if self.released {
            // (systems a release without a player kept)
            self.release(cx);
            return;
        }
        self.place_at(root, out);
        if let Some(vfx) = cx.vfx.as_deref_mut() {
            self.place_systems(vfx);
        }
    }

    /// `place` without the systems: the actor root at `root`, the parallax asked for applied, the shown meshes into
    /// `out`.
    pub fn place_at(&mut self, root: [V3; 3], out: &mut Vec<(MeshPose, i32)>) {
        if self.released {
            return;
        }
        self.root_w = Xf::of(root);
        if let Some((cursor, cam)) = self.look.take() {
            self.apply_look(cursor, cam);
        }
        self.relayout();
        self.meshes(out);
    }

    /// The shown mesh components into `out`, each with its TranslucencySortPriority.
    pub fn meshes(&self, out: &mut Vec<(MeshPose, i32)>) {
        for comp in &self.comps {
            let c = &self.data.components[comp.c];
            if comp.draw != Draw::Mesh || !c.visible || comp.hidden {
                continue;
            }
            let key = &c.materials[0];
            let draw = mvs_material::draw::parse_draw_ref(key)
                .unwrap_or_else(|| panic!("banner {}: {key} is not a DrawRef", self.slug));
            let material = MaterialRef {
                path: draw.material.into(),
                scalars: comp.scalars.iter().flatten().map(|(k, v)| (Arc::from(k.as_str()), *v as f32)).collect(),
                ..MaterialRef::default()
            };
            let mesh = c.mesh.as_deref().expect("a drawn mesh component has a mesh (build)");
            out.push((mesh_pose(mesh, comp.world.to_world(), material, 0), c.sort_priority));
        }
    }

    /// The systems moved to their components and shown or hidden.
    pub fn place_systems(&self, vfx: &mut mvs_vfx::Player) {
        for comp in &self.comps {
            if let Draw::System(id) = comp.draw {
                let c = &self.data.components[comp.c];
                vfx.attach(id, &attachment(&comp.world));
                vfx.set_visible(id, c.visible && !comp.hidden);
            }
        }
    }

    /// Its systems killed; it draws nothing after.
    /// A release with no effects player (`cx.vfx` None) keeps the systems' handles: the next `release` or `place`
    /// with a player kills them, so a level calls one of those again before it drops the element.
    pub fn release(&mut self, cx: &mut LevelCx<'_>) {
        self.released = true;
        let mut kept = false;
        for comp in &mut self.comps {
            match comp.draw {
                Draw::System(id) => match cx.vfx.as_deref_mut() {
                    Some(vfx) => {
                        vfx.kill(id);
                        comp.draw = Draw::None;
                    }
                    None => kept = true,
                },
                _ => comp.draw = Draw::None,
            }
        }
        if kept {
            warn_once(format!("banner {}: released with no effects player; its systems wait for one", self.slug));
        }
    }

    /// Released with systems still to kill (a release had no effects player).
    pub fn releasing(&self) -> bool {
        self.released && self.comps.iter().any(|c| matches!(c.draw, Draw::System(_)))
    }

    // ---- Initialize Banner Element

    /// The old path (OldInitialize): each Masked component an instance with StencilValue =
    /// BannerIndex; CounterPerspective; SlantAngle < 0.5: each Mirrored component's world scale x (MirrorAxis: -1, else
    /// 1, per axis) and its relative location x the MirrorOffsetAxis signs; HideDefaultBorder: the owner's Banner Trim
    /// Mask hidden.
    fn init_old(&mut self) {
        let BannerInit { banner_index, slant_angle, .. } = self.init;
        for n in self.data.masked.clone() {
            let k = self.comp(&n);
            if self.slots(k) > 0 {
                self.comps[k]
                    .scalars
                    .get_or_insert_with(IndexMap::new)
                    .insert("StencilValue".into(), f64::from(banner_index));
            }
        }
        self.counter_perspective();
        if slant_angle < 0.5 {
            for n in self.data.mirrored.clone() {
                let k = self.comp(&n);
                self.flip(k);
            }
        }
        if self.data.defaults.hide_default_border {
            self.hide_trim = true;
        }
    }

    /// The new path (InitializeBanner): each Mirrored component, per slot: a new instance of
    /// slot 0's material with its overrides copied; SlantAngle < 0.5: a MaskedLayers material -> Mirror Banner, else
    /// the old path's scale and offset flip (once per slot: the instance is dropped); SlantAngle >= 0.5: Reset Mirror
    /// Axis. Each Masked component, per slot: the slot's instance, CopyParameterOverrides from itself, StencilValue =
    /// BannerIndex; SlantAngle < 0.5 and a MaskedLayers material -> Mirror Banner; SlantAngle >= 0.5 -> Reset Mirror
    /// Axis; then CounterPerspective and HideDefaultBorder.
    // Unverified: CopyParameterOverrides(self) is taken to clear the instance's overrides first (4.26 source: it
    // clears, then copies the source's arrays, here its own emptied ones), so a component in both arrays is mirrored
    // once, by the Masked loop. Native, not read.
    fn init_new(&mut self) {
        let BannerInit { banner_index, slant_angle, .. } = self.init;
        for n in self.data.mirrored.clone() {
            let k = self.comp(&n);
            for _ in 0..self.slots(k) {
                let mut mid = self.comps[k].scalars.clone().unwrap_or_default();
                if slant_angle < 0.5 {
                    if self.layered(k) {
                        self.mirror_banner(k, &mut mid);
                        self.comps[k].scalars = Some(mid);
                    } else {
                        self.flip(k);
                    }
                } else {
                    self.reset_mirror_axis(k, &mut mid);
                    self.comps[k].scalars = Some(mid);
                }
            }
        }
        for n in self.data.masked.clone() {
            let k = self.comp(&n);
            if self.slots(k) == 0 {
                continue;
            }
            let mut mid: IndexMap<String, f64> = IndexMap::new();
            mid.insert("StencilValue".into(), f64::from(banner_index));
            if slant_angle < 0.5 {
                if self.layered(k) {
                    self.mirror_banner(k, &mut mid);
                }
            } else {
                self.reset_mirror_axis(k, &mut mid);
            }
            self.comps[k].scalars = Some(mid);
        }
        self.counter_perspective();
        if self.data.defaults.hide_default_border {
            self.hide_trim = true;
        }
    }

    /// GetNumMaterials: a mesh's sections (one), a particle component's emitters.
    // Unverified: a particle component's count is taken as its template's emitters (4.26 source), here the ones the
    // entry kept.
    fn slots(&self, k: usize) -> u32 {
        let c = &self.data.components[self.comps[k].c];
        match c.kind {
            ComponentKind::Mesh => 1,
            ComponentKind::Psc => c.system.as_ref().and_then(|s| self.data.systems.get(s)).map_or(0, |s| s.emitters),
        }
    }

    /// GetBaseMaterial is a MaskedLayers one (a particle component's emitter materials are not).
    fn layered(&self, k: usize) -> bool {
        self.data.components[self.comps[k].c].kind == ComponentKind::Mesh && self.comps[k].layered
    }

    /// The old path's mirror (and the new path's for a material that is not a MaskedLayers one).
    fn flip(&mut self, k: usize) {
        let (a, o) = (self.data.defaults.mirror_axis, self.data.defaults.mirror_offset_axis);
        let sign = |on: bool| if on { -1.0 } else { 1.0 };
        let rel = &mut self.comps[k].rel;
        for i in 0..3 {
            rel.s[i] *= sign(a[i]);
            rel.loc[i] *= sign(o[i]);
        }
    }

    /// Mirror Banner on the instance, layers 0..9: Mirror X < 0 and Flipbook Number of
    /// Rows above 0: OffsetX += UScale, UScale *= MirrorX, Alpha OffsetX += Alpha UScale, Alpha UScale *= MirrorX;
    /// Mirror Y < 0 and Flipbook Number of Columns above 0: the same with VScale / OffsetY. A plain layer: UScale *=
    /// MirrorX, then OffsetX by the "Layer n" texture's AddressX (Wrap: OffsetX x MirrorX; Clamp: OffsetX minus the
    /// new UScale; Mirror: kept); Y: VScale *= MirrorY, then OffsetY by AddressY (Wrap, Mirror: OffsetY x MirrorY;
    /// Clamp: OffsetY x MirrorY plus one). A texture the entry lacks is a failed cast: address 0.
    fn mirror_banner(&self, k: usize, mid: &mut IndexMap<String, f64>) {
        let defs = &self.comps[k].defs;
        let get = |mid: &IndexMap<String, f64>, n: &str| mid.get(n).or_else(|| defs.get(n)).copied().unwrap_or(0.0);
        for l in 0..=9 {
            let p = |n: &str| format!("Layer {l} {n}");
            let mx = get(mid, &p("Mirror X"));
            if mx < 0.0 {
                if get(mid, &p("Flipbook Number of Rows")) > 0.0 {
                    let (u, au) = (get(mid, &p("UScale")), get(mid, &p("Alpha UScale")));
                    let ox = get(mid, &p("OffsetX")) + u;
                    let aox = get(mid, &p("Alpha OffsetX")) + au;
                    mid.insert(p("OffsetX"), ox);
                    mid.insert(p("UScale"), u * mx);
                    mid.insert(p("Alpha OffsetX"), aox);
                    mid.insert(p("Alpha UScale"), au * mx);
                } else {
                    let u = get(mid, &p("UScale")) * mx;
                    mid.insert(p("UScale"), u);
                    let ox = get(mid, &p("OffsetX"));
                    let ax = self.layer_texture(k, l).map_or(0, |t| t.address_x);
                    mid.insert(
                        p("OffsetX"),
                        match ax {
                            0 => ox * mx,
                            1 => ox - u,
                            _ => ox,
                        },
                    );
                }
            }
            let my = get(mid, &p("Mirror Y"));
            if my < 0.0 {
                if get(mid, &p("Flipbook Number of Columns")) > 0.0 {
                    let (v, av) = (get(mid, &p("VScale")), get(mid, &p("Alpha VScale")));
                    let oy = get(mid, &p("OffsetY")) + v;
                    let aoy = get(mid, &p("Alpha OffsetY")) + av;
                    mid.insert(p("OffsetY"), oy);
                    mid.insert(p("VScale"), v * my);
                    mid.insert(p("Alpha OffsetY"), aoy);
                    mid.insert(p("Alpha VScale"), av * my);
                } else {
                    let vs = get(mid, &p("VScale")) * my;
                    mid.insert(p("VScale"), vs);
                    let oy = get(mid, &p("OffsetY"));
                    let ay = self.layer_texture(k, l).map_or(0, |t| t.address_y);
                    mid.insert(p("OffsetY"), if ay == 1 { oy * my + 1.0 } else { oy * my });
                }
            }
        }
    }

    /// K2_GetTextureParameterValue("Layer n") cast to Texture2D: the entry's record.
    fn layer_texture(&self, k: usize, l: u32) -> Option<&entry::LayerTexture> {
        let key = self.data.components[self.comps[k].c].materials.first()?;
        self.data.masked_layers.get(key)?.layers.get(&format!("Layer {l}"))
    }

    /// Reset Mirror Axis: Mirror X / Y = |value| for each layer.
    fn reset_mirror_axis(&self, k: usize, mid: &mut IndexMap<String, f64>) {
        for l in 0..=9 {
            for ax in ["X", "Y"] {
                let n = format!("Layer {l} Mirror {ax}");
                if let Some(v) = mid.get(&n).or_else(|| self.comps[k].defs.get(&n)).copied() {
                    mid.insert(n, v.abs());
                }
            }
        }
    }

    /// CounterPerspective (FakeOrtho only): k = |camera - component| / |camera -
    /// DefaultSceneRoot| per Masked component, relative scale x k, relative Z x (FakeOrthoMoveUp 0.5 (k - 1) + 1),
    /// once, with the camera of the spawn.
    fn counter_perspective(&mut self) {
        let d = &self.data.defaults;
        if !d.fake_ortho {
            return;
        }
        let move_up = d.fake_ortho_move_up;
        let cam = self.camera_loc;
        let to_root = dist(cam, self.root_w.loc);
        if to_root.is_nan() || to_root <= 0.0 {
            return;
        }
        for n in self.data.masked.clone() {
            let i = self.comp(&n);
            let k = dist(cam, self.world_of(i).loc) / to_root;
            let rel = &mut self.comps[i].rel;
            rel.s = rel.s.map(|x| x * k);
            rel.loc[2] *= move_up * 0.5 * (k - 1.0) + 1.0;
        }
    }

    // ---- the draws

    /// What draws each component: its mesh (slot 0's material; a ref with no record
    /// is one the bind refused), or its system spawned attached on the menus' player with the component's
    /// TranslucencySortPriority and instance scalars.
    fn build(&mut self, mut vfx: Option<&mut mvs_vfx::Player>, mesh_ok: &mut dyn FnMut(&str) -> bool) {
        for k in 0..self.comps.len() {
            let c = &self.data.components[self.comps[k].c];
            if !c.visible || c.materials.is_empty() {
                continue;
            }
            match c.kind {
                ComponentKind::Mesh => {
                    if c.materials.len() != 1 {
                        warn_once(format!(
                            "banner {}: {} has {} slots, the first drawn",
                            self.slug,
                            c.name,
                            c.materials.len()
                        ));
                    }
                    if !self.data.materials.contains_key(&c.materials[0]) {
                        continue;
                    }
                    if !c.mesh.as_deref().is_some_and(&mut *mesh_ok) {
                        warn_once(format!("banner {}: {}'s mesh {:?} has no data", self.slug, c.name, c.mesh));
                        continue;
                    }
                    self.comps[k].draw = Draw::Mesh;
                }
                ComponentKind::Psc => {
                    let Some(sys) = c.system.as_ref().and_then(|s| self.data.systems.get(s)) else { continue };
                    let Some(vfx) = vfx.as_deref_mut() else {
                        warn_once(format!("banner {}: no effects player for {}", self.slug, c.name));
                        continue;
                    };
                    let Some(id) = vfx.spawn_attached(&sys.path, &attachment(&self.comps[k].world), 1.0, &[]) else {
                        warn_once(format!("banner {}: {}'s system {} is not loaded", self.slug, c.name, sys.path));
                        continue;
                    };
                    if let Some(s) = vfx.system_mut(id) {
                        s.sort_priority = c.sort_priority;
                        if let Some(sc) = &self.comps[k].scalars {
                            s.material_scalars = sc.iter().map(|(n, v)| (Arc::from(n.as_str()), *v as f32)).collect();
                        }
                    }
                    self.comps[k].draw = Draw::System(id);
                }
            }
        }
    }

    /// Every component's world transform from the actor root and the pivot.
    fn relayout(&mut self) {
        self.pivot_w = self.root_w.compose(&self.pivot_rel);
        for k in 0..self.comps.len() {
            let pw = match self.comps[k].parent {
                Parent::Root => self.root_w,
                Parent::Pivot => self.pivot_w,
                Parent::Comp(p) => self.comps[p].world,
            };
            self.comps[k].world = pw.compose(&self.comps[k].rel);
        }
    }

    /// The parent's world transform now (the init's reads come after its own writes).
    fn world_of_parent(&mut self, k: usize) -> Xf {
        match self.comps[k].parent {
            Parent::Root => self.root_w,
            Parent::Pivot => {
                self.pivot_w = self.root_w.compose(&self.pivot_rel);
                self.pivot_w
            }
            Parent::Comp(p) => self.world_of(p),
        }
    }

    /// K2_GetComponentLocation and the rest: the component's world transform now.
    fn world_of(&mut self, k: usize) -> Xf {
        let w = self.world_of_parent(k).compose(&self.comps[k].rel);
        self.comps[k].world = w;
        w
    }

    fn comp(&self, name: &str) -> usize {
        *self.by_name.get(name).unwrap_or_else(|| panic!("banner {}: no component {name}", self.slug))
    }

    /// The drawn components now: name, world transform (FMatrix3x4) and instance scalars.
    pub fn components(&self) -> impl Iterator<Item = (&str, [f32; 12], Option<&IndexMap<String, f64>>)> + '_ {
        self.comps
            .iter()
            .filter(|c| c.draw != Draw::None)
            .map(|c| (self.data.components[c.c].name.as_str(), c.world.to_world(), c.scalars.as_ref()))
    }

    /// The entry the element was made from.
    pub fn entry(&self) -> &BannerEntry {
        &self.data
    }
}

/// A system's follow: world location, the world X / Y / Z axes as rows, world scale.
fn attachment(w: &Xf) -> Attachment {
    let axis = |i: usize| {
        let mut e = [0.0; 3];
        e[i] = 1.0;
        qrot(w.q, e)
    };
    Attachment { loc: w.loc, rot: [axis(0), axis(1), axis(2)], scale: Some(w.s) }
}

#[cfg(test)]
mod tests;
