//! The PreMatch level behind the pre-match screen: the camera's
//! moves, PM_NewSlants and the map picture background, the movers the previews and banners ride, the level
//! sequences (Seq_AnimInRed, Seq_Clash ...), the banners with their nametags' places, the check marks' places and
//! the level's emitters.
//!
//! The nametags' text and profile icons are the screen's (its WBP_3dText widget): the level places the banner and
//! tells where the NametagWidget stands (`plates`), where the check marks and perk cards go (`WorldLocation`
//! events), and the screen draws the plate there.

mod fx;

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_view::camera::CameraShaker;
use mvs_view::{M4, MeshPose};

use self::fx::LevelEmitters;
use super::banner::{BannerElement, BannerInit, BannerParts, BannerView, PIECE_BACKGROUND, PIECE_FILTER, PIECE_TRIM};
use super::format::{
    self, EventArg, LevelName, MainMenuScene, MapPicture, MenuSceneEntry, PreMatchPreview, PreMatchScene, SceneData,
    SceneMaterial, SeqTrack, V3, material_ref,
};
use super::preview::{PreviewSet, PreviewState};
use super::sequence::{SeqSpan, SequencePlayer, Timeline, TimelineStep, eval_curve, eval_seq_channel};
use super::{LevelCx, MenuEvent, MenuFrame, PreMatchSkinData, PreviewSpec, SceneWidget, SeqCmd};
use crate::fx::warn_once;
use crate::gpu::{Gpu, Texture};
use crate::stage::{to_world, transform};
use crate::{Assets, Error, Result};

const BANNER_P: [&str; 4] =
    ["BP_Banner_PM_Player1", "BP_Banner_PM_Player2", "BP_Banner_PM_Player3", "BP_Banner_PM_Player4"];
const BANNER_FFA: [&str; 4] =
    ["BP_Banner_PM_Player1", "BP_Banner_PM_Player2_FFA", "BP_Banner_PM_Player3_FFA", "BP_Banner_PM_Player4_FFA"];
/// The transitions the background ignores (BG@823..1252); any other string is a map path (NastyMapBGChange).
const NOT_MAPS: [&str; 8] = [
    "matchmaking",
    "cancelmatchmaking",
    "characterlocked",
    "playersfound",
    "matchready",
    "foundaftercancel",
    "movetocharselect",
    "backtomenu",
];
/// NastyMapBGChange's Tint (BG@1795).
const MAP_TINT: [f64; 4] = [0.4, 0.4, 0.4, 1.0];
/// TimelineMapChange's length (BGJ template).
const MAP_PAN_TIME: f64 = 2.0;
/// PM_NewSlants' Move update without a multiqueue config (SL@1128..1687): DarkenColor of the two plates' new MIDs.
const DARKEN_BLUE: [f64; 4] = [0.008, 0.008576, 0.02, 0.8];
const DARKEN_RED: [f64; 4] = [0.008023, 0.008568, 0.019382, 0.8];

/// SetupCharSelect's move of a side start point (its CharEndLoc, MMToCharSelect's end): Start_Char_2 (Player3's, the
/// party partner's) 70 along -Y, Start_Char_3 (Player2's) 70 along +Y; the others stay.
pub(super) fn char_select_shift(start: &str) -> V3 {
    match start {
        "Start_Char_2" => [0.0, -70.0, 0.0],
        "Start_Char_3" => [0.0, 70.0, 0.0],
        _ => [0.0; 3],
    }
}

/// The anchors, banner pieces and sequences the level needs, checked at `new`.
const START_CHAR: [&str; 4] = ["Start_Char_1", "Start_Char_2", "Start_Char_3", "Start_Char_4"];
const FACE_OFF: [&str; 4] = ["FaceOff_Char_1", "FaceOff_Char_2", "FaceOff_Char_3", "FaceOff_Char_4"];
const FACE_OFF_FFA: [&str; 4] =
    ["FaceOff_Char_1_FFA", "FaceOff_Char_2_FFA", "FaceOff_Char_3_FFA", "FaceOff_Char_4_FFA"];
const ANCHORS: [&str; 16] = [
    "Start_Char_1",
    "Start_Char_2",
    "Start_Char_3",
    "Start_Char_4",
    "FaceOff_Char_1",
    "FaceOff_Char_2",
    "FaceOff_Char_3",
    "FaceOff_Char_4",
    "FaceOff_Char_1_FFA",
    "FaceOff_Char_2_FFA",
    "FaceOff_Char_3_FFA",
    "FaceOff_Char_4_FFA",
    "PartnerNameplate_MM",
    "PartnerNameplate_PM",
    "CameraTarget_CharSelect",
    "CameraTarget_Faceoff",
];
const PIECES: [&str; 6] =
    ["Banner Placement", "Banner Mask", "Nametag Root", "NametagWidget", "CheckMarkPos", "PerkInfoPos"];

fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn lerp3(a: V3, b: V3, t: f64) -> V3 {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// A * B for the row-vector matrices (a point goes through A, then B).
fn mul4(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, x) in row.iter_mut().enumerate() {
            *x = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// The tail of an object path or asset name, lower case (the map lookup's match).
fn tail(s: &str) -> String {
    s[s.rfind(['.', '/']).map_or(0, |i| i + 1)..].to_lowercase()
}

/// The texture parameter of the background plane's material that a map picture replaces.
fn background_texture(assets: &Assets, m: &SceneMaterial) -> Result<String> {
    let path = material_ref(m, None).path;
    let mat = assets.material(&path)?;
    let t = mat.uniforms.textures.first();
    t.map(|t| t.name.clone()).ok_or_else(|| Error::Format {
        what: "PreMatch level".into(),
        why: format!("the background material {path} has no texture"),
    })
}

/// What a call into the level touches besides the level: the previews, the events, the assets.
struct Ports<'a> {
    previews: &'a mut PreviewSet,
    events: &'a mut Vec<MenuEvent>,
    assets: &'a Assets,
}

/// A plane of PM_NewSlants and its material instance.
#[derive(Debug)]
struct Slant {
    /// The material as the plane starts with it, and as it is now (the sequences and Move write the instance).
    base: SceneMaterial,
    cur: SceneMaterial,
    sort: i32,
    matrix: M4,
    /// A Visibility track's bHidden write; shown otherwise.
    visible: bool,
}

impl Slant {
    fn new(base: SceneMaterial, sort: i32, matrix: M4) -> Slant {
        Slant { cur: base.clone(), base, sort, matrix, visible: true }
    }

    /// CreateDynamicMaterialInstance on a material that is no MID: a new instance with the material's own values.
    fn fresh(&mut self) {
        self.cur = self.base.clone();
    }

    /// SetScalarParameterValue on the instance (a name the material lacks changes nothing).
    fn scalar(&mut self, name: &str, v: f64) {
        self.cur.scalars.insert(name.to_string(), v);
    }

    fn vector(&mut self, name: &str, v: [f64; 4]) {
        self.cur.vectors.insert(name.to_string(), v);
    }

    fn pose(&self, plane: &Arc<str>) -> MeshPose {
        MeshPose {
            mesh: plane.clone(),
            to_world: to_world(&self.matrix),
            materials: vec![material_ref(&self.cur, None)],
            ..MeshPose::default()
        }
    }
}

/// MainMenuBackground's plane: NastyMapBGChange's picture and Tint, and its XOffset.
#[derive(Debug)]
struct Background {
    cur: SceneMaterial,
    matrix: M4,
    /// The texture parameter the map picture replaces: the background material's first (MI_MapBG_*: `Texture`).
    param: String,
}

/// The next map's picture (NastyMapBGChange): asked for at match found (`shown` false) or by the change itself.
#[derive(Debug)]
struct MapLoad {
    name: String,
    shown: bool,
}

/// What a sequence's end does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeqEnd {
    None,
    /// The level's OnAnimInComplete @38233: BPI_TransitionComplete("FaceOff").
    FaceOffComplete,
}

#[derive(Debug)]
struct ActiveSeq {
    player: SequencePlayer,
    end: SeqEnd,
}

