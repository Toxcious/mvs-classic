//! A fighter's skin: its meshes, parts and material slots per team variant (from the fighter
//! kind's draws and the sim's skin sub-mesh tables), and each fighter's
//! colours in a match.

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_data::fighter::{Component, FighterData, Map, SubMesh};
use mvs_sim::setup::{MatchMode, MatchPlayer, MatchSetup, TeamStyle, player_team};

use crate::{Error, Result};

/// SkinVariant: the material set a fighter's body takes (SelectMaterialColor).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Red,
    Blue,
    Local,
    Menu,
}

impl Variant {
    pub fn key(self) -> &'static str {
        match self {
            Variant::Red => "red",
            Variant::Blue => "blue",
            Variant::Local => "local",
            Variant::Menu => "menu",
        }
    }
}

/// `FighterColour`: a fighter's material set, Team value, outline stencil and armor overlay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FighterColour {
    /// GetColorIndex Index: 1 blue, 0 red.
    pub color_index: u8,
    pub variant: Variant,
    /// SelectFloat(0.6, ColorIndex, IsLocalOnlinePlayer): the body materials' Team scalar.
    pub team: f64,
    /// BaseFighter YouOutline 4, Ally Outline 2, EnemyOutline 3, Team3Outline 5, Team4Outline 6.
    pub stencil: u8,
    /// IsLocalOrMeOnline: ArmorBuffOverlay_Yellow_MAT, else ArmorBuffOverlay_MAT.
    pub armor_local: bool,
    pub armor_team: f64,
    /// The HUD broker's TeamColor (UI_Functions Color for Team), as a palette index (`palette_for_index`).
    pub team_palette: usize,
    /// The broker's FocusColor (Color for Player; Local Play the team's), as a palette index.
    pub focus_palette: usize,
    /// The broker's IsLocal (offline: every pawn is locally controlled).
    pub broker_local: bool,
    /// APawn::IsLocallyControlled as a render-side graph reads it: a
    /// human player on this machine (offline every human, online the local player alone).
    pub local_human: bool,
    /// The outline preference's test of `stencil` (MatchHost.outlineShown: `outline_shown`).
    pub outline: bool,
}

/// The UI preferences the match's views read (Struct_Preferences_UI's world-useTeamColoredSkins, the outline toggles
/// and the colour bindings), as the match's start has them.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewPrefs {
    pub team_skins: bool,
    pub outlines: OutlinePrefs,
    /// colorAlly, colorEnemy, colorMe, colorTeam3, colorTeam4: the CentralPalette row each GLOBAL PaletteEntryType
    /// resolves to.
    pub colors: [String; 5],
}

impl Default for ViewPrefs {
    fn default() -> ViewPrefs {
        ViewPrefs { team_skins: true, outlines: OutlinePrefs::default(), colors: COLOR_ROWS.map(str::to_owned) }
    }
}

impl ViewPrefs {
    /// The colour bindings as row names.
    pub fn bound(&self) -> [&str; 5] {
        std::array::from_fn(|k| self.colors[k].as_str())
    }
}

/// world-showAllyOutlines, world-showEnemyOutlines, world-showMyOutline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutlinePrefs {
    pub ally: bool,
    pub enemy: bool,
    pub me: bool,
}

impl Default for OutlinePrefs {
    fn default() -> OutlinePrefs {
        OutlinePrefs { ally: true, enemy: true, me: true }
    }
}

/// Struct_Preferences_UI's colour bindings' defaults (colorAlly, colorEnemy, colorMe, colorTeam3, colorTeam4): the
/// CentralPalette row each GLOBAL PaletteEntryType resolves to.
const COLOR_ROWS: [&str; 5] = ["Blue1", "Red1", "Yellow1", "Green1", "Purple1"];

/// An RGBA colour of the central palette.
pub type Rgba = [f64; 4];

