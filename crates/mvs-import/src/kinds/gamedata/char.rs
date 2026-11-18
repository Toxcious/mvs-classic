//! A character key's entry: its fighter data, boxes, definition, perk part and skin audio

use mvs_data::{BoxIndex, CharData, FighterData, Value};

use super::boxes::bind_boxes;
use super::def::char_def;
use super::fighter::fighter_data;
use super::perks::char_perks;
use super::skin_audio::skin_audio;
use crate::fighter_port::CHARACTERS;
use crate::kind::{Cx, KindError};

/// The bone box tracks of spawned actors' components (BoxIndex.boneBoxes: Bugs's ItemHelper_BananaGuard_C
/// SpearHitBox) laid onto their components of the fighter data as `boneBox` (the component's last member) and taken off
/// the index.
fn lay_bone_boxes(fighter: &mut FighterData, boxes: &mut BoxIndex) -> Result<(), KindError> {
    let Some(by) = boxes.bone_boxes.take() else { return Ok(()) };
    for (cls, comps) in by {
        for (comp, track) in comps {
            let c = fighter.actors.as_mut().and_then(|a| a.get_mut(&cls)).and_then(|a| a.components.get_mut(&comp));
            let Some(Value::Obj(c)) = c else {
                return Err(KindError::Failed(format!(
                    "{cls}.{comp}: a bone box of the hit box tables for a component the fighter data lacks"
                )));
            };
            c.insert("boneBox".into(), Value::Str(track));
        }
    }
    Ok(())
}

/// One key's data as the sim's test data leaves it: boxes over the fighter data, the
/// bone boxes laid in, the perk part and skin audio (none for a data-only key), then the definition, whose bind writes
/// into the fighter data.
pub fn char_entry(cx: &Cx<'_>, key: &str) -> Result<CharData, KindError> {
    let mut fighter = fighter_data(cx, key)?;
    let (mut boxes, box_data) = bind_boxes(cx, key, &fighter)?;
    lay_bone_boxes(&mut fighter, &mut boxes)?;
    let own = CHARACTERS.contains(&key);
    let (perks, skin_audio) = if own { (Some(char_perks(cx, key)?), Some(skin_audio(cx, key)?)) } else { (None, None) };
    let (def, def_extra) = char_def(&mut fighter, key)?;
    Ok(CharData { key: key.to_owned(), fighter, boxes, box_data, def, def_extra, perks, skin_audio })
}
