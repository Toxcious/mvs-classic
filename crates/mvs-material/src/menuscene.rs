//! The `menuscene` kind's entries: the menus' levels (MainMenu, Podium, PreMatch) and one pre-match preview per skin,
//! as the importer binds them from the build's levels, Blueprints and sequences and the
//! renderer reads them. A jsonbin container (`MVMS`, no body): the header holds it all, its `systems` the particle
//! systems the entry's emitters spawn (the particles kind reads that member alone).
//!
//! The members are the bind's shapes, less what
//! drawing a material in GLSL needed: a material is a [`SceneMaterial`] naming the material kind's draw (a DrawRef)
//! and the values the scene sets on it, and the levels' texture tables are gone (the material kind holds a
//! material's textures). Times are seconds, places UE cm, rotations (pitch, yaw, roll) degrees.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::jsonbin::JsonBin;
use crate::{Error, Result};

/// The entry's magic.
pub const MAGIC: &[u8; 4] = b"MVMS";
/// The entry's layout: this file's. A change bumps it and the bind's version.
pub const FORMAT_VERSION: u32 = 5;

/// A UE vector or rotator.
pub type V3 = [f64; 3];

/// The three level entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LevelName {
    MainMenu,
    Podium,
    PreMatch,
}

impl LevelName {
    pub fn as_str(self) -> &'static str {
        match self {
            LevelName::MainMenu => "MainMenu",
            LevelName::Podium => "Podium",
            LevelName::PreMatch => "PreMatch",
        }
    }
}

/// A level entry's name below the kind's versioned folder: `/<name>.<name>`.
pub fn level_suffix(level: LevelName) -> String {
    let n = level.as_str();
    format!("/{n}.{n}")
}

/// A preview entry's name below the kind's versioned folder: `/Preview/<key>/<skin>.<skin>`, `skin` the SkinData
/// name or "" for the character's default skin (`Default`).
pub fn preview_suffix(key: &str, skin: &str) -> String {
    let n = if skin.is_empty() { "Default" } else { skin };
    format!("/Preview/{key}/{n}.{n}")
}

/// An entry: the header with its data typed by the entry's kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MenuSceneEntry {
    pub version: u32,
    /// MainMenu, Podium, PreMatch or Preview/<key>/<skin>.
    pub name: String,
    /// The bind's version and the kinds' it read (the entry's path names them too).
    pub bind_version: u64,
    pub object_version: u64,
    pub shadermap_version: u64,
    /// The particle systems its emitters spawn, by build object path (PreMatch's alone).
    pub systems: Vec<String>,
    pub data: SceneData,
}

/// An entry's data by its kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SceneData {
    MainMenu(Box<MainMenuScene>),
    Podium(Box<PodiumScene>),
    PreMatch(Box<PreMatchScene>),
    Preview(Box<PreviewEntry>),
}

// ---------------------------------------------------------------- shared parts

/// A material as a scene draws it: the material kind's draw and what the scene sets on it over the material's own
/// parameter values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneMaterial {
    /// A DrawRef (`<material object path>?ctx=<context>`, [`crate::draw::parse_draw_ref`]): the material kind's
    /// build of the material in the context the scene draws it with.
    pub draw: String,
    /// Scalar parameters set on the instance at the scene's start (a level's serialized dynamic instance's
    /// ScalarParameterValues), by parameter name.
    pub scalars: IndexMap<String, f64>,
    /// Vector parameters set the same way.
    pub vectors: IndexMap<String, [f64; 4]>,
    /// Texture parameters set the same way: parameter name -> the texture's build object path.
    pub textures: IndexMap<String, String>,
    /// The timeline tracks that drive the material's parameters while the scene runs (the preview glow's Dissolve
    /// timeline), each by its parameter.
    pub tracks: Vec<ParamTrack>,
}

/// A timeline's float track writing one parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParamTrack {
    pub param: String,
    pub keys: Vec<GlowKey>,
}

/// An FRichCurve key of a glow timeline: `mode` the interpolation of the segment after it (Linear, Constant, Cubic),
/// the tangents per second.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GlowKey {
    pub t: f64,
    pub v: f64,
    pub mode: String,
    pub arrive: f64,
    pub leave: f64,
}