/// UI_Functions PaletteForIndex(index).UI (@11-147, @2228): a palette index's PaletteEntryType (Ally, Enemy, Team3,
/// Team4, Me), its CentralPalette row (`rows`) through the colour bindings; an index outside 0..4 gets the zero
/// entry, a binding that names no row GetPaletteEntry's (1, 0, 1, 1) (@766-1034).
pub fn palette_for_index(rows: &IndexMap<String, Rgba>, index: usize) -> Rgba {
    palette_for_index_in(rows, index, COLOR_ROWS)
}

/// `palette_for_index` through the colour bindings `bound` (by GLOBAL PaletteEntryType 0..4).
pub fn palette_for_index_in(rows: &IndexMap<String, Rgba>, index: usize, bound: [&str; 5]) -> Rgba {
    const TYPES: [usize; 5] = [0, 1, 3, 4, 2];
    let Some(&ty) = TYPES.get(index) else { return [0.0; 4] };
    rows.get(bound[ty]).copied().unwrap_or([1.0, 0.0, 1.0, 1.0])
}

/// MPC TutorialEnvironmentDesat's object path: its one
/// scalar, Desaturation, is row 0's x.
pub const TUTORIAL_DESAT: &str =
    "/Game/Panda_Main/Blueprints/Tutorial/TutorialEnvironmentDesat.TutorialEnvironmentDesat";

/// MPC_CentralPalette's object path.
pub const CENTRAL_PALETTE: &str = "/Game/Panda_Main/Materials/MPC_CentralPalette.MPC_CentralPalette";
/// Its VectorParameters in the asset's order (no scalars): row k of its buffer is vector k.
pub const CENTRAL_PALETTE_VECTORS: [&str; 5] = ["ColorMe", "ColorAlly", "ColorEnemy", "ColorTeam3", "ColorTeam4"];

/// Each vector PreferencesManager "Apply UI Material Colors" sets, in the order of its GLOBAL PaletteEntryType (0 Ally
/// .. 4 Team4: the order of `COLOR_ROWS` and of the bindings a caller passes).
const MPC_BINDINGS: [&str; 5] = ["ColorAlly", "ColorEnemy", "ColorMe", "ColorTeam3", "ColorTeam4"];

