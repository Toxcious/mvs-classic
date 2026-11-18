//! What the game kinds share: their versions, their entry paths (which name the versions of what each reads, so a store
//! made at other versions has no entry where the game asks), the versions of the store's kinds they read, and the
//! stages and characters they are built for, read from the gamedata kind once per run.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::stage::ItemsData;
use mvs_data::{CharData, Index, StageEntry};

use crate::kind::{Cx, KindError};
use crate::kinds::animation::ANIMATION_KIND_VERSION;
use crate::kinds::config::CONFIG_KIND_VERSION;
use crate::kinds::image::IMAGE_KIND_VERSION;
use crate::kinds::lighting::LIGHTING_KIND_VERSION;
use crate::kinds::mesh::MESH_KIND_VERSION;
use crate::kinds::object::OBJECT_KIND_VERSION;
use crate::kinds::shader::SHADER_KIND_VERSION;
use crate::kinds::shadermap::SHADERMAP_KIND_VERSION;
use crate::kinds::skeleton::SKELETON_KIND_VERSION;
use crate::kinds::text::{GAME_CULTURE, TEXT_KIND_VERSION};
use crate::kinds::texture::TEXTURE_KIND_VERSION;

/// The folder of the stages' levels: a manifest's level is below it.
pub const MAPS_ROOT: &str = "/Game/Panda_Main/Maps/";

/// The stage bind's version: the stage manifest and data the game kinds read (through gamedata) are
/// that bind's, so it is in their versions and paths.
pub const STAGE_BIND_VERSION: u64 = 13;

const OBJECT: u64 = OBJECT_KIND_VERSION as u64;

/// Versions folded into one number, for a kind whose version names more versions than a digit composition holds:
/// FNV-1a (32 bits, as [`banner_kind_version`]) over each version's eight little-endian bytes, in order.
const fn fold(versions: &[u64]) -> u64 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < versions.len() {
        let bytes = versions[i].to_le_bytes();
        let mut j = 0;
        while j < bytes.len() {
            h = (h ^ bytes[j] as u32).wrapping_mul(0x0100_0193);
            j += 1;
        }
        i += 1;
    }
    h as u64
}

// gamedata

/// The native gamedata bind's own version (`kinds/gamedata`). Raised on any change to what a gamedata bind writes:
/// the kind's version holds it, so a player's store rebuilds the kind and two players of one build hold the same data.
/// 12: Jake_S01's intro from Pawn_Jake's graph (skin.rs PAWN_INTROS) with its clip (11: the Trail / AnimDynamics node props).
pub const GAMEDATA_BIND_VERSION: u64 = 12;

/// The `gamedata` kind's version: the fold of the bind's version and those of the kinds whose code the bind runs
/// in-process (the object reads, the mesh, skeleton, animation (the box frames' build), config, lighting and text
/// reads), times 1000, plus
/// the format the reader reads (`mvs_data::KIND_VERSION`, recovered by `mvs_data::format_of`).
pub const GAMEDATA_KIND_VERSION: u64 = fold(&[
    GAMEDATA_BIND_VERSION,
    OBJECT,
    MESH_KIND_VERSION,
    SKELETON_KIND_VERSION,
    ANIMATION_KIND_VERSION,
    CONFIG_KIND_VERSION,
    LIGHTING_KIND_VERSION,
    TEXT_KIND_VERSION,
]) * 1000
    + mvs_data::KIND_VERSION;

/// A path as the store names its entry: the export named as the package,
/// `<p>.<last segment of p>`.
pub fn exported(p: &str) -> String {
    format!("{p}.{}", &p[p.rfind('/').map_or(0, |i| i + 1)..])
}

// stage

/// The earlier `stage` kind version: the bind's, and those of what it reads
/// (the mesh, lighting, config and object kinds). The Items entry's path still names them.
const STAGE_ORACLE_VERSION: u64 = STAGE_BIND_VERSION * 1_000_000_000_000
    + MESH_KIND_VERSION * 1_000_000_000
    + LIGHTING_KIND_VERSION * 1_000_000
    + CONFIG_KIND_VERSION * 1000
    + OBJECT;

