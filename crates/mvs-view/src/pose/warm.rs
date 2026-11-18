//! The views' warm-up list (the port's own, a frame-cost rule: the first draw of a view's meshes under the loading
//! screen): every mesh and material set the match's views may draw, for `Renderer::prewarm`.

use std::sync::Arc;

use indexmap::IndexSet;
use mvs_sim::World;

use super::ViewPoser;
use crate::chars::render_of;
use crate::frame::WarmMesh;
use crate::skins::{self, ARMOR_OVERLAY, ARMOR_OVERLAY_LOCAL, Skin};

impl ViewPoser {
    /// Every mesh the views may draw with each material set it may take: per view (the fighters' and the minions'),
    /// and per skin a mode's pawn may be made in later, its skin's meshes (the main mesh, the parts, the props) in the
    /// red, blue and local sets and in the armor overlays; per character whose draws the views hold, its actors' and props' meshes in each of their sets; the
    /// static models; then every other mesh the views hold (the actors' components) with its own materials.
    // Unverified: an actor's mesh drawn with another entry's material (Jake's bottom half takes the body's slot 0)
    // is warmed by the material's own entry; the pair's input layout is made at its first draw when its streams
    // differ from the entry's mesh's.
    pub fn warm_meshes(&self, w: &World) -> Vec<WarmMesh> {
        let mut out: IndexSet<WarmMesh> = IndexSet::new();
        let arc = |s: &str| Arc::<str>::from(s);
        let set = |l: &[Option<String>]| l.iter().map(|m| m.as_deref().map(arc)).collect::<Vec<_>>();
        for r in &self.recs {
            self.warm_skin(w, &r.key, &r.skin, &mut out);
        }
        // (a mode's cast: its pawns get their views at the step that finds them (sync_pawns), each in its
        // character's default skin (no player slot names one) and its minions' skins)
        if let Some(mvs_sim::setup::MatchMode::Tutorial { cast, .. }) = &w.setup.mode {
            for k in cast {
                let Some(cd) = w.data.chars.get(k.as_str()) else { continue };
                let mut skins = Vec::new();
                skins.extend(skins::skin_of(&cd.fighter, None).ok().map(|s| (k.as_str(), s)));
                for m in render_of(k).minions {
                    let Some(md) = w.data.chars.get(m.key) else { continue };
                    skins.extend((m.skin)(&md.fighter).ok().map(|s| (m.key, s)));
                }
                for (key, sk) in skins {
                    if !self.recs.iter().any(|r| r.key == key && r.skin.name == sk.name) {
                        self.warm_skin(w, key, &sk, &mut out);
                    }
                }
            }
        }
        let mut keys: Vec<&String> = self.draws.keys().collect();
        keys.sort();
        for k in keys {
            for (mesh, sets) in self.draws[k].actor_sets() {
                if mesh.is_empty() {
                    continue;
                }
                for s in sets {
                    out.insert(WarmMesh {
                        mesh: Some(arc(&mesh)),
                        materials: set(&s),
                        outlined: true,
                        captured: true,
                        every_slot: false,
                    });
                }
            }
        }
        // (the items part's mesh components, each in its template's set and its SetTeamColor sets, for a match with
        // random items)
        for (mesh, sets) in self.items.iter().flat_map(|i| i.warm_sets()) {
            for s in sets {
                out.insert(WarmMesh {
                    mesh: Some(arc(&mesh)),
                    materials: set(&s),
                    outlined: true,
                    captured: true,
                    every_slot: false,
                });
            }
        }
        let mut models: Vec<_> = self.actors.models.iter().collect();
        models.sort_by_key(|(k, _)| **k);
        for (_, m) in models {
            let materials = m.mats.iter().map(|r| (!r.path.is_empty()).then(|| r.path.clone())).collect();
            out.insert(WarmMesh {
                mesh: Some(arc(&m.info.path)),
                materials,
                outlined: true,
                captured: true,
                every_slot: false,
            });
        }
        let warmed: IndexSet<Arc<str>> = out.iter().filter_map(|m| m.mesh.clone()).collect();
        let mut rest: Vec<&String> = self.meshes.keys().filter(|p| !warmed.contains(p.as_str())).collect();
        rest.sort();
        for p in rest {
            out.insert(WarmMesh { mesh: Some(arc(p)), ..WarmMesh::default() });
        }
        out.into_iter().collect()
    }

    /// Skin `sk` of character `key`: its meshes (the main mesh, the parts, the props) in each material set its
    /// draws hold, and its skinned meshes in the armor overlays.
    fn warm_skin(&self, w: &World, key: &str, sk: &Skin, out: &mut IndexSet<WarmMesh>) {
        let arc = |s: &str| Arc::<str>::from(s);
        let set = |l: &[Option<String>]| l.iter().map(|m| m.as_deref().map(arc)).collect::<Vec<_>>();
        let Some(d) = self.draws.get(key) else { return };
        let Some(cd) = w.data.chars.get(key) else { return };
        let fd = &cd.fighter;
        let look = fd.skins.as_ref().and_then(|m| m.get(&sk.name));
        let props = look.map_or(fd.skin.prop_meshes.as_ref(), |l| l.prop_meshes.as_ref());
        let path_of = |key: &str| -> Option<String> {
            if key.is_empty() {
                return Some(sk.mesh.clone());
            }
            if let Some(p) = sk.parts.iter().find(|p| p.key.eq_ignore_ascii_case(key)) {
                return p.path.clone();
            }
            let p = props?.iter().find(|p| p.key.eq_ignore_ascii_case(key))?;
            p.path.clone().or_else(|| fd.paths.meshes.get(&p.mesh).cloned())
        };
        let mut skinned = Vec::new();
        for (key, sets) in d.skin_sets(&sk.name) {
            let Some(path) = path_of(&key) else { continue };
            if key.is_empty() || sk.parts.iter().any(|p| p.key.eq_ignore_ascii_case(&key)) {
                skinned.push(path.clone());
            }
            for s in sets {
                out.insert(WarmMesh {
                    mesh: Some(arc(&path)),
                    materials: set(&s),
                    outlined: true,
                    captured: true,
                    every_slot: false,
                });
            }
        }
        // (the armor overlay copies: the overlay on every slot, no custom depth; a mesh no view has loaded yet, a mode
        // pawn's, takes the overlay on each of its slots)
        for path in skinned {
            let slots = self.meshes.get(&path).map(|m| m.slots.len().max(1));
            for armor in [ARMOR_OVERLAY, ARMOR_OVERLAY_LOCAL] {
                let materials = vec![Some(arc(armor)); slots.unwrap_or(1)];
                out.insert(WarmMesh {
                    mesh: Some(arc(&path)),
                    materials,
                    outlined: false,
                    captured: false,
                    every_slot: slots.is_none(),
                });
            }
        }
    }
}
