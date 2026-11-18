//! The buff classes' Blueprint parents (`/GameData/BuffSupers`, [`mvs_data::BuffSupers`]): every Blueprint class of
//! the build whose class chain reaches BuffParentActor_C, with its parents up to that class, nearest first. The buff
//! tests by class of Comp_State_Manager (CheckHasBuff @757, RemoveBuff @846, RemoveBuffStack @742) call
//! BuffParentActor.IsBuffOfClass, true for a buff actor whose class is the one asked or a child of it
//! (ClassIsChildOf @152, EqualEqual_SoftClassReference @190, @228 their OR). An earlier importer had no such table
//! (it tested the exact class) and its store has no entry.
//!
//! Read from the package headers alone: each BlueprintGeneratedClass export's SuperStruct index.

use indexmap::IndexMap;
use mvs_data::BuffSupers;
use rayon::prelude::*;

use crate::catalog::header_object_path;
use crate::kind::{Cx, KindError};
use crate::objview::short_name;

/// The class of a Blueprint class export.
const BLUEPRINT_CLASS: &str = "/Script/Engine.BlueprintGeneratedClass";

/// The class every buff derives from.
const BUFF_ROOT: &str = "BuffParentActor_C";

/// Every buff class's parents ([`BuffSupers`]), in catalog order; a class name the build holds twice keeps the first's.
pub fn buff_supers(cx: &Cx<'_>) -> Result<BuffSupers, KindError> {
    let classes: Vec<String> =
        cx.catalog.exports().filter(|e| e.class == BLUEPRINT_CLASS).map(|e| e.object_path()).collect();
    let supers = classes.par_iter().map(|c| super_of(cx, c)).collect::<Result<Vec<_>, KindError>>()?;
    let parent: IndexMap<String, &str> =
        classes.iter().zip(&supers).filter_map(|(c, s)| Some((c.to_lowercase(), s.as_deref()?))).collect();
    Ok(chains(classes.iter().map(String::as_str), |p| parent.get(&p.to_lowercase()).copied()))
}

/// The SuperStruct of the class export `path` as an object path, `None` without one.
fn super_of(cx: &Cx<'_>, path: &str) -> Result<Option<String>, KindError> {
    let (pkg, i) = cx.read(path)?;
    Ok(header_object_path(&pkg, pkg.exports[i].super_index))
}

/// The buff classes among `classes` (object paths) with their parents up to [`BUFF_ROOT`], `parent` giving a
/// Blueprint class's parent (`None`: a native parent or none, the chain ends there without the root).
fn chains<'p>(classes: impl Iterator<Item = &'p str>, parent: impl Fn(&str) -> Option<&'p str>) -> BuffSupers {
    let mut out = BuffSupers::new();
    for c in classes {
        let name = short_name(c);
        if name == BUFF_ROOT || out.contains_key(name) {
            continue;
        }
        let mut chain: Vec<String> = Vec::new();
        let mut at = parent(c);
        while let Some(p) = at {
            let n = short_name(p);
            // (a loop in the parents, which the build has not: no buff)
            if chain.len() > 64 {
                chain.clear();
                break;
            }
            chain.push(n.to_owned());
            if n == BUFF_ROOT {
                break;
            }
            at = parent(p);
        }
        if chain.last().is_some_and(|n| n == BUFF_ROOT) {
            out.insert(name.to_owned(), chain);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_buff_has_its_parents_up_to_the_root() {
        let parents = [
            (
                "/Game/B/Buff_IgnitedBasketballPass.Buff_IgnitedBasketballPass_C",
                "/Game/B/Buff_BasketballPass.Buff_BasketballPass_C",
            ),
            ("/Game/B/Buff_BasketballPass.Buff_BasketballPass_C", "/Game/Buffs/BuffParentActor.BuffParentActor_C"),
            ("/Game/Buffs/BuffParentActor.BuffParentActor_C", "/Script/Engine.Actor"),
            ("/Game/P/Pawn_Shaggy.Pawn_Shaggy_C", "/Game/P/BaseFighter.BaseFighter_C"),
            ("/Game/P/BaseFighter.BaseFighter_C", "/Script/Pfg.PfgFighter"),
        ];
        let parent = |p: &str| parents.iter().find(|(c, _)| *c == p).map(|(_, s)| *s);
        let got = chains(parents.iter().map(|(c, _)| *c), parent);
        let want: BuffSupers = [
            ("Buff_IgnitedBasketballPass_C".to_owned(), vec!["Buff_BasketballPass_C".to_owned(), BUFF_ROOT.to_owned()]),
            ("Buff_BasketballPass_C".to_owned(), vec![BUFF_ROOT.to_owned()]),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }
}