/// The `stage` kind's version: that number folded with the versions of the code it runs in-process besides its own
/// bind: the gamedata kind's (the items part's reach, `gamedata/items.rs`) and the fighter bind's (the draws'
/// mesh and slot reads, `fighter/draws.rs`).
// The entry's path is the earlier one, so the compare pairs the Items entries of the two packs.
pub const STAGE_KIND_VERSION: u64 = fold(&[STAGE_ORACLE_VERSION, GAMEDATA_KIND_VERSION, FIGHTER_BIND_VERSION]);

/// The stage kind's Items entry path, as the store names the export:
/// `/Stage/b<bind>o<object>c<config>l<lighting>m<mesh>/Items`.
pub fn stage_items_path() -> String {
    exported(&format!(
        "/Stage/b{STAGE_BIND_VERSION}o{OBJECT}c{CONFIG_KIND_VERSION}l{LIGHTING_KIND_VERSION}m{MESH_KIND_VERSION}/Items"
    ))
}

// material

/// The material entry's layout, the one mvs-material reads.
pub const MATERIAL_FORMAT_VERSION: u64 = mvs_material::MATERIAL_FORMAT_VERSION as u64;
/// The material bind's own version (4: the menuscene kind names DrawRefs).
pub const MATERIAL_BIND_VERSION: u64 = 4;
/// The `material` kind's version: its layout's, its bind's and those of what it reads.
pub const MATERIAL_KIND_VERSION: u64 = MATERIAL_FORMAT_VERSION * 1_000_000_000_000
    + MATERIAL_BIND_VERSION * 1_000_000_000
    + SHADERMAP_KIND_VERSION * 1_000_000
    + CONFIG_KIND_VERSION * 1000
    + OBJECT;

// stagedraw

/// The `stagedraw` kind's layout and bind version, the one mvs-render reads.
pub const STAGEDRAW_KIND_VERSION: u64 = 1;
const STAGEDRAW_DIR: &str = "/StageDraw";

/// A stage's stagedraw entry: `/StageDraw/<key>.<key>`.
pub fn stagedraw_path(key: &str) -> String {
    exported(&format!("{STAGEDRAW_DIR}/{key}"))
}

// sky

/// The sky bind's own version (3: the reflection captures).
pub const SKY_BIND_VERSION: u64 = 3;
/// The `sky` kind's version: the bind's and those of what it reads.
pub const SKY_KIND_VERSION: u64 = SKY_BIND_VERSION * 1_000_000_000_000
    + STAGE_BIND_VERSION * 1_000_000_000
    + LIGHTING_KIND_VERSION * 1_000_000
    + TEXTURE_KIND_VERSION * 1000
    + OBJECT;

/// A level's sky entry folder path: `/Sky/b<bind>s<stage>o<object>l<lighting>t<texture>/<level>`, the level below [`MAPS_ROOT`]
/// (a manifest's `sky`, `M011/Map_M011_Lighting`). The entry is its [`exported`] form.
pub fn sky_entry_path(level: &str) -> String {
    format!(
        "/Sky/b{SKY_BIND_VERSION}s{STAGE_BIND_VERSION}o{OBJECT}l{LIGHTING_KIND_VERSION}t{TEXTURE_KIND_VERSION}/{level}"
    )
}

/// The podium's level: its sky light lights the podium's fighters.
pub const PODIUM_SKY_LEVEL: &str = "Map_PodiumStage/Map_PodiumStage_New";

/// The levels the sky kind holds: each stage's sky level once, in first-seen order, then
/// the podium's (unless a stage's already).
pub fn sky_levels<'a>(skies: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in skies.into_iter().chain([PODIUM_SKY_LEVEL]) {
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    }
    out
}

// stagelight

/// The stagelight bind's own version.
pub const STAGELIGHT_BIND_VERSION: u64 = 1;
/// The `stagelight` kind's version: the bind's and those of what it reads.
pub const STAGELIGHT_KIND_VERSION: u64 =
    STAGELIGHT_BIND_VERSION * 1_000_000_000 + STAGE_BIND_VERSION * 1_000_000 + LIGHTING_KIND_VERSION * 1000 + OBJECT;

