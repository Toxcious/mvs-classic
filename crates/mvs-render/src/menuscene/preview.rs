//! The preview actors: BP_CharacterPreview1 .. 4 of the PreMatch level
//! and the main menu's Player1, each a fighter's view (mvs-view PreviewView) placed by its skin's ScaleOverrides,
//! playing its PreviewIdle (its HomescreenIdle in the main menu), lit by its skin's rig, with the glow shell on a change
//! of fighter.
//!
//! Slot i is the actor tagged Player<i + 1> (the game mode's BPI_GetPreviewActor(i)); Player1 is the main menu's
//! preview too (the main menu is a streamed level of PreMatch).
//!
//! Lighting: GetLightingChannels outside the podium gives Player1 / Player2 lighting channel 1 and Player3 / Player4
//! channel 2, besides channel 0; the skin's rig (BP_CharLights_*) is spawned at the actor's transform and attached to
//! it (KeepWorld: it follows the actor), its lights on the actor's channel; the lights under the rig's Lights
//! component are mirrored in Y when FacingLeft matches the rig's mirrorWhen; a SpawnLighting destroys the actor's
//! last rig (@30110), a ClearFighter does not. MM_PointLightHover (channel 0, [`HOVER_LIGHT`]) lights every preview.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use indexmap::IndexMap;
use mvs_vfx::{Attachment, Player, SystemId};
use mvs_view::{FighterPose, M4, MeshPose, PreviewAt, PreviewView};

use super::format::{
    self, BlobShadow, Glow, LevelName, MirrorWhen, MontageNotify, MontageSegment, Placement, Point, PreMatchPreview,
    PreviewEntry, SceneData, SceneMontage, ShadowPlace, SpotLight, material_ref,
};
use super::main_menu::GlowShell;
use super::notify_vfx::{NotifyVfx, set_tag_visible};
use super::{MenuEvent, MenuFrame, PreviewSpec};
use crate::fx::warn_once;
use crate::gpu::{Gpu, Recorded};
use crate::stage::{to_world, transform};
use crate::warm::WarmJob;
use crate::{Assets, Error, Result};

/// The game instance's match state as the previews read it: "Matchmaking"
/// places as "CharSelect"; the hover light is 15 in "CharSelect" alone. The PreMatch level writes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewState {
    #[default]
    MainMenu,
    CharSelect,
    Matchmaking,
    FaceOff,
}

impl PreviewState {
    /// The ScaleOverrides key's state part ("Matchmaking" reads as "CharSelect").
    fn placement_name(self) -> &'static str {
        match self {
            PreviewState::MainMenu => "MainMenu",
            PreviewState::CharSelect | PreviewState::Matchmaking => "CharSelect",
            PreviewState::FaceOff => "FaceOff",
        }
    }
}

/// A point light of BP_CharacterPreview's (its place in the actor's space and its radius).
#[derive(Clone, Copy, Debug)]
pub struct HoverLight {
    pub name: &'static str,
    /// MainMenuLights' RelativeLocation.
    pub parent: [f64; 3],
    /// The light's RelativeLocation under DefaultSceneRoot.
    pub loc: [f64; 3],
    /// AttenuationRadius.
    pub radius: f64,
}

/// The preview's hover light. BP_CharacterPreview's component templates (the class export; the level's instance
/// overrides nothing): PointLightComponent MM_PointLightHover, attached to the SceneComponent MainMenuLights
/// (RelativeLocation (0, 197, 0)), which is attached to DefaultSceneRoot: RelativeLocation (140, -90, 320),
/// IntensityUnits Candelas, Intensity 0, AttenuationRadius 350, CastShadows false; the rest is the class default
/// (white, SourceRadius / SoftSourceRadius 0, SpecularScale 1, inverse-squared falloff, lighting channel 0, which
/// every preview's mesh is on). Nothing in the ubergraph moves or mirrors MainMenuLights (FacingLeft does not reach
/// it). BPI_OnHovered (ubergraph @25355): SetIntensity(15.0) when the game instance's match state is "CharSelect",
/// else SetIntensity(3.0); BPI_OnUnhovered (@25657): SetIntensity(0.0). Both at once, no timeline.
// Unverified: the candela conversion (x 100 x 100, as the rig's spot lights) and the point light's class defaults are
// 4.26 source. The values are written here (the preview entries hold the rig's spot lights only).
// Why a hover shows next to nothing in the main menu: the data makes it that faint. The light sits at the actor +
// (140, 107, 320), 2.3 to 2.8 m from the upper body, and its radius window ((1 - (d / 350)^4)^2) is 0.33 at the chest
// and 0 below the hips (d >= 350); with the diffuse term's N.L / pi it adds 1 to 3 % of the ambient on the upper body.
pub const HOVER_LIGHT: HoverLight =
    HoverLight { name: "MM_PointLightHover", parent: [0.0, 197.0, 0.0], loc: [140.0, -90.0, 320.0], radius: 350.0 };

/// The largest rig's lights (BP_CharLights_WonderWoman's 7): a rig with more is a data error.
const RIG_SPOTS: usize = 7;

/// AnimNotify classes of the previews' PreviewIdle montages that draw nothing here: AnimNotify_C017_SetHurtBox (a
/// fighter's hurt box; nothing on a preview).
const NOTIFY_SKIPPED: [&str; 1] = ["AnimNotify_C017_SetHurtBox"];

// The loader's view of a fighter is made on the loader's threads, the actor shown at the first tick after it is in
// (the build's loader runs a Delay(0) after the change, its assets on disk). A view asked for again before it is in
// cancels the one before it (a sweep of the cursor across the grid: what has not begun is not made).
// A fighter not yet viewed begins to load at once, with no settle delay before the load.

/// PlayVFX's light beam and its life (PRE@9859: a Delay(1.0), not retriggerable), the swirl, the sound.
const BEAM: &str = "PS_CharacterSelectLightBeam";
const BEAM_LIFE: f64 = 1.0;
const SWIRL: &str = "PS_CharacterSelectSwirl04";
const BEAM_SOUND: &str = "ui_characterselect_lightbeam";

/// A view made on the loader, with the commands its warm-up recorded (None: no warm-up).
pub type Made = (PreviewView, Option<Recorded>);

/// A fighter's view loading on the loader's threads (`PreviewSet::load`).
struct ViewLoad {
    key: String,
    /// Set when the slot moved on: a job not begun makes nothing.
    cancel: Arc<AtomicBool>,
    ticket: mvs_load::Ticket<Option<std::result::Result<Made, String>>>,
}

impl std::fmt::Debug for ViewLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewLoad").field("key", &self.key).finish_non_exhaustive()
    }
}

impl Drop for ViewLoad {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A fighter's view on a slot with what shows it.
#[derive(Debug)]
struct Shown {
    view: PreviewView,
    shell: Option<GlowShell>,
    /// AnimNotify_PlayVFX's components on it.
    fx: NotifyVfx,
}

/// SpawnLighting's rig: its lights and whether the mirrored ones take Y -1.
#[derive(Debug)]
struct Rig {
    lights: Vec<SpotLight>,
    flip: bool,
}

#[derive(Debug)]
struct Slot {
    tag: String,
    anchor: [f64; 3],
    rel: [f64; 3],
    scale: [f64; 3],
    facing_left: bool,
    /// DesiredSkin: its SkinData name ("" the default skin); None none.
    skin: Option<String>,
    /// Its fighter.
    key: Option<String>,
    /// The fighter's view loading on the loader.
    loading: Option<ViewLoad>,
    /// LerpSizing asked before the skin's data was in.
    lerp: Option<(PreviewState, PreviewState, f64)>,
    /// Loaded: shown at the next tick.
    ready: Option<Shown>,
    fighter: Option<Shown>,
    /// The Reverse Dissolve plays.
    clearing: bool,
    montage: Option<SceneMontage>,
    pos: f64,
    section: usize,
    /// The notify states begun, in the order they began.
    active: Vec<usize>,
    /// PlayVFX: due on the next tick (its Delay(0)s).
    fx_due: bool,
    beam: Option<SystemId>,
    swirl: Option<SystemId>,
    /// The time left of the beam's Delay(1.0) (None: none pending).
    beam_left: Option<f64>,
    swirl_yaw: f64,
    rig: Option<Rig>,
    /// MM_PointLightHover's intensity (cd).
    hover: f64,
    /// BlobShadowCenter not bHiddenInGame.
    shadow: bool,
    /// BlobShadowCenter's SetVisibility.
    shadow_visible: bool,
    /// Its place under the actor's root (the level's, BlobShadowAdjustment's).
    shadow_place: ShadowPlace,
    /// The sequences' writes to its material's scalars.
    shadow_scalars: IndexMap<String, f64>,
    /// The MainMenu placements' scale factor `fit_slot` found (1: none).
    fit_k: f64,
    /// The MainMenu placements' lift `fit_slot` found, added to their Z (0: none).
    fit_lift: f64,
}

/// The preview actors. Its layout and the glow come from the store's PreMatch and
/// MainMenu entries at the first change of fighter, or from [`PreviewSet::with_layout`].
#[derive(Debug, Default)]
pub struct PreviewSet {
    pub state: PreviewState,
    /// The actors' preview entries, by slot (None: none in yet).
    pub assets: Vec<Option<PreviewEntry>>,
    slots: Vec<Slot>,
    /// The actors as the level places them (tag, start point, FacingLeft, BlobShadowCenter): a dispose puts the slots
    /// back to these.
    layout: Vec<(String, [f64; 3], bool, ShadowPlace)>,
    /// Each slot's start point's name (Start_Char_n).
    starts: Vec<String>,
    /// BlobShadowCenter's mesh and material (None: the set was made without them).
    blob_shadow: Option<BlobShadow>,
    /// BP_CharacterPreview's glow shell (None: no MainMenu entry, so no shell before its menu data).
    glow: Option<Glow>,
    /// The active match config's team style (None: none, as in the main menu).
    team_style: Option<u8>,
    /// UI_MainMenu is up and visible (New_MainMenu's level open): with the match state MainMenu, PandaGameInstance
    /// InMainMenu.
    main_menu_up: bool,
    /// A fighter's view is made on this thread, in at once (a scripted run's frames, which wait for no loader).
    wait_loads: bool,
    /// Effects to kill and events to send at the next tick (the calls between ticks have no player or event list).
    kills: Vec<SystemId>,
    pending: Vec<MenuEvent>,
    /// The renderer's warm-up job the next view load takes (`offer_warm`), and the commands of the views' warm-ups
    /// in, run at the next tick before their first draw.
    warm: Option<WarmJob>,
    recorded: Vec<Recorded>,
    rng: u64,
    /// The main menu camera's frame the MainMenu placements are fitted to (`fit_slot`).
    fit: Option<FitFrame>,
    /// The store the last `open` was given (the fit's vertex boxes).
    store: Option<mvs_store::Store>,
}

/// How far below the frame's top edge a fitted fighter's top bone may reach: a share of the frame's half-height,
/// room for the hair and the head above the highest bone.
const FIT_TOP: f64 = 0.85;

/// A fitted fighter whose top bone stays below this share of the frame's half-height above its centre line is too
/// low (Taz's C015_Default MainMenu_You, left at T 0): it is lifted until its top is at `FIT_LIFT_TO`. Every skin
/// whose MainMenu_You was set up reaches well above the centre line.
const FIT_LOW: f64 = 0.0;
/// Where a too-low fighter's top bone is lifted to: Wonder Woman's (the build's own main menu fighter) is near 0.68.
const FIT_LIFT_TO: f64 = 0.6;
/// How tall a too-low fighter may stand, its feet to its top bone, in half-heights of the frame: its placement was
/// never set up for the main menu, so its size is the port's too. A big one is shrunk to it before the lift (Iron
/// Giant's MainMenu_You leaves him 2.3 tall, his chest filling the frame; Taz stands 1.3).
const FIT_SPAN: f64 = 1.8;

/// The poses of the montage's last section the fit measures.
const FIT_SAMPLES: usize = 12;

/// A camera's 16:9 frame as the fit reads it: its place, its forward and up vectors (UE, Z up), and the tangent of
/// its vertical half-angle.
#[derive(Clone, Copy, Debug)]
pub struct FitFrame {
    pub origin: [f64; 3],
    pub front: [f64; 3],
    pub up: [f64; 3],
    pub tan_half_v: f64,
}

impl FitFrame {
    /// The main menu's CineCamera (`menu_camera`'s lens on its own 16:9 frame): rotator [pitch, yaw, roll] in
    /// degrees, `fov` the horizontal field of view.
    pub fn of_camera(loc: [f64; 3], rot: [f64; 3], fov: f64, aspect: f64) -> FitFrame {
        let (p, y) = (rot[0].to_radians(), rot[1].to_radians());
        FitFrame {
            origin: loc,
            front: [p.cos() * y.cos(), p.cos() * y.sin(), p.sin()],
            up: [-p.sin() * y.cos(), -p.sin() * y.sin(), p.cos()],
            tan_half_v: (fov.to_radians() / 2.0).tan() / aspect,
        }
    }