/// A BP_Banner_PM instance of the level.
#[derive(Debug)]
struct LevelBanner {
    /// Its place in `data.banners`.
    inst: usize,
    view: BannerView,
    /// The actor's offset from its mover's location (SetActorLocation on an attached actor changes it).
    rel: V3,
    shown: bool,
    /// CheckMarkPos (relative to NametagWidget) and PerkInfoPos (relative to Nametag Root) after Set*Positions.
    check_mark: V3,
    perk_info: V3,
    /// Apply Banner Appearance: the player's BannerData slug ("" none), asked for and not yet tried.
    slug: String,
    pending: bool,
    /// The default background and the filter hidden from then on.
    applied: bool,
    element: Option<BannerElement>,
    /// The element's HideDefaultBorder (setTrimVisible): the trim stays hidden through showBanner.
    trim_on: bool,
}

/// The PreMatch level.
#[derive(Debug)]
pub struct PreMatchLevel {
    pub data: Arc<PreMatchScene>,
    /// The lobby's team style (0 1v1, 1 2v2, 2 FFA), set by `Renderer::set_menu_team_style`.
    pub team_style: u8,
    /// UI_Cursor's MousePosNormalized (0..1), what the banners' parallax reads.
    pub cursor: [f64; 2],
    /// The engine Plane the background and the slants draw.
    plane: Arc<str>,
    /// The CineCamera's lens (horizontal field of view, the frame's aspect).
    lens: (f64, f64),
    anchors: IndexMap<String, V3>,
    /// The banner movers: the parent anchor and the offset from it.
    movers: IndexMap<String, (String, V3)>,
    /// The preview slot of each Start_Char anchor.
    slot_of: IndexMap<String, usize>,
    /// CharStartLoc / CharEndLoc (0..3: Start_Char_1..4).
    char_start: [V3; 4],
    char_end: [V3; 4],
    /// DoOnce gates: SetupCharSelect, PlayersFound (opened again by "Matchmaking").
    started: bool,
    char_select_done: bool,
    players_found_done: bool,
    /// A global switch value of the music group `mus` asked and not yet taken (`take_music`).
    music: Option<&'static str>,
    /// The CineCamera's actor transform, the lerp's ends (SetLerpingValues) and CameraLerp.
    cam_loc: V3,
    cam_rot: V3,
    cam_from: (V3, V3),
    cam_to: (V3, V3),
    cam_curve: String,
    cam_delay: bool,
    cam_lerp: Timeline,
    shaker: CameraShaker,
    /// The camera with the shake applied.
    shot_loc: V3,
    mm_to_char_select: Timeline,
    adjust_preview: Timeline,
    /// PM_NewSlants: UI_Plane_MainMenu's location (an invisible actor UI_Plane_BlueTeam is attached to), RedTeam's.
    move_tl: Timeline,
    timeline0: Timeline,
    main_loc: V3,
    main_rot: V3,
    main_scale: V3,
    blue_local: M4,
    red_loc: V3,
    red_rot: V3,
    red_scale: V3,
    blue: Slant,
    red: Slant,
    ffa_slant: Slant,
    background: Background,
    map_load: Option<MapLoad>,
    /// TimelineMapChange's seconds played; negative when not playing.
    map_pan: f64,
    sequence: Option<ActiveSeq>,
    emitters: LevelEmitters,
    banners: Vec<LevelBanner>,
    /// The banner elements' meshes of the last tick (their places follow the tick's).
    element_meshes: Vec<(MeshPose, i32)>,
    skins: Option<PreMatchSkinData>,
}

impl PreMatchLevel {
    /// The level's parts and the banners made, hidden. The open-level paths'
    /// SetupCharSelect and Move run in `open` (the previews are not at hand here).
    pub fn new(_gpu: &Gpu, assets: &Assets, entry: MenuSceneEntry, active_events: &[String]) -> Result<PreMatchLevel> {
        let SceneData::PreMatch(data) = entry.data else {
            return Err(Error::Format { what: entry.name, why: "not the PreMatch entry".into() });
        };
        let data = *data;
        let bad = |why: String| Error::Format { what: "PreMatch level".into(), why };
        // the menu scene the level runs on: its camera, the background and the two plates it moves
        let main_entry = format::load_level(assets.store(), LevelName::MainMenu)?;
        let SceneData::MainMenu(main) = main_entry.data else {
            return Err(bad("the MainMenu entry is not one".into()));
        };
        let main: MainMenuScene = *main;
        let plane: Arc<str> = main.plane.as_str().into();

        for n in ANCHORS {
            if !data.anchors.contains_key(n) {
                return Err(bad(format!("no anchor {n}")));
            }
        }
        let anchors: IndexMap<String, V3> = data.anchors.iter().map(|(k, p)| (k.clone(), p.loc)).collect();
        let mut movers = IndexMap::new();
        for (name, m) in &data.movers {
            if !anchors.contains_key(&m.parent) {
                return Err(bad(format!("mover {name}: no anchor {}", m.parent)));
            }
            movers.insert(name.clone(), (m.parent.clone(), m.loc));
        }
        for n in ["BannerMover1", "BannerMover2", "BannerMover4"] {
            if !movers.contains_key(n) {
                return Err(bad(format!("no mover {n}")));
            }
        }
        let slot_of: IndexMap<String, usize> =
            data.previews.iter().enumerate().map(|(i, p)| (p.anchor.clone(), i)).collect();
        for inst in &data.banners {
            if !movers.contains_key(&inst.mover) {
                return Err(bad(format!("{}: no mover {}", inst.name, inst.mover)));
            }
            for p in PIECES {
                if !inst.pieces.contains_key(p) {
                    return Err(bad(format!("{}: no piece {p}", inst.name)));
                }
            }
        }
        for n in BANNER_P.iter().chain(&BANNER_FFA[1..]) {
            if !data.banners.iter().any(|b| b.name == *n) {
                return Err(bad(format!("no banner {n}")));
            }
        }
        let timeline = |n: &str| data.timelines.get(n).ok_or_else(|| bad(format!("no timeline {n}")));
        for (n, t) in [("MMToCharSelect", "NewTrack_0"), ("AdjustPreviewActor", "NewTrack_0")] {
            if !timeline(n)?.tracks.contains_key(t) {
                return Err(bad(format!("timeline {n}: no track {t}")));
            }
        }
        for c in ["CRV_SCurve", "CRV_Exp"] {
            if !data.curves.contains_key(c) {
                return Err(bad(format!("no curve {c}")));
            }
        }
        let move_tl = data.slants.timelines.get("Move").ok_or_else(|| bad("no slant timeline Move".into()))?;
        let t0 = data.slants.timelines.get("Timeline_0").ok_or_else(|| bad("no slant timeline Timeline_0".into()))?;
        if !t0.tracks.contains_key("Curve") {
            return Err(bad("slant timeline Timeline_0: no track Curve".into()));
        }
        for s in ["Seq_AnimInRed", "Seq_AnimInFFA_New", "Seq_Clash", "Seq_Clash_FFA_New"] {
            if !data.sequences.contains_key(s) {
                return Err(bad(format!("no sequence {s}")));
            }
        }

        // PM_NewSlants
        let sl = &data.slants;
        let plane_of =
            |name: &str| sl.planes.iter().find(|p| p.name == name).ok_or_else(|| bad(format!("no slant plane {name}")));
        let main_plane = plane_of("UI_Plane_MainMenu")?;
        let bp = plane_of("UI_Plane_BlueTeam")?;
        if bp.parent.as_deref() != Some("UI_Plane_MainMenu") {
            return Err(bad(format!("UI_Plane_BlueTeam's parent is {:?}", bp.parent)));
        }
        let rp = plane_of("UI_Plane_RedTeam")?;
        let blue_material =
            sl.materials.get(&bp.material).ok_or_else(|| bad(format!("no slant material {}", bp.material)))?;
        let menu_slant = |name: &str| {
            main.slants
                .iter()
                .find(|s| s.name.as_deref() == Some(name))
                .ok_or_else(|| bad(format!("the MainMenu entry has no {name}")))
        };
        let (red_menu, ffa_menu) = (menu_slant("UI_Plane_RedTeam")?, menu_slant("UI_Plane_FFA")?);
        let blue_local = transform(bp.loc, bp.rot, bp.scale);
        let blue = Slant::new(blue_material.clone(), bp.sort, [[0.0; 4]; 4]);
        let red = Slant::new(red_menu.material.clone(), red_menu.sort.unwrap_or(0), [[0.0; 4]; 4]);
        let ffa_slant = Slant::new(
            ffa_menu.material.clone(),
            ffa_menu.sort.unwrap_or(0),
            transform(ffa_menu.loc, ffa_menu.rot, ffa_menu.scale),
        );
        // (the main menu's MainMenuBackground actor, its slot 0 as UpdateBGForEvents left it at BeginPlay: an event's
        // material keeps its Zoom and offsets under the map picture, BG @1397)
        let bg_material = main.background_material(active_events).clone();
        let background = Background {
            param: background_texture(assets, &bg_material)?,
            cur: bg_material,
            matrix: transform(main.background.loc, main.background.rot, main.background.scale),
        };

        // CameraLerp: its float curve set at run time (SetLerpingValues), LastKeyFrame = the curve's last key, the
        // play rate the curve's MaxTime / Length; MMToCharSelect and AdjustPreviewActor end at their last key
        let scurve = &data.curves["CRV_SCurve"].keys;
        let last_key = |tl: &format::Timeline, extra: &[format::CurveKey]| {
            tl.tracks.values().flat_map(|c| c.keys.iter()).chain(extra).fold(0.0f64, |m, k| m.max(k.t))
        };
        let (mm_len, adj_len) =
            (last_key(timeline("MMToCharSelect")?, &[]), last_key(timeline("AdjustPreviewActor")?, &[]));
        let (move_len, t0_len) = (last_key(move_tl, scurve), last_key(t0, &[]));

        // The banners (made once per pre-match, hidden: the level's BeginPlay hides them all, @21628 -> @19222)
        let mut banners = Vec::new();
        for (k, inst) in data.banners.iter().enumerate() {
            let parts = BannerParts::of_prematch(&data, inst, assets)?;
            // (Banner Placement's scale: 1.117 on Player2 / 3, 1.05 / 1.1 / 1.15 on the FFA ones)
            let s = inst.pieces["Banner Placement"].scale[0];
            let pieces = &inst.pieces;
            let color = pieces
                .get("Banner Trim Mask")
                .and_then(|p| p.set.as_ref())
                .and_then(|m| m.get("ColorType"))
                .copied()
                .unwrap_or(1.0);
            let view = BannerView::new(parts, [0.0; 3], s, inst.index, color as u8);
            banners.push(LevelBanner {
                inst: k,
                view,
                rel: [0.0; 3],
                shown: false,
                check_mark: pieces["CheckMarkPos"].loc,
                perk_info: pieces["PerkInfoPos"].loc,
                slug: String::new(),
                pending: false,
                applied: false,
                element: None,
                trim_on: true,
            });
        }
        let cam = &main.camera;
        let mut level = PreMatchLevel {
            team_style: 0,
            cursor: [0.0; 2],
            plane,
            lens: (cam.fov, cam.aspect),
            anchors,
            movers,
            slot_of,
            char_start: [[0.0; 3]; 4],
            char_end: [[0.0; 3]; 4],
            started: false,
            char_select_done: false,
            players_found_done: false,
            music: None,
            cam_loc: cam.loc,
            cam_rot: cam.rot,
            cam_from: ([0.0; 3], [0.0; 3]),
            cam_to: ([0.0; 3], [0.0; 3]),
            cam_curve: String::new(),
            cam_delay: false,
            cam_lerp: Timeline::new(1.0),
            shaker: CameraShaker::default(),
            shot_loc: cam.loc,
            mm_to_char_select: Timeline::new(mm_len),
            adjust_preview: Timeline::new(adj_len),
            move_tl: Timeline::new(move_len),
            timeline0: Timeline::new(t0_len),
            main_loc: main_plane.loc,
            main_rot: main_plane.rot,
            main_scale: main_plane.scale,
            blue_local,
            red_loc: rp.loc,
            red_rot: rp.rot,
            red_scale: rp.scale,
            blue,
            red,
            ffa_slant,
            background,
            map_load: None,
            map_pan: -1.0,
            sequence: None,
            emitters: LevelEmitters::new(&data.emitters),
            banners,
            element_meshes: Vec::new(),
            skins: None,
            data: Arc::new(data),
        };
        level.place_slants();
        for i in 0..level.banners.len() {
            level.place_banner(i);
        }
        Ok(level)
    }