/// A stage's stagelight entry folder path: `/StageLight/b<bind>s<stage>o<object>l<lighting>/<key>`. The entry is its
/// [`exported`] form.
pub fn stagelight_entry_path(key: &str) -> String {
    format!("/StageLight/b{STAGELIGHT_BIND_VERSION}s{STAGE_BIND_VERSION}o{OBJECT}l{LIGHTING_KIND_VERSION}/{key}")
}

// atmosphere

/// The atmosphere bind's own version (2: an entry per level component,
/// past the first one).
pub const ATMOSPHERE_BIND_VERSION: u64 = 2;
/// The `atmosphere` kind's version: the bind's and those of the kinds an earlier bind read (the shader kinds for its
/// GLSL halves, which this port does not write: kept so the path and version stay).
pub const ATMOSPHERE_KIND_VERSION: u64 =
    ATMOSPHERE_BIND_VERSION * 1_000_000_000 + SHADERMAP_KIND_VERSION * 1_000_000 + SHADER_KIND_VERSION * 1000 + OBJECT;

/// The atmosphere kind's entry folder path of the level `level` (below [`MAPS_ROOT`]: `M009/Map_M009_Lighting`)
/// whose AtmosphericFogComponent the entry holds; the entry is its [`exported`] form.
pub fn atmosphere_path(level: &str) -> String {
    format!("/Atmosphere/b{ATMOSPHERE_BIND_VERSION}m{SHADERMAP_KIND_VERSION}s{SHADER_KIND_VERSION}o{OBJECT}/{level}")
}

/// The drawmat bind's version the earlier kinds name in their versions and paths: this importer writes no drawmat kind
/// (the material kind's verdicts stand in), but the scenery and banner kinds keep those numbers, as atmosphere
/// kept its shader numbers.
pub const DRAWMAT_BIND_VERSION: u64 = 15;

// scenery

/// The scenery bind's own version (6 at first; 7 since the water plane's material is built;
/// 8 since the five lightmapped translucencies draw in `static`).
pub const SCENERY_BIND_VERSION: u64 = 8;
/// The `scenery` kind's version: the bind's and those of what it reads.
pub const SCENERY_KIND_VERSION: u64 =
    SCENERY_BIND_VERSION * 1_000_000_000 + DRAWMAT_BIND_VERSION * 1_000_000 + STAGE_BIND_VERSION * 1000 + OBJECT;

/// A stage's scenery entry folder path:
/// `/Scenery/b<bind>d<drawmat>s<stage>o<object>l<lighting>m<mesh>/<key>`; the entry is its [`exported`] form.
pub fn scenery_entry_path(key: &str) -> String {
    format!(
        "/Scenery/b{SCENERY_BIND_VERSION}d{DRAWMAT_BIND_VERSION}s{STAGE_BIND_VERSION}o{OBJECT}l{LIGHTING_KIND_VERSION}m{MESH_KIND_VERSION}/{key}"
    )
}

// particles

/// The particles bind's own version (9, plus one: the tutorial
/// Meeseeks' PS_MeeseeksDestroyed and PS_JetPackFlame_Looping; plus one: B2's effects (Stripe's saw and boombox
/// decals, Comp_State_Manager's PS_Buff_Haste_Speed, Knockout Bounce's PS_BatmanBombExplosion, the arcade
/// bosses' PS_RageEyes, Morty's scrub shield's three Plumbus systems).
pub const PARTICLES_BIND_VERSION: u64 = 11;
/// The `particles` kind's version: the bind's, those its banner probe's verdicts (`bannersprite` / `bannerlocal`)
/// rest on (DRAWMAT_BIND_VERSION, the banner kind's `d`; the shadermap kind's maps and the shader kind's code whose
/// custom stencil reads it decodes, as the menuscene kind's), and the mesh and object kinds'. Its change from
/// 11_002_011 also carries Morty's PS_C019_Ship_Portal and PS_C019_Ship_Summon (the bind's folder kept: the banner
/// kind's path names it).
pub const PARTICLES_KIND_VERSION: u64 = PARTICLES_BIND_VERSION * 1_000_000_000_000_000
    + DRAWMAT_BIND_VERSION * 1_000_000_000_000
    + SHADERMAP_KIND_VERSION * 1_000_000_000
    + SHADER_KIND_VERSION * 1_000_000
    + MESH_KIND_VERSION * 1000
    + OBJECT;