/// A movie scene channel's cubic key: tangents per second, unweighted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub t: f64,
    pub v: f64,
    pub at: f64,
    pub lt: f64,
}

/// An FRichCurve key with weighted tangents (CRV_SCurve).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightedKey {
    pub t: f64,
    pub v: f64,
    pub at: f64,
    pub aw: f64,
    pub lt: f64,
    pub lw: f64,
}

/// A podium sequence channel: its keys, or the channel's default value when it has none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Channel {
    Keys { keys: Vec<Key> },
    Value { value: Option<f64> },
}

/// A spot light of a preview's lighting rig.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpotLight {
    pub name: String,
    /// UE place: world (the main menu's), or in the rig's space (a preview entry's).
    pub pos: V3,
    /// Unit vector towards the light along its axis.
    pub dir: V3,
    /// Colour x intensity (candela x 100 x 100).
    pub color: V3,
    /// SourceRadius, SoftSourceRadius, SpecularScale.
    pub src: V3,
    /// 1 / AttenuationRadius, cos(outer cone), 1 / (cos(inner cone) - cos(outer cone)).
    pub att: V3,
    pub cast_shadows: bool,
    /// A preview entry's: the light sits under the rig's Lights component (mirrored in Y by `mirrorWhen`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<bool>,
}

/// A montage a preview actor plays: one slot track's
/// segments and its section chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneMontage {
    /// The slot (PreviewIdle's alone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// The montage's BlendIn time (PreviewIdle's; a podium montage that blends in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_in: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_out: Option<f64>,
    /// PreviewIdle's notifies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifies: Option<Vec<MontageNotify>>,
    pub name: String,
    pub segments: Vec<MontageSegment>,
    pub sections: Vec<MontageSection>,
    pub length: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MontageSegment {
    /// The animation sequence's name.
    pub clip: String,
    pub start: f64,
    pub from: f64,
    pub to: f64,
    pub rate: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MontageSection {
    pub name: String,
    pub next: String,
    pub start: f64,
}

/// A montage notify.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MontageNotify {
    pub name: Option<String>,
    /// The notify's Blueprint class, `_C` dropped (None: a notify with no object).
    pub class: Option<String>,
    pub state: bool,
    /// The link value.
    pub t: f64,
    pub duration: f64,
    pub offset: f64,
    pub end_offset: f64,
    /// The notify object's properties over its class default object's (references as their package below
    /// Content/): what each notify class reads differs, so it stays the export's value.
    // Deviation: a serde_json value writes its object members sorted, not in the build's property order (kept in
    // insertion order elsewhere); the values are the same.
    pub props: serde_json::Value,
}

/// A transform (location, rotation, scale).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Xform {
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
}

/// A camera target's place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub loc: V3,
    pub rot: V3,
}

// ---------------------------------------------------------------- MainMenu

/// The main menu's scene. Its Player1 preview actor is
/// the PreMatch entry's (New_MainMenu streams PreMatch: the actor's start point and FacingLeft are that level's); the
/// fighter shown on it is placed, animated and lit from its skin's preview entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MainMenuScene {
    pub camera: MenuCamera,
    /// The engine Plane the background, the slants and the podium's planes draw (build object path, the mesh kind).
    pub plane: String,
    /// PM_BG_TrophyRoom's MainMenuBackground plane.
    pub background: ScenePlane,
    /// MainMenuBackground's Event Lookup Table: an active event's slug -> the material UpdateBGForEvents sets on the
    /// background's slot 0 (BG @777), in the table's order.
    pub events: IndexMap<String, SceneMaterial>,
    /// PM_NewSlants' UI_Plane_FFA and UI_Plane_RedTeam.
    pub slants: Vec<ScenePlane>,
    /// BP_CharacterPreview's glow shell.
    pub glow: Glow,
}

impl MainMenuScene {
    /// UpdateBGForEvents (MainMenuBackground @777): the material of the first of `active` (CalendarManager
    /// GetActiveEvents' slugs, in its order) that the Event Lookup Table holds, else the background's own.
    pub fn background_material<S: AsRef<str>>(&self, active: &[S]) -> &SceneMaterial {
        active.iter().find_map(|s| self.events.get(s.as_ref())).unwrap_or(&self.background.material)
    }

