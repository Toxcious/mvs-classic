//! The `particles` kind: an entry per ParticleSystem the game spawns (the effects' table, the stickers', and the
//! systems the namer kinds' entries name: the scenery's, the banners', the menu scene's), and the index entry (per
//! system, material, mesh and decal its owners), JSON.
//!
//! The index is not gathered from the system entries' notes; each system's bound value is memoized per run
//! ([`system::system`]) and the index item calls it for its systems. A system the banner kind names takes its sprite
//! emitters' context from the banner's choice (`banner::stencil::banner_ref`).
//!
//! Reads the fighters' data and the items part from the store's gamedata kind (`game::chars`), and the namers' systems
//! from the store's scenery, banner and menuscene kinds; the systems and meshes in-process (`Cx::object`,
//! `mesh::header`).

pub mod owners;
pub mod system;
pub mod systems;
pub mod tables;

use indexmap::{IndexMap, IndexSet};
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::game::{
    BANNER, GAMEDATA, MENUSCENE, PARTICLES_KIND_VERSION, SCENERY, particles_dir, particles_index_path, particles_path,
    particles_system,
};
use crate::json;
use crate::kind::{Cx, Item, Kind, KindError};

/// The entry layout.
pub const PARTICLES_FORMAT_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(untagged)]
enum Materials {
    List(Vec<String>),
    Map(IndexMap<String, IgnoredAny>),
}

#[derive(Deserialize)]
struct Decal {
    #[serde(rename = "ref")]
    ref_: String,
}

#[derive(Deserialize)]
struct Named {
    kind: String,
    materials: Option<Materials>,
    decals: Option<IndexMap<String, Decal>>,
}

/// The DrawRefs an entry names (a system's materials; the index's materials and
/// decals), what the material kind's list reads.
pub fn draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let v: Named = serde_json::from_slice(bytes).map_err(|e| KindError::Failed(format!("an entry: {e}")))?;
    match (v.kind.as_str(), v.materials) {
        ("system", Some(Materials::List(l))) => out.extend(l),
        ("system", _) => return Err(KindError::Failed("a system without a materials list".into())),
        (_, m) => {
            if let Some(Materials::Map(m)) = m {
                out.extend(m.into_keys());
            }
            out.extend(v.decals.into_iter().flat_map(IndexMap::into_values).map(|d| d.ref_));
        }
    }
    Ok(())
}

pub struct Particles;

impl Kind for Particles {
    fn id(&self) -> &'static str {
        "particles"
    }

    fn version(&self) -> u64 {
        PARTICLES_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// The index's systems, then the namers' it does not hold, then the index.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let l = owners::listed(cx)?;
        Ok(l.own
            .paths
            .iter()
            .chain(&l.named)
            .map(|p| Item::new(particles_path(p)))
            .chain([Item::new(particles_index_path())])
            .collect())
    }

    /// A system's entry (the run's one bind of it), or the index over the kind's own systems.
    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        if item.path == particles_index_path() {
            let l = owners::listed(cx)?;
            let systems = owners::index_systems(cx, &l.own)?;
            return Ok(json::to_vec(&owners::bind_index(cx, &systems, &l.own.stickers)?)?);
        }
        let Some(path) = particles_system(&item.path) else {
            return Err(KindError::Failed(format!(
                "particles: {} is not a system entry of this bind ({}<system>)",
                item.path,
                particles_dir()
            )));
        };
        Ok(json::to_vec(&*system::system(cx, path)?)?)
    }

    /// The gamedata kind (the fighters' data, the items part) and the namers (scenery, banner, menuscene) at the
    /// versions the game reads.
    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA, SCENERY, BANNER, MENUSCENE]
    }
}
