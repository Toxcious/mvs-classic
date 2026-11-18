//! The skins a character's menu data is bound for: the default skin (`fighter_port::port_default_skin`: the port
//! table's, else the CharacterData's IsDefaultSkin one) and each further skin the character's data builds (the
//! gamedata kind's FighterData `skins`), found by its SkinData name among the CharacterData's Skins.

use indexmap::IndexMap;

use super::util::{Obj, Scope, asset_name, asset_of, fail, str_of};
use crate::fighter_port::{character_data, character_data_of, fighter_port, port_default_skin};
use crate::game::chars;
use crate::kind::KindError;

pub struct CharacterSkins<'a> {
    pub key: String,
    /// The default skin's SkinData object path.
    pub skin: String,
    /// The further skins' object paths, in the character data's order.
    pub further: Vec<String>,
    /// The CharacterData.
    pub data: Obj<'a>,
}

// The skins are read from the FighterData of the gamedata kind.
pub fn character_skins<'a>(scope: &Scope<'_, 'a>, key: &str) -> Result<CharacterSkins<'a>, KindError> {
    let cx = scope.cx;
    let port = fighter_port(key).ok_or_else(|| fail(format!("{key}: no row in the fighter port table")))?;
    let cdp = character_data(key);
    let view = character_data_of(cx, cdp)?;
    let (Some(cdp), Some(view)) = (cdp, view) else { return Err(fail(format!("{key}: no CharacterData"))) };
    let skin = asset_of(&port_default_skin(cx, key, port, Some(&view))?);
    let data = scope.load(&asset_of(cdp))?;
    let all = chars(cx)?;
    let built: Vec<&String> = all.get(key)?.fighter.skins.iter().flat_map(IndexMap::keys).collect();
    if built.is_empty() {
        return Err(fail(format!("{key}: the fighter data has no skins")));
    }
    let mut known: IndexMap<String, String> = IndexMap::new();
    known.insert(asset_name(&skin).to_owned(), skin.clone());
    for s in super::util::arr(data.get("Skins")) {
        if let Some(s) = str_of(Some(s)) {
            known.insert(asset_name(s).to_owned(), asset_of(s));
        }
    }
    let mut further = Vec::new();
    for name in built {
        let Some(p) = known.get(name) else {
            return Err(fail(format!(
                "{key}: skin {name} of the fighter data is neither its default nor in its CharacterData's Skins"
            )));
        };
        if *p != skin {
            further.push(p.clone());
        }
    }
    Ok(CharacterSkins { key: key.to_owned(), skin, further, data })
}
