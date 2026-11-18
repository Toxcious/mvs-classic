//! The scenery parts' registry: what a map's own Blueprint classes and level Blueprints add to its scenery entry, one
//! part per map ([`super::part::SceneryPart`]), entered by class name as the characters' classes are. No default.
//!
//! A placed Blueprint actor (or a level Blueprint) whose class is in no table is listed by the core, one line per
//! class; one whose components the core would draw fails there as any unknown owner does.

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};

use super::part::{JsonProps, SceneryBindCtx, SceneryPart};
use crate::kind::KindError;

pub mod batcave;
pub mod cromulons;
pub mod scoobys_haunted_mansion;
pub mod sky_arena;
pub mod the_court;
pub mod throne_room;
pub mod tree_fort;
pub mod trophys_edge;
pub mod util;

/// The parts, in the order their entries are written.
pub static SCENERY_PARTS: [&dyn SceneryPart; 8] = [
    &sky_arena::SkyArena,
    &trophys_edge::TrophysEdge,
    &tree_fort::TreeFort,
    &cromulons::Cromulons,
    &scoobys_haunted_mansion::ScoobysHauntedMansion,
    &batcave::Batcave,
    &throne_room::ThroneRoom,
    &the_court::TheCourt,
];

/// A Blueprint component of no one map on a drawn StaticMeshActor (or a part's Blueprint actor): (the component's
/// level properties) -> the instance's fields. A component class not here is read by nobody (Comp_EdgeTrace_C on a
/// floor is the stage data's).
pub type SceneryComponent = for<'a> fn(&JsonProps, &dyn SceneryBindCtx<'a>) -> Result<JsonProps, KindError>;

/// Classes of no one map that the scenery needs nothing of.
const SHARED_IGNORE: [(&str, &str); 3] = [
    // LowSpecEmitterDestroyer: destroys the level's emitters at BeginPlay with the visual effect quality at 1 or
    // lower or a mobile feature level: neither here.
    (
        "LowSpecEmitterDestroyer_C",
        "destroys the emitters only at a visual effect quality of 1 or lower or a mobile feature level: neither here",
    ),
    // LowSpecMeshDestroyer @661..783: destroys its listed actors (Trophy's foliage and two clouds) only at a visual
    // effect quality of 1 or lower or a mobile feature level; MobileVFXDisabler @35..207 deactivates its listed
    // particle components on the same test: neither here.
    (
        "LowSpecMeshDestroyer_C",
        "destroys its listed meshes only at a visual effect quality of 1 or lower or a mobile feature level: neither here",
    ),
    (
        "MobileVFXDisabler_C",
        "deactivates its listed particle components only at a visual effect quality of 1 or lower or a mobile feature level: neither here",
    ),
];

/// The tables the core dispatches by.
pub struct SceneryRegistry {
    /// Placed Blueprint actor class (with _C) -> its part.
    pub blueprint: IndexMap<&'static str, &'static dyn SceneryPart>,
    /// Level Blueprint class -> its part.
    pub level: IndexMap<&'static str, &'static dyn SceneryPart>,
    /// Class -> why the scenery needs nothing of it.
    pub ignore: IndexMap<&'static str, &'static str>,
    /// AnimBlueprint class of a placed SkeletalMeshActor -> its part.
    pub anim_bp: IndexMap<&'static str, &'static dyn SceneryPart>,
    pub level_tags: IndexSet<&'static str>,
    pub keep_params: IndexSet<&'static str>,
    pub gameplay_particles: IndexSet<&'static str>,
    pub wpo: IndexSet<&'static str>,
    /// Blueprint component class (with _C) of a StaticMeshActor -> the instance's fields.
    pub components: IndexMap<&'static str, SceneryComponent>,
}

fn parallax(p: &JsonProps, ctx: &dyn SceneryBindCtx<'_>) -> Result<JsonProps, KindError> {
    util::parallax_of(p, ctx)
}

/// The registry of `parts`; a class entered twice fails.
pub fn scenery_registry(parts: &[&'static dyn SceneryPart]) -> Result<SceneryRegistry, String> {
    let mut r = SceneryRegistry {
        blueprint: IndexMap::new(),
        level: IndexMap::new(),
        ignore: IndexMap::new(),
        anim_bp: IndexMap::new(),
        level_tags: IndexSet::new(),
        keep_params: IndexSet::new(),
        gameplay_particles: IndexSet::new(),
        wpo: IndexSet::new(),
        components: IndexMap::new(),
    };
    let twice = |r: &SceneryRegistry, kind: &str, name: &str| -> Result<(), String> {
        if r.blueprint.contains_key(name) || r.level.contains_key(name) || r.ignore.contains_key(name) {
            return Err(format!("scenery parts: {kind} {name} is entered twice"));
        }
        Ok(())
    };
    for (k, v) in SHARED_IGNORE {
        twice(&r, "ignored class", k)?;
        r.ignore.insert(k, v);
    }
    for &part in parts {
        for &c in part.blueprints() {
            twice(&r, "Blueprint class", c)?;
            r.blueprint.insert(c, part);
        }
        for &c in part.levels() {
            twice(&r, "level Blueprint", c)?;
            r.level.insert(c, part);
        }
        for &(c, why) in part.ignore() {
            twice(&r, "ignored class", c)?;
            r.ignore.insert(c, why);
        }
        for &c in part.anim_blueprints() {
            if r.anim_bp.insert(c, part).is_some() {
                return Err(format!("scenery parts: AnimBlueprint {c} is entered twice"));
            }
        }
        r.level_tags.extend(part.level_tags());
        r.keep_params.extend(part.keep_params());
        for &k in part.wpo() {
            if !r.wpo.insert(k) {
                return Err(format!("scenery parts: wpo {k} is entered twice"));
            }
        }
        for &c in part.gameplay_particles() {
            if !r.blueprint.get(c).is_some_and(|p| std::ptr::addr_eq(*p, part)) {
                return Err(format!("scenery parts: gameplayParticles {c} is not its part's Blueprint class"));
            }
            r.gameplay_particles.insert(c);
        }
    }
    r.components.insert("Comp_Parallax_C", parallax);
    Ok(r)
}

/// The registry of [`SCENERY_PARTS`] (`SCENERY_REGISTRY`).
pub fn registry() -> &'static SceneryRegistry {
    static R: OnceLock<SceneryRegistry> = OnceLock::new();
    R.get_or_init(|| scenery_registry(&SCENERY_PARTS).unwrap_or_else(|e| panic!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tables() {
        let r = registry();
        assert_eq!(r.blueprint.len(), 45);
        assert_eq!(r.level["Map_Batcave_C"].name(), "batcave");
        assert_eq!(r.ignore.len(), 3 + 4 + 4 + 1);
        assert!(r.level_tags.contains("Floating"));
        assert_eq!(r.gameplay_particles.len(), 2);
        assert!(r.components.contains_key("Comp_Parallax_C"));
        // a class entered twice fails
        assert!(scenery_registry(&[&batcave::Batcave, &batcave::Batcave]).is_err());
    }
}