    /// The backgrounds the plane can draw (the port's own choice of one, mvs-settings `UiSettings::menu_background`):
    /// "" for its own material, then each Event Lookup Table slug in the table's order.
    pub fn background_options(&self) -> Vec<String> {
        std::iter::once(String::new()).chain(self.events.keys().cloned()).collect()
    }
}

/// A camera target and its CineCamera's lens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuCamera {
    pub loc: V3,
    pub rot: V3,
    pub fov: f64,
    pub aspect: f64,
    /// The VignetteIntensity the camera overrides (None: no override).
    pub vignette: Option<f64>,
}

/// An engine Plane of a level with its material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePlane {
    /// The actor's name (the slants'; the background has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    /// TranslucencySortPriority (the slants').
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<i32>,
    pub material: SceneMaterial,
}

/// The glow shell (BP_CharacterPreview GlowVFX @29444): CharacterDissolve_MAT on duplicates of the meshes, the
/// Dissolve timeline's tracks in `material.tracks`; `reverse` BPI_ClearFighter's Reverse Dissolve timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    /// The Dissolve timeline's length.
    pub length: f64,
    pub reverse: GlowTimeline,
    pub material: SceneMaterial,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlowTimeline {
    pub length: f64,
    pub tracks: Vec<ParamTrack>,
}

// ---------------------------------------------------------------- Preview

/// A skin's pre-match preview.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewEntry {
    /// The SkinData name.
    pub skin: String,
    /// The ScaleOverrides placements by key (MainMenu_You ... FaceOff_Friend): the actor's relative location (Y
    /// negated by the reader when FacingLeft) and the mesh's scale per axis.
    pub placements: IndexMap<String, Placement>,
    /// None: the skin has none (BP_CharacterPreview @5999-6327): nothing plays.
    pub preview_idle: Option<SceneMontage>,
    /// The montage the main menu plays (BP_CharacterPreview @5999: BPI_IsInMainMenu picks HomescreenIdle over
    /// PreviewIdle). None: the skin has none: nothing plays.
    pub homescreen_idle: Option<SceneMontage>,
    /// The rig's class name.
    pub rig: String,
    pub mirror_when: MirrorWhen,
    /// In the rig's space (origin 0, unmirrored), each with `mirror`.
    pub lights: Vec<SpotLight>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub loc: V3,
    pub scale: V3,
}

/// When a lighting rig scales its Lights component Y by -1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MirrorWhen {
    Left,
    Right,
}

// ---------------------------------------------------------------- Podium

/// The podium.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodiumScene {
    pub camera: PodiumCamera,
    /// Plane_2, UI_Plane_TeamColorBG, UI_Plane_TrophyBG.
    pub planes: Vec<PodiumPlane>,
    pub text: WinnerText,
    pub banner: PodiumBanner,
    /// The four preview actors, Player1 .. Player4.
    pub previews: Vec<PodiumPreview>,
    /// By roster key (protocol roster CHARACTERS).
    pub fighters: IndexMap<String, PodiumFighterSkins>,
    /// The level's directional lights by lighting channel ("1" the winners', "2" the losers').
    pub lights: IndexMap<String, Vec<DirLight>>,
    pub sequence: PodiumSequence,
    /// BP_CharacterPreview's BlobShadowCenter: its mesh and material (the class's component template).
    pub blob_shadow: BlobShadow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodiumCamera {
    pub start: Point,
    pub zoom: Point,
    pub fov: f64,
    pub aspect: f64,
    pub vignette: Option<f64>,
    /// CameraLerp plays CRV_SCurve at (last key time / zoomLength) (ZoomToWInner).
    pub zoom_length: f64,
    pub zoom_curve: Vec<WeightedKey>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodiumPlane {
    pub name: String,
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    pub sort: i32,
    /// `default` the level's, `Blue` / `Red` the level Blueprint's swaps.
    pub materials: IndexMap<String, SceneMaterial>,
}

/// The winner text: BP_WinningTeam_2's world space WidgetComponent of `widget` (WBP_3dTeam).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WinnerText {
    pub actor: V3,
    pub child: V3,
    pub component: V3,
    pub widget: String,
}