/// The folder the particles entries are below.
pub fn particles_dir() -> String {
    format!("/Particles/b{PARTICLES_BIND_VERSION}o{OBJECT}m{MESH_KIND_VERSION}")
}

/// A ParticleSystem's entry path: its build object path below the folder.
pub fn particles_path(system: &str) -> String {
    particles_dir() + system
}

/// The index entry's path (the owners of every system, material, mesh and
/// decal).
pub fn particles_index_path() -> String {
    particles_dir() + "/Index.Owners"
}

/// The system's build object path of an entry path of this bind; `None` for
/// another path or the index.
pub fn particles_system(path: &str) -> Option<&str> {
    let dir = particles_dir();
    (path.starts_with(&(dir.clone() + "/")) && path != particles_index_path()).then(|| &path[dir.len()..])
}

/// The package of a build object path, cut at its first `.` (packageOf:
/// `/Game/A/B.B:Sub` -> `/Game/A/B`).
pub fn package_of(p: &str) -> &str {
    p.find('.').map_or(p, |i| &p[..i])
}

/// The short name of a system: its package's last part, as the game spawns it and the player keys it.
pub fn system_name(p: &str) -> &str {
    let pkg = package_of(p);
    &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..]
}

// ui

/// The ui bind's own version (27, plus one: the textures' `filter`; plus one: the menu
/// bundle's UI_LAB_Hotkeys; plus one: UI_SocialContextMenu; plus one: no UI_ConnectionState).
pub const UI_BIND_VERSION: u64 = 31;
/// The `ui` kind's version: the bind's and the object kind's.
pub const UI_KIND_VERSION: u64 = UI_BIND_VERSION * 1000 + OBJECT;
/// The ui kind's entries: the HUD's widget bundle, the menus' and the loading screen.
pub const UI_ENTRIES: [&str; 3] = ["hud", "menu", "loading"];

/// An entry's folder path: `/Ui/b<bind>o<object>t<text>i<image>/<entry>`; the entry is
/// its [`exported`] form (`uiPath`).
pub fn ui_entry_path(entry: &str) -> String {
    format!("/Ui/b{UI_BIND_VERSION}o{OBJECT}t{TEXT_KIND_VERSION}i{IMAGE_KIND_VERSION}/{entry}")
}

// fighter

/// The fighter bind's own version (12, plus one: the entry's `blendSpaces`,
/// plus one: the SM_JetBoot actor row; plus one: the TLM_Surface* slots named, the bubble and C017 podium rows).
pub const FIGHTER_BIND_VERSION: u64 = 15;
/// The `fighter` kind's version: the bind's and those of the mesh, text and object kinds.
pub const FIGHTER_KIND_VERSION: u64 =
    FIGHTER_BIND_VERSION * 1_000_000_000 + MESH_KIND_VERSION * 1_000_000 + TEXT_KIND_VERSION * 1000 + OBJECT;

/// A key's entry folder path:
/// `/Fighter/b<bind>m<mesh>o<object>t<text>/<culture>/<key>`; the entry is its [`exported`] form.
pub fn fighter_entry_path(key: &str) -> String {
    format!("/Fighter/b{FIGHTER_BIND_VERSION}m{MESH_KIND_VERSION}o{OBJECT}t{TEXT_KIND_VERSION}/{GAME_CULTURE}/{key}")
}

// banner

/// The banner bind's own version.
pub const BANNER_BIND_VERSION: u64 = 1;

/// The folder the banner entries are below.
pub fn banner_dir() -> String {
    format!(
        "/Banner/b{BANNER_BIND_VERSION}d{DRAWMAT_BIND_VERSION}s{SHADERMAP_KIND_VERSION}h{SHADER_KIND_VERSION}p{PARTICLES_BIND_VERSION}o{OBJECT}m{MESH_KIND_VERSION}"
    )
}

