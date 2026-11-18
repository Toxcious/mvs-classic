//! Every kind the import builds, entered once: the kinds built from the paks, then
//! the game kinds built from what other kinds wrote into the store, in the game kinds' order. The runner
//! and the CLI find a kind here by its id. A kind entered twice, or one that reads a kind of the registry at another
//! version than that kind's, is a test failure.

pub mod animation;
pub mod atmosphere;
pub mod banner;
pub mod config;
pub mod cosmetics;
pub mod fighter;
pub mod font;
pub mod gamedata;
pub mod image;
pub mod lighting;
pub mod literals;
pub mod material;
pub mod menuscene;
pub mod mesh;
pub mod object;
pub mod particles;
pub mod scenery;
pub mod shader;
pub mod shadermap;
pub mod skeleton;
pub mod sky;
pub mod sky_compute;
pub mod sound;
pub mod soundbank;
pub mod stage;
pub mod stagedraw;
pub mod stagelight;
pub mod text;
pub mod texture;
pub mod ui;

use crate::kind::Kind;

/// The kinds, in the order the import lists them.
pub static KINDS: &[&dyn Kind] = &[
    &object::Object,
    &font::Font,
    &text::Text,
    &config::Config,
    &texture::Texture,
    &lighting::Lighting,
    &mesh::Mesh,
    &skeleton::Skeleton,
    &animation::Animation,
    &gamedata::GameData,
    &sound::Sound,
    &soundbank::Soundbank,
    &shadermap::Shadermap,
    &shader::Shader,
    &image::Image,
    &literals::Literals,
    &fighter::Fighter,
    &stage::Stage,
    &sky::Sky,
    &scenery::Scenery,
    &ui::Ui,
    &banner::Banner,
    &menuscene::MenuScene,
    &particles::Particles,
    &stagelight::StageLight,
    &atmosphere::Atmosphere,
    &stagedraw::StageDraw,
    &material::Material,
];

/// The kind of `id`, if the import builds it.
pub fn kind(id: &str) -> Option<&'static dyn Kind> {
    KINDS.iter().copied().find(|k| k.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_once() {
        let mut ids: Vec<&str> = KINDS.iter().map(|k| k.id()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "a kind entered twice");
        // the versions the store holds (the earlier ones, where the kind is not bumped since; gamedata
        // and stage fold their code's versions, game.rs)
        let v: Vec<(&str, u64)> = KINDS.iter().map(|k| (k.id(), k.version())).collect();
        assert_eq!(
            v,
            [
                ("object", 11),
                ("font", 1),
                ("text", 1),
                ("config", 1),
                ("texture", 4),
                ("lighting", 1),
                ("mesh", 2),
                ("skeleton", 1),
                ("animation", 3),
                ("gamedata", 2_860_170_883_002),
                ("sound", 2),
                ("soundbank", 4),
                ("shadermap", 2),
                ("shader", 1),
                ("image", 1),
                ("literals", 1),
                ("fighter", 15_002_001_011),
                ("stage", 3_951_951_923),
                ("sky", 3_013_001_004_011),
                ("scenery", 8_015_013_011),
                ("ui", 31_011),
                // (a hash of the folder's versions, game.rs `banner_kind_version`: the other pins here move it)
                crate::game::BANNER,
                ("menuscene", 7_015_002_001_011),
                ("particles", 11_015_002_001_002_011),
                ("stagelight", 1_013_001_011),
                ("atmosphere", 2_002_001_011),
                ("stagedraw", 1),
                ("material", 1_004_002_001_011),
            ]
        );
        let unstored: Vec<&str> = KINDS.iter().filter(|k| !k.stored()).map(|k| k.id()).collect();
        assert_eq!(unstored, ["object", "shadermap"]);
    }

    #[test]
    fn reads_at_the_registry_versions() {
        for k in KINDS {
            for &(r, v) in k.reads() {
                if let Some(read) = kind(r) {
                    assert_eq!(read.version(), v, "{} reads {r} at v{v}", k.id());
                    let (at, of) =
                        (KINDS.iter().position(|x| x.id() == r), KINDS.iter().position(|x| x.id() == k.id()));
                    assert!(at < of, "{} reads {r}, entered after it", k.id());
                }
            }
        }
        let reads = |id: &str| kind(id).unwrap().reads().iter().map(|r| r.0).collect::<Vec<_>>();
        assert_eq!(
            reads("material"),
            ["stagedraw", "fighter", "stage", "particles", "scenery", "ui", "banner", "menuscene"]
        );
        assert_eq!(reads("menuscene"), ["gamedata", "shader"]);
        assert_eq!(reads("stagedraw"), ["gamedata", "scenery", "sky", "stagelight", "atmosphere"]);
        assert_eq!(reads("particles"), ["gamedata", "scenery", "banner", "menuscene"]);
        assert_eq!(reads("scenery"), ["gamedata"]);
        assert_eq!(reads("fighter"), ["gamedata"]);
        assert!(reads("ui").is_empty() && reads("banner").is_empty() && reads("literals").is_empty());
        assert_eq!(reads("atmosphere"), ["gamedata"]);
    }
}
