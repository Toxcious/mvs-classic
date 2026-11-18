//! A stage's scenery blocks (render/stages/*): what a map's own Blueprint
//! classes and level Blueprints wrote into its scenery entry, one block per top-level key the shared scenery does not
//! read, made by that key and run at the level's begin, each tick, each drawn frame and its end.

use serde_json::Value;

use super::host::SceneryHost;
use super::{
    batcave, cromulons, scoobys_haunted_mansion, sky_arena, the_court, throne_room, tree_fort, trophys_edge, tutorial,
};
use crate::Result;

/// The entry's own keys; any other is a stage's block.
pub(crate) const GENERIC_KEYS: [&str; 11] = [
    "levels",
    "lightmaps",
    "meshes",
    "instances",
    "foliage",
    "landscape",
    "skeletal",
    "emitters",
    "blastBorder",
    "decals",
    "decalReceivers",
];

/// A block of the entry, by the stage module that draws it.
#[derive(Debug)]
pub enum SceneryBlock {
    /// Sky Arena (M011): `floating`, `arm`, `ship`.
    SkyArena(sky_arena::SkyArena),
    /// Trophy's E.D.G.E. (TrophyRoom): `crate`.
    TrophysEdge(trophys_edge::TrophysEdge),
    /// Tree Fort (Map_TreeHouse): `treefort`, `treefortSky`.
    TreeFort(tree_fort::TreeFort),
    /// The Court (M010): `court`.
    TheCourt(the_court::TheCourt),
    /// Batcave: `batcave`.
    Batcave(batcave::Batcave),
    /// Batcave: `batcaveMotion`.
    BatcaveMotion(batcave::BatcaveMotion),
    /// Cromulons (M009): `cromulons`.
    Cromulons(cromulons::Cromulons),
    /// Scooby's Haunted Mansion: `scooby`.
    Scooby(scoobys_haunted_mansion::Scooby),
    /// Scooby's Haunted Mansion: `scoobyMotion`.
    ScoobyMotion(scoobys_haunted_mansion::ScoobyMotion),
    /// Throne Room (M008): `throneRoom`.
    ThroneRoom(throne_room::ThroneRoom),
    /// Tree Fort: `treefortActors`.
    TreefortActors(tree_fort::pillars::TreefortActors),
    /// The tutorial level (Map_TrophyRoom_Tutorial): `tutorialSolids`.
    Tutorial(tutorial::Tutorial),
    /// A block no ported module draws (its key): its instances stay as placed.
    Pending(String),
}

impl SceneryBlock {
    /// The block of the entry's key `key` with its data, made after the
    /// entry's instances and skeletal meshes are read and before the level's begin.
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<SceneryBlock> {
        Ok(match key {
            "floating" | "arm" | "ship" => SceneryBlock::SkyArena(sky_arena::SkyArena::make(key, data, host)?),
            "crate" => SceneryBlock::TrophysEdge(trophys_edge::TrophysEdge::make(key, data, host)?),
            "treefort" | "treefortSky" => SceneryBlock::TreeFort(tree_fort::TreeFort::make(key, data, host)?),
            "court" => SceneryBlock::TheCourt(the_court::TheCourt::make(key, data, host)?),
            "batcave" => SceneryBlock::Batcave(batcave::Batcave::make(key, data, host)?),
            "batcaveMotion" => SceneryBlock::BatcaveMotion(batcave::BatcaveMotion::make(key, data, host)?),
            "cromulons" => SceneryBlock::Cromulons(cromulons::Cromulons::make(key, data, host)?),
            "scooby" => SceneryBlock::Scooby(scoobys_haunted_mansion::Scooby::make(key, data, host)?),
            "scoobyMotion" => SceneryBlock::ScoobyMotion(scoobys_haunted_mansion::ScoobyMotion::make(key, data, host)?),
            "throneRoom" => SceneryBlock::ThroneRoom(throne_room::ThroneRoom::make(key, data, host)?),
            "treefortActors" => {
                SceneryBlock::TreefortActors(tree_fort::pillars::TreefortActors::make(key, data, host)?)
            }
            "tutorialSolids" => SceneryBlock::Tutorial(tutorial::Tutorial::make(key, data, host)?),
            _ => {
                eprintln!("scenery: the block {key} is not ported: its instances are drawn as placed");
                SceneryBlock::Pending(key.to_owned())
            }
        })
    }