/// MPC_CentralPalette's buffer rows as the game sets them: the collection's defaults (`defaults`, by vector
/// name; one missing is zero), then Apply UI Material Colors (@0-1246): each
/// vector of `MPC_BINDINGS` from the Game colour (`game`) of the row its binding names (`bound`, by PaletteEntryType
/// 0..4); a binding that names no row leaves its vector.
pub fn central_palette(
    defaults: &IndexMap<String, [f32; 4]>,
    game: &IndexMap<String, Rgba>,
    bound: [&str; 5],
) -> Vec<[f32; 4]> {
    CENTRAL_PALETTE_VECTORS
        .iter()
        .map(|n| {
            let ty = MPC_BINDINGS.iter().position(|b| b == n);
            match ty.and_then(|t| game.get(bound[t])) {
                #[allow(clippy::cast_possible_truncation, reason = "the collection's buffer holds float32")]
                Some(c) => c.map(|x| x as f32),
                None => defaults.get(*n).copied().unwrap_or([0.0; 4]),
            }
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct PaletteRow {
    #[serde(rename = "R", default)]
    r: f64,
    #[serde(rename = "G", default)]
    g: f64,
    #[serde(rename = "B", default)]
    b: f64,
    #[serde(rename = "A", default = "one")]
    a: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HudPalette {
    #[serde(default)]
    palette: IndexMap<String, PaletteRow>,
    #[serde(default)]
    palette_game: IndexMap<String, PaletteRow>,
    #[serde(default)]
    palette_defaults: IndexMap<String, Vec<f64>>,
}

/// The CentralPalette rows the HUD bundle holds, by row name, linear.
#[derive(Clone, Debug, Default)]
pub struct PaletteRows {
    /// The UI colour (the widgets').
    pub ui: IndexMap<String, Rgba>,
    /// The Game colour (MPC_CentralPalette's, PreferencesManager Apply UI Material Colors).
    pub game: IndexMap<String, Rgba>,
    /// MPC_CentralPalette's vectors' own defaults by parameter name.
    pub defaults: IndexMap<String, [f32; 4]>,
}

impl PaletteRows {
    /// MPC_CentralPalette's rows as the default UI preferences set them (`central_palette`).
    // Deviation: the default UI preferences' bindings (a picture's tool has no player's settings to read).
    pub fn central_palette(&self) -> Vec<[f32; 4]> {
        central_palette(&self.defaults, &self.game, COLOR_ROWS)
    }

    /// MPC_CentralPalette's ColorMe as Apply UI Material Colors sets it (@0-1246): the Game colour of the row the
    /// colorMe binding names; None: no such row (the vector keeps its value).
    // Deviation: the default UI preferences' binding (the poser has no menu state to read the player's pick from).
    pub fn color_me(&self) -> Option<Rgba> {
        self.color_me_in(COLOR_ROWS)
    }

    /// `color_me` through the colour bindings `bound` (by GLOBAL PaletteEntryType 0..4).
    pub fn color_me_in(&self, bound: [&str; 5]) -> Option<Rgba> {
        self.game.get(bound[2]).copied()
    }
}

/// The CentralPalette rows the HUD bundle (the ui kind's `hud` entry) holds; None: the store has no HUD (a match
/// with no broker: the paths that test for the broker's blueprint).
pub fn hud_palette(store: &mvs_store::Store) -> Result<Option<PaletteRows>> {
    let Ok(pack) = store.kind("ui") else { return Ok(None) };
    let Some(path) = pack.paths().find(|p| p.ends_with("/hud.hud")).map(str::to_string) else { return Ok(None) };
    let raw: HudPalette = serde_json::from_slice(&pack.bytes(&path)?)
        .map_err(|e| Error::Format { what: path.clone(), why: e.to_string() })?;
    let rows = |m: IndexMap<String, PaletteRow>| m.into_iter().map(|(k, c)| (k, [c.r, c.g, c.b, c.a])).collect();
    #[allow(clippy::cast_possible_truncation, reason = "the collection's buffer holds float32")]
    let defaults = raw
        .palette_defaults
        .into_iter()
        .map(|(k, v)| (k, std::array::from_fn(|i| v.get(i).copied().unwrap_or(0.0) as f32)))
        .collect();
    Ok(Some(PaletteRows { ui: rows(raw.palette), game: rows(raw.palette_game), defaults }))
}

/// `fighterColours` (the rules are in its body): `me` the local player,
/// `playing_online` a relay match, `team_skins` the world-useTeamColoredSkins preference.
pub fn fighter_colours(setup: &MatchSetup, me: usize, playing_online: bool, team_skins: bool) -> Vec<FighterColour> {
    // The port's own Waves mode: its set-up holds the player alone, and its enemies' slot (`ENEMY_SLOT`, 1) takes
    // the colours a 1v1's second player gets (a bot on team 1: the enemy outline, the red variant).
    if matches!(setup.mode, Some(MatchMode::Waves { .. })) && setup.players.len() == 1 {
        const _: () = assert!(mvs_sim::modes::waves::ENEMY_SLOT == 1);
        let mut s = setup.clone();
        s.players.push(MatchPlayer { bot: true, team: Some(1), ..MatchPlayer::default() });
        return fighter_colours(&s, me, playing_online, team_skins);
    }
    // a set-up without rules is the Lab: TrainGameMode, game mode type 3, GetColorIndex's online branch
    let lab = setup.rules.is_none();
    let online = lab || playing_online;
    let ffa = setup.team_style == TeamStyle::FreeForAll;
    let mine = player_team(setup, me);
    let one_human = setup.players.iter().filter(|p| !p.bot).count() == 1;
    (0..setup.players.len())
        .map(|i| {
            let p = &setup.players[i];
            let team = player_team(setup, i);
            // Unverified: teams 2 and 3 offline take the select's unwritten default, read as 0.
            let color_index = if online { u8::from(team == mine) } else { u8::from(team == 0) };
            let local_online = online && i == me;
            // Unverified: a bot is never IsMe.
            let is_me = i == me && !p.bot;
            // Unverified: in Local Play IsLocallyControlled is taken as true for the pawns of PCs 1..3 in standalone.
            let local = if setup.local_play {
                !p.bot && (ffa || (i == 0 && one_human))
            } else {
                is_me && (online || ffa || one_human)
            };
            let variant = if local || !team_skins {
                Variant::Local
            } else if color_index == 1 {
                Variant::Blue
            } else {
                Variant::Red
            };
            let palette = if lab || setup.local_play {
                usize::from(team)
            } else if team == mine {
                0
            } else if ffa {
                (0..4u8).filter(|&t| t != mine).position(|t| t == team).map_or(0, |k| k + 1)
            } else {
                1
            };
            // UI_Functions Color for Team: the Lab the team index, Free For All my team 0 and the others 1 + their
            // place among the other team indexes, two teams mine 0 and the other 1 (mvs-ui hud.rs team_palette)
            let team_palette = |t: u8| {
                if lab {
                    usize::from(t)
                } else if ffa && t != mine {
                    (0..4u8).filter(|&o| o != mine).position(|o| o == t).map_or(0, |k| 1 + k)
                } else {
                    usize::from(t != mine)
                }
            };
            // Color for Player: online IsMe ? 4 : my team 0, else 1; offline in the Lab
            // or with exactly one human IsMe ? 4 : the team index; Local Play the team colour (BaseFighter GetColor)
            let focus_palette = if setup.local_play {
                team_palette(team)
            } else if playing_online {
                if i == me { 4 } else { usize::from(team != mine) }
            } else if (lab || one_human) && i == me {
                4
            } else {
                usize::from(team)
            };
            // Unverified: a Local Play team past the build's four (the port's big matches) takes the fourth
            // team's stencil.
            let stencil = if palette == 0 { if local_online { 4 } else { 2 } } else { [3, 5, 6][(palette - 1).min(2)] };
            FighterColour {
                color_index,
                variant,
                team: if local_online { 0.6 } else { f64::from(color_index) },
                stencil,
                // Unverified: a bot on the hosting machine is taken as not locally controlled (IsLocalOrMeOnline).
                // Deviation: ColorIndex >= 2 (a null overlay: the copy keeps the mesh's own materials) never comes
                // up: the colour index is 0 or 1.
                armor_local: playing_online && is_me,
                armor_team: if local_online { 0.5 } else { f64::from(color_index) },
                team_palette: team_palette(team),
                focus_palette,
                // (IsLocallyControlled OR IsLocalPlay: every pawn offline and in Local Play, online
                // the local player's own alone)
                broker_local: setup.local_play || !playing_online || i == me,
                // Unverified: a bot is not, though a standalone game's AI controller may well be local.
                local_human: !p.bot && (!playing_online || i == me),
                outline: outline_shown(stencil, OutlinePrefs::default()),
            }
        })
        .collect()
}

/// `outlineShown` (BaseFighter SetOutline @166537-167737): whether the outline of a fighter with this StencilValue
/// is on: Ally Outline with world-showAllyOutlines, EnemyOutline / Team3Outline / Team4Outline with
/// world-showEnemyOutlines, YouOutline with world-showMyOutline (the gate before them is always open:
/// ShowOutlinesInLocalPlay is true in the CDO and nothing writes it).
pub fn outline_shown(stencil: u8, o: OutlinePrefs) -> bool {
    match stencil {
        2 => o.ally,
        3 | 5 | 6 => o.enemy,
        4 => o.me,
        _ => false,
    }
}

// Comp_Pawn_MeshManager's role tags.
const MESH_ROLE_TAGS: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];

fn component_tags(c: &Component) -> Option<Vec<&str>> {
    let tags = c.p.as_obj()?.get("ComponentTags")?.as_arr()?;
    Some(tags.iter().filter_map(Value::as_str).collect())
}

/// `subMeshTag`: the first tag that is not a role tag, in lower case.
fn sub_mesh_tag(tags: &[&str]) -> Option<String> {
    tags.iter().find(|t| !MESH_ROLE_TAGS.contains(t)).map(|t| t.to_lowercase())
}

/// The pawn's components by Meshes key (BaseFighter's, then the pawn's over them).
fn filed(d: &FighterData) -> Map<(Vec<String>, &Component)> {
    let mut out = Map::new();
    for set in [&d.components.base, &d.components.pawn] {
        for c in set.values() {
            if let Some(tags) = component_tags(c)
                && let Some(k) = sub_mesh_tag(&tags)
            {
                out.insert(k, (tags.iter().map(|t| (*t).to_owned()).collect(), c));
            }
        }
    }
    out
}

/// The armor overlay copy a part gets (skinSubMeshArmor).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartArmor {
    Primary,
    Receive,
}

/// A skeletal sub-mesh part of the skin.
#[derive(Clone, Debug)]
pub struct SkinPart {
    pub key: String,
    pub mesh: String,
    pub path: Option<String>,
    /// The template's mesh: its own materials, no skin set.
    pub own: bool,
}

/// A fighter's skin as the views draw it: look, mesh, parts and their tags.
#[derive(Clone, Debug)]
pub struct Skin {
    /// The SkinData name drawn.
    pub name: String,
    /// Its Slug (skin_c009_s05; empty: the default skin with no look of its own, or a look the view makes).
    pub slug: String,
    /// The main mesh's object path.
    pub mesh: String,
    pub parts: Vec<SkinPart>,
    /// Per part, the Meshes key the sim's subHidden names it by (None: a part the sim keeps no tag of).
    pub part_tags: Vec<Option<String>>,
    /// Per part, its initial visibility when the sim keeps no tag of it.
    pub part_init: Vec<bool>,
    pub part_armor: Vec<Option<PartArmor>>,
    /// The look's own intro montage and the default intro (a skin intro of its own).
    pub intro: Option<String>,
}

/// A path lookup: by name, else the first key of that name in any case.
fn lookup<'a>(m: &'a Map<String>, name: &str) -> Option<&'a str> {
    m.get(name).or_else(|| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)).map(String::as_str)
}

