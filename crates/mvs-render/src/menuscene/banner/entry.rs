//! The `banner` kind as the renderer reads it (the importer's kinds/banner writes it): one
//! entry per released BannerData, the element actor of its one BannerElements entry as the menus draw it, and the
//! index entry (slug -> entry). JSON.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The kind's name in the store.
pub const KIND: &str = "banner";
/// The entry layout.
pub const FORMAT_VERSION: u32 = 1;
/// The index entry's name below the kind's versioned folder.
const INDEX_NAME: &str = "/Index.Banners";

pub type V3 = [f64; 3];

/// A relative transform: location, rotator (pitch, yaw, roll degrees), scale; UE space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rel {
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
}

/// A transform with its rotation as the quaternion x, y, z, w.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Xform {
    pub loc: V3,
    pub quat: [f64; 4],
    pub scale: V3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    Mesh,
    Psc,
}

/// A component of the element class's SCS trees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BannerComponent {
    /// The SCS variable name.
    pub name: String,
    pub kind: ComponentKind,
    /// `DefaultSceneRoot`, `Parallax Pivot` or another component's name.
    pub parent: String,
    /// Relative, rot as pitch / yaw / roll degrees.
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    /// A mesh component's StaticMesh (the mesh kind's object path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<String>,
    /// Per slot, or a particle component's kept emitters' materials once each: the DrawRefs it draws with. Empty for a
    /// component that never draws. A ref with no record in the entry's `materials` is one the bind refused.
    pub materials: Vec<String>,
    /// A particle component's system: a key of the entry's `systems`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// SCS bVisible (false never draws).
    pub visible: bool,
    /// SCS bHiddenInGame.
    pub hidden_in_game: bool,
    /// TranslucencySortPriority.
    pub sort_priority: i32,
}

/// The element class defaults after the parents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BannerDefaults {
    pub fake_ortho: bool,
    pub fake_ortho_move_up: f64,
    pub old_initialize: bool,
    pub hide_default_border: bool,
    pub strength_h: f64,
    pub strength_v: f64,
    pub horizontal_offset: f64,
    pub mirror_axis: [bool; 3],
    pub mirror_offset_axis: [bool; 3],
}

/// A scalar parameter of a material the bind built, with its value (the drawmat entry's layout).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarDefault {
    pub name: String,
    pub def: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerMaterial {
    pub scalars: Vec<ScalarDefault>,
}

/// A "Layer n" texture parameter of a MaskedLayers material: its texture (None: an import not in the paks) and that
/// Texture2D's AddressX / AddressY (0 Wrap, 1 Clamp, 2 Mirror).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerTexture {
    pub texture: Option<String>,
    pub address_x: u8,
    pub address_y: u8,
}

/// A MaskedLayers material's base and its layers' textures (Mirror Banner's plain-layer branch reads them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskedLayers {
    pub base: String,
    pub layers: IndexMap<String, LayerTexture>,
}

/// A particle component's system: its build object path and the emitters the effects player keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerSystem {
    pub path: String,
    pub emitters: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Banner,
    Index,
}

/// A banner's entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BannerEntry {
    pub kind: EntryKind,
    pub version: u32,
    /// The BannerData's slug (`banner_*`), asset name and build object path.
    pub slug: String,
    pub asset: String,
    pub data: String,
    /// The element ActorClass's name and its Blueprint parents up to BP_Banner_Template, nearest first.
    pub cls: String,
    pub parents: Vec<String>,
    pub defaults: BannerDefaults,
    /// The UCS arrays: MaskedComponents, UnmaskedComponents, MirroredComponents.
    pub masked: Vec<String>,
    pub unmasked: Vec<String>,
    pub mirrored: Vec<String>,
    /// Parallax Pivot's relative transform.
    pub pivot: Rel,
    /// The BannerElements entry's BaseOffset / SlantOffset as cooked (no graph reads either).
    pub base_offset: Xform,
    pub slant_offset: Xform,
    pub components: Vec<BannerComponent>,
    /// Per DrawRef the bind built: its scalar parameters and their values.
    pub materials: IndexMap<String, BannerMaterial>,
    /// Per mesh slot DrawRef whose base is M_Banner_MaskedLayers_Basic / _Advanced.
    pub masked_layers: IndexMap<String, MaskedLayers>,
    /// The particle components' systems by short name.
    pub systems: IndexMap<String, BannerSystem>,
    /// The static meshes its drawn mesh components draw.
    pub meshes: Vec<String>,
    /// The classes on the chain with a graph beyond the UCS (classes.rs entries).
    pub graphs: Vec<String>,
    /// What the bind could not build, each with its reason.
    pub refused: Vec<String>,
}

/// An index row: the entry's path, the BannerData's asset and its element class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexRow {
    pub path: String,
    pub asset: String,
    pub cls: String,
}

/// The index entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerIndex {
    pub kind: EntryKind,
    pub version: u32,
    pub banners: IndexMap<String, IndexRow>,
}

fn bad(what: &str, why: String) -> Error {
    Error::Format { what: what.to_string(), why }
}

fn parse<T: for<'de> Deserialize<'de>>(bytes: &[u8], path: &str, want: EntryKind) -> Result<T> {
    #[derive(Deserialize)]
    struct Head {
        kind: EntryKind,
        version: u32,
    }
    let h: Head = serde_json::from_slice(bytes).map_err(|e| bad(path, e.to_string()))?;
    if h.version != FORMAT_VERSION {
        return Err(bad(path, format!("entry version {}, the reader's {FORMAT_VERSION}", h.version)));
    }
    if h.kind != want {
        return Err(bad(path, format!("a {:?} entry, not {want:?}", h.kind)));
    }
    serde_json::from_slice(bytes).map_err(|e| bad(path, e.to_string()))
}

/// The index entry's path: the kind's one entry named `Index.Banners`.
pub fn index_path(store: &mvs_store::Store) -> Result<String> {
    let pack = store.kind(KIND)?;
    let mut found = pack.paths().filter(|p| p.ends_with(INDEX_NAME));
    let Some(path) = found.next().map(str::to_string) else {
        return Err(bad(KIND, format!("no {INDEX_NAME} entry")));
    };
    if let Some(other) = found.next() {
        return Err(bad(KIND, format!("two index entries: {path}, {other}")));
    }
    Ok(path)
}

/// The index entry.
pub fn load_index(store: &mvs_store::Store) -> Result<BannerIndex> {
    let path = index_path(store)?;
    parse(&store.kind(KIND)?.bytes(&path)?, &path, EntryKind::Index)
}

/// A banner's entry by its slug; None for a slug the index does not have.
pub fn load_banner(store: &mvs_store::Store, index: &BannerIndex, slug: &str) -> Result<Option<BannerEntry>> {
    let Some(row) = index.banners.get(slug) else { return Ok(None) };
    let pack = store.kind(KIND)?;
    let e: BannerEntry = parse(&pack.bytes(&row.path)?, &row.path, EntryKind::Banner)?;
    if e.slug != slug {
        return Err(bad(&row.path, format!("the entry of {} under the index's {slug}", e.slug)));
    }
    Ok(Some(e))
}