/// BP_Banner_PM's pieces the end of game shows with the default banner (the podium's
/// additions).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodiumBanner {
    pub mask: BannerPiece,
    pub background: BannerPiece,
    pub filter: BannerPiece,
    pub trim: BannerPiece,
    pub nametag: BannerWidget,
    pub toast_button: BannerWidget,
    pub stat_sheet: BannerWidget,
    pub base_element_root: BannerScene,
    pub parallax_rail: BannerScene,
    pub parallax_element_root: BannerScene,
    pub slant_angle: f64,
    /// The Entrance Timeline's last key time (played at rate 10).
    pub entrance_length: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerPiece {
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    pub material: SceneMaterial,
}

/// A widget component on Banner Placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerWidget {
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    /// The widget class, `_C` dropped.
    pub widget: String,
}

/// A scene component a banner element is attached under.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerScene {
    pub parent: String,
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodiumPreview {
    /// Player1 .. Player4.
    pub tag: String,
    /// The TargetPoint the actor is attached to.
    pub point: String,
    pub loc: V3,
    pub facing_left: bool,
    pub scale: f64,
    /// The banner's spawn point (GetBannerSpawn): relative location and uniform scale.
    pub banner: ScaledPoint,
    /// Its BlobShadowCenter's place under the actor's root as the level holds it (the template's: the construction
    /// script returns on a Podium level).
    pub shadow: ShadowPlace,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaledPoint {
    pub loc: V3,
    pub scale: f64,
}

/// A character's podium values: its default skin's, and each further skin's by SkinData name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PodiumFighterSkins {
    #[serde(flatten)]
    pub default: PodiumFighter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skins: Option<IndexMap<String, PodiumFighter>>,
}

/// A skin's EOG_Winner / EOG_Loser placements and its Victory / Defeat montages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodiumFighter {
    pub skin: String,
    pub mesh_yaw: f64,
    pub winner: ScaledPoint,
    pub loser: ScaledPoint,
    pub victory: SceneMontage,
    pub defeat: SceneMontage,
}

/// A directional light of the podium (its podium members).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirLight {
    pub name: String,
    /// Unit vector towards the light.
    pub dir: V3,
    /// Colour x illuminance (lux).
    pub color: V3,
    /// sin(half LightSourceAngle), sin(half LightSourceSoftAngle), SpecularScale.
    pub src: V3,
    pub cast_shadows: bool,
    /// bAffectTranslucentLighting.
    pub translucent: bool,
    /// LightGuid as the level's MapBuildDataRegistry keys its LightBuildData (no dashes): the static shadow depth map
    /// the translucency volume's inject reads. None: the component has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guid: Option<String>,
}

/// The intro sequences: Seq_WinnerIntro_Geo's fade, sounds and transform channels, and the event keys of
/// Seq_WinnerIntro_2v2 / _FFA (by "2v2" / "FFA").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodiumSequence {
    pub length: f64,
    pub fade: Option<Vec<Key>>,
    pub sounds: Vec<PodiumSound>,
    /// By possessable name: the nine transform channels (TRANSFORM_CHANNELS), None a masked-out one.
    pub transforms: IndexMap<String, Vec<Option<Channel>>>,
    pub events: IndexMap<String, Vec<SceneEvent>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodiumSound {
    pub t: f64,
    pub ev: String,
}

/// A sequence event key through its director.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneEvent {
    pub t: f64,
    #[serde(rename = "fn")]
    pub call: String,
    pub args: Vec<EventArg>,
    /// The call's first parameter is the event's bound Target (the pre-match sequences' event tracks alone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventArg {
    Bool(bool),
    Num(f64),
    Str(String),
}

// ---------------------------------------------------------------- PreMatch