/// The skin a pick stands for: its look and its parts.
pub fn skin_of(d: &FighterData, pick: Option<&str>) -> Result<Skin> {
    let def = &d.skin;
    let look = match pick {
        Some(s) if !s.is_empty() && s != def.name => d.skins.as_ref().and_then(|m| m.get(s)),
        _ => None,
    };
    // (a pick the data has no look of draws the default)
    let default_look = d.skins.as_ref().and_then(|m| m.get(&def.name));
    let slug = look.or(default_look).map_or("", |l| l.slug.as_str());
    let (name, mesh_name, mesh_path, subs, intro): (&str, &str, Option<&str>, &[SubMesh], Option<&str>) = match look {
        Some(l) => (&l.name, &l.mesh, l.mesh_path.as_deref(), &l.sub_meshes, l.intro_montage.as_deref()),
        None => match default_look {
            Some(l) => (&l.name, &l.mesh, l.mesh_path.as_deref(), &l.sub_meshes, l.intro_montage.as_deref()),
            None => (&def.name, &def.mesh, None, &def.sub_meshes, def.intro_montage.as_deref()),
        },
    };
    let mesh = mesh_path
        .or_else(|| lookup(&d.paths.meshes, mesh_name))
        .ok_or_else(|| Error::Format {
            what: format!("skin {name}"),
            why: format!("no mesh {mesh_name} among the paths"),
        })?
        .to_owned();
    let mut parts: Vec<SkinPart> = subs
        .iter()
        .map(|m| SkinPart { key: m.key.clone(), mesh: m.mesh.clone(), path: m.path.clone(), own: false })
        .collect();
    let comps = filed(d);
    if name != def.name {
        for m in &def.sub_meshes {
            if subs.iter().any(|s| s.key.eq_ignore_ascii_case(&m.key)) {
                continue;
            }
            if let Some((_, c)) = comps.get(&m.key.to_lowercase())
                && c.ty == "SkeletalMeshComponent"
                && let Some(sm) = c.p.as_obj().and_then(|p| p.get("SkeletalMesh")).and_then(Value::as_str)
            {
                parts.push(SkinPart { key: m.key.clone(), mesh: sm.to_owned(), path: None, own: true });
            }
        }
    }
    for p in &mut parts {
        if p.path.is_none() {
            p.path = lookup(&d.paths.meshes, &p.mesh).map(str::to_owned);
        }
    }
    // the sim's tags: the default skin's SubMeshes keys a component is filed under
    let sim_tags: Vec<String> =
        def.sub_meshes.iter().map(|m| m.key.to_lowercase()).filter(|k| comps.contains_key(k)).collect();
    let mut part_tags = Vec::new();
    let mut part_init = Vec::new();
    let mut part_armor = Vec::new();
    for p in &parts {
        let tag = p.key.to_lowercase();
        let (has, primary, armor) = match comps.get(&tag) {
            Some((tags, _)) => {
                let primary = tags.iter().any(|t| t == "PrimaryMesh");
                let armor = if primary {
                    Some(PartArmor::Primary)
                } else if tags.iter().any(|t| t == "RecieveArmor") {
                    Some(PartArmor::Receive)
                } else {
                    None
                };
                (true, primary, armor)
            }
            None => (false, false, None),
        };
        let sim = has && sim_tags.contains(&tag);
        part_tags.push(sim.then(|| tag.clone()));
        part_init.push(has && (sim || primary));
        part_armor.push(armor);
    }
    let def_intro =
        def.intro_montage.clone().or_else(|| d.pawn.get("IntroAnimation").and_then(Value::as_str).map(str::to_owned));
    let intro = match (intro, def_intro) {
        (Some(own), Some(def)) if own != def => Some(own.to_owned()),
        _ => None,
    };
    Ok(Skin { name: name.to_owned(), slug: slug.to_owned(), mesh, parts, part_tags, part_init, part_armor, intro })
}

