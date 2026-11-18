//! The build object paths of a character's meshes and AnimSequences (FighterData.paths):
//! what the game asks the asset manager for by name, keyed by the export's name as the build spells it.
//!
//! clips: the AnimSequences of the character's folder: those of its anim_dirs (default Animations, each with its
//! subfolders; '.' the folder's own files) and of EXTRA_CLIPS, and the sequences of its skins' montages (the slot
//! segments of each skin's Intro / Victory / DefeatMontageOverride, PreviewIdle and HomescreenIdle) that are under the
//! folder, minus its anim_skip; a data-only key that shares its owner's folder (companion_of: velmapolice) takes only
//! those of its own anim_dirs; then every sequence of ACTOR_CLIPS' folders. Two sequences of one name are one clip when
//! their objects' properties are the same (the first is taken), else an error; two names that differ by case alone are
//! an error.
//! meshes: every SkeletalMesh and StaticMesh package under the character's folder (recursive), then ACTOR_MESHES: a
//! name is a mesh of the folder (an error when the folder has none), a path a package of another folder. Two meshes of
//! one name: the first in listing order (a later one of another folder is an error).

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_data::fighter::Paths;

use super::bag::{arr_of, at, str_of};
use super::montages::{js_cmp, skipped};
use crate::fighter_port::{FighterPort, asset_name, asset_of, character_data_of, pkg_of, port_default_skin};
use crate::kind::{Cx, KindError};
use crate::kinds::gamedata::shape::{fail, properties};

const ANIM_SEQUENCE: &str = "/Script/Engine.AnimSequence";
const SKELETAL_MESH: &str = "/Script/Engine.SkeletalMesh";
const STATIC_MESH: &str = "/Script/Engine.StaticMesh";