/// The `banner` kind's version: the folder's versions as one number, FNV-1a of [`banner_dir`] over its UTF-16
/// code units.
pub fn banner_kind_version() -> u64 {
    let mut h: u32 = 0x811c_9dc5;
    for c in banner_dir().encode_utf16() {
        h = (h ^ u32::from(c)).wrapping_mul(0x0100_0193);
    }
    u64::from(h)
}

/// FNV-1a over the ASCII `s` from `h` (its UTF-16 code units are its bytes).
const fn fnv_str(mut h: u32, s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// FNV-1a over `v` written in decimal from `h`.
const fn fnv_num(mut h: u32, v: u64) -> u32 {
    let mut digits = [0u8; 20];
    let (mut n, mut v) = (0, v);
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while n > 0 {
        n -= 1;
        h = (h ^ digits[n] as u32).wrapping_mul(0x0100_0193);
    }
    h
}

/// [`banner_kind_version`] at compile time: the same folder name hashed piece by piece.
const fn banner_kind_version_const() -> u64 {
    let parts: [(&str, u64); 7] = [
        ("/Banner/b", BANNER_BIND_VERSION),
        ("d", DRAWMAT_BIND_VERSION),
        ("s", SHADERMAP_KIND_VERSION),
        ("h", SHADER_KIND_VERSION),
        ("p", PARTICLES_BIND_VERSION),
        ("o", OBJECT),
        ("m", MESH_KIND_VERSION),
    ];
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < parts.len() {
        h = fnv_num(fnv_str(h, parts[i].0), parts[i].1);
        i += 1;
    }
    h as u64
}

/// A banner's entry path: its BannerData's build object path below the folder.
pub fn banner_path(data: &str) -> String {
    banner_dir() + data
}

/// The index entry's path (slug -> entry).
pub fn banner_index_path() -> String {
    banner_dir() + "/Index.Banners"
}

// literals

/// The `literals` kind's version.
pub const LITERALS_KIND_VERSION: u64 = 1;

// menuscene

/// The menuscene bind's version (3: materials as DrawRefs, the skins
/// from the gamedata kind; 4: a preview entry holds its skin's HomescreenIdle, the MainMenu entry no character; 5: the
/// MainMenu entry holds MainMenuBackground's Event Lookup Table; 6: the PreMatch entry holds the previews'
/// BlobShadowCenter, the podium's lights their LightGuid; 7: the Podium entry holds the previews' BlobShadowCenter).
pub const MENUSCENE_BIND_VERSION: u64 = 7;

/// The menuscene kind's versioned folder; its entries' names below it are
/// `mvs_material::menuscene` level_suffix and preview_suffix.
pub fn menuscene_dir() -> String {
    format!(
        "/MenuScene/b{MENUSCENE_BIND_VERSION}d{DRAWMAT_BIND_VERSION}s{SHADERMAP_KIND_VERSION}h{SHADER_KIND_VERSION}o{OBJECT}"
    )
}

// the store's kinds the game kinds read: this importer's versions where it builds the kind, else the earlier ones

/// The gamedata kind (the stage list, manifests and data; the characters' data, the items part).
pub const GAMEDATA: (&str, u64) = (mvs_data::KIND, GAMEDATA_KIND_VERSION);
/// The scenery kind.
pub const SCENERY: (&str, u64) = ("scenery", SCENERY_KIND_VERSION);
/// The material namers: the kinds whose entries name DrawRefs.
pub const FIGHTER: (&str, u64) = ("fighter", FIGHTER_KIND_VERSION);
/// The stage kind: the native import builds its Items entry alone (`kinds/stage.rs`).
pub const STAGE: (&str, u64) = ("stage", STAGE_KIND_VERSION);
pub const PARTICLES: (&str, u64) = ("particles", PARTICLES_KIND_VERSION);
/// The sky, stagelight and atmosphere kinds: their entries' paths hold their versions, so the stagedraw kind, which
/// names them, is rebuilt with them.
pub const SKY: (&str, u64) = ("sky", SKY_KIND_VERSION);
pub const STAGELIGHT: (&str, u64) = ("stagelight", STAGELIGHT_KIND_VERSION);
pub const ATMOSPHERE: (&str, u64) = ("atmosphere", ATMOSPHERE_KIND_VERSION);
pub const UI: (&str, u64) = ("ui", UI_KIND_VERSION);
/// The banner kind; its version is [`banner_kind_version`], hashed at compile time (a test holds them equal).
pub const BANNER: (&str, u64) = ("banner", banner_kind_version_const());
/// The menuscene kind: a namer of the particle systems the menus'
/// levels spawn and of the DrawRefs their materials draw with.
/// Its number holds the bind's version and those its banner probe's verdicts rest on (DRAWMAT_BIND_VERSION, the
/// banner kind's `d`; the shadermap kind's maps and the shader kind's code whose custom stencil reads it decodes),
/// and the object kind's.
pub const MENUSCENE: (&str, u64) = (
    "menuscene",
    MENUSCENE_BIND_VERSION * 1_000_000_000_000
        + DRAWMAT_BIND_VERSION * 1_000_000_000
        + SHADERMAP_KIND_VERSION * 1_000_000
        + SHADER_KIND_VERSION * 1000
        + OBJECT,
);

/// The stages as the gamedata kind gives them: the keys in the stage manifest's order (`Index.stages`)
/// and each one's entry (its StageData and manifest entry).
pub struct Stages {
    pub keys: Vec<String>,
    pub entries: IndexMap<String, StageEntry>,
}

impl Stages {
    /// The entry of stage `key`.
    pub fn get(&self, key: &str) -> Result<&StageEntry, KindError> {
        self.entries.get(key).ok_or_else(|| KindError::NotFound(format!("gamedata: no stage {key}")))
    }
}

/// The run's one read of them.
struct StagesSlot(Result<Arc<Stages>, String>);

/// The stages, read from the store's gamedata kind once per run (the first asker reads; a failure is every asker's).
/// A kind that calls it has [`GAMEDATA`] in its `reads`.
pub fn stages(cx: &Cx<'_>) -> Result<Arc<Stages>, KindError> {
    let slot = cx.shared.get_or_init(|| StagesSlot(read_stages(cx).map(Arc::new).map_err(|e| e.to_string())));
    slot.0.clone().map_err(KindError::Failed)
}

/// A gamedata entry, decoded.
fn read<T: serde::de::DeserializeOwned>(cx: &Cx<'_>, path: &str) -> Result<T, KindError> {
    let bytes = cx.store.entry(GAMEDATA.0, path)?;
    mvs_data::decode(&bytes).map_err(|e| KindError::Failed(format!("gamedata {path}: {e}")))
}

fn read_stages(cx: &Cx<'_>) -> Result<Stages, KindError> {
    let index: Index = read(cx, mvs_data::path::INDEX)?;
    let mut entries = IndexMap::new();
    for key in &index.stages {
        entries.insert(key.clone(), read::<StageEntry>(cx, &mvs_data::path::stage(key))?);
    }
    Ok(Stages { keys: index.stages, entries })
}

/// The characters as the gamedata kind gives them: the keys (the roster's, `Index.characters`, then the data-only
/// keys a match spawns, `Index.data_only`), each one's data (its FighterData as `fighter`; its skin
/// audio), and the items part (`/GameData/Items`: the stage kind's Items data).
pub struct Chars {
    pub keys: Vec<String>,
    pub entries: IndexMap<String, CharData>,
    pub items: ItemsData,
}

impl Chars {
    /// The data of key `key`.
    pub fn get(&self, key: &str) -> Result<&CharData, KindError> {
        self.entries.get(key).ok_or_else(|| KindError::NotFound(format!("gamedata: no character {key}")))
    }
}

/// The run's one read of them.
struct CharsSlot(Result<Arc<Chars>, KindError>);

/// The characters and the items part, read from the store's gamedata kind once per run (the first asker reads; a
/// failure is every asker's). A kind that calls it has [`GAMEDATA`] in its `reads`.
pub fn chars(cx: &Cx<'_>) -> Result<Arc<Chars>, KindError> {
    cx.shared.get_or_init(|| CharsSlot(read_chars(cx).map(Arc::new))).0.clone()
}

fn read_chars(cx: &Cx<'_>) -> Result<Chars, KindError> {
    let index: Index = read(cx, mvs_data::path::INDEX)?;
    let keys: Vec<String> = index.characters.iter().chain(&index.data_only).cloned().collect();
    let mut entries = IndexMap::new();
    for key in &keys {
        entries.insert(key.clone(), read::<CharData>(cx, &mvs_data::path::char(key))?);
    }
    Ok(Chars { keys, entries, items: read(cx, mvs_data::path::ITEMS)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_paths() {
        // the earlier store's versions and paths
        assert_eq!(MATERIAL_KIND_VERSION, 1_004_002_001_011);
        assert_eq!(SKY_KIND_VERSION, 3_013_001_004_011);
        assert_eq!(STAGELIGHT_KIND_VERSION, 1_013_001_011);
        assert_eq!(ATMOSPHERE_KIND_VERSION, 2_002_001_011);
        assert_eq!(stagedraw_path("M011"), "/StageDraw/M011.M011");
        assert_eq!(
            exported(&sky_entry_path("M011/Map_M011_Lighting")),
            "/Sky/b3s13o11l1t4/M011/Map_M011_Lighting.Map_M011_Lighting"
        );
        assert_eq!(stagelight_entry_path("M011"), "/StageLight/b1s13o11l1/M011");
        assert_eq!(
            exported(&atmosphere_path("M009/Map_M009_Lighting")),
            "/Atmosphere/b2m2s1o11/M009/Map_M009_Lighting.Map_M009_Lighting"
        );
        assert_eq!(exported(&scenery_entry_path("M011")), "/Scenery/b8d15s13o11l1m2/M011.M011");
        assert_eq!(SCENERY_KIND_VERSION, 8_015_013_011);
        assert_eq!(PARTICLES_KIND_VERSION, 11_015_002_001_002_011);
        assert_eq!(UI_KIND_VERSION, 31_011);
        assert_eq!(FIGHTER_KIND_VERSION, 15_002_001_011);
        assert_eq!(MENUSCENE.1, 7_015_002_001_011);
        assert_eq!(menuscene_dir(), "/MenuScene/b7d15s2h1o11");
        assert_eq!(banner_kind_version(), BANNER.1);
        assert_eq!(banner_dir(), "/Banner/b1d15s2h1p11o11m2");
        let sys = "/Game/Panda_Main/VFX/P_Hit.P_Hit";
        assert_eq!(particles_path(sys), "/Particles/b11o11m2/Game/Panda_Main/VFX/P_Hit.P_Hit");
        assert_eq!(particles_system(&particles_path(sys)), Some(sys));
        assert_eq!(particles_system(&particles_index_path()), None);
        assert_eq!(particles_index_path(), "/Particles/b11o11m2/Index.Owners");
        assert_eq!(system_name("/Game/A/P_X.P_X:Sub"), "P_X");
        assert_eq!(package_of("/Game/A/P_X"), "/Game/A/P_X");
        assert_eq!(exported(&ui_entry_path("menu")), "/Ui/b31o11t1i1/menu.menu");
        assert_eq!(exported(&fighter_entry_path("arya")), "/Fighter/b15m2o11t1/en/arya.arya");
        assert_eq!(banner_index_path(), "/Banner/b1d15s2h1p11o11m2/Index.Banners");
        assert_eq!(sky_levels(["A/a", "B/b", "A/a"]), ["A/a", "B/b", PODIUM_SKY_LEVEL]);
        assert_eq!(sky_levels([PODIUM_SKY_LEVEL, "A/a"]), [PODIUM_SKY_LEVEL, "A/a"]);
    }

    #[test]
    fn gamedata_and_stage_versions() {
        // the reader's format is recoverable; the fold moves with any version folded in
        assert_eq!(mvs_data::format_of(GAMEDATA_KIND_VERSION), mvs_data::KIND_VERSION);
        assert_ne!(fold(&[1, 11]), fold(&[2, 11]));
        assert_ne!(fold(&[1, 11]), fold(&[11, 1]));
        assert_eq!(STAGE_ORACLE_VERSION, 13_002_001_001_011);
        assert_ne!(STAGE_KIND_VERSION, STAGE_ORACLE_VERSION);
        assert_eq!(exported("/Stage/b13o11c1l1m2/Items"), stage_items_path());
    }
}
