//! A stage's levels as the game kinds that draw it bind from them (the
//! part the stagedraw and stagelight kinds call): the manifest's levels loaded as objects, ordered gameplay, lighting,
//! then scenery and vfx, the audio level apart; their objects and actors in export order, an object's level, an
//! actor's components. The rest of stageBindCtx (templates, transforms, the collision table) is the stage bind's,
//! ported with gamedata.

use crate::game::MAPS_ROOT;
use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, short_name, string};

/// One level of a stage.
pub struct StageLevel<'a> {
    /// Below [`crate::game::MAPS_ROOT`]: `M011/Map_M011`.
    pub level: String,
    /// `gameplay`, `lighting`, `scenery`, `vfx` or `audio`.
    pub role: String,
    /// Its place in the order (gameplay 0, the lighting levels 1.., then scenery and vfx); -1 for the audio level.
    pub li: i32,
    /// The level's World.
    pub world: ObjView<'a>,
    /// Every object inside the World, in export order.
    pub objects: Vec<ObjView<'a>>,
    /// The indexes in `objects` of its actors: the objects whose chain is `PersistentLevel.<name>`.
    pub actors: Vec<usize>,
}

/// A stage's levels (`StageBindCtx`: levels, audioLevel, actors, sceneryActors, levelOf, comps).
pub struct StageLevels<'a> {
    pub key: String,
    /// In the order above, the audio level left out.
    pub levels: Vec<StageLevel<'a>>,
    pub audio: Option<StageLevel<'a>>,
}

impl<'a> StageLevels<'a> {
    /// The levels' actors, in level order, then export order: those of the gameplay and lighting levels (`actors`),
    /// or with `scenery` those of the scenery and vfx levels (`sceneryActors`).
    pub fn actors(&self, scenery: bool) -> impl Iterator<Item = &ObjView<'a>> {
        self.levels
            .iter()
            .filter(move |l| matches!(l.role.as_str(), "scenery" | "vfx") == scenery)
            .flat_map(|l| l.actors.iter().map(|&i| &l.objects[i]))
    }

    /// The level an object of any of its levels (the audio level's too) is in (`levelOf`).
    pub fn level_of(&self, o: &ObjView<'_>) -> Result<&StageLevel<'a>, KindError> {
        self.levels
            .iter()
            .chain(&self.audio)
            .find(|l| l.objects.iter().any(|x| x.path() == o.path()))
            .ok_or_else(|| KindError::Failed(format!("stage data {}: {} is in none of its levels", self.key, o.path())))
    }

    /// The components of `actor`: the objects whose path is the actor's and one name more, in export order; with
    /// `class`, those whose class's short name is it (`comps`).
    pub fn comps(&self, actor: &ObjView<'_>, class: Option<&str>) -> Result<Vec<&ObjView<'a>>, KindError> {
        let level = self.level_of(actor)?;
        fn parent(p: &str) -> Option<&str> {
            let colon = p.find(':')?;
            p.rfind('.').filter(|&d| d > colon).map(|d| &p[..d])
        }
        Ok(level
            .objects
            .iter()
            .filter(|o| parent(o.path()) == Some(actor.path()) && class.is_none_or(|c| short_name(o.class()) == c))
            .collect())
    }
}

/// The levels of stage `key` from its manifest's `levels` (`[level, role]` in LabLevelsToLoad order): exactly one
/// gameplay level and at most one audio level, else an error.
pub fn stage_levels<'a>(cx: &Cx<'a>, key: &str, levels: &[(String, String)]) -> Result<StageLevels<'a>, KindError> {
    let role = |r: &str| levels.iter().filter(|l| l.1 == r).map(|l| l.0.as_str()).collect::<Vec<_>>();
    let gameplay = role("gameplay");
    if gameplay.len() != 1 {
        return Err(KindError::Failed(format!("stage data {key}: {} gameplay levels", gameplay.len())));
    }
    let audio = role("audio");
    if audio.len() > 1 {
        return Err(KindError::Failed(format!("stage data {key}: {} audio levels", audio.len())));
    }
    let mut listed: Vec<(&str, &str)> = vec![(gameplay[0], "gameplay")];
    listed.extend(role("lighting").into_iter().map(|l| (l, "lighting")));
    for l in levels.iter().filter(|l| l.1 == "scenery" || l.1 == "vfx") {
        // (the role of the first entry of that level)
        let r = levels.iter().find(|x| x.0 == l.0).map_or(l.1.as_str(), |x| x.1.as_str());
        listed.push((l.0.as_str(), r));
    }
    let load = |level: &str, role: &str, li: i32| -> Result<StageLevel<'a>, KindError> {
        let world = cx.object(&format!("{MAPS_ROOT}{level}"))?;
        let objects = world.inner()?;
        let actors = objects.iter().enumerate().filter(|(_, o)| is_actor_chain(o.chain())).map(|(i, _)| i).collect();
        Ok(StageLevel { level: level.to_owned(), role: role.to_owned(), li, world, objects, actors })
    };
    let loaded = listed
        .iter()
        .enumerate()
        .map(|(i, (l, r))| load(l, r, i32::try_from(i).expect("a few levels")))
        .collect::<Result<Vec<_>, _>>()?;
    let audio = audio.first().map(|l| load(l, "audio", -1)).transpose()?;
    Ok(StageLevels { key: key.to_owned(), levels: loaded, audio })
}

/// `PersistentLevel.<name>`: an actor's chain below its level (`/^PersistentLevel\.[^.]+$/`).
fn is_actor_chain(chain: &str) -> bool {
    chain.strip_prefix("PersistentLevel.").is_some_and(|n| !n.is_empty() && !n.contains('.'))
}

/// A level's MapBuildDataRegistry: its PersistentLevel's
/// `MapBuildData` as an object path, `None` without one.
pub fn level_built_data(objects: &[ObjView<'_>]) -> Option<String> {
    let pl = objects.iter().find(|o| o.path().ends_with(":PersistentLevel"))?;
    let v = pl.prop("MapBuildData")?;
    string(pl.cache(), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_chains() {
        assert!(is_actor_chain("PersistentLevel.Floor_1"));
        assert!(!is_actor_chain("PersistentLevel.Floor_1.Box"));
        assert!(!is_actor_chain("PersistentLevel."));
        assert!(!is_actor_chain("PersistentLevel"));
    }
}