/// Meshes of the character's spawned actors, props and podium montages that are not skin sub-meshes (each reason is in
/// the comments of its row). A name is a mesh package of the character's folder (checked to
/// be there), a path one of another folder under /Game.
const ACTOR_MESHES: &[(&str, &[&str])] = &[
    (
        "morty",
        &[
            "SM_c019_Ship_SkelMesh",
            "C019_Juice_Carton_SkelMesh",
            "C019_Juice_Stream_SkelMesh",
            "C019_Bones_SkelMesh",
            "SM_C019_AcidVat_01",
        ],
    ),
    // the podium montages' nunchucks and the sandwich (also the mesh of his projectile and item)
    ("shaggy", &["nunchucks_SkelMesh", "Sandwich"]),
    (
        "irongiant",
        &[
            "C017_Rocket",
            "AntenaMesh",
            "BoltMesh",
            "C017_Scrap_Base",
            "C017_Scrap_Switch",
            "C017_Scrap_Large",
            "C017_Base_Props_Low",
            "C017_DiscArms_SkelMesh",
        ],
    ),
    ("wonderwoman", &["WW_SkelMesh_JumpRope", "WonderWoman_Emote_Weight"]),
    (
        "harley",
        &["HarleyQuinn_Glove", "HarleyQuinn_ConfettiGun", "HarleyQuinn_Detonator", "HarleyQuinn_Pistol_SkelMesh"],
    ),
    // The file is c016_Hoop_SkelMesh (the object is C016_Hoop_SkelMesh).
    ("lebron", &["c016_Hoop_SkelMesh"]),
    ("arya", &["/Game/Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Pie", "Arya_Cloth_SkelMesh"]),
    (
        "batman",
        &[
            "SM_BatarangV2",
            "/Game/Panda_Main/Characters/Batman/VFX/SM_BatmanGrappleHook",
            "/Game/Panda_Main/Characters/Batman/VFX/SM_BatmanBomb01",
            "/Game/Panda_Main/Characters/Batman/VFX/SM_Bombarang_Bomb",
            "BatmanGun",
            "BatmanGun_Broken_Front",
            "BatmanGun_Broken_Back",
            "Batman_Radio",
            "Batman_Remote",
            "Batman_SharkRepelent",
        ],
    ),
    ("creature", &["/Game/Panda_Main/Characters/Common/SM_heart"]),
    ("blackadam", &["C021_Throne"]),
    (
        "bugs",
        &[
            "/Game/Panda_Main/Blueprints/Items/Item_TV/Item_TV_Mesh",
            "/Game/Panda_Main/Characters/ItemHelpers/Neptr/Neptr_Skelmesh",
            "/Game/Panda_Main/Characters/ItemHelpers/BananaGuard/BananaGuard_Skelmesh",
            "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/Meeseeks_SkelMesh",
            "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/SM_MeeseeksBox",
            "BugsBunny_Prop_Rocket_02",
            "BugsBunny_Prop_Box",
            "BugsBunny_Prop_Breifcase",
            "BugsBunny_Prop_Target",
            "BugsBunny_Prop_Pie",
            "BugsBunny_Prop_Brush",
            "BugsBunny_Prop_Safe",
            "BugsBunny_Prop_TopHat",
            "BugsBunny_Prop_Dynamite",
        ],
    ),
    (
        "rick",
        &[
            "/Game/Panda_Main/Characters/C020/VFX/SM_C020_LaserGun",
            "/Game/Panda_Main/Characters/C020/VFX/SM_C020PortalGun",
            "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/SM_MeeseeksBox",
            "PolymorphGun",
            "C020_MicProp",
            "C020_Megaseed",
            "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/Meeseeks_SkelMesh",
            "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/MrMeeseeks_GolfClub_SkelMesh",
            "/Game/Panda_Main/Characters/C020/VFX/SM_C020Portal",
            "/Game/Panda_Main/Characters/C020/VFX/SM_C020Satchel",
            "C020_fartbomb_SkelMesh",
            "/Game/Panda_Main/Characters/C017/Submeshes/C017_Rocket",
        ],
    ),
    (
        "taz",
        &[
            "C015_Cooking_Pot_Mesh",
            "C015_Pepper_Shaker_Mesh",
            "C015_Salad_Bowl_Mesh",
            "C015_Salt_Shaker_Mesh",
            "/Game/Panda_Main/Characters/Finn/Submeshes/Apple",
            "Chicken_Drumstick",
            "Chicken_SkelMesh",
            "C015_Anvil_Mesh_C015_anvil",
            "C015_Bouquet_SkelMesh",
            "C015_BouquetPetal_Mesh",
        ],
    ),
    (
        "steven",
        &[
            "Steven_Shield_SkelMesh",
            "Steven_Bubble_SkelMesh",
            "Steven_Barrier_SkelMesh",
            "Steven_SeparateShield_SkelMesh",
            "Steven_Cookie_Static",
        ],
    ),
    ("garnet", &["Garnet_Emote_Intro_Gems", "Butterfly_Skelmesh", "Garnet_TinyCar_SkelMesh"]),
    ("finn", &["Bmo_SkeletalMesh", "ChooseGoose_SkelMesh", "SkelMesh_FinnCoin", "Gem", "Apple", "Finn_Tooth"]),
    ("jake", &["Jake_BotHalf_SkelMesh", "Jake_StretchMesh", "SpitAimer", "S01_Stretch", "S01_LowerBody_Split"]),
    (
        "tomandjerry",
        &[
            "Champagne_Mesh",
            "Cheese",
            "Dynamite",
            "MouseTrap",
            "JerryRocket",
            "JerryRocket_NoRope",
            "Tom_FishingPole",
            "Trash_Lids",
            "Tennis_Racket",
            "Tom_Mallet",
            "PaddleBall",
            "PaddleBall_NoBall",
            "DefeatFlag",
            "Milk_Bottle",
            "SlingShot",
            "Champagne_Cork",
            "Cheese_Clump",
            "Cheese_SM",
            "FryingPan",
            "JerryBomb",
            "TomClub",
            "JoyStick",
            "JoyStick_Pad",
            "Lure",
            "LureBase",
            "Tennis_Rachet_Mesh",
            "TennisBall",
        ],
    ),
    (
        "velma",
        &[
            "Velma_MagnifyingGlass_SkelMesh",
            "Velma_Flashlight_SkelMesh",
            "Velma_Megaphone_SkelMesh",
            "Velma_Vial_SkelMesh",
            "Velma_Camera_SkelMesh",
            "Velma_Calculator_SkelMesh",
            "Velma_Evidence_SkelMesh",
            "Velma_Lightbulb_SkelMesh",
            "Velma_Proxy_Tablet",
            "SM_VelmaClue",
            "Velma_Broken_Glasses_L_Mesh",
            "Velma_Broken_Glasses_R_Mesh",
        ],
    ),
    // VelmaJenkinsPin's Mesh (Pawn_VelmaAIBotPolice @792 spawns it)
    ("velmapolice", &["Velma_JenkinsPin_SkelMesh"]),
    (
        "marvin",
        &[
            "C018_Spaceship_SkelMesh",
            "C018_SpaceshipRocket_SkelMesh",
            "C018_Blaster_gun_SkelMesh",
            "C018_Dynamite_stick_SkelMesh",
            "C018_RemoteButton_SkelMesh",
            "C018_TimeSpaceGun_SkelMesh",
            "C018_Trumpet_SkelMesh",
            "C018_UltimatumAnswerer_SkelMesh",
            "C018_Canon_Mesh",
        ],
    ),
    (
        "stripe",
        &[
            "Sawblade",
            "C023B_Boombox_SkelMesh",
            "C023B_Tricycle_SkelMesh",
            "Toys_Emote_Taunt",
            "C023B_Eggshell_SkelMesh",
        ],
    ),
    (
        "gizmo",
        &[
            "C023A_Corvette_SkelMesh",
            "C023A_Arrow_Mesh",
            "MusicNote_01",
            "MusicNote_02",
            "MusicNote_TrebleClef",
            "C023A_PopcornKernel_SkelMesh",
            "C023A_WallClock",
            "C023A_Glasses_SkelMesh",
            "C023A_Beanbag",
            "C023A_TV",
            // Pawn_C023A's FireArrowNullMesh, an engine shape a skin keeps as a template mesh
            "/Engine/BasicShapes/Plane",
        ],
    ),
];