// ---- the fighter entry's draws ----------------------------------------------------------------------------------

/// The armor overlay's materials: ArmorBuffOverlay_MAT, and the local online player's.
pub const ARMOR_OVERLAY: &str = "/Game/Panda_Main/Characters/Superman/VFX/ArmorBuffOverlay_MAT.ArmorBuffOverlay_MAT";
pub const ARMOR_OVERLAY_LOCAL: &str =
    "/Game/Panda_Main/Characters/Superman/VFX/ArmorBuffOverlay_Yellow_MAT.ArmorBuffOverlay_Yellow_MAT";

/// What a fighter's meshes are drawn with (the fighter kind entry's `skinSlots` and `actors`):
/// per skin and mesh the material of each slot by variant, and per actor or prop its mesh and materials.
pub struct FighterDraws {
    skin_slots: serde_json::Value,
    actors: serde_json::Value,
    materials: std::sync::Arc<mvs_store::Pack>,
}

/// A DrawRef's material object path (the `?ctx=` context off).
fn ref_path(r: &str) -> &str {
    r.split_once('?').map_or(r, |(p, _)| p)
}

impl std::fmt::Debug for FighterDraws {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FighterDraws").finish_non_exhaustive()
    }
}

impl FighterDraws {
    /// The fighter kind's entry of the character `key` (`/<dir>/<key>.<key>`).
    pub fn load(store: &mvs_store::Store, key: &str) -> Result<FighterDraws> {
        FighterDraws::read(&*store.kind("fighter")?, store.kind("material")?, key)
    }

