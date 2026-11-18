//! A character key's fighter data, bound part by part: body (pawn, components, anim sets, skeleton), montages,
//! actors (with buffs), skin (skins, taunts,
//! voices), paths (the meshes' and clips' object paths). They run in that order, a later one reading an earlier one's
//! keys; the data is then laid out in FIGHTER_KEY_ORDER, the member order of [`FighterData`].

use mvs_data::value::from_value;
use mvs_data::{FighterData, Props, Value};

use crate::fighter_port::{CHARACTER_DATA, FighterPort, fighter_port};
use crate::kind::{Cx, KindError};

pub mod actors;
pub mod anim_graph;
pub mod bag;
pub mod body;
pub mod montages;
pub mod paths;
pub mod skin;

/// What a part binds through and the keys the parts have written so far.
pub struct Bind<'c, 'a> {
    pub cx: &'c Cx<'a>,
    pub key: String,
    pub port: &'static FighterPort,
    /// The CharacterData package (none for a data-only key).
    pub character_data: Option<&'static str>,
    pub out: Props,
}

/// A key's CharacterData package: the playable table's, else the port table's.
pub fn character_data_of_key(key: &str, port: &'static FighterPort) -> Option<&'static str> {
    CHARACTER_DATA.iter().find(|(k, _)| *k == key).map(|(_, p)| *p).or(port.character_data)
}

/// The key's FighterData as the bind leaves it.
pub fn fighter_data(cx: &Cx<'_>, key: &str) -> Result<FighterData, KindError> {
    let Some(port) = fighter_port(key) else {
        return Err(KindError::Failed(format!("fighter data {key}: no entry in the port table")));
    };
    let at =
        |part: &'static str| move |e: KindError| KindError::Failed(format!("fighter data {key}, part {part}: {e}"));
    let mut b =
        Bind { cx, key: key.to_owned(), port, character_data: character_data_of_key(key, port), out: Props::new() };
    let mb = montages::MontageBind::new(*cx, key);
    body::bind_body(&mut b).map_err(at("body"))?;
    montages::bind_montages(&mut b, &mb).map_err(at("montages"))?;
    let actors = actors::bind_actors(&b, &mb).map_err(at("actors"))?;
    skin::bind_skin(&mut b, &mb).map_err(at("skin"))?;
    let skin_names: Vec<String> = match b.out.get("skins") {
        Some(Value::Obj(s)) => s.keys().cloned().collect(),
        _ => Vec::new(),
    };
    let paths = paths::bind_paths(cx, key, port, b.character_data, &skin_names).map_err(at("paths"))?;
    b.out.insert(
        "paths".into(),
        Value::Obj(Props::from_iter([
            ("meshes".to_owned(), Value::Obj(paths.meshes.into_iter().map(|(k, v)| (k, Value::Str(v))).collect())),
            ("clips".to_owned(), Value::Obj(paths.clips.into_iter().map(|(k, v)| (k, Value::Str(v))).collect())),
        ])),
    );
    let mut fd: FighterData =
        from_value(Value::Obj(b.out)).map_err(|e| KindError::Failed(format!("fighter data {key}: {e}")))?;
    let mut buffs = mvs_data::fighter::Map::new();
    for (k, v) in actors.buffs {
        let Value::Obj(p) = v else {
            return Err(KindError::Failed(format!("fighter data {key}: buff {k} is not an object")));
        };
        buffs.insert(k, p);
    }
    fd.buffs = Some(buffs);
    fd.actors = Some(actors.actors);
    fd.actor_montages = actors.actor_montages;
    Ok(fd)
}
