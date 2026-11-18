//! A character's skin audio, bound from the build: per skin
//! of the CharacterData its AudioName, VO table, tags and SkinAudioOverride events (Lib_Audio PF_Skin_Manager,
//! Comp_Actor_VO InitVOData), and the rows of every skin VO table but the port default skin's (that one is
//! FighterData.vo, the skin part's).

use mvs_data::Value;
use mvs_data::fighter::Map;
use mvs_data::tables::{SkinAudio, SkinAudioSkin};

use super::fighter::skin::override_events;
use super::shape::{fail, properties, short_name};
use super::vo_rows::vo_rows;
use crate::fighter_port::{asset_of, character_data, character_data_of, fighter_port, pkg_of, port_default_skin};
use crate::kind::{Cx, KindError};

fn str_or_empty(v: Option<&Value>) -> String {
    match v {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The skins: CharacterData.Skins less the `_Server` ones (the dedicated server's, Comp_Pawn_SkinManager
/// ApplySkinToPawn @26-428) and those the build lacks, in their order. `default`: the AudioName of the skin with
/// IsDefaultSkin (GetDefaultSkin @465: the build's default, Iron Giant's C017_S01, not the port's S00), PF_Skin_Manager's
/// Replace source. Event names lower case, as the posts name them.
pub fn skin_audio(cx: &Cx<'_>, key: &str) -> Result<SkinAudio, KindError> {
    let Some(port) = fighter_port(key) else { return fail(format!("skin audio {key}: no entry in the port table")) };
    let Some(cd) = character_data_of(cx, character_data(key))? else { return fail(format!("{key}: no CharacterData")) };
    let def = port_default_skin(cx, key, port, Some(&cd))?;
    let port_table = match properties(&cx.object(&asset_of(&def))?)?.get("VODataTable") {
        Some(Value::Str(t)) => short_name(t).to_owned(),
        _ => String::new(),
    };
    let cdp = properties(&cd)?;
    let listed: Vec<String> = match cdp.get("Skins") {
        Some(Value::Arr(l)) => l
            .iter()
            .filter_map(|p| match p {
                Value::Str(p) if !short_name(p).to_lowercase().ends_with("_server") => Some(p.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut skins = Vec::new();
    for p in listed {
        if cx.exists(pkg_of(&p)) {
            let props = properties(&cx.object(&p)?)?;
            skins.push((short_name(&p).to_owned(), props));
        }
    }
    let defaults: Vec<_> =
        skins.iter().filter(|(_, p)| matches!(p.get("IsDefaultSkin"), Some(Value::Bool(true)))).collect();
    if defaults.len() != 1 {
        return fail(format!("{key}: {} default skins", defaults.len()));
    }
    let mut out =
        SkinAudio { default: str_or_empty(defaults[0].1.get("AudioName")), skins: Map::new(), vo: Map::new() };
    for (name, p) in &skins {
        let table = str_or_empty(p.get("VODataTable"));
        let tname = if table.is_empty() { String::new() } else { short_name(&table).to_owned() };
        out.skins.insert(
            name.clone(),
            SkinAudioSkin {
                audio_name: str_or_empty(p.get("AudioName")),
                vo: tname.clone(),
                ally: str_or_empty(p.get("TagForAllies")),
                enemy: str_or_empty(p.get("TagForEnemies")),
                events: override_events(cx, p)?.into_iter().map(|e| e.to_lowercase()).collect(),
            },
        );
        if !tname.is_empty() && tname != port_table && !out.vo.contains_key(&tname) {
            out.vo.insert(tname, vo_rows(cx, &table, &|_| true)?);
        }
    }
    Ok(out)
}