    /// [`FighterDraws::load`] once per store: the store's one copy, shared by every match and preview of the fighter.
    pub fn shared(store: &mvs_store::Store, key: &str) -> Result<std::sync::Arc<FighterDraws>> {
        store.cached(&format!("fighterdraws:{key}"), || FighterDraws::load(store, key), |_| 0)
    }

    /// `load` from the fighter kind's pack, `materials` the material kind's.
    pub fn read(pack: &mvs_store::Pack, materials: std::sync::Arc<mvs_store::Pack>, key: &str) -> Result<FighterDraws> {
        let tail = format!("/{key}.{key}").to_lowercase();
        let path = pack
            .paths()
            .find(|p| p.to_lowercase().ends_with(&tail))
            .ok_or_else(|| Error::Format {
                what: format!("fighter {key}"),
                why: "no entry in the fighter kind".into(),
            })?
            .to_owned();
        let v: serde_json::Value = serde_json::from_slice(&pack.bytes(&path)?)
            .map_err(|e| Error::Format { what: path.clone(), why: e.to_string() })?;
        Ok(FighterDraws {
            skin_slots: v.get("skinSlots").cloned().unwrap_or_default(),
            actors: v.get("actors").cloned().unwrap_or_default(),
            materials,
        })
    }

    /// An actors table alone (the items entry's draws: crate::items), no skin sets; `materials` the material kind's.
    pub(crate) fn of_actors(actors: serde_json::Value, materials: std::sync::Arc<mvs_store::Pack>) -> FighterDraws {
        FighterDraws { skin_slots: serde_json::Value::Null, actors, materials }
    }