    /// The open-level paths: BeginPlay, then SetupCharSelect; PM_NewSlants plays Move at its
    /// BeginPlay (the level's name has "PreMatch", or a rematch: RematchForce SL@2460..2646). The queue:
    /// "MoveToCharSelect" to the three actors (PGI@33995): SetupCharSelect, Move (not a rematch, SL@3286),
    /// TimelineTransition (its update is empty). Once; the renderer calls it as it opens the level.
    pub fn open(&mut self, previews: &mut PreviewSet, events: &mut Vec<MenuEvent>, assets: &Assets) -> Result<()> {
        self.start(&mut Ports { previews, events, assets })
    }

    fn start(&mut self, p: &mut Ports<'_>) -> Result<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        p.previews.open(p.assets.store())?;
        self.setup_char_select(p);
        self.move_tl.play();
        Ok(())
    }

    /// The switch value of the music group `mus` the level asked since the last take (PF_Post_Global_Switch).
    pub fn take_music(&mut self) -> Option<&'static str> {
        self.music.take()
    }

    fn ffa(&self) -> bool {
        self.team_style == 2
    }

    /// The banner names of the preview slots (Usernames[i] and the arrays are by preview slot): [P1, P3, P2, P4]
    /// (FFA: the _FFA ones).
    fn slot_banners(&self) -> [&'static str; 4] {
        let l = if self.ffa() { BANNER_FFA } else { BANNER_P };
        [l[0], l[2], l[1], l[3]]
    }

    fn banner_named(&self, name: &str) -> Option<usize> {
        self.banners.iter().position(|b| self.data.banners[b.inst].name == name)
    }

    /// BPI_PreMatchTransition, SkinDataReceived, the map's preload and the
    /// cosmetics' arrival. `Cosmetics` asks nothing of the level: the screen reads the banners and icons again and
    /// sends `set_banner`.
    pub fn command(
        &mut self,
        previews: &mut PreviewSet,
        events: &mut Vec<MenuEvent>,
        assets: &Assets,
        cmd: SeqCmd,
    ) -> Result<()> {
        let mut p = Ports { previews, events, assets };
        self.start(&mut p)?;
        match cmd {
            SeqCmd::Transition(name) => self.transition(&mut p, &name),
            SeqCmd::PreloadMap(level) => {
                self.preload_map(&p, &level);
                Ok(())
            }
            SeqCmd::SkinData(d) => {
                self.skin_data_received(*d);
                Ok(())
            }
            SeqCmd::Cosmetics => Ok(()),
            SeqCmd::Podium(_) => {
                warn_once("pre-match level: a podium command".into());
                Ok(())
            }
        }
    }

    // ---------- PlayTransition ----------

    /// Each level actor gets every BPI_PreMatchTransition the widget sends (the player
    /// controller forwards it to the level's PlayTransition @26461, PM_NewSlants' PlayTransition SL@2651 and
    /// MainMenuBackground's PlayPrematchTransition BG@823); the level here is the one receiver of the three.
    fn transition(&mut self, p: &mut Ports<'_>, name: &str) -> Result<()> {
        let n = name.to_lowercase();
        // the level @26461
        match n.as_str() {
            "matchmaking" => {
                // the PlayersFound DoOnce opens again; SetMatchState("Matchmaking")
                self.players_found_done = false;
                p.previews.set_match_state(PreviewState::Matchmaking);
            }
            "cancelmatchmaking" => p.previews.set_match_state(PreviewState::CharSelect),
            "playersfound" => self.players_found(p),
            "movetocharselect" => self.setup_char_select(p),
            "updatelocations" => self.update_check_mark_pos(p),
            _ => {}
        }
        // PM_NewSlants SL@2651: PlayersFound Timeline_0.PlayFromStart, MatchReady the clash sequence
        if n == "playersfound" {
            self.timeline0.play_from_start();
        } else if n == "matchready" {
            self.play_sequence(if self.ffa() { "Seq_Clash_FFA_New" } else { "Seq_Clash" }, SeqEnd::None);
        }
        // MainMenuBackground BG@823
        if !NOT_MAPS.contains(&n.as_str()) {
            self.nasty_map_bg_change(p, name);
        }
        Ok(())
    }

    /// SetupCharSelect @29969: AnimateInvite (a party's, not ported), StartCameraLerp(0,
    /// CRV_SCurve, 2.1, CameraTarget_CharSelect), the music switch mus-mus_menu_chaselect (PF_Post_Global_Switch
    /// @18136: asked of the program, `take_music`), the match state, the two side anchors' ends and
    /// MoveMMToCharSelect(true).
    fn setup_char_select(&mut self, p: &mut Ports<'_>) {
        if self.char_select_done {
            return;
        }
        self.char_select_done = true;
        self.start_camera_lerp("CRV_SCurve", 2.1, "CameraTarget_CharSelect");
        self.music = Some("mus_menu_chaselect");
        p.previews.set_match_state(PreviewState::CharSelect);
        let (s2, s3) = (self.anchors["Start_Char_2"], self.anchors["Start_Char_3"]);
        self.char_start[1] = s2;
        self.char_start[2] = s3;
        self.char_end[1] = add3(s2, char_select_shift("Start_Char_2"));
        self.char_end[2] = add3(s3, char_select_shift("Start_Char_3"));
        self.mm_to_char_select.play();
    }

    /// StartCameraLerp(InitialDelay, Curve, Length, EndPoint) @27822: SetLerpingValues now (the
    /// camera's transform as the start), CameraLerp.PlayFromStart after RetriggerableDelay(InitialDelay = 0): the next
    /// tick.
    fn start_camera_lerp(&mut self, curve: &str, length: f64, target: &str) {
        let max = self.data.curves[curve].keys.last().map_or(0.0, |k| k.t);
        self.cam_curve = curve.to_string();
        self.cam_lerp.length = max;
        self.cam_lerp.rate = max / length;
        self.cam_from = (self.cam_loc, self.cam_rot);
        let a = &self.data.anchors[target];
        self.cam_to = (a.loc, a.rot);
        self.cam_delay = true;
    }

    /// PlayersFound @13406 -> @12332, once until "Matchmaking".
    fn players_found(&mut self, p: &mut Ports<'_>) {
        if self.players_found_done {
            return;
        }
        self.players_found_done = true;
        let ffa = self.ffa();
        p.previews.set_match_state(PreviewState::FaceOff);
        self.start_camera_lerp("CRV_Exp", 1.25, "CameraTarget_Faceoff");
        for (start, name) in self.char_start.iter_mut().zip(START_CHAR) {
            *start = self.anchors[name];
        }
        self.determine_end_location();
        // SetPlayRate, Play (@27096: from where it stopped, its end after a first PlayersFound)
        self.adjust_preview.rate = 1.0 / if ffa { 1.35 } else { 1.25 };
        self.adjust_preview.play();
        self.play_sequence(if ffa { "Seq_AnimInFFA_New" } else { "Seq_AnimInRed" }, SeqEnd::FaceOffComplete);
        // @12942..13323: Player2's banner hidden and put on FaceOff_Char_2 at scale 1, its social menu off
        if let Some(i) = self.banner_named("BP_Banner_PM_Player2") {
            self.show_banner(i, false);
            let mover = self.mover_world(&self.data.banners[self.banners[i].inst].mover);
            self.banners[i].rel = sub3(self.anchors["FaceOff_Char_2"], mover);
            self.place_banner(i);
        }
        self.update_check_mark_pos(p);
    }

    /// DetermineEndLocation @33320: 1v1 moves the two outer face-off anchors (and their banner
    /// movers) apart; the ends are the face-off anchors (FFA: the _FFA ones) @21134.
    fn determine_end_location(&mut self) {
        if self.team_style == 0 {
            if let Some(a) = self.anchors.get_mut("FaceOff_Char_1") {
                a[1] -= 175.0;
            }
            if let Some(a) = self.anchors.get_mut("FaceOff_Char_4") {
                a[1] += 175.0;
            }
            if let Some(m) = self.movers.get_mut("BannerMover1") {
                m.1[1] -= 10.0;
            }
            if let Some(m) = self.movers.get_mut("BannerMover4") {
                m.1[1] += 10.0;
            }
        }
        let ends = if self.ffa() { FACE_OFF_FFA } else { FACE_OFF };
        for (end, name) in self.char_end.iter_mut().zip(ends) {
            *end = self.anchors[name];
        }
        for i in 0..self.banners.len() {
            self.place_banner(i);
        }
    }

    /// AdjustPreviewActor's end @33948.
    fn adjust_preview_finished(&mut self, p: &mut Ports<'_>) {
        self.update_check_mark_pos(p);
        let style = self.team_style;
        if style == 0 {
            p.previews.show_shadow(0);
            p.previews.show_shadow(3);
            for n in ["BP_Banner_PM_Player1", "BP_Banner_PM_Player4"] {
                if let Some(i) = self.banner_named(n) {
                    self.show_banner(i, true);
                }
            }
        } else {
            for slot in 0..4 {
                p.previews.show_shadow(slot);
            }
            for n in if style == 2 { BANNER_FFA } else { BANNER_P } {
                if let Some(i) = self.banner_named(n) {
                    self.show_banner(i, true);
                }
            }
        }
    }

    /// BPI_ShowPlayer(Index, IsTeammate) @28287: the preview at Index takes the skin of
    /// Teammates / Enemies [Index] (team 0 / 1), then BPI_SequenceEvent(Index) to the widgets.
    // Unverified: an empty entry (a slot nobody holds) is not sent to the preview; the event goes all the same (the
    // build's arrays are not exported).
    fn show_player(&mut self, p: &mut Ports<'_>, index: i64, is_teammate: bool) -> Result<()> {
        let Ok(slot) = usize::try_from(index) else { return Ok(()) };
        if slot > 3 {
            return Ok(());
        }
        let list = self.skins.as_ref().map(|d| if is_teammate { &d.teammates } else { &d.enemies });
        if let Some(Some(spec)) = list.and_then(|l| l.get(slot)) {
            let spec = PreviewSpec { team: if is_teammate { 0 } else { 1 }, ..spec.clone() };
            p.previews.change_fighter(p.assets.store(), slot, &spec, false)?;
        }
        p.events.push(MenuEvent::SequenceUi(slot.to_string()));
        Ok(())
    }

    /// SkinDataReceived @28007: the arrays by preview slot; the usernames to the banners @17144
    /// (Set User Data, BAN): the slot's banner takes its positions. The names and icons on the plate are the
    /// screen's.
    fn skin_data_received(&mut self, d: PreMatchSkinData) {
        self.skins = Some(d);
        let style = self.team_style;
        for name in self.slot_banners() {
            if let Some(i) = self.banner_named(name) {
                self.set_user_data(i, style);
            }
        }
    }

    /// Set User Data (BAN)'s positions: the style's check mark and perk info places, set from
    /// the instance's values each time (the build's adds run once). (WBP_3dText's name, the founder treatment and
    /// FlipIcons on a 1v1 / 2v2 banner whose name has neither Player1 nor Player2 are the screen's plate.)
    fn set_user_data(&mut self, i: usize, style: u8) {
        let inst = &self.data.banners[self.banners[i].inst];
        let (mut check_mark, mut perk_info) = (inst.pieces["CheckMarkPos"].loc, inst.pieces["PerkInfoPos"].loc);
        let has = |s: &str| inst.name.contains(s);
        match style {
            0 => {
                // Set1v1Positions
                if has("Player1") {
                    check_mark = [0.0, 300.0, -35.0];
                    perk_info = add3(perk_info, [0.0, 0.0, 15.0]);
                } else if has("Player4") {
                    check_mark = [0.0, -300.0, -35.0];
                    perk_info = add3(perk_info, [0.0, 15.0, 15.0]);
                }
            }
            1 => {
                // Set2v2Positions
                if has("Player1") {
                    check_mark = [0.0, 300.0, -35.0];
                    perk_info = add3(perk_info, [0.0, 5.0, 10.0]);
                } else if has("Player2") {
                    check_mark = [0.0, 180.0, -35.0];
                    perk_info = add3(perk_info, [0.0, 0.0, -13.0]);
                } else if has("Player3") {
                    check_mark = [0.0, -180.0, -35.0];
                    perk_info = add3(perk_info, [0.0, 17.0, -13.0]);
                } else if has("Player4") {
                    check_mark = [0.0, -300.0, -35.0];
                    perk_info = add3(perk_info, [0.0, 10.0, 10.0]);
                }
            }
            2 => check_mark = [0.0, 200.0, -35.0], // SetFFAPositions
            _ => {}
        }
        let b = &mut self.banners[i];
        b.check_mark = check_mark;
        b.perk_info = perk_info;
    }

    /// The banner of a preview slot takes the BannerData `slug` (None
    /// or "": the default banner, which a banner applied keeps). Apply Banner Appearance (BAN @179-2311) does nothing
    /// unless the BannerData is valid with an element (@5-159; the level calls it only for a valid Banner,
    /// @10746-10785): the element is made at the next tick (the effects' player is the tick's), and a bad slug keeps
    /// the default banner.
    // Deviation: the screen sends the slug of each preview slot's player (read through the session's
    // banner slug, widget index 1 for the 1v1 opponent's slot 3, none for slot 1).
    // Deviation: the element is made at the next tick, not at the call (the build's LoadAsset_Blocking has it at
    // once; the banner is applied once the files are in).
    pub fn set_banner(&mut self, slot: usize, slug: Option<&str>) -> Result<()> {
        let Some(name) = self.slot_banners().get(slot).copied() else { return Ok(()) };
        let Some(i) = self.banner_named(name) else { return Ok(()) };
        let b = &mut self.banners[i];
        if b.applied {
            return Ok(());
        }
        b.slug = slug.unwrap_or("").to_string();
        b.pending = !b.slug.is_empty();
        Ok(())
    }

    /// Set Profile Icon (BAN @0-415): the nameplate's ProfileTexture of a preview slot's banner, a store texture path
    /// (None or one that does not load: the material's own picture).
    pub fn set_banner_icon(&mut self, assets: &Assets, slot: usize, path: Option<&str>) {
        let Some(name) = self.slot_banners().get(slot).copied() else { return };
        let Some(i) = self.banner_named(name) else { return };
        let icon = path.filter(|p| match assets.texture(p) {
            Ok(_) => true,
            Err(e) => {
                warn_once(format!("pre-match profile icon {p}: {e}"));
                false
            }
        });
        self.banners[i].view.icon = icon.map(str::to_owned);
    }

    /// Apply Banner Appearance (BAN @179-2311): the default background's StencilValue -1 @179 (it never draws again)
    /// and the Banner Filter's SetVisibility(false) @229 (hidden in game already); the element spawned at Base Element
    /// Root's world transform and attached to it (@686-884), initialized with BannerIndex = the instance's Banner
    /// Index, ColorType = FTrunc(the trim's ColorType), SlantAngle = the trim's Angle (the mask instance's, as
    /// makeBanner's) @1183-1255, and shown or hidden with the actor (@2311).
    fn apply_element(&mut self, cx: &mut LevelCx<'_>, i: usize) -> Result<()> {
        let b = &self.banners[i];
        let inst = &self.data.banners[b.inst];
        let trim_color =
            inst.pieces.get("Banner Trim Mask").and_then(|p| p.set.as_ref()).and_then(|m| m.get("ColorType"));
        let init = BannerInit {
            banner_index: inst.index,
            color_type: trim_color.map_or(1.0, |c| c.trunc()) as u8,
            slant_angle: b.view.parts.slant_angle,
        };
        let slug = b.slug.clone();
        let root = self.element_root(i);
        let Some(mut el) = BannerElement::new(cx, &slug, init, root, self.cam_loc)? else { return Ok(()) };
        let b = &mut self.banners[i];
        b.applied = true;
        b.trim_on = !el.hide_trim;
        el.show(b.shown);
        b.element = Some(el);
        self.show_pieces(i);
        Ok(())
    }

    // ---------- the map picture ----------

    /// The Background Lookup Table's picture for a map. A key with no picture (Map_WaterTower:
    /// None) or one the build's files lack (MapBG_M007, not exported) and a string the table lacks give none. The
    /// lookup's keys are object paths; the port's map has the level's name: matched by the path's last part.
    fn map_picture(&self, selection: &str) -> Option<(String, MapPicture)> {
        let want = tail(selection);
        let bg = &self.data.background;
        let (_, name) = bg.lookup.iter().find(|(k, _)| tail(k) == want)?;
        let name = name.as_ref()?;
        Some((name.clone(), bg.textures.get(name)?.clone()))
    }

    /// The map's picture ahead of NastyMapBGChange: the build's lookup holds hard references to
    /// resident textures (MapBG_* NeverStream), so its swap is the transition's frame. The picture is read into the
    /// texture cache when the screen stores the found match's map (Handle All Players Registered PM@85491), 5.5 s
    /// before the swap.
    fn preload_map(&mut self, p: &Ports<'_>, selection: &str) {
        let Some((name, pic)) = self.map_picture(selection) else { return };
        if self.map_load.as_ref().is_some_and(|l| l.name == name) {
            return;
        }
        let _ = self.load_map_picture(p.assets, name, &pic, false);
    }

    /// NastyMapBGChange(Selection) BG@1319: the next map's MapBG from the Background Lookup
    /// Table on the background plane's dynamic instance, and Tint (0.4, 0.4, 0.4) on its `Material` variable, in one
    /// call: no timeline (the class's TimelineMapChange is never played). With no picture the background keeps its
    /// own.
    // Unverified: the two are read as one instance (CreateDynamicMaterialInstance on a plane whose material is
    // already a MID returns it).
    fn nasty_map_bg_change(&mut self, p: &Ports<'_>, selection: &str) {
        let Some((name, pic)) = self.map_picture(selection) else { return };
        match &mut self.map_load {
            Some(l) if l.name == name => {
                if l.shown {
                    return;
                }
                l.shown = true;
            }
            _ => {
                // A picture that fails to load leaves the background its own
                if !self.load_map_picture(p.assets, name, &pic, true) {
                    return;
                }
            }
        }
        self.show_map_picture(&pic);
    }

    /// Reads the picture into the texture cache; false (logged) when it does not load, and nothing is recorded.
    fn load_map_picture(&mut self, assets: &Assets, name: String, pic: &MapPicture, shown: bool) -> bool {
        if let Err(e) = assets.texture(&pic.path) {
            warn_once(format!("pre-match background {name}: {e}"));
            return false;
        }
        self.map_load = Some(MapLoad { name, shown });
        true
    }

    // Deviation: the texture wrap is the material's own sampler's, not the picture's (ax, ay).
    fn show_map_picture(&mut self, pic: &MapPicture) {
        let bg = &mut self.background;
        bg.cur.textures.insert(bg.param.clone(), pic.path.clone());
        bg.cur.vectors.insert("Tint".to_string(), MAP_TINT);
        // Deviation: the class's TimelineMapChange played at the swap, which no statement of this build starts
        self.map_pan = 0.0;
        self.tick_map_pan(0.0);
    }

    /// TimelineMapChange (BG@1875-1922): 2.0 s, ignoring time dilation; its update sets XOffset
    /// = Lerp(0.2, 0.001, Curve), CurveFloat_1_2_3 from (0, 0) to (2.0, 1) with cubic keys and zero tangents (3u^2 -
    /// 2u^3). Its finish is empty: the picture stays at 0.001 (the MI's own XOffset is -0.028).
    fn tick_map_pan(&mut self, dt: f64) {
        if self.map_pan < 0.0 {
            return;
        }
        self.map_pan = (self.map_pan + dt).min(MAP_PAN_TIME);
        let u = self.map_pan / MAP_PAN_TIME;
        let c = u * u * (3.0 - 2.0 * u);
        self.background.cur.scalars.insert("XOffset".to_string(), 0.2 + (0.001 - 0.2) * c);
        if self.map_pan >= MAP_PAN_TIME {
            self.map_pan = -1.0;
        }
    }

    // ---------- sequences ----------

    /// Starts the level sequence `name`.
    fn play_sequence(&mut self, name: &str, end: SeqEnd) {
        let s = &self.data.sequences[name];
        self.sequence = Some(ActiveSeq { player: SequencePlayer::new(name, s), end });
    }

    /// One evaluation of the sequence from `span.prev` (exclusive) to `span.t`:
    /// event keys and section starts in between fire (a key at the start fires on the first evaluation); material and
    /// fade channels at t, within their sections (every section is KeepState: after its end the last value stays).
    /// Particle keys and 3DTransform channels go to the level's Emitter actors; material channels to the slants and
    /// the previews' BlobShadowCenter. Bindings the port has no actor for (PreMatch's UI_Plane_TeamA / B,
    /// PS_AmbientDust, the banners' "Plane") are skipped; the camera cut is the level's camera.
    fn eval_sequence(&mut self, cx: &mut LevelCx<'_>, name: &str, span: SeqSpan) -> Result<()> {
        let data = Arc::clone(&self.data);
        let Some(sequence) = data.sequences.get(name) else { return Ok(()) };
        let SeqSpan { prev, t, .. } = span;
        for tr in &sequence.tracks {
            match tr {
                SeqTrack::Event { keys, .. } => {
                    for k in keys.iter().filter(|k| span.crossed(k.t)) {
                        self.sequence_event(cx, &k.call, &k.args)?;
                    }
                }
                SeqTrack::CameraShake { sections, .. } => {
                    // Deviation: the shake starts at its section's start and runs its own length (the section's end
                    // is not applied).
                    // PM_CameraShake (Seq_Clash) plays through CameraShaker's rotation oscillation.
                    for sec in sections {
                        if sec.range[0].is_some_and(|lo| span.crossed(lo)) {
                            self.shaker.play_scaled(&format!("{}_C", sec.shake), sec.play_scale);
                        }
                    }
                }
                SeqTrack::Particle { binding, keys, .. } => {
                    for k in keys.iter().filter(|k| span.crossed(k.t)) {
                        for b in binding.iter().flatten() {
                            self.emitters.key(cx.vfx.as_deref_mut(), b, &k.key);
                        }
                    }
                }
                SeqTrack::Transform { binding, range, channels, .. } => {
                    let (lo, hi) = (range[0].unwrap_or(f64::NEG_INFINITY), range[1].unwrap_or(f64::INFINITY));
                    if t < lo || prev >= hi {
                        continue;
                    }
                    let at = t.min(hi);
                    for (i, c) in channels.iter().enumerate() {
                        let Some(v) = c.as_ref().and_then(|c| eval_seq_channel(c, at)) else { continue };
                        for b in binding.iter().flatten() {
                            self.emitters.set_channel(b, i, v);
                        }
                    }
                }
                SeqTrack::AkAudioEvent { sections, .. } => {
                    for sec in sections {
                        if let (Some(lo), Some(event)) = (sec.range[0], &sec.event)
                            && span.crossed(lo)
                        {
                            cx.events.push(MenuEvent::Sound { event: event.clone(), at: None });
                        }
                    }
                }
                SeqTrack::Fade { sections, .. } => {
                    for sec in sections {
                        if let Some(v) = eval_seq_channel(&sec.fade, t) {
                            cx.events.push(MenuEvent::Fade(v));
                        }
                    }
                }
                SeqTrack::Visibility { binding, property, sections, .. } => {
                    // bHidden of a plate actor (Seq_AnimInFFA_New: UI_Plane_RedTeam shown from 0): the last key at or
                    // before t
                    if property.as_deref() != Some("bHidden") {
                        continue;
                    }
                    let Some(slant) = binding.as_ref().and_then(|b| b.first()).and_then(|b| slant_of(b)) else {
                        continue;
                    };
                    for sec in sections {
                        if t < sec.range[0].unwrap_or(f64::NEG_INFINITY) {
                            continue;
                        }
                        if let Some(k) = sec.bools.keys.iter().rfind(|k| k.t <= t) {
                            self.slant_mut(slant).visible = !k.v;
                        }
                    }
                }
                SeqTrack::ComponentMaterial { binding, sections, .. } => {
                    let first = binding.as_ref().and_then(|b| b.first());
                    // a preview's BlobShadowCenter (Seq_Clash's OpacityOverride fade)
                    if let Some(slot) = first.and_then(|b| shadow_slot(&data.previews, b)) {
                        for sec in sections {
                            let (lo, hi) =
                                (sec.range[0].unwrap_or(f64::NEG_INFINITY), sec.range[1].unwrap_or(f64::INFINITY));
                            if t < lo || prev >= hi {
                                continue;
                            }
                            let at = t.min(hi);
                            for (param, c) in &sec.scalars {
                                if let Some(v) = eval_seq_channel(c, at) {
                                    cx.previews.shadow_scalar(slot, param, v);
                                }
                            }
                        }
                        continue;
                    }
                    let Some(slant) = first.and_then(|b| slant_of(b)) else {
                        continue;
                    };
                    for sec in sections {
                        let (lo, hi) =
                            (sec.range[0].unwrap_or(f64::NEG_INFINITY), sec.range[1].unwrap_or(f64::INFINITY));
                        if t < lo || prev >= hi {
                            continue;
                        }
                        let at = t.min(hi);
                        for (param, c) in &sec.scalars {
                            if let Some(v) = eval_seq_channel(c, at) {
                                self.slant_mut(slant).scalar(param, v);
                            }
                        }
                    }
                }
                SeqTrack::CameraCut { .. } => {}
            }
        }
        Ok(())
    }

    fn slant_mut(&mut self, k: Plate) -> &mut Slant {
        match k {
            Plate::Blue => &mut self.blue,
            Plate::Red => &mut self.red,
            Plate::Ffa => &mut self.ffa_slant,
        }
    }

    /// The director's functions (SequenceDirector_C): ShowPlayer -> the level's BPI_ShowPlayer,
    /// SequenceUI_Event -> BPI_SequenceEvent on the widgets.
    fn sequence_event(&mut self, cx: &mut LevelCx<'_>, call: &str, args: &[EventArg]) -> Result<()> {
        let mut p = Ports { previews: &mut *cx.previews, events: &mut *cx.events, assets: cx.assets };
        match call {
            "ShowPlayer" => {
                // Deviation: the build's ShowPlayer goes through the voice's actor map; this reaches the preview slot
                // directly (the voice has no port in the menus).
                // Deviation: a non-number first argument is taken as NaN: the preview stays.
                let Some(EventArg::Num(n)) = args.first() else { return Ok(()) };
                let index = *n as i64;
                self.show_player(&mut p, index, matches!(args.get(1), Some(EventArg::Bool(true))))
            }
            "SequenceUI_Event" => {
                let name = match args.first() {
                    Some(EventArg::Str(s)) => s.clone(),
                    Some(EventArg::Num(n)) => n.to_string(),
                    Some(EventArg::Bool(b)) => b.to_string(),
                    None => String::new(),
                };
                p.events.push(MenuEvent::SequenceUi(name));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Dispose: the level's emitters and the banner elements let go with it.
    pub fn dispose(&mut self, cx: &mut LevelCx<'_>) {
        self.emitters.dispose(cx.vfx.as_deref_mut());
        for b in &mut self.banners {
            if let Some(el) = &mut b.element {
                el.release(cx);
            }
        }
    }

    // ---------- frame ----------

    /// The timelines, the camera lerp, the sequence, the shake, the movers and
    /// banners, the map's pan, and the banner elements (made, placed, with the cursor's parallax).
    pub fn tick(&mut self, cx: &mut LevelCx<'_>, dt: f64) -> Result<()> {
        {
            let mut p = Ports { previews: &mut *cx.previews, events: &mut *cx.events, assets: cx.assets };
            self.start(&mut p)?;
        }
        for i in 0..self.banners.len() {
            if self.banners[i].pending {
                self.banners[i].pending = false;
                self.apply_element(cx, i)?;
            }
        }
        if dt > 0.0 {
            self.tick_timelines(cx, dt);
            self.tick_map_pan(dt);
            if let Some(q) = &mut self.sequence {
                let data = Arc::clone(&self.data);
                let (name, s) = data.sequences.get_key_value(q.player.name.as_str()).ok_or_else(|| Error::Format {
                    what: "PreMatch level".into(),
                    why: format!("no sequence {}", q.player.name),
                })?;
                let span = q.player.advance(s, dt);
                self.eval_sequence(cx, name, span)?;
                if span.end {
                    let end = self.sequence.take().map_or(SeqEnd::None, |q| q.end);
                    if end == SeqEnd::FaceOffComplete {
                        cx.events.push(MenuEvent::TransitionComplete("FaceOff".into()));
                    }
                }
            }
            self.shaker.update(dt);
        }
        // the shake's offset in the camera's space: x along the view direction, z along its up
        let [pitch, yaw] = [self.cam_rot[0].to_radians(), self.cam_rot[1].to_radians()];
        let (sx, sz) = (self.shaker.x, self.shaker.z);
        self.shot_loc = [
            self.cam_loc[0] + pitch.cos() * yaw.cos() * sx - pitch.sin() * yaw.cos() * sz,
            self.cam_loc[1] + pitch.cos() * yaw.sin() * sx - pitch.sin() * yaw.sin() * sz,
            self.cam_loc[2] + pitch.sin() * sx + pitch.cos() * sz,
        ];
        self.place_slants();
        // BAN ReceiveTick @11081: each applied element's parallax from UI_Cursor's MousePosNormalized (FindUICursor(0),
        // @11134) and the camera's location
        self.element_meshes.clear();
        for i in 0..self.banners.len() {
            if self.banners[i].shown {
                self.place_banner(i);
            }
            if self.banners[i].element.is_some() {
                let root = self.element_root(i);
                let (cursor, cam, shown) = (self.cursor, self.cam_loc, self.banners[i].shown);
                let Some(el) = &mut self.banners[i].element else { continue };
                if shown {
                    el.parallax(cursor, cam);
                }
                el.place(cx, root, &mut self.element_meshes);
            }
        }
        if let Some(v) = cx.vfx.as_deref_mut() {
            self.emitters.follow(v);
        }
        Ok(())
    }

    /// The tick's timelines in this order: CameraLerp (started the tick after the one that asks), MMToCharSelect,
    /// AdjustPreviewActor, Move, Timeline_0. A step's update is followed by its finished event.
    fn tick_timelines(&mut self, cx: &mut LevelCx<'_>, dt: f64) {
        let mut p = Ports { previews: &mut *cx.previews, events: &mut *cx.events, assets: cx.assets };
        let step = if self.cam_delay {
            self.cam_delay = false;
            self.cam_lerp.play_from_start();
            TimelineStep::Idle
        } else {
            self.cam_lerp.tick(dt)
        };
        if let TimelineStep::Update(pos) | TimelineStep::Finished(pos) = step {
            // LerpCamera (TLerp of the two transforms); the focus distance is not ported (no depth of field)
            // Deviation: the CineCamera's manual focus distance (UpdateFocusDistance @35437) is not kept: the
            // menus have no depth of field.
            let a = eval_curve(&self.data.curves[&self.cam_curve].keys, pos);
            self.cam_loc = lerp3(self.cam_from.0, self.cam_to.0, a);
            self.cam_rot = lerp3(self.cam_from.1, self.cam_to.1, a);
            if matches!(step, TimelineStep::Finished(_)) {
                // its end sends BPI_SequenceEvent("") to the widgets (@29472 -> @15796)
                p.events.push(MenuEvent::SequenceUi(String::new()));
            }
        }
        // MMToCharSelect (MoveMMToCharSelect(true) @33850): update @37792 -> @37527, finished @35589 (forward only:
        // BPI_SequenceEvent("ToCharSelectFinished") @35635 -> @5407)
        let step = self.mm_to_char_select.tick(dt);
        if let TimelineStep::Update(pos) | TimelineStep::Finished(pos) = step {
            let a = eval_curve(&self.data.timelines["MMToCharSelect"].tracks["NewTrack_0"].keys, pos);
            self.lerp_anchor(&mut p, "Start_Char_2", self.char_start[1], self.char_end[1], a);
            self.lerp_anchor(&mut p, "Start_Char_3", self.char_start[2], self.char_end[2], a);
            let at = lerp3(self.anchors["PartnerNameplate_MM"], self.anchors["PartnerNameplate_PM"], a);
            self.set_mover_world("BannerMover2", at);
            for slot in [0, 2, 1, 3] {
                p.previews.lerp_sizing(slot, PreviewState::MainMenu, PreviewState::CharSelect, a);
            }
            if matches!(step, TimelineStep::Finished(_)) {
                p.events.push(MenuEvent::SequenceUi("ToCharSelectFinished".into()));
            }
        }
        // AdjustPreviewActor (PlayersFound @12332..): update @34818 -> @33916 -> @29622 -> @10981 (the four anchors,
        // LerpSizing CharSelect -> FaceOff), finished @33948 (forward only)
        let step = self.adjust_preview.tick(dt);
        if let TimelineStep::Update(pos) | TimelineStep::Finished(pos) = step {
            let a = eval_curve(&self.data.timelines["AdjustPreviewActor"].tracks["NewTrack_0"].keys, pos);
            for (i, name) in START_CHAR.into_iter().enumerate() {
                self.lerp_anchor(&mut p, name, self.char_start[i], self.char_end[i], a);
            }
            for slot in 0..4 {
                p.previews.lerp_sizing(slot, PreviewState::CharSelect, PreviewState::FaceOff, a);
            }
            if matches!(step, TimelineStep::Finished(_)) {
                self.adjust_preview_finished(&mut p);
            }
        }
        // Move: Move.SetFloatCurve(CRV_SCurve) at BeginPlay (SL@1845); update SL@20..1687
        if let TimelineStep::Update(pos) | TimelineStep::Finished(pos) = self.move_tl.tick(dt) {
            let a = eval_curve(&self.data.curves["CRV_SCurve"].keys, pos);
            self.main_loc[1] = 380.0 + (3665.0 - 380.0) * a;
            // MIDCharSelect's HorizontalMask (SL@327).
            // Unverified: the MID the update's own CreateDynamicMaterialInstance calls replace each tick (SetMaterial
            // with the instance asset first, SL@1128 / @1200), so it writes to a material no plane shows; not applied.
            self.blue.fresh();
            self.blue.vector("DarkenColor", DARKEN_BLUE);
            self.red.fresh();
            self.red.vector("DarkenColor", DARKEN_RED);
        }
        // Timeline_0 (PlayersFound SL@2884): update SL@4465..4909 and @3495..4281
        if let TimelineStep::Update(pos) | TimelineStep::Finished(pos) = self.timeline0.tick(dt) {
            let a = eval_curve(&self.data.slants.timelines["Timeline_0"].tracks["Curve"].keys, pos);
            if self.team_style == 2 {
                self.main_loc[0] = -950.0 + (-1450.0 - -950.0) * a;
            }
            self.main_loc[1] = 3665.0 + (3250.0 - 3665.0) * a;
            self.red_loc[1] = -1000.0 + (-250.0 - -1000.0) * a;
            // FFA: MIDCharSelect's TeamColor / OffsetX / Angle (SL@3905..4281): the orphaned MID as above, not applied
        }
    }

    /// LerpLocation(Actor, Start, End, Alpha) @33202 on a Start_Char anchor: the preview
    /// attached to it follows.
    fn lerp_anchor(&mut self, p: &mut Ports<'_>, anchor: &str, from: V3, to: V3, a: f64) {
        let v = lerp3(from, to, a);
        if let Some(a) = self.anchors.get_mut(anchor) {
            *a = v;
        }
        if let Some(&slot) = self.slot_of.get(anchor) {
            p.previews.set_anchor(slot, v);
        }
    }

    /// UI_Plane_BlueTeam on UI_Plane_MainMenu, UI_Plane_RedTeam, UI_Plane_FFA: their
    /// transforms.
    fn place_slants(&mut self) {
        let main = transform(self.main_loc, self.main_rot, self.main_scale);
        self.blue.matrix = mul4(&self.blue_local, &main);
        self.red.matrix = transform(self.red_loc, self.red_rot, self.red_scale);
    }

    /// The level's camera of the moment for an output of `size` (`frame`'s).
    pub fn camera(&self, size: [u32; 2]) -> mvs_view::Camera {
        super::main_menu::menu_camera_turned(
            self.shot_loc,
            self.cam_rot,
            self.shaker.rot,
            self.lens.0,
            self.lens.1,
            size,
        )
    }

    /// The level's camera, planes and banners into the
    /// frame.
    pub fn frame(&mut self, out: &mut MenuFrame) {
        out.camera = Some(self.camera(out.size));
        let bg = &self.background;
        out.meshes.push((
            MeshPose {
                mesh: self.plane.clone(),
                to_world: to_world(&bg.matrix),
                materials: vec![material_ref(&bg.cur, None)],
                ..MeshPose::default()
            },
            0,
        ));
        // (the planes' order within a priority is the translucent pass's, by view depth)
        for s in [&self.blue, &self.red, &self.ffa_slant] {
            if s.visible {
                out.meshes.push((s.pose(&self.plane), s.sort));
            }
        }
        for b in &self.banners {
            b.view.meshes(&mut out.meshes);
            b.view.custom_depth(&mut out.custom_depth);
        }
        out.meshes.extend(self.element_meshes.iter().cloned());
    }

    // ---------- actors ----------

    /// A mover's world location.
    fn mover_world(&self, name: &str) -> V3 {
        let (parent, local) = &self.movers[name];
        add3(self.anchors[parent], *local)
    }

    /// Moves a mover to the world location `loc`.
    fn set_mover_world(&mut self, name: &str, loc: V3) {
        let Some((parent, _)) = self.movers.get(name) else { return };
        let local = sub3(loc, self.anchors[parent]);
        if let Some(m) = self.movers.get_mut(name) {
            m.1 = local;
        }
        for i in 0..self.banners.len() {
            if self.data.banners[self.banners[i].inst].mover == name {
                self.place_banner(i);
            }
        }
    }

    /// Banner Placement's (uniform) scale, which its children take.
    fn placement_scale(&self, i: usize) -> f64 {
        self.data.banners[self.banners[i].inst].pieces["Banner Placement"].scale[0]
    }

    /// The banner actor's location (its root; rotation 0, scale 1).
    fn banner_root(&self, i: usize) -> V3 {
        add3(self.mover_world(&self.data.banners[self.banners[i].inst].mover), self.banners[i].rel)
    }

    /// Nametag Root: Banner Placement (the Offset, its scale S) + S x its own location.
    fn nametag_root(&self, i: usize) -> V3 {
        let p = &self.data.banners[self.banners[i].inst].pieces;
        let (pl, nr, s) = (p["Banner Placement"].loc, p["Nametag Root"].loc, self.placement_scale(i));
        add3(self.banner_root(i), [pl[0] + s * nr[0], pl[1] + s * nr[1], pl[2] + s * nr[2]])
    }

    /// NametagWidget's world location (Nametag Root's scale is 1: S x its location).
    fn nametag_widget(&self, i: usize) -> V3 {
        let w = self.data.banners[self.banners[i].inst].pieces["NametagWidget"].loc;
        let s = self.placement_scale(i);
        add3(self.nametag_root(i), [s * w[0], s * w[1], s * w[2]])
    }

    /// The element's actor root: Base Element Root on Banner Placement (its location S x its own, its scale S x its
    /// own; the actor and Banner Placement unrotated), as [location, rotator, scale].
    fn element_root(&self, i: usize) -> [V3; 3] {
        let p = &self.data.banners[self.banners[i].inst].pieces;
        let pl = p["Banner Placement"].loc;
        let s = self.placement_scale(i);
        let Some(r) = p.get("Base Element Root") else { return [self.banner_root(i), [0.0; 3], [s; 3]] };
        let loc = add3(self.banner_root(i), [pl[0] + s * r.loc[0], pl[1] + s * r.loc[1], pl[2] + s * r.loc[2]]);
        [loc, r.rot, [s * r.scale[0], s * r.scale[1], s * r.scale[2]]]
    }

    /// The mask's world location (Banner Placement + S x the mask's own).
    fn place_banner(&mut self, i: usize) {
        let p = &self.data.banners[self.banners[i].inst].pieces;
        let (pl, m, s) = (p["Banner Placement"].loc, p["Banner Mask"].loc, self.placement_scale(i));
        let at = add3(self.banner_root(i), [pl[0] + s * m[0], pl[1] + s * m[1], pl[2] + s * m[2]]);
        let view = &mut self.banners[i].view;
        view.start = at;
        view.place(0.0);
    }

    /// SetActorHiddenInGame(!show): the pieces the class shows and the element.
    fn show_banner(&mut self, i: usize, show: bool) {
        self.banners[i].shown = show;
        if show {
            self.banners[i].view.start();
            self.place_banner(i);
        }
        self.show_pieces(i);
        if let Some(el) = &mut self.banners[i].element {
            el.show(show);
        }
    }

    /// The pieces shown: none while the actor is hidden; the filter never (hidden in game);
    /// the default background not once a banner is applied (StencilValue -1); the trim unless the element hid it; the
    /// nameplate with its material's default icon (it does not wait for the icon's load); the mask's custom depth copy
    /// with the actor.
    fn show_pieces(&mut self, i: usize) {
        let b = &mut self.banners[i];
        let show = b.shown;
        for k in 0..b.view.shown.len() {
            b.view.shown[k] =
                show && k != PIECE_FILTER && (k != PIECE_BACKGROUND || !b.applied) && (k != PIECE_TRIM || b.trim_on);
        }
    }

    /// UpdateCheckMarkPos @37623 -> @37555 -> @10953: the banners' CheckMarkPos and PerkInfoPos
    /// world points to the widgets' BPI_SendWorldLocation("CheckMark<n>" / "PerkInfo<n>"); the widget projects them
    /// (`Renderer::menu_project`) when it places its plates and cards.
    fn update_check_mark_pos(&mut self, p: &mut Ports<'_>) {
        let list: [&str; 4] = match self.team_style {
            0 => [BANNER_P[0], BANNER_P[3], BANNER_P[3], BANNER_P[3]],
            2 => BANNER_FFA,
            _ => BANNER_P,
        };
        for (n, name) in list.iter().enumerate() {
            let Some(i) = self.banner_named(name) else { continue };
            let s = self.placement_scale(i);
            let ws = self.data.banners[self.banners[i].inst].pieces["NametagWidget"].scale;
            let (wl, cm, pi) = (self.nametag_widget(i), self.banners[i].check_mark, self.banners[i].perk_info);
            let check = [wl[0] + cm[0] * s * ws[0], wl[1] + cm[1] * s * ws[1], wl[2] + cm[2] * s * ws[2]];
            p.events.push(MenuEvent::WorldLocation { id: format!("CheckMark{}", n + 1), at: check });
            let nr = self.nametag_root(i);
            let perk = [nr[0] + pi[0] * s, nr[1] + pi[1] * s, nr[2] + pi[2] * s];
            p.events.push(MenuEvent::WorldLocation { id: format!("PerkInfo{}", n + 1), at: perk });
        }
    }

    /// The screen draws a banner's
    /// NametagWidget (WBP_3dText) itself, at the place `plates` gives; the level keeps no texture. Any widget but a
    /// nametag is the podium's.
    pub fn widget(&mut self, w: SceneWidget, texture: Option<&Texture>) -> Result<()> {
        let _ = texture;
        match w {
            SceneWidget::Nametag(_) => {}
            SceneWidget::WinnerText | SceneWidget::StatSheet { .. } => {
                warn_once("pre-match level: a podium widget".into());
            }
        }
        Ok(())
    }

    /// The NametagWidget's world place by preview slot, for each shown banner
    /// (the screen projects it with `Renderer::menu_project`, as its Space Screen component is drawn at the
    /// projected point).
    // Deviation: the component's scale is not applied to the widget.
    pub fn plates(&self) -> Vec<(usize, V3)> {
        let mut out = Vec::new();
        for (slot, name) in self.slot_banners().iter().enumerate() {
            if let Some(i) = self.banner_named(name)
                && self.banners[i].shown
            {
                out.push((slot, self.nametag_widget(i)));
            }
        }
        out
    }
}

/// The plates of PM_NewSlants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plate {
    Blue,
    Red,
    Ffa,
}

/// The plate a sequence binding names.
/// The preview slot whose BlobShadowCenter a binding names (`PreMatch:<actor>/BlobShadowCenter`).
fn shadow_slot(previews: &[PreMatchPreview], binding: &str) -> Option<usize> {
    let actor = binding.strip_prefix("PreMatch:")?.strip_suffix("/BlobShadowCenter")?;
    previews.iter().position(|p| p.actor == actor)
}

fn slant_of(binding: &str) -> Option<Plate> {
    if binding.starts_with("PM_NewSlants:UI_Plane_BlueTeam") {
        Some(Plate::Blue)
    } else if binding.starts_with("PM_NewSlants:UI_Plane_RedTeam") {
        Some(Plate::Red)
    } else if binding.starts_with("PM_NewSlants:UI_Plane_FFA") {
        Some(Plate::Ffa)
    } else {
        None
    }
}