    /// The events the block of key `key` posts by name (SceneryBlockRule.sounds), besides those its data names.
    pub fn sounds(key: &str) -> &'static [&'static str] {
        match key {
            "cromulons" => &cromulons::SPARK_SOUNDS,
            _ => &[],
        }
    }

    /// The level begins (Scenery.begin): after the level's emitters spawn and its skeletal meshes are back at their
    /// start.
    pub fn begin(&mut self, host: &mut SceneryHost) {
        match self {
            SceneryBlock::SkyArena(b) => b.begin(host),
            SceneryBlock::TrophysEdge(b) => b.begin(host),
            SceneryBlock::TreeFort(b) => b.begin(host),
            SceneryBlock::TheCourt(b) => b.begin(host),
            SceneryBlock::Batcave(b) => b.begin(host),
            SceneryBlock::BatcaveMotion(b) => b.begin(host),
            SceneryBlock::Cromulons(b) => b.begin(host),
            SceneryBlock::Scooby(b) => b.begin(host),
            SceneryBlock::ScoobyMotion(b) => b.begin(host),
            SceneryBlock::ThroneRoom(b) => b.begin(host),
            SceneryBlock::TreefortActors(b) => b.begin(host),
            SceneryBlock::Tutorial(b) => b.begin(host),
            SceneryBlock::Pending(_) => {}
        }
    }

    /// One world tick of `dt` seconds (Scenery.tick), before the skeletal meshes' animations.
    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        match self {
            SceneryBlock::SkyArena(b) => b.tick(host, dt),
            SceneryBlock::TrophysEdge(b) => b.tick(host, dt),
            SceneryBlock::TreeFort(b) => b.tick(host, dt),
            SceneryBlock::TheCourt(b) => b.tick(host, dt),
            SceneryBlock::Batcave(b) => b.tick(host, dt),
            SceneryBlock::BatcaveMotion(b) => b.tick(host, dt),
            SceneryBlock::Cromulons(b) => b.tick(host, dt),
            SceneryBlock::Scooby(b) => b.tick(host, dt),
            SceneryBlock::ScoobyMotion(b) => b.tick(host, dt),
            SceneryBlock::ThroneRoom(b) => b.tick(host, dt),
            SceneryBlock::TreefortActors(b) => b.tick(host, dt),
            SceneryBlock::Tutorial(b) => b.tick(host, dt),
            SceneryBlock::Pending(_) => {}
        }
    }

    /// A drawn frame (Scenery.frame), after the camera is placed: `alpha` between the previous tick and the latest.
    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        match self {
            SceneryBlock::SkyArena(b) => b.frame(host, alpha),
            SceneryBlock::TrophysEdge(b) => b.frame(host, alpha),
            SceneryBlock::TreeFort(b) => b.frame(host, alpha),
            SceneryBlock::TheCourt(b) => b.frame(host, alpha),
            SceneryBlock::Batcave(b) => b.frame(host, alpha),
            SceneryBlock::BatcaveMotion(b) => b.frame(host, alpha),
            SceneryBlock::Cromulons(b) => b.frame(host, alpha),
            SceneryBlock::Scooby(b) => b.frame(host, alpha),
            SceneryBlock::ScoobyMotion(b) => b.frame(host, alpha),
            SceneryBlock::ThroneRoom(b) => b.frame(host, alpha),
            SceneryBlock::TreefortActors(b) => b.frame(host, alpha),
            SceneryBlock::Tutorial(b) => b.frame(host, alpha),
            SceneryBlock::Pending(_) => {}
        }
    }

    /// The level ends (SceneryBlockView.end): what the block handed out goes back.
    pub fn end(&mut self, host: &mut SceneryHost) {
        match self {
            SceneryBlock::SkyArena(b) => b.end(host),
            SceneryBlock::TrophysEdge(b) => b.end(host),
            SceneryBlock::TreeFort(b) => b.end(host),
            SceneryBlock::TheCourt(b) => b.end(host),
            SceneryBlock::Batcave(b) => b.end(host),
            SceneryBlock::BatcaveMotion(b) => b.end(host),
            SceneryBlock::Cromulons(b) => b.end(host),
            SceneryBlock::Scooby(b) => b.end(host),
            SceneryBlock::ScoobyMotion(b) => b.end(host),
            SceneryBlock::ThroneRoom(b) => b.end(host),
            SceneryBlock::TreefortActors(b) => b.end(host),
            SceneryBlock::Tutorial(b) => b.end(host),
            SceneryBlock::Pending(_) => {}
        }
    }
}
