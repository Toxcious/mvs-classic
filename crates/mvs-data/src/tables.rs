//! The tables the sim keeps beside its GameData: perks, skin audio, ring outs, the announcer, the mutators, the bot
//! trees and the lists a match is put together from.

use serde::{Deserialize, Serialize};

use crate::fighter::{ActorDef, Map, VoRow};
use crate::value::Props;

/// A perk table: the general one, or a character's part.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerkTable {
    pub perks: Map<PerkEntry>,
    pub buffs: Map<Props>,
    /// Whole actor entries (`p`, `components`, `supers`, `ifaces`) the table's perks spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actors: Option<Map<ActorDef>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerkEntry {
    pub buffs: Vec<String>,
    pub spawn_on_allies: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkinAudio {
    pub default: String,
    pub skins: Map<SkinAudioSkin>,
    /// The skins' VO tables but the port's default skin's (that one is FighterData.vo).
    pub vo: Map<Vec<VoRow>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkinAudioSkin {
    pub audio_name: String,
    pub vo: String,
    pub ally: String,
    pub enemy: String,
    pub events: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RingOutTable {
    pub maps: Vec<String>,
    pub assets: Map<RingOutAsset>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RingOutAsset {
    pub hold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random: Option<f64>,
    pub entries: Vec<RingOutEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RingOutEntry {
    pub ps: String,
    pub off: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceil: Option<f64>,
    pub lerp: f64,
    pub scale: Vec<f64>,
    pub rot: Vec<f64>,
    pub rotate: bool,
    pub perp: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_l: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_r: Option<f64>,
    pub ally: String,
    pub enemy: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnnouncerPack {
    pub path: String,
    pub name: String,
    pub rows: Vec<AnnouncerRow>,
}

/// alias, tags, priority, event, length in seconds.
pub type AnnouncerRow = (String, String, f64, String, Option<f64>);

/// Every AnnouncerPackData of the build (the Collection's packs, DefaultAnnouncerPack among them), in the asset
/// catalogue's order: the pack a player equips is read from it (`crate::stored_announcer_pack`).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnnouncerPacks {
    pub packs: Vec<AnnouncerPack>,
}

impl AnnouncerPacks {
    /// The pack at `path` (an object path or its package, any case), None for a path no pack has.
    pub fn get(&self, path: &str) -> Option<&AnnouncerPack> {
        let pkg = |p: &str| p.split('.').next().unwrap_or(p).to_lowercase();
        let want = pkg(path);
        self.packs.iter().find(|p| pkg(&p.path) == want)
    }
}

/// The arcade buff classes' defaults, by class.
pub type ArcadeBuffs = Map<Props>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MutatorsData {
    pub map: Map<String>,
    pub world: Vec<String>,
    pub buffs: Map<Props>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BotTrees {
    pub trees: Map<BtTree>,
    /// character key -> lowercase gameplay tag -> tree name
    pub inject: Map<Map<String>>,
    pub blackboard: Vec<BlackboardKey>,
    pub persona: Map<f64>,
    /// The class defaults of the Blueprint node classes.
    pub defaults: Map<Props>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BtTree {
    pub root: BtNodeDef,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlackboardKey {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// A decorator or a service.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BtAuxDef {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub props: Props,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BtNodeDef {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub props: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<BtAuxDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<BtChildDef>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BtChildDef {
    pub node: BtNodeDef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorators: Option<Vec<BtAuxDef>>,
    /// Prefix decorator logic: And n / Or n / Not 1 / Test index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ops: Option<Vec<(String, f64)>>,
}

/// The lists a match's data is put together from (the sim's character data).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Index {
    pub characters: Vec<String>,
    pub data_only: Vec<String>,
    /// The data-only keys a character's match needs beside its players'.
    pub match_needs: Map<Vec<String>>,
    pub default_character: String,
    pub stages: Vec<String>,
    pub default_stage: String,
}

/// The DataTable InputDefaults (/Game/Panda_Main/UI/InputBinding/InputDefaults): per row
/// (`Default-P0`, `Legacy-P0`, ...) the action names in table order, each with its mappings' key names in mapping
/// order. No mapping of the build carries a modifier, so a mapping is its key name.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputDefaults {
    pub rows: Map<Map<Vec<String>>>,
}

/// One ForceFeedbackEffect of Blueprints/ForceFeedback: GetDuration's duration (the
/// largest last key time over the channels) and its channels.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RumbleEffect {
    pub duration: f64,
    pub channels: Vec<RumbleChannel>,
}

/// A FForceFeedbackChannelDetails: the motors its four bAffects flags name (1 LeftLarge, 2 LeftSmall, 4 RightLarge,
/// 8 RightSmall) and its curve's keys (time s, value), linear with constant extrapolation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RumbleChannel {
    pub motors: u8,
    pub keys: Vec<(f64, f64)>,
}

/// The ForceFeedbackEffects by asset name.
pub type RumbleTable = Map<RumbleEffect>;

/// The buff classes' Blueprint parents: every class of the build whose chain reaches BuffParentActor_C, by class name
/// (`Buff_IgnitedBasketballPass_C`), its parents nearest first up to BuffParentActor_C
/// (`[Buff_BasketballPass_C, BuffParentActor_C]`). BuffParentActor.IsBuffOfClass (@152-228) takes a buff actor of the
/// class asked or of a child of it.
pub type BuffSupers = Map<Vec<String>>;

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry with no body, as the encoder writes one.
    fn entry(json: &str) -> Vec<u8> {
        let mut out = b"MVGD".to_vec();
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(json.as_bytes());
        out.resize((out.len() + 15) & !15, 0);
        out
    }

    #[test]
    fn input_defaults_keep_row_action_and_mapping_order() {
        let json = r#"{"rows":{"Legacy-P0":{"Jump":["SpaceBar","Gamepad_FaceButton_Bottom"]},"Default-P0":{"Up":["W"],"Attack":["J","Gamepad_FaceButton_Left"]}}}"#;
        let t: InputDefaults = crate::decode(&entry(json)).unwrap();
        let rows: Vec<&str> = t.rows.keys().map(String::as_str).collect();
        assert_eq!(rows, ["Legacy-P0", "Default-P0"]);
        let p0 = &t.rows["Default-P0"];
        assert_eq!(p0.keys().map(String::as_str).collect::<Vec<_>>(), ["Up", "Attack"]);
        assert_eq!(p0["Attack"], ["J", "Gamepad_FaceButton_Left"]);
    }

    #[test]
    fn rumble_effects_read_motors_and_keys() {
        let json = r#"{"FF_Hit":{"duration":0.25,"channels":[{"motors":5,"keys":[[0,1],[0.25,-0]]},{"motors":10,"keys":[]}]}}"#;
        let t: RumbleTable = crate::decode(&entry(json)).unwrap();
        let fx = &t["FF_Hit"];
        assert_eq!(fx.duration, 0.25);
        assert_eq!(fx.channels[0].motors, 5);
        assert_eq!(fx.channels[0].keys, [(0.0, 1.0), (0.25, -0.0)]);
        assert_eq!(fx.channels[0].keys[1].1.to_bits(), (-0f64).to_bits());
        assert!(fx.channels[1].keys.is_empty());
        // A motor mask past u8 is refused, not truncated.
        assert!(
            crate::decode::<RumbleTable>(&entry(r#"{"x":{"duration":0,"channels":[{"motors":256,"keys":[]}]}}"#))
                .is_err()
        );
    }
}