    /// The largest scale `s` at which the point `feet + s q` (`q` a mesh point's offset at scale 1) stays
    /// `FIT_TOP` of the half-height below the frame's top edge (None: the point sets no such limit).
    pub fn most_scale(&self, feet: [f64; 3], q: [f64; 3]) -> Option<f64> {
        let d = [0, 1, 2].map(|i| feet[i] - self.origin[i]);
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let k = self.tan_half_v * FIT_TOP;
        // up . (d + s q) <= k front . (d + s q)
        let per = dot(q, self.up) - k * dot(q, self.front);
        (per > 0.0 && dot(d, self.front) > 0.0).then(|| (k * dot(d, self.front) - dot(d, self.up)) / per)
    }

    /// The height in the frame of the point `d` from the camera: a share of the half-height above the centre line
    /// (None: behind the camera).
    pub fn height(&self, d: [f64; 3]) -> Option<f64> {
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let f = dot(d, self.front);
        (f > 0.0).then(|| dot(d, self.up) / (f * self.tan_half_v))
    }

    /// The scale factor (1 at most) about the feet `f` that makes the fighter, from `f` to its top point `d` (both
    /// from the camera), at most `span` half-heights tall in the frame, and the Z lift that then puts the top point at
    /// `share` of the half-height (None: a point behind the camera).
    pub fn shrink_and_lift(&self, f: [f64; 3], d: [f64; 3], span: f64, share: f64) -> Option<(f64, f64)> {
        let tall = self.height(d)? - self.height(f)?;
        let k = if tall > span { span / tall } else { 1.0 };
        let top = [0, 1, 2].map(|i| f[i] + k * (d[i] - f[i]));
        Some((k, self.lift_to(top, share)?))
    }

    /// How far up (world Z) the point `d` from the camera moves to sit at `share` of the half-height above the
    /// centre line.
    pub fn lift_to(&self, d: [f64; 3], share: f64) -> Option<f64> {
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let k = self.tan_half_v * share;
        // up . (d + z e) = k front . (d + z e)
        let per = self.up[2] - k * self.front[2];
        (per > 0.0).then(|| (k * dot(d, self.front) - dot(d, self.up)) / per)
    }
}

impl PreviewSet {
    /// The set of the PreMatch level's preview actors and start points, with the
    /// glow shell's data (None: no shell).
    pub fn with_layout(
        previews: &[PreMatchPreview],
        anchors: &IndexMap<String, Point>,
        glow: Option<Glow>,
    ) -> Result<PreviewSet> {
        let mut layout = Vec::with_capacity(previews.len());
        let starts = previews.iter().map(|p| p.anchor.clone()).collect();
        for (index, p) in previews.iter().enumerate() {
            if p.tag != format!("Player{}", index + 1) {
                return Err(Error::Format { what: "pre-match previews".into(), why: format!("{} at {index}", p.tag) });
            }
            let Some(a) = anchors.get(&p.anchor) else {
                return Err(Error::Format {
                    what: "pre-match previews".into(),
                    why: format!("start point {}", p.anchor),
                });
            };
            layout.push((p.tag.clone(), a.loc, p.facing_left, p.shadow));
        }
        let slots: Vec<Slot> = layout.iter().map(|(t, a, f, sh)| Slot::new(t, *a, *f, *sh)).collect();
        Ok(PreviewSet { assets: vec![None; slots.len()], slots, layout, starts, glow, ..PreviewSet::default() })
    }

    /// The layout from the store (the PreMatch entry's previews and anchors, the MainMenu entry's glow), once: at the
    /// first change of fighter or tick, or by the scene before it places an actor.
    pub fn open(&mut self, store: &mvs_store::Store) -> Result<()> {
        self.store = Some(store.clone());
        if !self.slots.is_empty() {
            return Ok(());
        }
        let pm = format::load_level(store, LevelName::PreMatch)?;
        let SceneData::PreMatch(pm) = pm.data else {
            return Err(Error::Format { what: pm.name, why: "not the PreMatch entry".into() });
        };
        let glow = match format::load_level(store, LevelName::MainMenu) {
            Ok(e) => match e.data {
                SceneData::MainMenu(m) => Some(m.glow),
                _ => None,
            },
            Err(e) => {
                warn_once(format!("pre-match preview: no glow shell: {e}"));
                None
            }
        };
        let (state, wait_loads, team_style, up, fit) =
            (self.state, self.wait_loads, self.team_style, self.main_menu_up, self.fit);
        *self = PreviewSet::with_layout(&pm.previews, &pm.anchors, glow)?;
        self.blob_shadow = Some(pm.blob_shadow);
        self.state = state;
        self.wait_loads = wait_loads;
        self.team_style = team_style;
        self.main_menu_up = up;
        self.fit = fit;
        self.store = Some(store.clone());
        Ok(())
    }

    /// Views made on this thread, in at the change (a scripted run: the frames wait for no loader).
    pub fn set_wait_loads(&mut self, wait: bool) {
        self.wait_loads = wait;
    }

    /// Whether UI_MainMenu is up and visible (the renderer's MainMenu level is open).
    pub fn set_main_menu_up(&mut self, up: bool) {
        self.main_menu_up = up;
    }

    /// PandaGameInstance InMainMenu (@0-301): GetMainMenu valid AND IsVisibleInHierarchy(it) AND the match state is
    /// "MainMenu" (@99-254). False on the PreMatch level of a custom game, whose state is MainMenu after a dispose.
    // Unverified: UI_MainMenu is taken as valid and visible on the loader tick after New_MainMenu's BeginPlay
    // (@193 BPI_LoadPreferredCharacter): the port has it up whenever the MainMenu level is open.
    fn in_main_menu(&self) -> bool {
        self.main_menu_up && self.state == PreviewState::MainMenu
    }

    /// The actor's start point.
    pub fn set_match_state(&mut self, state: PreviewState) {
        self.state = state;
    }

    /// The active match config's team style the actors read at a change of fighter (preMatchSession.setup's
    /// teamStyle; None: no config).
    pub fn set_team_style(&mut self, style: Option<u8>) {
        self.team_style = style;
    }

    /// The actor's world location.
    pub fn location(&self, slot: usize) -> [f64; 3] {
        match self.slots.get(slot) {
            Some(s) => add(s.anchor, s.rel),
            None => {
                warn_once(format!("pre-match preview: location of slot {slot} before the layout"));
                [0.0; 3]
            }
        }
    }

    /// `at`, a place beside the slot's fighter on the PreMatch level's character select (the slot's start point as
    /// SetupCharSelect moves it, the fighter at its CharSelect placement), carried to where the slot's fighter stands
    /// now: the same offset from its feet, scaled as the fighter is (its fit on the main menu included). None: no
    /// layout, or the fighter's preview entry is not in.
    pub fn char_select_follow(&self, slot: usize, at: [f64; 3]) -> Option<[f64; 3]> {
        let s = self.slots.get(slot)?;
        let start = add(self.layout.get(slot)?.1, super::pre_match::char_select_shift(self.starts.get(slot)?));
        self.assets.get(slot)?.as_ref()?;
        let then = self.placement(slot, PreviewState::CharSelect)?;
        if then.scale[2] <= 0.0 {
            return None;
        }
        let my = if s.facing_left { -1.0 } else { 1.0 };
        let feet = add(start, [then.loc[0], then.loc[1] * my, then.loc[2]]);
        let k = s.scale[2] / then.scale[2];
        let now = add(s.anchor, s.rel);
        Some([0, 1, 2].map(|i| now[i] + (at[i] - feet[i]) * k))
    }

    /// K2_SetActorLocation (world).
    pub fn set_location(&mut self, slot: usize, loc: [f64; 3]) {
        let Some(s) = self.slot_mut(slot) else { return };
        s.rel = [loc[0] - s.anchor[0], loc[1] - s.anchor[1], loc[2] - s.anchor[2]];
    }

    /// The start point the actor is attached to (Start_Char_n) moved to `loc` (world):
    /// the actor follows.
    pub fn set_anchor(&mut self, slot: usize, loc: [f64; 3]) {
        if let Some(s) = self.slot_mut(slot) {
            s.anchor = loc;
        }
    }

    /// LerpSizing(NewState, CurrentState, Alpha) (PreMatch @10801-14743): with a
    /// DesiredSkin, the mesh scale and the actor's relative location = VLerp(ScaleOverrides[Current + suffix],
    /// ScaleOverrides[New + suffix], Alpha), the location's Y negated when FacingLeft.
    pub fn lerp_sizing(&mut self, slot: usize, from: PreviewState, to: PreviewState, alpha: f64) {
        if self.slot_mut(slot).is_none_or(|s| s.skin.is_none()) {
            return;
        }
        if self.assets[slot].is_none() {
            self.slots[slot].lerp = Some((from, to, alpha));
            return;
        }
        let (Some(a), Some(b)) = (self.placement(slot, from), self.placement(slot, to)) else { return };
        let (a, b) = (self.fitted(slot, from, a), self.fitted(slot, to, b));
        let s = &mut self.slots[slot];
        let my = if s.facing_left { -1.0 } else { 1.0 };
        let l = |k: usize, p: &[f64; 3], q: &[f64; 3]| p[k] + (q[k] - p[k]) * alpha;
        s.rel = [l(0, &a.loc, &b.loc), l(1, &a.loc, &b.loc) * my, l(2, &a.loc, &b.loc)];
        s.scale = [l(0, &a.scale, &b.scale), l(1, &a.scale, &b.scale), l(2, &a.scale, &b.scale)];
    }