/// The pre-match level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreMatchScene {
    /// The TargetPoints by actor name.
    pub anchors: IndexMap<String, Point>,
    /// The four preview actors, Player1 .. Player4.
    pub previews: Vec<PreMatchPreview>,
    /// The BannerMover actors.
    pub movers: IndexMap<String, Mover>,
    /// The seven BP_Banner_PM instances.
    pub banners: Vec<BannerInstance>,
    /// The banners' materials by name (a piece names one).
    pub banner_materials: IndexMap<String, SceneMaterial>,
    /// MMToCharSelect, AdjustPreviewActor, CameraLerp.
    pub timelines: IndexMap<String, Timeline>,
    /// CRV_SCurve, CRV_Exp.
    pub curves: IndexMap<String, Curve>,
    pub slants: Slants,
    pub background: Background,
    /// Seq_AnimInRed, Seq_AnimInFFA_New, Seq_Clash, Seq_Clash_FFA_New.
    pub sequences: IndexMap<String, Sequence>,
    /// The Emitter actors the sequences drive, by actor name.
    pub emitters: IndexMap<String, LevelEmitter>,
    /// BP_CharacterPreview's BlobShadowCenter: its mesh and material (the class's component template).
    pub blob_shadow: BlobShadow,
}

/// BP_CharacterPreview's BlobShadowCenter (a StaticMeshComponent under DefaultSceneRoot): the engine Plane with
/// M_PreMatch_BlobShadow, bVisible false in the template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlobShadow {
    /// The static mesh, by build object path.
    pub mesh: String,
    pub material: SceneMaterial,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreMatchPreview {
    pub tag: String,
    pub actor: String,
    /// The TargetPoint it is attached to.
    pub anchor: String,
    pub facing_left: bool,
    pub tags: Vec<String>,
    /// Its BlobShadowCenter's place under the actor's root as the level holds it (the construction script's).
    pub shadow: ShadowPlace,
}

/// A BlobShadowCenter's relative transform and bHiddenInGame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShadowPlace {
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mover {
    pub parent: String,
    pub loc: V3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BannerInstance {
    pub name: String,
    pub mover: String,
    pub hidden: bool,
    /// Banner Index.
    pub index: i32,
    pub offset: V3,
    pub text_offset: V3,
    pub start_hidden: bool,
    /// The pieces by component name (Banner Placement ... Parallax Element Root).
    pub pieces: IndexMap<String, PieceAsset>,
}

/// A banner component: a scene component, a Plane (`material`) or a widget component (`widget`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PieceAsset {
    pub parent: String,
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_in_game: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// A Plane's material: a key of `bannerMaterials`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The level's dynamic instance's scalars over the material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<IndexMap<String, f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_depth: Option<CustomDepth>,
    /// bRenderInDepthPass false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_pass: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<i32>,
    /// A widget component's class, `_C` dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
    /// World or Screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_size: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_at_desired_size: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomDepth {
    pub stencil: i32,
    pub main_pass: bool,
    /// The DrawRef of the piece's custom depth pass (context `customdepth`); its main pass, where it has one, draws
    /// the piece's `material`.
    pub draw: String,
}

/// A Blueprint timeline's template (float tracks only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Timeline {
    pub length: f64,
    /// TimelineLength or LastKeyFrame.
    pub length_mode: String,
    pub ignore_time_dilation: bool,
    pub tracks: IndexMap<String, Curve>,
}

/// An FRichCurve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub keys: Vec<CurveKey>,
    /// PreInfinityExtrap / PostInfinityExtrap other than Constant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
}

/// An FRichCurve key: `i` the interpolation (Linear, Constant, Cubic), tangents per second, the
/// weights on a weighted key alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveKey {
    pub t: f64,
    pub v: f64,
    pub i: String,
    pub at: f64,
    pub lt: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aw: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lw: Option<f64>,
}

/// PM_NewSlants: every Plane, the material instances the Move update swaps in, the two timelines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slants {
    pub planes: Vec<SlantPlane>,
    pub materials: IndexMap<String, SceneMaterial>,
    pub start_color: [f64; 4],
    /// Move, Timeline_0.
    pub timelines: IndexMap<String, Timeline>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlantPlane {
    pub name: String,
    pub parent: Option<String>,
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    pub visible: bool,
    pub sort: i32,
    /// A key of `materials`.
    pub material: String,
}