/// Folders of other characters' or the game's actors whose AnimSequences a character plays (the sequences of a spawned
/// actor's own skeleton that its view plays). Bugs Bunny: the helpers' AnimBP sequences and the box runner's Meeseeks
/// sequences; Rick: the Meeseeks skeleton's clips.
const ACTOR_CLIPS: &[(&str, &[&str])] = &[
    ("bugs", &["/Game/Panda_Main/Characters/ItemHelpers", "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove"]),
    ("rick", &["/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove"]),
];

/// Further sequences under the character's folder: subpaths of the folder, a trailing '/' a folder with its subfolders.
const EXTRA_CLIPS: &[(&str, &[&str])] = &[
    (
        "morty",
        &["Skins/S02/Animations/C019_S02_Attack_Air_Axe_Swing", "Skins/S02/Animations/C019_S02_Attack_Air_Axe_Swing2"],
    ),
    ("irongiant", &["Submeshes/DiscArms/"]),
    ("wonderwoman", &["VFX/WW_JumpRope_Enter", "VFX/WW_SkelMesh_JumpRope_Loop"]),
    (
        "rick",
        &[
            "Meeseeks/",
            "Skins/S02/Animations/C020_S02_Attack_BlasterCharge",
            "Skins/S02/Animations/C020_S02_Attack_BlasterFire",
            "Skins/S02/Animations/C020_S02_Attack_BlasterChargedFire",
            "Skins/S02/Animations/C020_S02_Attack_Air_BlasterCharge",
            "Skins/S02/Animations/C020_S02_Attack_Air_BlasterFire",
            "Skins/S02/Animations/C020_S02_Attack_Air_BlasterChargedFire",
        ],
    ),
    ("taz", &["Chicken/"]),
    ("finn", &["Submeshes/"]),
    // Jake_S02's eat sequences: Jake_AnimBP's OriginsEatOverride machine (SequencePlayer_2..7) and the eat montages'
    // OriginsSkin / OriginsSpit* tracks, which the view draws on that skin (mvs-view chars/jake.rs)
    (
        "jake",
        &[
            "SubMeshes/JakeHorse/",
            "SubMeshes/JakeHouse/",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Idle",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Walk",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Fall",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Land",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Jump",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_In",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Hit",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Spit_F",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Spit_Up",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Spit_D",
            "Skins/S02/Animations/Jake_S02_Eat_Attack_Spit_Ally",
        ],
    ),
    ("tomandjerry", &["SubMeshes/"]),
    ("gizmo", &["SubMeshes/C023A_WallClock_Anim"]),
    // Garnet_Emote_Intro_Montage's and Garnet_Emote_Defeat_Montage's SubMeshSpawn gem clips, in the folder's own files
    ("garnet", &["Garnet_Emote_Intro_Gems_Anim", "Garnet_Emote_Defeat_Gems_Anim"]),
];