    /// BPI_ChangeFighter(Skin, TeamIndex, Force) (ubergraph @24442): nothing unless
    /// Skin differs from DesiredSkin or Force; DesiredSkin = Skin (TeamIndex is stored and read nowhere); FacingLeft
    /// (outside the podium, @24696-25029): false for Player1 and Player3, else NOT(the team style is 2: FFA), true
    /// without a config; then PlayVFX(), the old meshes destroyed at once (no dissolve) and, a Delay(0) later
    /// (@27161 -> 785), the loader: SetupMeshes(DesiredSkin), the placement from ScaleOverrides, SpawnLighting
    /// (@5044), the montage, SetupInitialVisibility (@8754) and GlowVFX(false) (@8790). The skin's preview entry from
    /// `store` (a skin with no entry of its own: the default's); its view made on the loader, shown at the first tick
    /// after it is in.
    pub fn change_fighter(
        &mut self,
        store: &mvs_store::Store,
        slot: usize,
        spec: &PreviewSpec,
        force: bool,
    ) -> Result<()> {
        self.open(store)?;
        if slot >= self.slots.len() {
            return Err(no_slot(slot));
        }
        let facing_left = self.facing_left_of(slot);
        let s = &mut self.slots[slot];
        // (the build compares the SkinData object, which is one character's: the default skin is "" for every
        // character, so the character is compared too)
        // (a roster entry with no fighter: no key, so two such entries compare equal)
        let key = (!spec.key.is_empty()).then_some(spec.key.as_str());
        if s.skin.as_deref() == Some(spec.skin.as_str()) && s.key.as_deref() == key && !force {
            return Ok(());
        }
        s.skin = Some(spec.skin.clone());
        s.loading = None;
        s.key = (!spec.key.is_empty()).then(|| spec.key.clone());
        s.lerp = None;
        s.ready = None;
        s.facing_left = facing_left;
        // BlobShadowAdjustment (@25232 -> @35035) in team style 2 for the actors not tagged Player1 / Player3:
        // relative location (-100, 0, -10), scale (3, 4, 1) x 0.75
        if self.team_style == Some(2) && s.tag != "Player1" && s.tag != "Player3" {
            s.shadow_place.loc = [-100.0, 0.0, -10.0];
            s.shadow_place.scale = [3.0 * 0.75, 4.0 * 0.75, 0.75];
        }
        // (@25247-25290: Player3's BlobShadowCenter shown at every change)
        if s.tag == "Player3" {
            s.shadow = true;
        }
        self.assets[slot] = None;
        self.play_vfx(slot);
        self.remove_fighter(slot);
        // (a roster entry with no fighter: its effect plays, nothing is shown)
        let Some(key) = self.slots[slot].key.clone() else { return Ok(()) };
        let asset = match preview_asset(store, &key, &spec.skin) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("pre-match preview: {key} {}: {e}", spec.skin);
                return Ok(());
            }
        };
        self.assets[slot] = Some(asset);
        // (the placement, early: the level reads it)
        self.place(slot);
        if let Some((from, to, alpha)) = self.slots[slot].lerp.take() {
            self.lerp_sizing(slot, from, to, alpha);
        }
        self.load(store, slot);
        Ok(())
    }

    /// The main menu's fighter (New_MainMenu's BeginPlay:
    /// BPI_LoadPreferredCharacter with a hard skin reference, so it is all in for the first tick): a ChangeFighter
    /// whose view is in, shown at the next tick, placed, animated and lit from `asset`, the skin's preview entry.
    pub fn set_fighter(
        &mut self,
        store: &mvs_store::Store,
        slot: usize,
        view: PreviewView,
        asset: PreviewEntry,
        skin: &str,
    ) -> Result<()> {
        self.open(store)?;
        if slot >= self.slots.len() {
            return Err(no_slot(slot));
        }
        let facing_left = self.facing_left_of(slot);
        let shell = self.glow.as_ref().map(GlowShell::new);
        let s = &mut self.slots[slot];
        s.skin = Some(skin.to_owned());
        s.loading = None;
        s.key = Some(view.key.clone());
        s.lerp = None;
        s.facing_left = facing_left;
        self.assets[slot] = Some(asset);
        self.play_vfx(slot);
        self.remove_fighter(slot);
        self.place(slot);
        self.slots[slot].ready = Some(Shown { fx: NotifyVfx::new(&view), view, shell });
        Ok(())
    }

    /// BPI_ClearFighter (@10648): only with a DesiredSkin; DesiredSkin = none,
    /// GlowVFX(true) (the Reverse Dissolve timeline, 0.75 s, whose end @21887 destroys the meshes), PlayVFX(), the
    /// blob shadow's SetVisibility(false) (@10766).
    pub fn clear_fighter(&mut self, slot: usize) {
        let Some(s) = self.slots.get_mut(slot) else { return };
        if s.skin.is_none() {
            return;
        }
        s.skin = None;
        s.loading = None;
        s.ready = None;
        if s.fighter.is_some() {
            s.clearing = true;
            let started = match (s.fighter.as_mut().and_then(|f| f.shell.as_mut()), &self.glow) {
                (Some(sh), Some(g)) => {
                    sh.start(g, true);
                    true
                }
                _ => false,
            };
            if !started {
                self.remove_fighter(slot);
            }
        }
        self.play_vfx(slot);
        self.slots[slot].shadow_visible = false;
    }

    /// ShowShadow (@34588 -> @14774): BlobShadowCenter.SetHiddenInGame(false).
    pub fn show_shadow(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.shadow = true;
        }
    }

    /// A level sequence's write to a scalar parameter of the slot's BlobShadowCenter material (Seq_Clash's
    /// OpacityOverride).
    pub fn shadow_scalar(&mut self, slot: usize, name: &str, v: f64) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.shadow_scalars.insert(name.to_owned(), v);
        }
    }

    /// BPI_OnHovered / BPI_OnUnhovered ([`HOVER_LIGHT`]).
    pub fn hover(&mut self, slot: usize, on: bool) {
        let cd = if self.state == PreviewState::CharSelect { 15.0 } else { 3.0 };
        if let Some(s) = self.slots.get_mut(slot) {
            s.hover = if on { cd } else { 0.0 };
        }
    }

    /// The slot shows a fighter (its meshes are set up).
    pub fn has_fighter(&self, slot: usize) -> bool {
        self.slots.get(slot).is_some_and(|s| s.fighter.is_some())
    }

    /// Every fighter and effect let go, the lights reset. A
    /// new set for each scene, so the slots go back to the level's actors (their start points and FacingLeft, no
    /// skin, no placement) and the match state to MainMenu; the layout, the glow, the team style and the way views
    /// load stay.
    pub fn dispose(&mut self, vfx: Option<&mut Player>) {
        for s in &mut self.slots {
            self.kills.extend([s.beam.take(), s.swirl.take()].into_iter().flatten());
            if let Some(f) = s.fighter.as_mut() {
                self.kills.extend(f.fx.drain());
            }
        }
        if let Some(p) = vfx {
            for id in self.kills.drain(..) {
                p.kill(id);
            }
        }
        self.kills.clear();
        self.pending.clear();
        self.slots = self.layout.iter().map(|(t, a, f, sh)| Slot::new(t, *a, *f, *sh)).collect();
        self.assets = vec![None; self.slots.len()];
        self.state = PreviewState::MainMenu;
    }

    /// The actors' tick: PlayVFX's delays, the loader's Delay(0), the
    /// glow shell's timeline and the montage.
    pub fn tick(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        mut vfx: Option<&mut Player>,
        events: &mut Vec<MenuEvent>,
        dt: f64,
    ) -> Result<()> {
        if let Err(e) = self.open(assets.store()) {
            warn_once(format!("pre-match preview: no layout: {e}"));
        }
        events.append(&mut self.pending);
        match vfx.as_deref_mut() {
            Some(p) => self.kills.drain(..).for_each(|id| p.kill(id)),
            None => self.kills.clear(),
        }
        for i in 0..self.slots.len() {
            self.tick_slot(i, vfx.as_deref_mut(), events, dt);
        }
        // (the views make their buffers at the draw; their warm-ups' commands go before it)
        for r in self.recorded.drain(..) {
            gpu.run_recorded(&r);
        }
        Ok(())
    }

    /// Whether the next view load has a warm-up job (`offer_warm`).
    pub(crate) fn has_warm(&self) -> bool {
        self.warm.is_some()
    }

    /// The warm-up job the next view load takes: the meshes, materials and textures of the view's first pose made
    /// on the loader with it, so its first draw makes none of them.
    pub(crate) fn offer_warm(&mut self, job: WarmJob) {
        self.warm = Some(job);
    }

    /// The posed views, their glow shells and the lights into the frame: each preview on its lighting channel, its
    /// rig's lights on that channel, the hover lights on channel 0 (every preview's).
    pub fn frame(&mut self, out: &mut MenuFrame) {
        for i in 0..self.slots.len() {
            let world = self.mesh_world(i);
            let channel = channel_of(i);
            let s = &mut self.slots[i];
            if let Some(f) = &mut s.fighter {
                let mut pose: FighterPose = f.view.pose(&world);
                pose.fighter = i as u32;
                if let (Some(sh), Some(g)) = (&f.shell, &self.glow) {
                    sh.meshes(g, &pose.meshes, &mut out.meshes);
                }
                out.fighters.push((pose, channel));
            }
            let w = add(s.anchor, s.rel);
            if let Some(r) = &s.rig {
                for l in &r.lights {
                    out.spots.push((channel, rig_light(l, w, r.flip)));
                }
            }
            if s.hover > 0.0 {
                out.spots.push((0, hover_light(w, s.hover)));
            }
            // BlobShadowCenter: under the actor's root, which has no rotation or scale (the SkeletalMesh carries
            // the placement's scale and the yaw)
            if let Some(b) = self.blob_shadow.as_ref().filter(|_| s.shadow && s.shadow_visible) {
                let p = &s.shadow_place;
                let m = transform(add(w, p.loc), p.rot, p.scale);
                out.meshes.push((
                    MeshPose {
                        mesh: b.mesh.as_str().into(),
                        to_world: to_world(&m),
                        materials: vec![material_ref(&b.material, Some(&s.shadow_scalars))],
                        ..MeshPose::default()
                    },
                    0,
                ));
            }
        }
    }

    // ---------- inside ----------

    /// The slot, or None with a warning (a call before the layout is open).
    fn slot_mut(&mut self, slot: usize) -> Option<&mut Slot> {
        if slot >= self.slots.len() {
            warn_once(format!("pre-match preview: slot {slot} before the layout"));
        }
        self.slots.get_mut(slot)
    }

    /// FacingLeft at a change of fighter: false for Player1 and Player3, else NOT(team style 2).
    fn facing_left_of(&self, slot: usize) -> bool {
        let tag = self.slots[slot].tag.as_str();
        if tag == "Player1" || tag == "Player3" { false } else { self.team_style != Some(2) }
    }

    /// The ScaleOverrides suffix: "_You" for Player1, Player4 or team style 2, else "_Friend".
    fn suffix(&self, slot: usize) -> &'static str {
        let tag = self.slots[slot].tag.as_str();
        if tag == "Player1" || tag == "Player4" || self.team_style == Some(2) { "_You" } else { "_Friend" }
    }

    fn placement(&self, slot: usize, state: PreviewState) -> Option<Placement> {
        let key = format!("{}{}", state.placement_name(), self.suffix(slot));
        let p = self.assets[slot].as_ref().and_then(|a| a.placements.get(&key)).cloned();
        if p.is_none() {
            warn_once(format!(
                "pre-match preview: no ScaleOverrides {key} for {}",
                self.slots[slot].key.as_deref().unwrap_or("none")
            ));
        }
        p
    }

    /// The loader's placement: ScaleOverrides[match state + suffix], fitted to the main menu's frame (`fitted`).
    fn place(&mut self, slot: usize) {
        let Some(p) = self.placement(slot, self.state) else { return };
        let p = self.fitted(slot, self.state, p);
        let s = &mut self.slots[slot];
        s.rel = [p.loc[0], p.loc[1] * if s.facing_left { -1.0 } else { 1.0 }, p.loc[2]];
        s.scale = p.scale;
    }

    /// The main menu camera's frame the MainMenu placements are fitted to (None: none fitted).
    pub fn set_fit_frame(&mut self, frame: Option<FitFrame>) {
        self.fit = frame;
    }

    /// `p` (the slot's placement for `state`) with the slot's fit (`fit_slot`) on a MainMenu placement's scale.
    fn fitted(&self, slot: usize, state: PreviewState, mut p: Placement) -> Placement {
        let (k, lift) = (self.slots[slot].fit_k, self.slots[slot].fit_lift);
        if state == PreviewState::MainMenu && k < 1.0 {
            p.scale = p.scale.map(|v| v * k);
        }
        if state == PreviewState::MainMenu {
            p.loc[2] += lift;
        }
        p
    }

    /// The fit of a fighter shown at a MainMenu placement (`show`, before its first frame): the scale cut so its
    /// posed meshes stay inside the main menu camera's 16:9 frame, `FIT_TOP` of the half-height below the top
    /// edge, over `FIT_SAMPLES` poses of its montage's last section (the loop it holds; the sections before it are
    /// its entrance); the feet stay where they are. A fighter that fits but whose top stays under `FIT_LOW` is
    /// shrunk to stand at most `FIT_SPAN` tall, its feet kept, then lifted, its top to `FIT_LIFT_TO`. True: the
    /// scale or the lift changed (the slot is placed again).
    // Deviation: the port's own rule. The build's main menu only ever shows
    // Skin_WonderWoman (New_MainMenu @193); the port shows the last chosen fighter, and some skins'
    // MainMenu_You placements, which the build never put on screen, put the fighter out of the frame (LeBron's C016
    // skins at scale 1.0, Garnet's at T 0). A fighter that fits is drawn as its placement says.
    fn fit_slot(&mut self, slot: usize) -> bool {
        let (Some(fit), Some(store)) = (self.fit, self.store.clone()) else { return false };
        let world = self.mesh_world(slot);
        let s = &mut self.slots[slot];
        let (feet, s0, facing_left) = (add(s.anchor, s.rel), s.scale[2], s.facing_left);
        let Some(f) = s.fighter.as_mut() else { return false };
        let mut poses = Vec::new();
        match &s.montage {
            Some(m) => {
                let from = m.sections.last().map_or(0.0, |l| l.start);
                for k in 0..FIT_SAMPLES {
                    let pos = from + (m.length - from) * k as f64 / FIT_SAMPLES as f64;
                    let Some(seg) = segment_at(m, pos) else { continue };
                    let time = (seg.from + (pos - seg.start) * seg.rate).min(seg.to);
                    let at = PreviewAt { clip: &seg.clip, time, facing_left, montage: &m.name, blend_in: 0.0 };
                    f.view.step(&at, 0.0);
                    poses.push(f.view.pose(&world));
                }
            }
            None => poses.push(f.view.pose(&world)),
        }
        let row = |a: &[f32; 12], r: usize, v: [f64; 3]| {
            f64::from(a[r * 4]) * v[0]
                + f64::from(a[r * 4 + 1]) * v[1]
                + f64::from(a[r * 4 + 2]) * v[2]
                + f64::from(a[r * 4 + 3])
        };
        let mut most = f64::INFINITY;
        // the highest corner in the frame: its height over the centre line (a share of tan_half_v) and its offset
        // from the camera
        let mut top: Option<(f64, [f64; 3])> = None;
        // (every shown skinned mesh of the pose: a fighter's body may be split, LeBron's C016Core holding no skin)
        for m in poses.iter().flat_map(|p| &p.meshes).filter(|m| !m.bones.is_empty() && m.shown.iter().any(|&s| s)) {
            let boxes = match mvs_view::mesh::bone_boxes(&store, &m.mesh) {
                Ok(b) => b,
                Err(e) => {
                    warn_once(format!("main menu fit: {}: {e}", m.mesh));
                    continue;
                }
            };
            for (b, bx) in m.bones.iter().zip(boxes.iter()) {
                let Some([lo, hi]) = bx else { continue };
                for c in 0..8 {
                    let v = [0, 1, 2].map(|k| f64::from(if c & (1 << k) == 0 { lo[k] } else { hi[k] }));
                    let posed = [0, 1, 2].map(|r| row(b, r, v));
                    let world = [0, 1, 2].map(|r| row(&m.to_world, r, posed));
                    // (the corner's offset from the feet at scale 1)
                    let q = [0, 1, 2].map(|k| (world[k] - feet[k]) / s0);
                    if let Some(l) = fit.most_scale(feet, q) {
                        most = most.min(l);
                    }
                    let d = [0, 1, 2].map(|k| world[k] - fit.origin[k]);
                    if let Some(t) = fit.height(d)
                        && top.is_none_or(|(h, _)| t > h)
                    {
                        top = Some((t, d));
                    }
                }
            }
        }
        if most < s0 && most > 0.0 {
            s.fit_k *= most / s0;
            return true;
        }
        let to_feet = [0, 1, 2].map(|k| feet[k] - fit.origin[k]);
        match top {
            Some((t, d)) if t < FIT_LOW => match fit.shrink_and_lift(to_feet, d, FIT_SPAN, FIT_LIFT_TO) {
                Some((k, dz)) if k < 1.0 || dz > 0.0 => {
                    s.fit_k *= k;
                    s.fit_lift = dz.max(0.0);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn mesh_world(&self, slot: usize) -> M4 {
        let s = &self.slots[slot];
        mesh_world(add(s.anchor, s.rel), s.scale, s.facing_left)
    }

    /// PlayVFX (@27321): unless BlockSpawnEffect (no montage of the build sets it), CharParticles, CharBeam and
    /// CharSwirl released; the new ones at the next tick.
    fn play_vfx(&mut self, slot: usize) {
        let s = &mut self.slots[slot];
        self.kills.extend([s.beam.take(), s.swirl.take()].into_iter().flatten());
        s.fx_due = true;
    }

    /// The fighter's view from the store, ready for the next tick (the loader's Delay(0)): the asked skin as its view
    /// resolves it (a name the character lacks: the default skin). One view make for every actor and skin, the main
    /// menu's included (BP_CharacterPreview has one SetupMeshes path).
    fn load(&mut self, store: &mvs_store::Store, slot: usize) {
        let s = &mut self.slots[slot];
        let (Some(key), Some(skin)) = (s.key.clone(), s.skin.clone()) else { return };
        let make = {
            let (store, key, skin, warm) = (store.clone(), key.clone(), skin.clone(), self.warm.take());
            move || make_view(&store, &key, &skin, warm)
        };
        if self.wait_loads {
            let made = make();
            self.loaded(slot, key, made, None);
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let ticket = mvs_load::spawn(move || (!flag.load(Ordering::Relaxed)).then(make));
        self.slots[slot].loading = Some(ViewLoad { key, cancel, ticket });
    }

    /// A slot's view in (`load`): ready for the next tick, the slot's fighter settled (the pre-match voice reads it).
    fn loaded(
        &mut self,
        slot: usize,
        key: String,
        made: std::result::Result<Made, String>,
        events: Option<&mut Vec<MenuEvent>>,
    ) {
        match made {
            Ok((view, recorded)) => {
                self.recorded.extend(recorded);
                let shell = self.glow.as_ref().map(GlowShell::new);
                self.slots[slot].ready = Some(Shown { fx: NotifyVfx::new(&view), view, shell });
                let e = MenuEvent::PreviewSettled { slot, key };
                match events {
                    Some(ev) => ev.push(e),
                    None => self.pending.push(e),
                }
            }
            Err(e) => eprintln!("pre-match preview: {key}: {e}"),
        }
    }

    /// The loaded fighter shown: SpawnLighting, the placement, the montage, the glow.
    /// A view with no entry waits (warned once); a rig of more lights than RIG_SPOTS keeps its first ones.
    fn show(&mut self, slot: usize) {
        let Some(asset) = self.assets[slot].as_ref() else {
            warn_once(format!("pre-match preview {slot}: a view with no entry"));
            return;
        };
        let Some(r) = self.slots[slot].ready.take() else { return };
        if asset.lights.len() > RIG_SPOTS {
            warn_once(format!(
                "pre-match preview: rig {} has {} lights, the first {RIG_SPOTS} drawn",
                asset.rig,
                asset.lights.len()
            ));
        }
        // @5999: BPI_IsInMainMenu (in_main_menu) ? the skin's HomescreenIdle : its PreviewIdle. (a skin with none,
        // Batman C004_S05's PreviewIdle: @5999 casts None, and @6237 CreateProxyObjectForPlayMontage is given nothing
        // to play, so the mesh shows no montage)
        // Unverified: the pose the preview's AnimBP gives with no montage and no pawn is not read; the view holds the
        // pose it was made in.
        let montage = if self.in_main_menu() { asset.homescreen_idle.clone() } else { asset.preview_idle.clone() };
        let s = &mut self.slots[slot];
        s.clearing = false;
        // SpawnLighting: the skin's rig at the actor
        s.rig = Some(Rig {
            lights: asset.lights.iter().take(RIG_SPOTS).cloned().collect(),
            flip: s.facing_left == (asset.mirror_when == MirrorWhen::Left),
        });
        s.pos = 0.0;
        s.section = montage.as_ref().map_or(0, |m| section_at(m, 0.0));
        s.montage = montage;
        s.active.clear();
        s.fighter = Some(r);
        s.fit_k = 1.0;
        s.fit_lift = 0.0;
        // @8754-8844 (the meshes set up, past the Podium's Delay): BlobShadowCenter.SetVisibility(true)
        s.shadow_visible = true;
        self.place(slot);
        if self.state == PreviewState::MainMenu && self.fit_slot(slot) {
            self.place(slot);
        }
        let s = &mut self.slots[slot];
        if let (Some(sh), Some(g)) = (s.fighter.as_mut().and_then(|f| f.shell.as_mut()), &self.glow) {
            sh.start(g, false);
        }
    }

    /// The meshes destroyed (ChangeFighter at once, ClearFighter at the reverse timeline's end): the view goes, with
    /// the notifies' effects on it.
    // Unverified: the PlayVFX components are attached to the destroyed mesh (SpawnEmitterAttached @36639) and are taken
    // to go with it; the ubergraph destroys VFXInstances only at a notify's end and in DestroyVFX (BlockSpawnEffect).
    fn remove_fighter(&mut self, slot: usize) {
        let s = &mut self.slots[slot];
        if let Some(mut f) = s.fighter.take() {
            self.kills.extend(f.fx.drain());
        }
        s.clearing = false;
        s.montage = None;
        s.active.clear();
    }

    fn tick_slot(&mut self, slot: usize, mut vfx: Option<&mut Player>, events: &mut Vec<MenuEvent>, dt: f64) {
        // (the loader's view, in since the last tick: shown at this one)
        if let Some(made) = self.slots[slot].loading.as_mut().and_then(|l| l.ticket.try_take()) {
            let l = self.slots[slot].loading.take().expect("polled above");
            if let Some(made) = made {
                self.loaded(slot, l.key.clone(), made, Some(events));
            }
        }
        let s = &mut self.slots[slot];
        // PlayVFX's Delay(0) and RetriggerableDelay(0): the beam on DefaultSceneRoot, the swirl at a world yaw of
        // Random(0..360), then ui_characterselect_lightbeam unless the match state is "MainMenu" and the actor is
        // Player1 (@0-759)
        let w = add(s.anchor, s.rel);
        match vfx.as_deref_mut() {
            Some(p) if s.fx_due => {
                s.fx_due = false;
                // (any uniform source)
                self.rng = next_random(self.rng);
                s.swirl_yaw = (self.rng >> 11) as f64 / (1u64 << 53) as f64 * 2.0 * std::f64::consts::PI;
                s.beam = p.spawn_attached(BEAM, &attachment(w, 0.0), 1.0, &[]);
                s.swirl = p.spawn_attached(SWIRL, &attachment(w, s.swirl_yaw), 1.0, &[]);
                // (PRE@9859: a Delay, not retriggerable: an effect played again within the second keeps the first
                // countdown, whose end destroys the beam then shown)
                s.beam_left.get_or_insert(BEAM_LIFE);
                if !(self.state == PreviewState::MainMenu && s.tag == "Player1") {
                    events.push(MenuEvent::Sound { event: BEAM_SOUND.into(), at: None });
                }
            }
            mut p => {
                if let Some(left) = s.beam_left {
                    let left = left - dt;
                    s.beam_left = Some(left);
                    if left <= 1e-6 {
                        if let (Some(id), Some(p)) = (s.beam, p.as_deref_mut()) {
                            p.kill(id);
                        }
                        s.beam = None;
                        s.beam_left = None;
                    }
                }
                // (the effects are attached to the actor's root: they follow it)
                if let Some(p) = p {
                    if let Some(id) = s.beam {
                        p.attach(id, &attachment(w, 0.0));
                    }
                    if let Some(id) = s.swirl {
                        p.attach(id, &attachment(w, s.swirl_yaw));
                    }
                }
            }
        }
        // BPI_ChangeFighter's Delay(0) -> the loader
        if self.slots[slot].ready.is_some() && self.slots[slot].fighter.is_none() {
            self.show(slot);
        }
        if self.slots[slot].fighter.is_some() {
            let s = &mut self.slots[slot];
            let ended = match (s.fighter.as_mut().and_then(|f| f.shell.as_mut()), &self.glow) {
                (Some(sh), Some(g)) => sh.tick(g, dt),
                _ => false,
            };
            if ended && s.clearing {
                self.remove_fighter(slot);
            } else {
                self.pose_fighter(slot, dt, vfx);
            }
        }
    }

    /// Montage_Advance: the position moves by dt; at a section's end it continues at
    /// the next section's start. The notifies over this tick's steps (as the podium's), then the view posed at the
    /// segment's time (no montage: the view keeps the pose it was made in), then the notifies' effects placed on its
    /// pose.
    fn pose_fighter(&mut self, slot: usize, dt: f64, mut vfx: Option<&mut Player>) {
        let world = self.mesh_world(slot);
        let s = &mut self.slots[slot];
        let (Some(m), Some(f)) = (&s.montage, &mut s.fighter) else { return };
        let (pos, section, steps) = advance(m, s.pos, s.section, dt);
        s.pos = pos;
        s.section = section;
        if let Some(list) = m.notifies.as_deref().filter(|l| !l.is_empty()) {
            let mut fx = Vec::new();
            notifies(&mut s.active, &mut f.view, list, &steps, &mut fx);
            if let Some(p) = vfx.as_deref_mut() {
                for (i, begin) in fx {
                    if begin {
                        let props = json_props(&list[i].props);
                        f.fx.begin(&mut f.view, &world, i, &props, s.facing_left, PREVIEW_COLOR_INDEX, p);
                    } else {
                        f.fx.end(i, p);
                    }
                }
            }
        }
        let Some(seg) = segment_at(m, pos) else { return };
        let t = (seg.from + (pos - seg.start) * seg.rate).min(seg.to);
        // (the montage's BlendIn, as the montage players pass it: CreateProxyObjectForPlayMontage PRE@6237 plays it
        // with its own blend)
        // Deviation: a fresh view has no pose to blend from, so it shows the montage's at once.
        let at = PreviewAt {
            clip: &seg.clip,
            time: t,
            facing_left: s.facing_left,
            montage: &m.name,
            blend_in: m.blend_in.unwrap_or(0.0),
        };
        f.view.step(&at, dt);
        if let Some(p) = vfx {
            f.fx.follow(&mut f.view, &world, s.facing_left, p);
        }
    }
}

impl Slot {
    fn new(tag: &str, anchor: [f64; 3], facing_left: bool, shadow_place: ShadowPlace) -> Slot {
        Slot {
            tag: tag.to_owned(),
            anchor,
            rel: [0.0; 3],
            scale: [1.0; 3],
            facing_left,
            skin: None,
            key: None,
            loading: None,
            lerp: None,
            ready: None,
            fighter: None,
            clearing: false,
            montage: None,
            pos: 0.0,
            section: 0,
            active: Vec::new(),
            fx_due: false,
            beam: None,
            swirl: None,
            beam_left: None,
            swirl_yaw: 0.0,
            rig: None,
            hover: 0.0,
            shadow: !shadow_place.hidden,
            shadow_visible: false,
            shadow_place,
            shadow_scalars: IndexMap::new(),
            fit_k: 1.0,
            fit_lift: 0.0,
        }
    }
}

fn no_slot(slot: usize) -> Error {
    Error::Format { what: "pre-match previews".into(), why: format!("no slot {slot}") }
}

/// The lighting channel of a slot: Player1 / Player2 1, Player3 / Player4 2.
fn channel_of(slot: usize) -> u8 {
    if slot < 2 { 1 } else { 2 }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A skin's preview entry: its own, else the default skin's.
pub(super) fn preview_asset(store: &mvs_store::Store, key: &str, skin: &str) -> Result<PreviewEntry> {
    match format::load_preview(store, key, skin) {
        Ok(a) => Ok(a),
        Err(_) if !skin.is_empty() => format::load_preview(store, key, ""),
        Err(e) => Err(e),
    }
}

/// The effects' attachment to the actor's root at a world yaw (radians), scale 1.
pub(super) fn attachment(loc: [f64; 3], yaw: f64) -> Attachment {
    let r = mvs_vfx::math::rotation_rows(0.0, yaw, 0.0);
    Attachment { loc, rot: [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]], scale: Some([1.0; 3]) }
}

/// xorshift64*.
fn next_random(x: u64) -> u64 {
    let mut x = if x == 0 { 0x9e37_79b9_7f4a_7c15 } else { x };
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

/// The mesh component's LocalToWorld, UE row-vector form: the mesh's scale in its own axes, its yaw of -90, facing
/// left the reflection across the actor's Y the view's root takes (CharacterView.place), the actor's place.
pub(super) fn mesh_world(loc: [f64; 3], scale: [f64; 3], facing_left: bool) -> M4 {
    let my = if facing_left { -1.0 } else { 1.0 };
    [
        [0.0, -scale[0] * my, 0.0, 0.0],
        [scale[1], 0.0, 0.0, 0.0],
        [0.0, 0.0, scale[2], 0.0],
        [loc[0], loc[1], loc[2], 1.0],
    ]
}

/// A rig light at the actor's place `w`: Y x -1 when flipped and under the Lights
/// component.
fn rig_light(l: &SpotLight, w: [f64; 3], flip: bool) -> SpotLight {
    let my = if flip && l.mirror == Some(true) { -1.0 } else { 1.0 };
    SpotLight {
        name: l.name.clone(),
        pos: [w[0] + l.pos[0], w[1] + l.pos[1] * my, w[2] + l.pos[2]],
        dir: [l.dir[0], l.dir[1] * my, l.dir[2]],
        color: l.color,
        src: l.src,
        att: l.att,
        cast_shadows: l.cast_shadows,
        mirror: l.mirror,
    }
}

/// MM_PointLightHover of the actor at `w`: a point light has no cone: cos(outer) -2
/// and a difference of 1 leave the cone term at 1 in every direction.
fn hover_light(w: [f64; 3], cd: f64) -> SpotLight {
    let h = &HOVER_LIGHT;
    SpotLight {
        name: h.name.into(),
        pos: [0, 1, 2].map(|k| w[k] + h.parent[k] + h.loc[k]),
        dir: [0.0, 0.0, 1.0],
        color: [cd * 100.0 * 100.0; 3],
        src: [0.0, 0.0, 1.0],
        att: [1.0 / h.radius, -2.0, 1.0],
        cast_shadows: false,
        mirror: None,
    }
}

/// The section of a menu preview montage at a position, by the exe's rule
/// (GetAnimCompositeSectionIndexFromPos 142854b80 takes the first section in array order with start <= pos < the
/// next section's start, the last one up to the montage's length). Array order need not be position order: Jake's
/// Cake victory (S01_Emote_Victory_Montage) lists Loop at 0 before Default at 0, so Loop is empty and a link back to
/// 0 lands in Default, which loops.
pub fn section_at(m: &SceneMontage, pos: f64) -> usize {
    let s = &m.sections;
    for i in 0..s.len() {
        let end = if i + 1 < s.len() { s[i + 1].start } else { m.length };
        if pos >= s[i].start && (pos < end || (i + 1 == s.len() && pos <= end)) {
            return i;
        }
    }
    0
}

/// Montage_Advance over `dt` from `pos` in `section`: the new position and section
/// and the steps (from, to) the notifies are listed over.
pub(super) fn advance(m: &SceneMontage, mut pos: f64, mut section: usize, dt: f64) -> (f64, usize, Vec<[f64; 2]>) {
    let mut steps = Vec::new();
    let mut from = pos;
    pos += dt;
    if !m.sections.is_empty() {
        for _ in 0..8 {
            let end = if section + 1 < m.sections.len() { m.sections[section + 1].start } else { m.length };
            if pos < end {
                break;
            }
            let link = &m.sections[section].next;
            let Some(next) = m.sections.iter().position(|x| x.name.eq_ignore_ascii_case(link)) else {
                pos = end;
                break;
            };
            steps.push([from, end]);
            from = m.sections[next].start;
            pos = m.sections[next].start + (pos - end);
            section = section_at(m, pos);
        }
    }
    steps.push([from, pos]);
    (pos, section, steps)
}

/// The last segment starting at or before `pos` (the first when none does).
pub(super) fn segment_at(m: &SceneMontage, pos: f64) -> Option<&MontageSegment> {
    let mut seg = m.segments.first()?;
    for g in &m.segments {
        if pos >= g.start {
            seg = g;
        }
    }
    Some(seg)
}

/// The notify states listed for the steps (queued notifies, 1428719e0: listed for a step when trigger <= current
/// and end > previous, the end rounded to float).
pub(super) fn listed(list: &[MontageNotify], steps: &[[f64; 2]]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, n) in list.iter().enumerate() {
        if !n.state {
            continue;
        }
        let end = f64::from((n.t + n.duration) as f32);
        if steps.iter().any(|[prev, cur]| n.t <= *cur && end > *prev) {
            out.push(i);
        }
    }
    out
}

/// The active notifies no longer listed, taken out of `active` (in the order they began).
pub(super) fn take_ended(active: &mut Vec<usize>, now: &[usize]) -> Vec<usize> {
    let ended: Vec<usize> = active.iter().copied().filter(|i| !now.contains(i)).collect();
    active.retain(|i| now.contains(i));
    ended
}

/// AnimNotify_SetSubmeshVisibility by its SubmeshTag (Comp_FighterVisibilityManager;
/// the preview is no BPI_Base_Fighter, so never the MirrorSubmeshTag), reversed at its end with
/// ReverseAtNotifyEnd; AnimNotify_Equip ([`equip`]). AnimNotify_PlayVFX's begins, and its ends with KillOnNotifyEnd,
/// go to `fx` (notify index, begin) for the slot's effects.
/// Comp_Pawn_SkinManager GetColorIndex on a preview actor: no PlayerData has it as its fighter (GetByFighter fails,
/// @1525 -> @3068), so 0 (and not a local online player).
const PREVIEW_COLOR_INDEX: u8 = 0;

fn notifies(
    active: &mut Vec<usize>,
    view: &mut PreviewView,
    list: &[MontageNotify],
    steps: &[[f64; 2]],
    fx: &mut Vec<(usize, bool)>,
) {
    let now = listed(list, steps);
    let prop = |n: &MontageNotify, k: &str| n.props.get(k).is_some_and(truthy);
    for i in take_ended(active, &now) {
        let n = &list[i];
        match n.class.as_deref().unwrap_or("None") {
            "AnimNotify_SetSubmeshVisibility" if prop(n, "ReverseAtNotifyEnd") => {
                set_tag_visible(view, tag_of(n, "SubmeshTag"), !prop(n, "Visible"));
            }
            "AnimNotify_Equip" => equip(view, n, false),
            // Received_NotifyEnd @5: KillOnNotifyEnd -> the preview's RemoveVFX(self) @462
            "AnimNotify_PlayVFX" if prop(n, "KillOnNotifyEnd") => fx.push((i, false)),
            _ => {}
        }
    }
    for i in now {
        if active.contains(&i) {
            continue;
        }
        active.push(i);
        let n = &list[i];
        let class = n.class.as_deref().unwrap_or("None");
        match class {
            "AnimNotify_SetSubmeshVisibility" => {
                if prop(n, "ReverseWhenAnimBlendsOut") {
                    warn_once("pre-match preview: SetSubmeshVisibility ReverseWhenAnimBlendsOut not ported".into());
                }
                set_tag_visible(view, tag_of(n, "SubmeshTag"), prop(n, "Visible"));
            }
            "AnimNotify_Equip" => equip(view, n, true),
            "AnimNotify_PlayVFX" => fx.push((i, true)),
            _ if NOTIFY_SKIPPED.contains(&class) => {}
            _ => warn_once(format!("pre-match preview: notify {class} not ported")),
        }
    }
}

/// A notify's name property ("None" unset).
fn tag_of<'a>(n: &'a MontageNotify, k: &str) -> &'a str {
    n.props.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("None")
}

/// AnimNotify_Equip on a preview (no Comp_Spawner, no BP_FighterCharacter, no game state of a match: the equip list,
/// ItemEquipped and the tutorial events are skipped). Begin (Received_NotifyBegin @5-369): when
/// RandomBoolWithWeight(RandomWeight), SetSubMeshVisibility(GetTagToUse, EquipAtStart); End (@169-337, @872-985):
/// UnequipAtEnd hides the tag, else EquipAtEnd shows it. Batman's PreviewIdle swaps his capes so.
// Deviation: the mesh manager's PlayAnimation(Animation) / Stop() on a component mesh of the tag (@751-1043, @593-871)
// and BPI_HoldingItem for RockMesh (@546-705) are not run: no PreviewIdle of the build gives an Equip an animation or
// the RockMesh tag (warned when one does).
// Unverified: RandomWeight below 1 and RandomMeshTags (GetTagToUse) are taken as 1 and MeshTag (warned when set); no
// PreviewIdle of the build sets them.
fn equip(view: &mut PreviewView, n: &MontageNotify, begin: bool) {
    let p = &n.props;
    let prop = |k: &str| p.get(k).is_some_and(truthy);
    let tag = tag_of(n, "MeshTag");
    if begin {
        let weight = p.get("RandomWeight").and_then(serde_json::Value::as_f64).unwrap_or(1.0);
        let random = p.get("RandomMeshTags").and_then(serde_json::Value::as_array).is_some_and(|a| !a.is_empty());
        if weight != 1.0 || random {
            warn_once(format!("pre-match preview: AnimNotify_Equip {tag}: random weight / tags not handled"));
        }
        let on = prop("EquipAtStart");
        if on && tag_of(n, "Animation") != "None" {
            warn_once(format!("pre-match preview: AnimNotify_Equip {tag}: its animation not played"));
        }
        set_tag_visible(view, tag, on);
    } else if prop("UnequipAtEnd") {
        set_tag_visible(view, tag, false);
    } else {
        if prop("EquipAtEnd") {
            set_tag_visible(view, tag, true);
        }
        if tag_of(n, "EndAnimation") != "None" {
            warn_once(format!("pre-match preview: AnimNotify_Equip {tag}: its end animation not played"));
        }
    }
    if tag.eq_ignore_ascii_case("RockMesh") {
        warn_once("pre-match preview: AnimNotify_Equip RockMesh: BPI_HoldingItem not run".into());
    }
}

/// A notify's properties as the effects' readers take them.
fn json_props(v: &serde_json::Value) -> mvs_data::value::Props {
    fn value(v: &serde_json::Value) -> mvs_data::value::Value {
        use mvs_data::value::Value;
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => Value::Num(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => Value::Str(s.clone()),
            serde_json::Value::Array(a) => Value::Arr(a.iter().map(value).collect()),
            serde_json::Value::Object(o) => Value::Obj(o.iter().map(|(k, x)| (k.clone(), value(x))).collect()),
        }
    }
    match value(v) {
        mvs_data::value::Value::Obj(o) => o,
        _ => mvs_data::value::Props::new(),
    }
}

/// JavaScript's truthiness of a property (`!!n.props.Visible`).
pub(super) fn truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        serde_json::Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// A preview's view made on this thread (`PreviewSet::load`'s job), with the
/// warm-up of what its first pose draws when `warm` is given: the commands it recorded are the frame thread's to run
/// (`Gpu::run_recorded`) before the view draws.
pub fn make_view(
    store: &mvs_store::Store,
    key: &str,
    skin: &str,
    warm: Option<WarmJob>,
) -> std::result::Result<Made, String> {
    let made = PreviewView::new(store, key, skin);
    let mut view = made.map_err(|e| e.to_string())?;
    let recorded = warm.and_then(|job| {
        let (meshes, textures) = view_warm(&mut view);
        job.run_meshes(&meshes, &textures).inspect_err(|e| eprintln!("pre-match preview: {key} warm-up: {e}")).ok()
    });
    Ok((view, recorded))
}

/// What a view's first pose draws: its meshes with their material sets (as `Renderer::prewarm` takes them) and the
/// textures its materials override.
fn view_warm(view: &mut PreviewView) -> (Vec<mvs_view::WarmMesh>, Vec<Arc<str>>) {
    let identity: M4 = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 }));
    let pose = view.pose(&identity);
    let mut textures = Vec::new();
    let meshes = pose
        .meshes
        .iter()
        .map(|m| {
            textures.extend(m.materials.iter().flat_map(|r| r.textures.iter().map(|(_, t)| t.clone())));
            mvs_view::WarmMesh {
                mesh: Some(m.mesh.clone()),
                materials: m.materials.iter().map(|r| Some(r.path.clone())).collect(),
                outlined: m.custom_stencil != 0,
                captured: false,
                every_slot: false,
            }
        })
        .collect();
    (meshes, textures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use format::MontageSection;

    fn montage(sections: &[(&str, &str, f64)], length: f64) -> SceneMontage {
        SceneMontage {
            slot: None,
            blend_in: None,
            blend_out: None,
            notifies: None,
            name: "M".into(),
            segments: vec![MontageSegment { clip: "A".into(), start: 0.0, from: 0.0, to: length, rate: 1.0 }],
            sections: sections
                .iter()
                .map(|(n, x, s)| MontageSection { name: (*n).into(), next: (*x).into(), start: *s })
                .collect(),
            length,
        }
    }

    /// sectionAt's array-order rule: a section listed first at the same start is empty.
    #[test]
    fn section_at_array_order() {
        let m = montage(&[("Loop", "Default", 0.0), ("Default", "Default", 0.0)], 2.0);
        assert_eq!(section_at(&m, 0.0), 1);
        assert_eq!(section_at(&m, 2.0), 1);
        let m = montage(&[("A", "B", 0.0), ("B", "B", 1.0)], 2.0);
        assert_eq!(section_at(&m, 0.5), 0);
        assert_eq!(section_at(&m, 1.0), 1);
        assert_eq!(section_at(&m, 2.0), 1);
        assert_eq!(section_at(&m, 3.0), 0);
    }

    /// Montage_Advance: a looping section (its link in another case) wraps to its start, a section with no next
    /// holds at its end.
    #[test]
    fn advance_loops_and_holds() {
        let m = montage(&[("Intro", "Loop", 0.0), ("Loop", "loop", 1.0)], 2.0);
        let (pos, sec, steps) = advance(&m, 0.9, 0, 0.2);
        assert!((pos - 1.1).abs() < 1e-12 && sec == 1, "{pos}");
        assert_eq!(steps.len(), 2);
        let (pos, sec, steps) = advance(&m, 1.95, 1, 0.1);
        assert!((pos - 1.05).abs() < 1e-12 && sec == 1, "{pos}");
        assert_eq!(steps, vec![[1.95, 2.0], [1.0, pos]]);
        let m = montage(&[("Only", "", 0.0)], 1.0);
        let (pos, _, steps) = advance(&m, 0.95, 0, 0.1);
        assert_eq!(pos, 1.0);
        assert_eq!(steps, vec![[0.95, 1.0]]);
    }

    fn layout() -> PreviewSet {
        let previews: Vec<PreMatchPreview> = (1..=4)
            .map(|i| PreMatchPreview {
                tag: format!("Player{i}"),
                actor: format!("BP_CharacterPreview{i}"),
                anchor: format!("Start_Char_{i}"),
                facing_left: i > 2,
                tags: Vec::new(),
                shadow: ShadowPlace {
                    loc: [-20.0, 0.0, 0.0],
                    rot: [-15.0, 0.0, 0.0],
                    scale: [3.0, 4.0, 1.0],
                    hidden: true,
                },
            })
            .collect();
        let anchors =
            (1..=4).map(|i| (format!("Start_Char_{i}"), Point { loc: [i as f64, 10.0, 0.0], rot: [0.0; 3] })).collect();
        PreviewSet::with_layout(&previews, &anchors, None).unwrap()
    }

    fn entry() -> PreviewEntry {
        let p = |loc: [f64; 3], s: f64| Placement { loc, scale: [s; 3] };
        PreviewEntry {
            skin: "S".into(),
            placements: [
                ("CharSelect_You".to_string(), p([0.0, 20.0, 0.0], 1.0)),
                ("FaceOff_You".to_string(), p([0.0, 40.0, 10.0], 2.0)),
                ("CharSelect_Friend".to_string(), p([0.0, 30.0, 0.0], 1.0)),
                ("FaceOff_Friend".to_string(), p([0.0, 50.0, 0.0], 1.0)),
            ]
            .into_iter()
            .collect(),
            preview_idle: None,
            homescreen_idle: None,
            rig: "BP_CharLights_Test".into(),
            mirror_when: MirrorWhen::Left,
            lights: Vec::new(),
        }
    }

    /// The placement by state and suffix (Matchmaking as CharSelect), Y negated facing left; LerpSizing between two
    /// states; a LerpSizing before the entry is in is kept for it; the FFA style's facing and suffix.
    #[test]
    fn placement_and_lerp() {
        let mut set = layout();
        set.state = PreviewState::Matchmaking;
        for slot in [0, 1] {
            set.slots[slot].skin = Some(String::new());
            set.slots[slot].facing_left = set.facing_left_of(slot);
            set.assets[slot] = Some(entry());
            set.place(slot);
        }
        assert_eq!(set.location(0), [1.0, 30.0, 0.0]);
        // (Player2 faces left with no team style; its suffix is _Friend)
        assert!(set.slots[1].facing_left);
        assert_eq!(set.location(1), [2.0, 10.0 - 30.0, 0.0]);
        set.lerp_sizing(0, PreviewState::CharSelect, PreviewState::FaceOff, 0.5);
        assert_eq!(set.location(0), [1.0, 40.0, 5.0]);
        assert_eq!(set.slots[0].scale, [1.5; 3]);
        set.slots[2].skin = Some(String::new());
        set.lerp_sizing(2, PreviewState::CharSelect, PreviewState::FaceOff, 1.0);
        assert_eq!(set.slots[2].lerp, Some((PreviewState::CharSelect, PreviewState::FaceOff, 1.0)));
        set.set_team_style(Some(2));
        assert!(!set.facing_left_of(1));
        assert_eq!(set.suffix(1), "_You");
    }

    /// The hover light by match state, on channel 0; the rig's mirrored lights flipped, on the slot's channel.
    #[test]
    fn lights() {
        let mut set = layout();
        set.state = PreviewState::CharSelect;
        set.hover(2, true);
        assert_eq!(set.slots[2].hover, 15.0);
        set.state = PreviewState::MainMenu;
        set.hover(2, true);
        assert_eq!(set.slots[2].hover, 3.0);
        let l = SpotLight {
            name: "L".into(),
            pos: [1.0, 2.0, 3.0],
            dir: [0.0, 1.0, 0.0],
            color: [10.0; 3],
            src: [0.0; 3],
            att: [0.0; 3],
            cast_shadows: false,
            mirror: Some(true),
        };
        set.slots[2].rig = Some(Rig { lights: vec![l], flip: true });
        let mut out = MenuFrame::default();
        set.frame(&mut out);
        assert_eq!(out.spots.len(), 2);
        assert_eq!(out.spots[0].0, 2);
        assert_eq!(out.spots[0].1.pos, [4.0, 8.0, 3.0]);
        assert_eq!(out.spots[0].1.dir, [0.0, -1.0, 0.0]);
        assert_eq!(out.spots[0].1.color, [10.0; 3]);
        assert_eq!(out.spots[1].0, 0);
        assert_eq!(out.spots[1].1.pos, [143.0, 117.0, 320.0]);
        set.hover(2, false);
        let mut out = MenuFrame::default();
        set.frame(&mut out);
        assert_eq!(out.spots.len(), 1);
    }

    /// A notify state is listed over the steps it spans; one no longer listed is taken out.
    #[test]
    fn notify_listing() {
        let n = |t: f64, d: f64| MontageNotify {
            name: None,
            class: Some("AnimNotify_SetSubmeshVisibility".into()),
            state: true,
            t,
            duration: d,
            offset: 0.0,
            end_offset: 0.0,
            props: serde_json::json!({ "SubmeshTag": "Hat", "Visible": true, "ReverseAtNotifyEnd": true }),
        };
        let list = [n(0.0, 0.5), n(1.0, 0.25)];
        assert_eq!(listed(&list, &[[0.0, 0.1]]), vec![0]);
        assert_eq!(listed(&list, &[[0.5, 0.6]]), Vec::<usize>::new());
        assert_eq!(listed(&list, &[[0.9, 1.0]]), vec![1]);
        let mut active = vec![0, 1];
        assert_eq!(take_ended(&mut active, &[1]), vec![0]);
        assert_eq!(active, vec![1]);
    }

    /// The main menu fit: a point keeps FIT_TOP of the half-height below the frame's top edge; a point below the
    /// camera's line sets no limit.
    #[test]
    fn fit_frame_limit() {
        // (looking along +X, level, a 90 degree square frame: tan of the half-angle 1)
        let f = FitFrame::of_camera([0.0; 3], [0.0; 3], 90.0, 1.0);
        let s = f.most_scale([100.0, 0.0, 0.0], [0.0, 0.0, 100.0]).unwrap();
        assert!((s - FIT_TOP).abs() < 1e-9, "{s}");
        assert_eq!(f.most_scale([100.0, 0.0, 0.0], [0.0, 0.0, -100.0]), None);
        // (a point toward the camera sits higher in the frame: a smaller scale)
        let near = f.most_scale([100.0, 0.0, 0.0], [-20.0, 0.0, 100.0]).unwrap();
        assert!(near < s, "{near}");
    }

    /// The main menu lift: a point's height in the frame, and the Z lift that puts it at a share of the half-height,
    /// on a level and on a pitched camera.
    #[test]
    fn fit_frame_lift() {
        for pitch in [0.0, -12.0] {
            let f = FitFrame::of_camera([0.0; 3], [pitch, 0.0, 0.0], 60.0, 16.0 / 9.0);
            let d = [500.0, 30.0, -200.0];
            let low = f.height(d).unwrap();
            assert!(low < FIT_LOW, "{low}");
            let dz = f.lift_to(d, FIT_LIFT_TO).unwrap();
            assert!(dz > 0.0, "{dz}");
            let h = f.height([d[0], d[1], d[2] + dz]).unwrap();
            assert!((h - FIT_LIFT_TO).abs() < 1e-9, "{h}");
        }
        let f = FitFrame::of_camera([0.0; 3], [0.0; 3], 90.0, 1.0);
        assert_eq!(f.height([-10.0, 0.0, 0.0]), None);
    }

    /// A too-low fighter taller than the span is shrunk about its feet to the span, then lifted to the share; a
    /// short one is only lifted.
    #[test]
    fn fit_frame_shrink() {
        let f = FitFrame::of_camera([0.0; 3], [0.0; 3], 90.0, 1.0);
        let (feet, top) = ([100.0, 0.0, -250.0], [100.0, 0.0, -10.0]);
        let (k, dz) = f.shrink_and_lift(feet, top, 1.8, 0.6).unwrap();
        assert!((k - 0.75).abs() < 1e-9, "{k}");
        let at = |p: [f64; 3]| f.height([p[0], p[1], feet[2] + k * (p[2] - feet[2]) + dz]).unwrap();
        assert!((at(top) - 0.6).abs() < 1e-9);
        assert!((at(top) - at(feet) - 1.8).abs() < 1e-9);
        let (k, dz) = f.shrink_and_lift([100.0, 0.0, -100.0], top, 1.8, 0.6).unwrap();
        assert_eq!(k, 1.0);
        assert!((dz - 70.0).abs() < 1e-9, "{dz}");
    }

    /// The world matrix: facing right the mesh's X is the world's -Y (yaw -90); facing left Y is reflected.
    #[test]
    fn world_matrix() {
        let w = mesh_world([1.0, 2.0, 3.0], [2.0, 3.0, 4.0], false);
        assert_eq!(w[0], [0.0, -2.0, 0.0, 0.0]);
        assert_eq!(w[1], [3.0, 0.0, 0.0, 0.0]);
        let w = mesh_world([1.0, 2.0, 3.0], [1.0; 3], true);
        assert_eq!(w[0], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(w[3], [1.0, 2.0, 3.0, 1.0]);
    }

    fn dev_store() -> Option<mvs_store::Store> {
        let s = mvs_store::Store::open(mvs_store::dev_store_dir()).ok();
        if s.is_none() {
            eprintln!("no developer store: skipped");
        }
        s
    }

    fn spec(key: &str) -> PreviewSpec {
        PreviewSpec { key: key.into(), skin: String::new(), team: 0 }
    }

    /// The store-less calls before the layout is open do nothing (location: the origin).
    #[test]
    fn calls_before_the_layout_do_not_panic() {
        let mut set = PreviewSet::default();
        assert_eq!(set.location(0), [0.0; 3]);
        set.set_location(0, [1.0; 3]);
        set.set_anchor(1, [1.0; 3]);
        set.lerp_sizing(2, PreviewState::CharSelect, PreviewState::FaceOff, 0.5);
        set.hover(3, true);
        set.clear_fighter(0);
        assert!(!set.has_fighter(0));
    }

    /// A roster entry with no fighter plays its effect at the first call only (two such entries compare equal).
    #[test]
    fn an_entry_with_no_fighter_plays_its_effect_once() {
        let Some(store) = dev_store() else { return };
        let mut set = layout();
        set.change_fighter(&store, 1, &spec(""), false).unwrap();
        assert!(set.slots[1].fx_due);
        set.slots[1].fx_due = false;
        set.change_fighter(&store, 1, &spec(""), false).unwrap();
        assert!(!set.slots[1].fx_due);
        set.change_fighter(&store, 1, &spec(""), true).unwrap();
        assert!(set.slots[1].fx_due);
    }

    /// The meshes a view's pose draws.
    fn drawn(view: &mut PreviewView) -> Vec<String> {
        let world = mesh_world([0.0; 3], [1.0; 3], false);
        let mut v: Vec<String> = view.pose(&world).meshes.iter().map(|m| m.mesh.to_string()).collect();
        v.sort();
        v
    }

    /// Batman's PreviewIdle: AnimNotify_Equip shows CapeFullA and hides the other capes at its start, then swaps
    /// CapeFullA (UnequipAtEnd) for CapeDetailedA (equipped, kept) at 0.53 s.
    #[test]
    fn batman_preview_idle_swaps_capes() {
        let Some(store) = dev_store() else { return };
        let entry = preview_asset(&store, "batman", "").unwrap();
        let m = entry.preview_idle.expect("Batman's PreviewIdle");
        let list = m.notifies.as_deref().unwrap_or(&[]);
        assert!(list.iter().filter(|n| n.class.as_deref() == Some("AnimNotify_Equip")).count() >= 6);
        let mut view = PreviewView::new(&store, "batman", "").unwrap();
        let mut active = Vec::new();
        let mut fx = Vec::new();
        notifies(&mut active, &mut view, list, &[[0.0, 0.3]], &mut fx);
        let full = drawn(&mut view);
        notifies(&mut active, &mut view, list, &[[0.3, 0.55]], &mut fx);
        notifies(&mut active, &mut view, list, &[[0.55, 0.8]], &mut fx);
        let detailed = drawn(&mut view);
        assert!(fx.is_empty());
        assert_eq!(full.len(), detailed.len(), "one cape for another: {full:?} {detailed:?}");
        assert_ne!(full, detailed);
        notifies(&mut active, &mut view, list, &[[0.8, 1.2]], &mut fx);
        assert_eq!(drawn(&mut view), detailed, "CapeDetailedA stays at its end");
    }

    /// Black Adam's PreviewIdle: AnimNotify_PlayVFX plays its five systems on their sockets, facing either way, and
    /// they follow the pose.
    #[test]
    fn black_adam_preview_idle_plays_its_effects() {
        let Some(store) = dev_store() else { return };
        let entry = preview_asset(&store, "blackadam", "").unwrap();
        let m = entry.preview_idle.expect("Black Adam's PreviewIdle");
        let list = m.notifies.as_deref().unwrap_or(&[]);
        let mut view = PreviewView::new(&store, "blackadam", "").unwrap();
        let mut player = mvs_vfx::Player::new(&store, 7).unwrap();
        let mut active = Vec::new();
        let mut fx = Vec::new();
        notifies(&mut active, &mut view, list, &[[0.0, m.length]], &mut fx);
        let begun: Vec<usize> = fx.iter().filter(|(_, b)| *b).map(|(i, _)| *i).collect();
        assert_eq!(begun.len(), 5, "{fx:?}");
        let world = mesh_world([0.0; 3], [1.0; 3], false);
        for left in [false, true] {
            let mut live = NotifyVfx::new(&view);
            for &i in &begun {
                live.begin(&mut view, &world, i, &json_props(&list[i].props), left, PREVIEW_COLOR_INDEX, &mut player);
            }
            assert_eq!(live.len(), 5, "facing left {left}: {live:?}");
            live.follow(&mut view, &world, left, &mut player);
            for id in live.drain() {
                player.kill(id);
            }
        }
    }

    /// A view with no entry waits for one (show keeps it); a dispose puts the slots back to the level's actors, and
    /// a change of fighter after it shows the fighter again.
    #[test]
    fn dispose_puts_the_slots_back() {
        let Some(store) = dev_store() else { return };
        let mut set = layout();
        let mut events = Vec::new();
        let view = || PreviewView::new(&store, "shaggy", "").unwrap();
        set.slots[0].ready = Some({
            let view = view();
            Shown { fx: NotifyVfx::new(&view), view, shell: None }
        });
        set.tick_slot(0, None, &mut events, 1.0 / 60.0);
        assert!(set.slots[0].ready.is_some() && !set.has_fighter(0));
        set.slots[0].ready = None;

        set.state = PreviewState::CharSelect;
        set.set_fighter(&store, 0, view(), entry(), "").unwrap();
        set.set_anchor(0, [9.0; 3]);
        set.tick_slot(0, None, &mut events, 1.0 / 60.0);
        assert!(set.has_fighter(0));
        set.dispose(None);
        assert_eq!(set.state, PreviewState::MainMenu);
        assert!(!set.has_fighter(0) && set.slots[0].skin.is_none() && set.assets[0].is_none());
        assert_eq!(set.location(0), [1.0, 10.0, 0.0]);
        assert!(set.slots[2].facing_left);

        // (the change is taken: the slot's skin was put back to none)
        set.change_fighter(&store, 0, &spec("shaggy"), false).unwrap();
        assert!(set.slots[0].fx_due && set.slots[0].key.as_deref() == Some("shaggy"));
        // (a store of the old menuscene format has no preview entry the reader takes)
        if format::load_preview(&store, "shaggy", "").is_err() {
            eprintln!("no version-2 preview entry for shaggy: the change after the dispose skipped");
            return;
        }
        // (the view is made on the loader: the slot shows it at the first tick after it is in, and says so)
        for _ in 0..500 {
            set.tick_slot(0, None, &mut events, 1.0 / 60.0);
            if set.has_fighter(0) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(set.has_fighter(0));
        assert!(events.iter().any(|e| matches!(e, MenuEvent::PreviewSettled { slot: 0, key } if key == "shaggy")));
    }

    /// The party partner's nameplate on the main menu (`char_select_follow` of PartnerNameplate_PM's Nameplate place,
    /// mvs-ui main_menu.rs PARTNER_NAMEPLATE_AT): with the build's partner, HarleyWithJacket on Player3 (slot 2), at
    /// 1920 x 1080 under the main menu's camera, the plate sits on the fighter as it does on the fighter select (its
    /// centre over the fighter's middle, near its top), where the build's own place is far to its right.
    #[test]
    fn partner_plate_follows_the_partner_on_the_main_menu() {
        let Some(store) = dev_store() else { return };
        let Ok(e) = format::load_level(&store, mvs_material::menuscene::LevelName::MainMenu) else { return };
        let SceneData::MainMenu(mm) = e.data else { panic!("the MainMenu entry") };
        let c = &mm.camera;
        let mut set = PreviewSet::default();
        set.open(&store).unwrap();
        set.set_fit_frame(Some(FitFrame::of_camera(c.loc, c.rot, c.fov, c.aspect)));
        set.set_main_menu_up(true);
        set.set_wait_loads(true);
        let harley = PreviewSpec { key: "harley".into(), skin: "HarleyWithJacket".into(), team: 0 };
        set.change_fighter(&store, 2, &harley, false).unwrap();
        let mut events = Vec::new();
        set.tick_slot(2, None, &mut events, 1.0 / 60.0);
        assert!(set.has_fighter(2));

        // (Player3 at Start_Char_2 (-350, 265, -15): MainMenu_Friend (0, -15, 30) x 0.9 here, CharSelect_Friend
        // (0, -10.44, 35.7) x 0.925 from Start_Char_2 less 70 along Y there)
        let build = [290.0, 260.0, 216.5];
        let plate = set.char_select_follow(2, build).unwrap();
        let k = 0.9 / 0.925;
        let feet_then = [-350.0, 265.0 - 70.0 - 10.44, -15.0 + 35.7];
        let want = [0, 1, 2].map(|i| set.location(2)[i] + (build[i] - feet_then[i]) * k);
        assert!((0..3).all(|i| (plate[i] - want[i]).abs() < 1e-9), "{plate:?} {want:?}");

        // the fighter's posed meshes' box on the screen
        let size = [1920, 1080];
        let cam = super::super::main_menu::menu_camera(c.loc, c.rot, c.fov, c.aspect, size);
        let px = |a: [f64; 3]| super::super::main_menu::project_pixels(&cam, a, size).unwrap();
        let world = set.mesh_world(2);
        let pose = set.slots[2].fighter.as_mut().unwrap().view.pose(&world);
        let row = |a: &[f32; 12], r: usize, v: [f64; 3]| {
            (0..3).map(|k| f64::from(a[r * 4 + k]) * v[k]).sum::<f64>() + f64::from(a[r * 4 + 3])
        };
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for m in pose.meshes.iter().filter(|m| !m.bones.is_empty() && m.shown.iter().any(|&s| s)) {
            let boxes = mvs_view::mesh::bone_boxes(&store, &m.mesh).unwrap();
            for (b, bx) in m.bones.iter().zip(boxes.iter()) {
                let Some([l, h]) = bx else { continue };
                for corner in 0..8 {
                    let v = [0, 1, 2].map(|k| f64::from(if corner & (1 << k) == 0 { l[k] } else { h[k] }));
                    let posed = [0, 1, 2].map(|r| row(b, r, v));
                    let p = px([0, 1, 2].map(|r| row(&m.to_world, r, posed)));
                    (0..2).for_each(|k| (lo[k], hi[k]) = (lo[k].min(p[k]), hi[k].max(p[k])));
                }
            }
        }
        let (p, old) = (px(plate), px(build));
        let (mid, w, h) = ((lo[0] + hi[0]) / 2.0, hi[0] - lo[0], hi[1] - lo[1]);
        eprintln!(
            "fighter box x {:.0}..{:.0} y {:.0}..{:.0}; plate {p:?} (world {plate:?}); build's {old:?}",
            lo[0], hi[0], lo[1], hi[1]
        );
        // (the bones' boxes are loose, well past the skin: the box's middle is the check across, the feet and the
        // plate's world height (the char select's 195.8 above the feet, x 0.9 / 0.925) the one up)
        let feet = px(set.location(2));
        assert!((p[0] - mid).abs() < 0.1 * w, "the plate over the fighter's middle: {p:?}, box {lo:?} {hi:?}");
        assert!(p[1] > lo[1] && p[1] < feet[1] - 0.25 * h, "the plate above the fighter's feet {feet:?}: {p:?}");
        assert!((plate[2] - set.location(2)[2] - 195.8 * k).abs() < 1e-9);
        assert!(old[0] > hi[0], "the build's place is right of the fighter: {old:?}, box {lo:?} {hi:?}");
    }
}