/// MainMenuBackground's Background Lookup Table and its timelines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Background {
    /// Map asset path -> picture name (a key of `textures`; None: not in the paks).
    pub lookup: IndexMap<String, Option<String>>,
    pub textures: IndexMap<String, MapPicture>,
    /// The pictures the paks lack.
    pub missing: Vec<String>,
    /// TimelineTransition, TimelineMapChange.
    pub timelines: IndexMap<String, Timeline>,
}

/// A map picture: a texture of the store by build object path, its sampling, the build's size and pixel format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapPicture {
    pub path: String,
    pub srgb: bool,
    pub ax: String,
    pub ay: String,
    pub size: [f64; 2],
    pub format: String,
}

/// A level sequence: times in seconds, tangents per second.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sequence {
    pub start: f64,
    pub end: f64,
    pub display_rate: f64,
    pub director: Option<String>,
    pub tracks: Vec<SeqTrack>,
}

/// A section range: its bounds, None an open one.
pub type Range = [Option<f64>; 2];

/// A sequence track: its bound objects' paths (`<level>:<actor>`, `/<component>` below a parent possessable; None a
/// master track) and its possessable's name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SeqTrack {
    #[serde(rename = "3DTransform")]
    Transform {
        binding: Option<Vec<String>>,
        name: Option<String>,
        range: Range,
        mask: u32,
        /// The nine channels (TRANSFORM_CHANNELS), None a masked-out one.
        channels: Vec<Option<SeqChannel>>,
    },
    ComponentMaterial {
        binding: Option<Vec<String>>,
        name: Option<String>,
        material_index: u32,
        sections: Vec<MaterialSection>,
    },
    Particle {
        binding: Option<Vec<String>>,
        name: Option<String>,
        keys: Vec<ParticleKey>,
    },
    Visibility {
        binding: Option<Vec<String>>,
        name: Option<String>,
        property: Option<String>,
        sections: Vec<VisibilitySection>,
    },
    CameraShake {
        binding: Option<Vec<String>>,
        name: Option<String>,
        sections: Vec<ShakeSection>,
    },
    CameraCut {
        binding: Option<Vec<String>>,
        name: Option<String>,
        sections: Vec<CutSection>,
    },
    AkAudioEvent {
        binding: Option<Vec<String>>,
        name: Option<String>,
        sections: Vec<AudioSection>,
    },
    Fade {
        binding: Option<Vec<String>>,
        name: Option<String>,
        sections: Vec<FadeSection>,
    },
    Event {
        binding: Option<Vec<String>>,
        name: Option<String>,
        /// EFireEventsAtPosition.
        position: String,
        keys: Vec<SceneEvent>,
    },
}