fn table(t: &'static [(&str, &'static [&'static str])], key: &str) -> &'static [&'static str] {
    t.iter().find(|(k, _)| *k == key).map_or(&[], |(_, v)| v)
}

fn base_name(p: &str) -> &str {
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

fn export_name(p: &str) -> &str {
    &p[p.rfind('.').map_or(0, |i| i + 1)..]
}

/// Whether a package under the character's folder is one of its sequences: under its anim_dirs, or (no companion key)
/// under EXTRA_CLIPS or among the skins' sequences. Compared without case: the build's paths are case-blind.
fn taken(key: &str, port: &FighterPort, pkg: &str, skin_seqs: &HashSet<String>) -> bool {
    let rel = pkg.get(port.folder.len() + 1..).unwrap_or_default().to_lowercase();
    if port.companion_of.is_none() && skin_seqs.contains(&pkg.to_lowercase()) {
        return true;
    }
    let under = |s: &str| {
        let s = s.to_lowercase();
        if s.ends_with('/') { rel.starts_with(&s) } else { rel == s }
    };
    if port
        .anim_dirs
        .unwrap_or(&["Animations"])
        .iter()
        .any(|d| if *d == "." { !rel.contains('/') } else { under(&format!("{d}/")) })
    {
        return true;
    }
    port.companion_of.is_none() && table(EXTRA_CLIPS, key).iter().any(|s| under(s))
}

/// The skin montage properties whose sequences are the character's clips too.
const SKIN_MONTAGES: [&str; 5] =
    ["IntroMontageOverride", "VictoryMontageOverride", "DefeatMontageOverride", "PreviewIdle", "HomescreenIdle"];

/// The packages (lower case) of the sequences the montages of the skins `skin_names` (FighterData.skins' keys) play in
/// their slot tracks: the default skin and the CharacterData Skins entries of those names.
fn skin_sequences(
    cx: &Cx<'_>,
    key: &str,
    port: &FighterPort,
    character_data: Option<&str>,
    skin_names: &[String],
) -> Result<HashSet<String>, KindError> {
    let mut out = HashSet::new();
    if skin_names.is_empty() {
        return Ok(out);
    }
    let cd = character_data_of(cx, character_data)?;
    let def = port_default_skin(cx, key, port, cd.as_ref())?;
    let mut by_name: HashMap<String, String> = HashMap::new();
    by_name.insert(asset_name(&def).to_lowercase(), def.clone());
    if let Some(cd) = &cd {
        for p in arr_of(properties(cd)?.get("Skins")) {
            let p = pkg_of(str_of(Some(p)).unwrap_or_default());
            by_name.entry(asset_name(p).to_lowercase()).or_insert_with(|| p.to_owned());
        }
    }
    let mut montages: Vec<String> = Vec::new();
    for n in skin_names {
        let Some(pkg) = by_name.get(&n.to_lowercase()) else {
            return fail(format!(
                "fighter paths {key}: skin {n} is neither the default nor in the CharacterData's Skins"
            ));
        };
        let sp = properties(&cx.object(&asset_of(pkg))?)?;
        for prop in SKIN_MONTAGES {
            if let Some(m) = super::skin::montage_ref(asset_name(pkg), &sp, prop) {
                let m = pkg_of(m).to_owned();
                if !montages.contains(&m) {
                    montages.push(m);
                }
            }
        }
    }
    for m in montages {
        if !cx.exists(&m) {
            continue;
        }
        let mp = properties(&cx.object(&asset_of(&m))?)?;
        for t in arr_of(mp.get("SlotAnimTracks")) {
            for seg in arr_of(at(t, "AnimTrack").and_then(|a| at(a, "AnimSegments"))) {
                if let Some(Value::Str(r)) = at(seg, "AnimReference") {
                    out.insert(pkg_of(r).to_lowercase());
                }
            }
        }
    }
    Ok(out)
}

/// The paths of a key (FighterData.paths): `skin_names` are FighterData.skins' keys, `character_data` its
/// CharacterData package (none for a data-only key).
pub fn bind_paths(
    cx: &Cx<'_>,
    key: &str,
    port: &FighterPort,
    character_data: Option<&str>,
    skin_names: &[String],
) -> Result<Paths, KindError> {
    let skin_seqs = skin_sequences(cx, key, port, character_data, skin_names)?;

    // clips
    let mut seq_pkgs: Vec<String> = cx
        .catalog
        .list_packages(port.folder, true, Some(ANIM_SEQUENCE))
        .into_iter()
        .filter(|p| taken(key, port, p, &skin_seqs) && !skipped(port, p))
        .collect();
    for d in table(ACTOR_CLIPS, key) {
        seq_pkgs.extend(cx.catalog.list_packages(d, true, Some(ANIM_SEQUENCE)));
    }
    let mut clip_obj: IndexMap<String, (String, String, mvs_data::Props)> = IndexMap::new();
    let mut lower: HashMap<String, String> = HashMap::new();
    for pkg in &seq_pkgs {
        let o = cx.object(&asset_of(pkg))?;
        let path = o.path().to_owned();
        let n = export_name(&path).to_owned();
        let props = properties(&o)?;
        if let Some((had_path, had_class, had_props)) = clip_obj.get(&n) {
            // one clip in two packages: the first is taken
            if had_class == o.class() && *had_props == props {
                continue;
            }
            return fail(format!("fighter paths {key}: two sequences named {n} ({had_path}, {path})"));
        }
        if let Some(l) = lower.get(&n.to_lowercase()) {
            return fail(format!("fighter paths {key}: two sequences differ by case alone: {l}, {n}"));
        }
        lower.insert(n.to_lowercase(), n.clone());
        clip_obj.insert(n, (path, o.class().to_owned(), props));
    }
    let mut names: Vec<&String> = clip_obj.keys().collect();
    names.sort_by(|a, b| js_cmp(a, b));
    let clips = names.into_iter().map(|n| (n.clone(), clip_obj[n].0.clone())).collect();

    // meshes
    let mut folder_pkgs = cx.catalog.list_packages(port.folder, true, Some(SKELETAL_MESH));
    folder_pkgs.extend(cx.catalog.list_packages(port.folder, true, Some(STATIC_MESH)));
    let by_base: HashSet<String> = folder_pkgs.iter().map(|p| base_name(p).to_lowercase()).collect();
    let mut far: Vec<String> = Vec::new();
    for m in table(ACTOR_MESHES, key) {
        if m.starts_with('/') {
            far.push((*m).to_owned());
        } else if !by_base.contains(&m.to_lowercase()) {
            return fail(format!("fighter paths {key}: ACTOR_MESHES names {m}, no mesh package of {}", port.folder));
        }
    }
    let mut meshes: IndexMap<String, String> = IndexMap::new();
    let mut mesh_lower: HashMap<String, String> = HashMap::new();
    let n_folder = folder_pkgs.len();
    for (i, pkg) in folder_pkgs.iter().chain(far.iter()).enumerate() {
        let o = cx.object(&asset_of(pkg))?;
        let path = o.path().to_owned();
        let n = export_name(&path).to_owned();
        let l = n.to_lowercase();
        if let Some(had) = mesh_lower.get(&l) {
            if i >= n_folder && meshes[had] != path {
                return fail(format!("fighter paths {key}: two meshes named {n} ({}, {path})", meshes[had]));
            }
            // two of the folder's own: the first in listing order
            continue;
        }
        meshes.insert(n.clone(), path);
        mesh_lower.insert(l, n);
    }
    Ok(Paths { meshes, clips })
}