    /// K2_GetScalarParameterValue of an unset instance: the material's own value of scalar `name` (None: the material
    /// lists no such parameter, or is not in the kind).
    pub(crate) fn material_scalar(&self, path: &str, name: &str) -> Option<f64> {
        match mvs_material::read_entry(&self.materials, path).ok()? {
            mvs_material::Entry::Material(m) => m.uniforms.scalars.iter().find(|s| s.name == name).map(|s| s.value),
            mvs_material::Entry::Global(_) => None,
        }
    }

    /// `drawn`: each ref's material path, None where the slot has none or the material kind lacks it.
    fn drawn(&self, l: &serde_json::Value) -> Vec<Option<String>> {
        l.as_array()
            .map(|a| {
                a.iter()
                    .map(|r| r.as_str().map(ref_path).filter(|p| self.materials.has(p)).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `CharacterMaterials.set`: the slot materials of the skin's mesh `mesh` ('' the main one) in a variant.
    pub fn set(&self, skin: &str, mesh: &str, variant: Variant) -> Result<Vec<Option<String>>> {
        let s = self.skin_slots.get(skin).and_then(|s| s.get(mesh)).and_then(|m| m.get(variant.key()));
        match s {
            Some(l) => Ok(self.drawn(l)),
            None => Err(Error::Format {
                what: format!("material set {skin} / {mesh} / {}", variant.key()),
                why: "not in the fighter entry".into(),
            }),
        }
    }

    /// `skinned`: the main mesh slots the skin overrides (the hit flash's instances).
    pub fn skinned(&self, skin: &str) -> Vec<bool> {
        self.skin_slots
            .get(skin)
            .and_then(|s| s.get(""))
            .and_then(|m| m.get("skinned"))
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|v| v.as_bool().unwrap_or(false)).collect())
            .unwrap_or_default()
    }

    /// `actor`: an actor's or prop's mesh path and slot materials (`variant`'s set where it has one).
    pub fn actor(&self, cls: &str, variant: Option<&str>) -> Option<(String, Vec<Option<String>>)> {
        let a = self.actors.get(cls)?;
        let l = variant.and_then(|v| a.get("variants")?.get(v)).or_else(|| a.get("materials"))?;
        Some((a.get("mesh")?.as_str()?.to_owned(), self.drawn(l)))
    }

    /// Every actor's and prop's mesh path (RenderChar.actorMeshes' keys' meshes).
    pub fn actor_meshes(&self) -> Vec<String> {
        let Some(a) = self.actors.as_object() else { return Vec::new() };
        a.values().filter_map(|v| Some(v.get("mesh")?.as_str()?.to_owned())).collect()
    }

    /// Every mesh key of skin `skin` (the main mesh '', its parts' and props' keys) with its slot materials in each
    /// variant a match draws (red, blue, local): the warm-up's sets.
    pub fn skin_sets(&self, skin: &str) -> Vec<(String, Vec<Vec<Option<String>>>)> {
        let Some(s) = self.skin_slots.get(skin).and_then(|s| s.as_object()) else { return Vec::new() };
        s.iter()
            .map(|(key, m)| {
                let sets = [Variant::Red, Variant::Blue, Variant::Local]
                    .iter()
                    .filter_map(|v| m.get(v.key()).map(|l| self.drawn(l)))
                    .collect();
                (key.clone(), sets)
            })
            .collect()
    }

    /// Every actor's and prop's mesh path with its slot materials and each of its variants' sets: the warm-up's
    /// sets.
    pub fn actor_sets(&self) -> Vec<(String, Vec<Vec<Option<String>>>)> {
        let Some(a) = self.actors.as_object() else { return Vec::new() };
        a.values()
            .filter_map(|v| {
                let mesh = v.get("mesh")?.as_str()?.to_owned();
                let mut sets = vec![self.drawn(v.get("materials").unwrap_or(&serde_json::Value::Null))];
                if let Some(vs) = v.get("variants").and_then(|v| v.as_object()) {
                    sets.extend(vs.values().map(|l| self.drawn(l)));
                }
                Some((mesh, sets))
            })
            .collect()
    }

    /// `actorKey`: the actors key spelt as `cls` in any case.
    pub fn actor_key(&self, cls: &str) -> Option<String> {
        let a = self.actors.as_object()?;
        if a.contains_key(cls) {
            return Some(cls.to_owned());
        }
        a.keys().find(|k| k.eq_ignore_ascii_case(cls)).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_takes_the_bound_rows_over_the_defaults() {
        let defaults: IndexMap<String, [f32; 4]> =
            CENTRAL_PALETTE_VECTORS.iter().enumerate().map(|(k, n)| ((*n).to_owned(), [k as f32; 4])).collect();
        let game: IndexMap<String, Rgba> = [("Blue1", [0.0, 0.0, 1.0, 1.0]), ("Green1", [0.0, 1.0, 0.5, 1.0])]
            .into_iter()
            .map(|(k, c)| (k.to_owned(), c))
            .collect();
        // (Ally bound to Blue1, Team3 to Green1; the others name no row and keep their defaults)
        let rows = central_palette(&defaults, &game, ["Blue1", "None", "None", "Green1", ""]);
        assert_eq!(rows, [[0.0; 4], [0.0, 0.0, 1.0, 1.0], [2.0; 4], [0.0, 1.0, 0.5, 1.0], [4.0; 4]]);
    }

    #[test]
    fn a_waves_runs_enemies_take_an_opponents_colours() {
        let player = MatchPlayer { character: Some("superman".to_owned()), ..MatchPlayer::default() };
        let setup = mvs_sim::modes::waves::setup(mvs_sim::setup::WavesVariant::Endless, player, Vec::new());
        let c = fighter_colours(&setup, 0, false, true);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].stencil, 4, "the player's own outline");
        let enemy = c[usize::try_from(mvs_sim::modes::waves::ENEMY_SLOT).unwrap()];
        assert_eq!((enemy.stencil, enemy.variant), (3, Variant::Red), "the enemy outline, the red variant");
    }
}
