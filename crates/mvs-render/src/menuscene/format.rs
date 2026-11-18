//! The `menuscene` kind as the renderer reads it: the entry types (mvs-material's `menuscene`, which the importer
//! writes) and their loads from the store by level or by a skin's preview.

pub use mvs_material::menuscene::*;
use mvs_view::MaterialRef;

use crate::{Error, Result};

/// The kind's name in the store.
pub const KIND: &str = "menuscene";

/// The entry whose path ends with `suffix` (the kind's one versioned folder holds every entry).
fn load_suffix(store: &mvs_store::Store, suffix: &str) -> Result<MenuSceneEntry> {
    let pack = store.kind(KIND)?;
    let mut found = pack.paths().filter(|p| p.ends_with(suffix));
    let Some(path) = found.next() else {
        return Err(Error::Format { what: format!("{KIND}{suffix}"), why: "no such entry in the store".into() });
    };
    if let Some(other) = found.next() {
        return Err(Error::Format { what: format!("{KIND}{suffix}"), why: format!("two entries: {path}, {other}") });
    }
    let path = path.to_string();
    Ok(read_entry(&pack.bytes(&path)?, &path)?)
}

/// A level's entry.
pub fn load_level(store: &mvs_store::Store, level: LevelName) -> Result<MenuSceneEntry> {
    load_suffix(store, &level_suffix(level))
}

/// A skin's preview entry (`skin` its SkinData name, "" the default skin's).
pub fn load_preview(store: &mvs_store::Store, key: &str, skin: &str) -> Result<PreviewEntry> {
    let what = preview_suffix(key, skin);
    match load_suffix(store, &what)?.data {
        SceneData::Preview(p) => Ok(*p),
        _ => Err(Error::Format { what, why: "not a preview entry".into() }),
    }
}

/// The material a scene material draws, with the values the scene sets on it (`set`: a banner piece's scalars over
/// the material's own), as a mesh slot takes it.
pub fn material_ref(m: &SceneMaterial, set: Option<&indexmap::IndexMap<String, f64>>) -> MaterialRef {
    let draw = mvs_material::draw::parse_draw_ref(&m.draw)
        .unwrap_or_else(|| panic!("menuscene: {} is not a DrawRef (read_entry checks it)", m.draw));
    let mut scalars: Vec<(std::sync::Arc<str>, f32)> =
        m.scalars.iter().map(|(k, v)| (std::sync::Arc::from(k.as_str()), *v as f32)).collect();
    for (k, v) in set.into_iter().flatten() {
        match scalars.iter_mut().find(|(n, _)| &**n == k) {
            Some(s) => s.1 = *v as f32,
            None => scalars.push((k.as_str().into(), *v as f32)),
        }
    }
    MaterialRef {
        path: draw.material.into(),
        scalars,
        vectors: m.vectors.iter().map(|(k, v)| (k.as_str().into(), v.map(|x| x as f32))).collect(),
        textures: m.textures.iter().map(|(k, v)| (k.as_str().into(), v.as_str().into())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The developer's store's preview entries read by this format's reader (`read_entry`: the version, the entry's
    /// data kind, deny_unknown_fields). A tree with no store, a store with no menuscene kind, or one still holding
    /// format-1 entries says so and passes. The entry round-trips through its JSON.
    #[test]
    fn store_previews_read() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no developer store: skipped");
            return;
        };
        let Ok(pack) = store.kind(KIND) else {
            eprintln!("no menuscene kind in the store: skipped");
            return;
        };
        let mut n = 0;
        for path in pack.paths().filter(|p| p.contains("/Preview/")).map(str::to_string).collect::<Vec<_>>() {
            let bytes = pack.bytes(&path).unwrap();
            let header: serde_json::Value =
                mvs_material::jsonbin::JsonBin::split(MAGIC, &bytes, &path).unwrap().parse(&path).unwrap();
            if header.get("version").and_then(serde_json::Value::as_u64) != Some(FORMAT_VERSION.into()) {
                eprintln!("{path}: not format {FORMAT_VERSION} (the store is not re-imported): skipped");
                return;
            }
            let e = read_entry(&bytes, &path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let SceneData::Preview(p) = &e.data else { panic!("{path}: not a preview's data") };
            let again: PreviewEntry = serde_json::from_slice(&serde_json::to_vec(p).unwrap()).unwrap();
            assert_eq!(&again, &**p, "{path}");
            n += 1;
        }
        eprintln!("{n} preview entries");
    }
}
