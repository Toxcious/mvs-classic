//! The DrawRefs the namer kinds' entries name (each kind's
//! `GameKind.drawRefs`): the fighter kind's skin slots and actors, the stage kind's Items entry, the particles,
//! scenery, ui, banner and menuscene entries, each read from the store (`Cx::store`) in the namers' order, then path
//! order. Each kind's reader of its own entries is that kind's `draw_refs`; the stage kind's, which the importer does
//! not build, is here.

use indexmap::{IndexMap, IndexSet};
use mvs_material::draw::parse_draw_ref;
use serde::Deserialize;

use crate::game;
use crate::kind::{Cx, KindError};
use crate::kinds::{banner, fighter, menuscene, particles, scenery, ui};

/// A list of refs, a slot with none null.
type Slots = Vec<Option<String>>;

#[derive(Deserialize)]
struct Actor {
    materials: Slots,
    variants: Option<IndexMap<String, Slots>>,
}

#[derive(Deserialize)]
struct Items {
    draws: Option<IndexMap<String, Actor>>,
}

/// What a namer's reader does with one entry's bytes.
type EntryFn = fn(&[u8], &mut IndexSet<String>) -> Result<(), KindError>;

/// The stage kind's Items entry's draws; the stage's other
/// entries name none.
fn items_draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let e: Items = serde_json::from_slice(bytes).map_err(|e| KindError::Failed(format!("an entry: {e}")))?;
    // (a null slot names nothing)
    for d in e.draws.iter().flat_map(IndexMap::values) {
        out.extend(d.materials.iter().flatten().cloned());
        for l in d.variants.iter().flat_map(IndexMap::values) {
            out.extend(l.iter().flatten().cloned());
        }
    }
    Ok(())
}

/// The DrawRefs the namers' entries name, each once, in the order met (`namedDrawRefs`: the namers in the order the
/// material kind takes them, each one's entries in path order).
pub fn named_draw_refs(cx: &Cx<'_>) -> Result<IndexSet<String>, KindError> {
    let mut out = IndexSet::new();
    let items = game::stage_items_path();
    let namers: [(&str, EntryFn); 7] = [
        (game::FIGHTER.0, fighter::draw_refs),
        (game::STAGE.0, items_draw_refs),
        (game::PARTICLES.0, particles::draw_refs),
        (game::SCENERY.0, scenery::draw_refs),
        (game::UI.0, ui::draw_refs),
        (game::BANNER.0, banner::draw_refs),
        (game::MENUSCENE.0, menuscene::draw_refs),
    ];
    for (kind, f) in namers {
        for p in cx.store.paths(kind)? {
            // (the stage kind's Items entry alone)
            if kind == game::STAGE.0 && p != items {
                continue;
            }
            let bytes = cx.store.entry(kind, &p)?;
            f(&bytes, &mut out).map_err(|e| KindError::Failed(format!("material: {kind} {p}: {e}")))?;
        }
    }
    Ok(out)
}

/// The material a DrawRef names (`parseDrawRef`).
pub fn draw_ref_material(r: &str) -> Result<&str, KindError> {
    parse_draw_ref(r)
        .map(|d| d.material)
        .ok_or_else(|| KindError::Failed(format!("parseDrawRef: {r} is not <package>.<export>?ctx=<context>")))
}