/// A movie scene float channel: cubic / linear / constant keys (`i`), tangents per second.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeqChannel {
    pub keys: Vec<SeqKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeqKey {
    pub t: f64,
    pub v: f64,
    pub i: String,
    pub at: f64,
    pub lt: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSection {
    pub range: Range,
    pub scalars: IndexMap<String, SeqChannel>,
}

/// A Particle key: Activate, Deactivate or Trigger.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleKey {
    pub t: f64,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibilitySection {
    pub range: Range,
    pub bools: BoolChannel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoolChannel {
    pub keys: Vec<BoolKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoolKey {
    pub t: f64,
    pub v: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShakeSection {
    pub range: Range,
    /// The shake class, `_C` dropped.
    pub shake: String,
    pub play_scale: f64,
    pub space: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutSection {
    pub range: Range,
    pub camera: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioSection {
    pub range: Range,
    pub event: Option<String>,
    pub max_duration: Option<f64>,
    pub stop_at_end: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FadeSection {
    pub range: Range,
    pub fade: SeqChannel,
    pub color: [f64; 4],
    pub fade_audio: bool,
}

/// A PreMatch Emitter actor a sequence drives: its ParticleSystemComponent0's
/// system (its name: the particles kind's), transform and bVisible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelEmitter {
    pub system: String,
    pub loc: V3,
    pub rot: V3,
    pub scale: V3,
    pub visible: bool,
}

// ---------------------------------------------------------------- read

/// An entry's bytes read: the container, the layout's version, and every material's DrawRef.
pub fn read_entry(bytes: &[u8], what: &str) -> Result<MenuSceneEntry> {
    let e: MenuSceneEntry = JsonBin::split(MAGIC, bytes, what)?.parse(what)?;
    if e.version != FORMAT_VERSION {
        return Err(Error::format(what, format!("entry version {}, the reader's {FORMAT_VERSION}", e.version)));
    }
    let kind_ok = match (&e.data, e.name.as_str()) {
        (SceneData::MainMenu(_), "MainMenu")
        | (SceneData::Podium(_), "Podium")
        | (SceneData::PreMatch(_), "PreMatch") => true,
        (SceneData::Preview(_), n) => n.starts_with("Preview/"),
        _ => false,
    };
    if !kind_ok {
        return Err(Error::format(what, format!("entry {} holds another entry's data", e.name)));
    }
    for d in e.draws() {
        if crate::draw::parse_draw_ref(d).is_none() {
            return Err(Error::format(what, format!("{d} is not a DrawRef")));
        }
    }
    Ok(e)
}

impl MenuSceneEntry {
    /// Every material the entry names, in the entry's order.
    pub fn materials(&self) -> Vec<&SceneMaterial> {
        let mut out = Vec::new();
        match &self.data {
            SceneData::MainMenu(m) => {
                out.push(&m.background.material);
                out.extend(m.events.values());
                out.extend(m.slants.iter().map(|s| &s.material));
                out.push(&m.glow.material);
            }
            SceneData::Podium(p) => {
                for pl in &p.planes {
                    out.extend(pl.materials.values());
                }
                let b = &p.banner;
                out.extend([&b.mask.material, &b.background.material, &b.filter.material, &b.trim.material]);
            }
            SceneData::PreMatch(p) => {
                out.extend(p.banner_materials.values());
                out.extend(p.slants.materials.values());
                out.push(&p.blob_shadow.material);
            }
            SceneData::Preview(_) => {}
        }
        out
    }

    /// Every DrawRef the entry names, in the entry's order (the material kind's reach).
    pub fn draws(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.materials().into_iter().map(|m| m.draw.as_str()).collect();
        if let SceneData::PreMatch(p) = &self.data {
            let depth = p.banners.iter().flat_map(|b| b.pieces.values()).filter_map(|x| x.custom_depth.as_ref());
            out.extend(depth.map(|c| c.draw.as_str()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(draw: &str) -> SceneMaterial {
        SceneMaterial { draw: draw.to_owned(), ..SceneMaterial::default() }
    }

    #[test]
    fn background_follows_the_first_event_with_an_entry() {
        let plane =
            ScenePlane { name: None, loc: [0.0; 3], rot: [0.0; 3], scale: [1.0; 3], sort: None, material: mat("bg") };
        let m = MainMenuScene {
            camera: MenuCamera { loc: [0.0; 3], rot: [0.0; 3], fov: 30.0, aspect: 1.0, vignette: None },
            plane: String::new(),
            background: plane,
            events: IndexMap::from([("event_002".to_owned(), mat("e2")), ("event_003".to_owned(), mat("e3"))]),
            slants: Vec::new(),
            glow: Glow {
                length: 1.0,
                reverse: GlowTimeline { length: 1.0, tracks: Vec::new() },
                material: mat("glow"),
            },
        };
        let none: [&str; 0] = [];
        assert_eq!(m.background_material(&none).draw, "bg");
        assert_eq!(m.background_material(&["event_001"]).draw, "bg");
        assert_eq!(m.background_material(&["event_001", "event_003", "event_002"]).draw, "e3");
        assert_eq!(m.background_material(&["event_002".to_owned()]).draw, "e2");
        // the choices: the own material, then the table's slugs in its order, each drawing its own material
        let options = m.background_options();
        assert_eq!(options, ["", "event_002", "event_003"]);
        let drawn: Vec<&str> = options.iter().map(|o| m.background_material(&[o]).draw.as_str()).collect();
        assert_eq!(drawn, ["bg", "e2", "e3"]);
    }
}
