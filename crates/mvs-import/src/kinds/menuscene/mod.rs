//! The `menuscene` kind's entries: the menus' three levels and one preview per skin, bound
//! from the build's levels, Blueprints and sequences into mvs-material's `menuscene` types, written as their jsonbin.
//! A namer of the material kind (the entries' DrawRefs) and of the particles kind (the PreMatch entry's `systems`).

mod mainmenu;
mod material;
mod podium;
mod prematch;
mod preview;
mod skins;
mod tables;
mod util;

use std::sync::Arc;

use indexmap::IndexSet;
use mvs_material::menuscene::{
    FORMAT_VERSION, LevelName, MAGIC, MenuSceneEntry, SceneData, level_suffix, preview_suffix, read_entry,
};

use self::prematch::PreviewJob;
use self::util::{Scope, fail};
use crate::game::{GAMEDATA, MENUSCENE, MENUSCENE_BIND_VERSION, menuscene_dir};
use crate::jsonbin::json_bin;
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::object::OBJECT_KIND_VERSION;
use crate::kinds::shader::SHADER_KIND_VERSION;
use crate::kinds::shadermap::SHADERMAP_KIND_VERSION;

const LEVELS: [LevelName; 3] = [LevelName::MainMenu, LevelName::Podium, LevelName::PreMatch];

/// What an entry is bound from (`menuSceneJobs`).
enum Job {
    Level(LevelName),
    Preview(PreviewJob),
}

/// The run's one list of the entries by path, each with its job: the three levels, then every skin's preview (the
/// first asker lists; a failure is every asker's).
struct Jobs(Result<Arc<Vec<(String, Job)>>, String>);

fn jobs(cx: &Cx<'_>) -> Result<Arc<Vec<(String, Job)>>, KindError> {
    let slot = cx.shared.get_or_init(|| Jobs(list_jobs(cx).map(Arc::new).map_err(|e| e.to_string())));
    slot.0.clone().map_err(KindError::Failed)
}

fn list_jobs(cx: &Cx<'_>) -> Result<Vec<(String, Job)>, KindError> {
    let dir = menuscene_dir();
    let mut out: Vec<(String, Job)> =
        LEVELS.iter().map(|l| (dir.clone() + &level_suffix(*l), Job::Level(*l))).collect();
    for p in prematch::preview_list(&Scope::new(cx))? {
        out.push((dir.clone() + &preview_suffix(&p.key, &p.skin), Job::Preview(p)));
    }
    Ok(out)
}

/// An entry bound (`menuSceneEntry`).
fn bind(cx: &Cx<'_>, job: &Job) -> Result<MenuSceneEntry, KindError> {
    let scope = Scope::new(cx);
    let mut systems = Vec::new();
    let (name, data) = match job {
        Job::Level(l) => (
            l.as_str().to_owned(),
            match l {
                LevelName::MainMenu => SceneData::MainMenu(Box::new(mainmenu::bind_main_menu(&scope)?)),
                LevelName::Podium => SceneData::Podium(Box::new(podium::bind_podium(&scope)?)),
                LevelName::PreMatch => {
                    let (d, s) = prematch::bind_pre_match(&scope)?;
                    systems = s;
                    SceneData::PreMatch(Box::new(d))
                }
            },
        ),
        Job::Preview(p) => {
            let n = if p.skin.is_empty() { "Default" } else { &p.skin };
            let d = prematch::bind_preview(&scope, &p.key, &p.path, p.skin.is_empty())?;
            (format!("Preview/{}/{n}", p.key), SceneData::Preview(Box::new(d)))
        }
    };
    Ok(MenuSceneEntry {
        version: FORMAT_VERSION,
        name,
        bind_version: MENUSCENE_BIND_VERSION,
        object_version: OBJECT_KIND_VERSION as u64,
        shadermap_version: SHADERMAP_KIND_VERSION,
        systems,
        data,
    })
}

/// The entry's bytes, `MVMS` and its header's JSON (the members in the
/// types' order, numbers as JSON.stringify writes them), no body.
pub fn entry_bytes(entry: &MenuSceneEntry) -> Result<Vec<u8>, KindError> {
    Ok(json_bin(MAGIC, entry, &[] as &[&[u8]], &[])?)
}

/// The DrawRefs an entry names (the material kind's reader of this namer).
pub fn draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let e = read_entry(bytes, "menuscene").map_err(|e| KindError::Failed(e.to_string()))?;
    out.extend(e.draws().into_iter().map(str::to_owned));
    Ok(())
}

pub struct MenuScene;

impl Kind for MenuScene {
    fn id(&self) -> &'static str {
        MENUSCENE.0
    }

    fn version(&self) -> u64 {
        MENUSCENE.1
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(jobs(cx)?.iter().map(|(p, _)| Item::new(p.clone())).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let all = jobs(cx)?;
        let Some((_, job)) = all.iter().find(|(p, _)| *p == item.path) else {
            return Err(fail(format!("no menuscene entry {}", item.path)));
        };
        entry_bytes(&bind(cx, job)?)
    }

    /// The gamedata kind: the skins each character's data builds (skins.rs); the shader kind: the code whose custom
    /// stencil reads the banner probe decodes; the object and shadermap
    /// kinds are built in-process.
    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA, ("shader", SHADER_KIND_VERSION)]
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use mvs_material::menuscene::{MirrorWhen, Placement, PreviewEntry};

    use super::*;

    #[test]
    fn entry_reads_back() {
        let mut placements = IndexMap::new();
        placements.insert("MainMenu_You".to_owned(), Placement { loc: [1.5, -0.0, 2.0], scale: [-1.0, 1.0, 1.0] });
        let e = MenuSceneEntry {
            version: FORMAT_VERSION,
            name: "Preview/arya/Default".into(),
            bind_version: MENUSCENE_BIND_VERSION,
            object_version: OBJECT_KIND_VERSION as u64,
            shadermap_version: SHADERMAP_KIND_VERSION,
            systems: Vec::new(),
            data: SceneData::Preview(Box::new(PreviewEntry {
                skin: "Arya_S01".into(),
                placements,
                preview_idle: None,
                homescreen_idle: None,
                rig: "BP_Rig".into(),
                mirror_when: MirrorWhen::Left,
                lights: Vec::new(),
            })),
        };
        let b = entry_bytes(&e).unwrap();
        assert_eq!(&b[..4], MAGIC);
        let back = read_entry(&b, "test").unwrap();
        assert_eq!(back, e);
        assert_eq!(entry_bytes(&back).unwrap(), b);
        assert_eq!(
            menuscene_dir() + &preview_suffix("arya", ""),
            "/MenuScene/b7d15s2h1o11/Preview/arya/Default.Default"
        );
    }
}
