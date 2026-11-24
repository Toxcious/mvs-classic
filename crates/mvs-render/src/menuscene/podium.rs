//! The podium after a match: Map_PodiumStage_New's camera, planes and lights, the
//! winners' and losers' fighters on their preview places, the winner intro sequence and its director calls, the
//! banners with their nametags, toast buttons and stat sheets, and WBP_3dTeam's text.
//!
//! The planes, banner pieces and glow shells draw with the build's cooked materials (the menuscene kind's material
//! references).

use std::sync::Arc;

use mvs_vfx::SystemId;
use mvs_view::{Camera, FighterPose, M4, MeshPose, PreviewAt, PreviewView};

use super::banner::{
    BannerElement, BannerInit, BannerParts, BannerView, PIECE_BACKGROUND, PIECE_FILTER, PIECE_TRIM, PLANE,
};
use super::format::{
    Channel, Glow, LevelName, MenuSceneEntry, PodiumFighter, PodiumScene, SceneData, SceneEvent, SceneMontage,
    material_ref,
};
use super::main_menu::{GlowShell, menu_camera};
use super::notify_vfx::{NotifyVfx, set_tag_visible};
use super::preview::{attachment, mesh_world, section_at, segment_at};
use super::sequence::{eval_channel, eval_podium_channel, eval_weighted};
use super::{LevelCx, MenuEvent, MenuFrame, PodiumRecord, PodiumStart, ScenePoint, SceneWidget, SeqCmd};
use crate::fx::warn_once;
use crate::gpu::{Gpu, Texture};
use crate::stage::{to_world, transform};
use crate::{Assets, Error, Result};
use mvs_data::fighter::Notify;

/// The planes with the slanted cut on their right edge (place).
const WIDE_PLANES: [&str; 2] = ["UI_Plane_TeamColorBG", "UI_Plane_TrophyBG"];
/// PlayVFX's light beam, its life (a Delay(1.0)), the swirl and the sound.
const BEAM: &str = "PS_CharacterSelectLightBeam";
const SWIRL: &str = "PS_CharacterSelectSwirl04";
const BEAM_SOUND: &str = "ui_characterselect_lightbeam";
const BEAM_LIFE: f64 = 1.0;
/// The banner landing's camera shake class (CameraShakeMedium_C), played through the shared shaker.
// Deviation: the shared CameraShaker (mvs_view::camera) builds a new shake per play, as the engine does.
const BANNER_SHAKE: &str = "CameraShakeMedium_C";
/// The Ak events a montage notify posts on the preview actor.
const SOUND_NOTIFIES: [&str; 2] = ["Ak_OneShot", "Ak_VO_Effort"];
/// The banner landing's sound (the Entrance Timeline's Finished event).
const BANNER_SOUND: &str = "ui_postmatch_banner_show";
/// The mesh's delay behind ChangeFighter before it shows, s.
const SHOW_DELAY: f64 = 0.2;
/// BPI_ShowFighter's first call arms the victory VO after this Delay (level Blueprint @6904).
const VICTORY_VO_DELAY: f64 = 3.0;

/// DeterminePreviewActor (level Blueprint @598): the preview actors per result index, by team style.
fn slots_of(team_style: u8) -> Option<([usize; 2], usize, [usize; 3], usize)> {
    // (winners, winner count, losers, loser count)
    match team_style {
        0 => Some(([2, 0], 1, [3, 0, 0], 1)),
        1 | 3 => Some(([1, 2], 2, [0, 3, 0], 2)),
        2 => Some(([1, 0], 1, [0, 2, 3], 3)),
        _ => None,
    }
}

/// The preview actor of the `index`th winner or loser (DeterminePreviewActor).
fn actor_of(team_style: u8, index: usize, winner: bool) -> Option<usize> {
    let (w, wn, l, ln) = slots_of(team_style)?;
    if winner { w[..wn].get(index).copied() } else { l[..ln].get(index).copied() }
}

/// The team style whose places a result list takes: a 2v2 list of one (the port's Versus lobby plays 2 against 1 as
/// a 2v2 match) takes the 1v1's.
/// Deviation: the build never has a 2v2 podium with a side of one; the port stands its one winner or loser where the
/// 1v1 stands its own (actor and Winner2 / Loser2 offset), with no empty place.
fn side_style(team_style: u8, count: usize) -> u8 {
    if team_style == 1 && count == 1 { 0 } else { team_style }
}

/// The preview actor of the `index`th of `count` winners or losers (`actor_of` by the list's `side_style`).
fn actor_in(team_style: u8, count: usize, index: usize, winner: bool) -> Option<usize> {
    actor_of(side_style(team_style, count), index, winner)
}

/// BeginPlay's world offsets of the target points by game mode (level Blueprint @7502..7991).
fn offset_of(team_style: u8, point: &str) -> [f64; 3] {
    match (team_style, point) {
        (0, "Winner2" | "Loser2") => [0.0, 325.0, 0.0],
        (2, "Winner2") => [-550.0, -40.0, -50.0],
        _ => [0.0; 3],
    }
}

/// BP_CharacterPreview BPI_SetPodiumPlayerData (@22896): the banner's Offset, its Banner Placement location.
const BANNER_PLACEMENT: [f64; 3] = [0.0, 0.0, 230.0];

/// Team style 2, BeginPlay (level Blueprint @7946, @7991): TP_BannerPlayer3's scale and world offset.
fn banner_ffa(tag: &str) -> Option<(f64, [f64; 3])> {
    (tag == "Player3").then_some((1.1, [20.0, 0.0, 10.0]))
}

/// BP_CharacterPreview GetNametagOffset (@286): added to the EOG Nametag's relative Y (banner construction script
/// @1033). The EOG Nametag Scale (1.1 for Player1 / Player4) scales a screen space component, without effect.
fn nametag_offset(tag: &str) -> f64 {
    match tag {
        "Player2" => 5.0,
        "Player3" => 35.0,
        "Player4" => 63.0,
        _ => 0.0,
    }
}

/// BP_CharacterPreview GetLightingChannels on a podium level: Channel1 / Channel2 by tag and team style.
fn channel_of(tag: &str, team_style: u8) -> u8 {
    let one = match team_style {
        0 => tag == "Player3",
        1 | 2 => tag == "Player2" || tag == "Player3",
        _ => false,
    };
    let two = match team_style {
        0 => tag == "Player4",
        1 | 2 => tag == "Player1" || tag == "Player4",
        _ => false,
    };
    if one {
        1
    } else if two {
        2
    } else {
        0
    }
}

/// How far a keyed Y value is along the channel's last key, the one that parks the piece left of the frame (place):
/// 0 up to the key before it (sequence time `t`), 1 at it.
fn leaving(c: Option<&Channel>, t: f64, y: f64) -> f64 {
    let Some(Channel::Keys { keys }) = c else { return 0.0 };
    if keys.len() < 2 {
        return 0.0;
    }
    let (k, a, b) = (&keys[keys.len() - 2], keys[keys.len() - 2].v, keys[keys.len() - 1].v);
    if t > k.t && b > a { ((y - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 }
}

fn format_error(what: &str, why: String) -> Error {
    Error::Format { what: what.to_owned(), why }
}

/// A fighter of the podium: a preview actor with its view.
#[derive(Debug)]
struct Fighter {
    /// The preview actor's index.
    pa: usize,
    view: PreviewView,
    loc: [f64; 3],
    /// The mesh scale.
    scale: f64,
    montage: SceneMontage,
    channel: u8,
    /// The preview's FacingLeft.
    left: bool,
    /// BlobShadowCenter's relative location under the actor's root (ShadowChanges' offset in).
    shadow_loc: [f64; 3],
    /// The montage's notify states that have begun.
    active: Vec<usize>,
    /// Seconds since ChangeFighter.
    age: f64,
    shown: bool,
    pos: f64,
    section: usize,
    beam: Option<SystemId>,
    swirl: Option<SystemId>,
    /// GlowVFX's duplicates and their Dissolve timeline, from the tick the mesh shows.
    glow: Option<GlowShell>,
    /// The montage's notifies (the character's data), the montage meshes on the view and the PlayVFX components by
    /// notify.
    notifies: Vec<Notify>,
    meshes: Vec<MeshOn>,
    fx: NotifyVfx,
    /// GetColorIndex: 1 for a winner, 0 for a loser.
    color_index: u8,
}

/// A montage mesh on a fighter's view: its name, whether its animation loops, and seconds since it began.
#[derive(Debug)]
struct MeshOn {
    mesh: String,
    looped: bool,
    age: f64,
}

/// A banner of a preview actor: its record's BannerData slug ("" none), and once Apply Banner Appearance ran, the
/// element on Base Element Root.
#[derive(Debug)]
struct PodiumBanner {
    pa: usize,
    view: BannerView,
    slug: String,
    /// The element was asked for (a missing banner keeps the default one).
    tried: bool,
    element: Option<BannerElement>,
    /// The element's root (Base Element Root's world location, rotator and scale).
    root: [[f64; 3]; 3],
}

/// What the podium was started with (PodiumScene's constructor arguments).
#[derive(Debug)]
struct Started {
    team_style: u8,
    winners: Vec<PodiumRecord>,
    losers: Vec<PodiumRecord>,
    events: Vec<SceneEvent>,
    /// `PodiumStart::big`.
    big: bool,
}

impl Started {
    /// The preview actor of the `index`th winner or loser (`actor_in` by the list's length; a big match's second
    /// list takes the team style's losers' places as they are).
    fn actor(&self, index: usize, winner: bool) -> Option<usize> {
        if self.big && !winner {
            return actor_of(self.team_style, index, false);
        }
        let count = if winner { self.winners.len() } else { self.losers.len() };
        actor_in(self.team_style, count, index, winner)
    }
}

/// A world space widget component and the picture it shows (None: hidden).
#[derive(Debug)]
struct WidgetView {
    w: SceneWidget,
    picture: Option<Arc<Texture>>,
}

/// Another handle on `t`'s GPU texture and views (each a reference of its COM object).
fn share(t: &Texture) -> Texture {
    Texture {
        res: t.res.clone(),
        srv: t.srv.clone(),
        rtv: t.rtv.clone(),
        dsv: t.dsv.clone(),
        uav: t.uav.clone(),
        desc: t.desc,
    }
}

/// The podium.
#[derive(Debug)]
pub struct PodiumLevel {
    pub data: PodiumScene,
    /// BP_CharacterPreview's glow shell data (the MainMenu entry's).
    glow: Option<Glow>,
    plane: Arc<str>,
    started: Option<Started>,
    /// The planes' material set: the local result (DetermineWinningTeam).
    you_won: bool,
    /// Sequence time; -1 until the first tick (the sequences start one frame after the level's BeginPlay).
    time: f64,
    vo_armed: bool,
    vo_delay: f64,
    next_event: usize,
    next_sound: usize,
    /// The camera's location (UE; the two camera targets share one rotation).
    cam_loc: [f64; 3],
    /// CameraLerp: the timeline position in curve time, -1 while it does not play.
    lerp: f64,
    lerp_from: [f64; 3],
    shaker: mvs_view::camera::CameraShaker,
    fighters: Vec<Fighter>,
    banners: Vec<PodiumBanner>,
    widgets: Vec<WidgetView>,
    /// UI_Cursor's MousePosNormalized, the banners' parallax.
    // Unverified: the cursor is not handed to the level; the centre of the screen stands for it.
    cursor: [f64; 2],
    /// The post chain's fade to black the sequence keys (`Renderer::menu_tick` hands it to the post).
    pub fade: f32,
}

impl PodiumLevel {
    /// The camera on CameraTarget_Start, the
    /// fade black, the glow shell's data. The planes are placed per frame.
    pub fn new(_gpu: &Gpu, assets: &Assets, entry: MenuSceneEntry) -> Result<PodiumLevel> {
        let SceneData::Podium(data) = entry.data else {
            return Err(format_error(&entry.name, "not the Map_PodiumStage_New entry".into()));
        };
        for (name, f) in &data.fighters {
            if f.default.mesh_yaw != -90.0 {
                return Err(format_error(&entry.name, format!("preview mesh yaw {} ({name})", f.default.mesh_yaw)));
            }
        }
        let glow = match super::format::load_level(assets.store(), LevelName::MainMenu) {
            Ok(e) => match e.data {
                SceneData::MainMenu(m) => Some(m.glow),
                _ => None,
            },
            Err(e) => {
                warn_once(format!("podium: no glow shell: {e}"));
                None
            }
        };
        let cam_loc = data.camera.start.loc;
        Ok(PodiumLevel {
            data: *data,
            glow,
            plane: PLANE.into(),
            started: None,
            you_won: false,
            time: -1.0,
            vo_armed: false,
            vo_delay: 0.0,
            next_event: 0,
            next_sound: 0,
            cam_loc,
            lerp: -1.0,
            lerp_from: cam_loc,
            shaker: mvs_view::camera::CameraShaker::default(),
            fighters: Vec::new(),
            banners: Vec::new(),
            widgets: Vec::new(),
            cursor: [0.5, 0.5],
            fade: 1.0,
        })
    }

    /// The constructor's result lists, materials and sequence start (SeqCmd::Podium); the
    /// sequences begin at the next tick.
    // Deviation: the fighters', glow shells' and banners' programs are not warmed under the fade before the sequence
    // starts; the sequence starts at once and the views are made when ShowFighter calls them.
    pub fn command(&mut self, cmd: SeqCmd) -> Result<()> {
        let SeqCmd::Podium(start) = cmd else {
            warn_once("podium: a pre-match command".into());
            return Ok(());
        };
        let PodiumStart { team_style, winners, losers, you_won, big } = *start;
        let events = self
            .data
            .sequence
            .events
            .get(if team_style == 2 { "FFA" } else { "2v2" })
            .cloned()
            .ok_or_else(|| format_error("podium", "no event sequence for the team style".into()))?;
        self.you_won = you_won;
        self.started = Some(Started { team_style, winners, losers, events, big });
        self.time = -1.0;
        self.next_event = 0;
        self.next_sound = 0;
        self.fade = 1.0;
        Ok(())
    }

    /// A banner's element from `slug`, `slot` the
    /// winners then the losers; it is made at the next tick once the banner's entrance has started.
    pub fn set_banner(&mut self, slot: usize, slug: Option<&str>) -> Result<()> {
        let Some(s) = &mut self.started else { return Ok(()) };
        let n = s.winners.len();
        let rec = if slot < n { s.winners.get_mut(slot) } else { s.losers.get_mut(slot - n) };
        let Some(rec) = rec else { return Ok(()) };
        rec.banner = slug.map(str::to_owned);
        let (index, winner) = if slot < n { (slot, true) } else { (slot - n, false) };
        if let Some(pa) = s.actor(index, winner)
            && let Some(b) = self.banners.iter_mut().find(|b| b.pa == pa)
        {
            b.slug = slug.unwrap_or("").to_owned();
        }
        Ok(())
    }

    /// A UE world place (None: no actor
    /// for the index, an index past the result list).
    pub fn point(&self, p: ScenePoint) -> Option<[f64; 3]> {
        match p {
            ScenePoint::Fighter { index, winner } => {
                let pa = self.preview_of(index, winner)?;
                self.root_loc(pa).ok()
            }
            ScenePoint::Nametag { index, winner } => {
                let pa = self.preview_of(index, winner)?;
                let n = self.data.banner.nametag.loc;
                let tag = &self.data.previews[pa].tag;
                Some(self.banner_point(pa, [n[0], n[1] + nametag_offset(tag), n[2]])?.0)
            }
            ScenePoint::ToastButton { index, winner } => {
                let pa = self.preview_of(index, winner)?;
                Some(self.banner_point(pa, self.data.banner.toast_button.loc)?.0)
            }
        }
    }

    /// Whether the level has a place for `w`: the winner text, and a stat sheet
    /// of a banner with a preview.
    pub fn shows(&self, w: SceneWidget) -> bool {
        match w {
            SceneWidget::WinnerText => true,
            SceneWidget::StatSheet { index, winner } => self.preview_of(index, winner).is_some(),
            SceneWidget::Nametag(_) => false,
        }
    }

    /// A world space widget's quad drawn from
    /// `texture` (the menus' render target of it: RGBA8, premultiplied, one texel per Slate unit), from the next
    /// frame on; None hides it. The level keeps a handle on the texture: the program paints it again in place.
    pub fn widget(&mut self, w: SceneWidget, texture: Option<&Texture>) -> Result<()> {
        if matches!(w, SceneWidget::Nametag(_)) {
            warn_once("podium: a pre-match widget".into());
            return Ok(());
        }
        let at = self.widgets.iter().position(|v| v.w == w);
        // (the same texture painted again in place keeps its handle)
        let same = |v: &WidgetView| match (&v.picture, texture) {
            (Some(p), Some(t)) => p.res == t.res,
            _ => false,
        };
        if at.is_some_and(|k| same(&self.widgets[k])) {
            return Ok(());
        }
        let picture = texture.map(|t| Arc::new(share(t)));
        match at {
            Some(k) => self.widgets[k].picture = picture,
            None => self.widgets.push(WidgetView { w, picture }),
        }
        Ok(())
    }

    /// The sequence's time, its sounds, events and fade, the camera's lerp, the
    /// banners, the fighters and their notifies.
    pub fn tick(&mut self, cx: &mut LevelCx<'_>, dt: f64) -> Result<()> {
        if self.started.is_none() {
            return Ok(());
        }
        let length = self.data.sequence.length;
        let prev = self.time;
        self.time = if prev < 0.0 { 0.0 } else { (prev + dt).min(length) };
        // the Delay's latent action: its time left drops by each frame's delta, done at <= 0
        if self.vo_delay > 0.0 && dt > 0.0 {
            self.vo_delay -= dt;
            if self.vo_delay <= 0.0 {
                cx.events.push(MenuEvent::Podium { call: "VictoryVO".into(), args: Vec::new() });
            }
        }
        if self.time != prev {
            let time = self.time;
            let sounds = &self.data.sequence.sounds;
            while self.next_sound < sounds.len() && sounds[self.next_sound].t <= time {
                cx.events.push(MenuEvent::Sound { event: sounds[self.next_sound].ev.clone(), at: None });
                self.next_sound += 1;
            }
            while let Some(e) =
                self.started.as_ref().and_then(|s| s.events.get(self.next_event)).filter(|e| e.t <= time)
            {
                let e = e.clone();
                self.next_event += 1;
                self.fire(cx, &e)?;
            }
            if let Some(keys) = self.data.sequence.fade.as_deref().filter(|k| !k.is_empty()) {
                self.fade = eval_channel(keys, time) as f32;
            }
        }
        if self.lerp >= 0.0 && dt > 0.0 {
            let c = &self.data.camera;
            if let Some(last) = c.zoom_curve.last() {
                let end = last.t;
                self.lerp = (self.lerp + dt * end / c.zoom_length).min(end);
                let a = eval_weighted(&c.zoom_curve, self.lerp);
                self.cam_loc = std::array::from_fn(|k| self.lerp_from[k] + (c.zoom.loc[k] - self.lerp_from[k]) * a);
                if self.lerp >= end {
                    self.lerp = -1.0;
                }
            }
        }
        if dt > 0.0 {
            self.tick_banners(cx, dt);
            self.shaker.update(dt);
        }
        self.tick_banner_elements(cx);
        if dt > 0.0 {
            self.tick_fighters(cx, dt);
        }
        Ok(())
    }

    /// The camera of the moment for an output of `size`: the shake's offset
    /// in the camera's space, x along the view direction, z along its up.
    pub fn camera(&self, size: [u32; 2]) -> Camera {
        let rot = self.data.camera.start.rot;
        let (p, y) = (rot[0].to_radians(), rot[1].to_radians());
        let front = [p.cos() * y.cos(), p.cos() * y.sin(), p.sin()];
        let up = [-p.sin() * y.cos(), -p.sin() * y.sin(), p.cos()];
        let at: [f64; 3] = std::array::from_fn(|k| self.cam_loc[k] + front[k] * self.shaker.x + up[k] * self.shaker.z);
        let c = &self.data.camera;
        menu_camera(at, rot, c.fov, c.aspect, size)
    }

    /// The camera, planes, lights, fighters and banners into the frame.
    pub fn frame(&mut self, out: &mut MenuFrame) {
        let t = self.time.max(0.0);
        out.camera = Some(self.camera(out.size));
        self.place_planes(t, out);
        self.place_widgets(t, out);
        for (key, lights) in &self.data.lights {
            let Ok(channel) = key.parse::<u8>() else {
                warn_once(format!("podium: light channel {key}"));
                continue;
            };
            out.directional.extend(lights.iter().map(|l| (channel, l.clone())));
        }
        if self.started.is_none() {
            return;
        }
        for f in self.fighters.iter_mut().filter(|f| f.shown) {
            let world: M4 = mesh_world(f.loc, [f.scale; 3], f.left);
            let mut pose: FighterPose = f.view.pose(&world);
            pose.fighter = f.pa as u32;
            if let (Some(sh), Some(g)) = (&f.glow, &self.glow) {
                sh.meshes(g, &pose.meshes, &mut out.meshes);
            }
            out.fighters.push((pose, f.channel));
            // BlobShadowCenter, shown with the mesh (SetVisibility(true) @8844 past the Podium's Delay 0.2): under
            // the actor's root, which has the level's uniform scale and no rotation (the mesh carries the
            // placement's scale and the yaw)
            let slot = &self.data.previews[f.pa];
            if !slot.shadow.hidden {
                let s = slot.scale;
                let p = &slot.shadow;
                let m =
                    transform(std::array::from_fn(|k| f.loc[k] + s * f.shadow_loc[k]), p.rot, p.scale.map(|v| v * s));
                let b = &self.data.blob_shadow;
                out.meshes.push((
                    MeshPose {
                        mesh: b.mesh.as_str().into(),
                        to_world: to_world(&m),
                        materials: vec![material_ref(&b.material, None)],
                        ..MeshPose::default()
                    },
                    0,
                ));
            }
        }
        for b in &mut self.banners {
            if let Some(el) = &mut b.element {
                el.place_at(b.root, &mut out.meshes);
            }
            b.view.meshes(&mut out.meshes);
            b.view.custom_depth(&mut out.custom_depth);
        }
    }

    // ---------- the actors ----------

    /// The preview actor of a sequence call (DeterminePreviewActor); None: no actor for it (a 1v1 match's index-1
    /// calls).
    fn preview_of(&self, index: usize, winner: bool) -> Option<usize> {
        let s = self.started.as_ref()?;
        if index >= if winner { s.winners.len() } else { s.losers.len() } {
            return None;
        }
        let pa = s.actor(index, winner)?;
        (pa < self.data.previews.len()).then_some(pa)
    }

    /// The record of a preview actor: its character, skin and banner slug.
    fn record_of(&self, pa: usize) -> Option<&PodiumRecord> {
        let s = self.started.as_ref()?;
        let find = |winner: bool| {
            let list = if winner { &s.winners } else { &s.losers };
            (0..list.len()).find(|&i| s.actor(i, winner) == Some(pa)).map(|i| &list[i])
        };
        find(true).or_else(|| find(false))
    }

    /// The team style whose places preview actor `pa` takes: its result list's (`side_style`), the match's when no
    /// record stands there (0 before the start).
    fn style_of(&self, pa: usize) -> u8 {
        let Some(s) = self.started.as_ref() else { return 0 };
        for (winner, list) in [(true, &s.winners), (false, &s.losers)] {
            if (0..list.len()).any(|i| s.actor(i, winner) == Some(pa)) {
                return if s.big && !winner { s.team_style } else { side_style(s.team_style, list.len()) };
            }
        }
        s.team_style
    }

    /// The skin values of the fighter a preview actor takes (ChangeFighter): its record's skin's (the skin's EOG
    /// ScaleOverrides, VictoryMontageOverride / DefeatMontageOverride), the default skin's when the skin has none of
    /// its own; an actor without a record reads the first character's (nothing is placed by it).
    fn fighter_of(&self, pa: usize) -> Result<(&PodiumFighter, &str)> {
        let rec = self.record_of(pa);
        let fs = &self.data.fighters;
        let f = match rec {
            Some(r) => {
                fs.get(&r.key).ok_or_else(|| format_error("podium", format!("no fighter data for {}", r.key)))?
            }
            None => fs.values().next().ok_or_else(|| format_error("podium", "no fighters".into()))?,
        };
        let skin = rec.map_or("", |r| r.skin.as_str());
        match f.skins.as_ref().and_then(|m| m.get(skin)) {
            Some(own) if !skin.is_empty() => Ok((own, skin)),
            _ => Ok((&f.default, "")),
        }
    }

    fn facing_left(&self, pa: usize) -> bool {
        let slot = &self.data.previews[pa];
        let ts = self.style_of(pa);
        slot.facing_left && !(ts == 0 && slot.tag == "Player3")
    }

    /// A preview root's location: its target point, the game mode's offset (a 2v2 list of one: the 1v1's,
    /// `side_style`) and the skin's EOG entry.
    fn root_loc(&self, pa: usize) -> Result<[f64; 3]> {
        let slot = &self.data.previews[pa];
        let (f, _) = self.fighter_of(pa)?;
        let ts = self.style_of(pa);
        let ov = if slot.tag == "Player2" || slot.tag == "Player3" { &f.winner } else { &f.loser };
        let off = offset_of(ts, &slot.point);
        let my = if self.facing_left(pa) { -1.0 } else { 1.0 };
        Ok([slot.loc[0] + off[0] + ov.loc[0], slot.loc[1] + off[1] + ov.loc[1] * my, slot.loc[2] + off[2] + ov.loc[2]])
    }

    /// A point of a preview's banner, given relative to its Banner Placement, and the banner actor's scale: the spawn
    /// point under the preview's root (root scale = the level's), then Banner Placement.
    fn banner_point(&self, pa: usize, rel: [f64; 3]) -> Option<([f64; 3], f64)> {
        let slot = &self.data.previews[pa];
        let ts = self.started.as_ref()?.team_style;
        let ffa = if ts == 2 { banner_ffa(&slot.tag) } else { None };
        let root = self.root_loc(pa).ok()?;
        let s = slot.scale * ffa.map_or(slot.banner.scale, |f| f.0);
        let loc = std::array::from_fn(|k| {
            root[k] + slot.scale * slot.banner.loc[k] + ffa.map_or(0.0, |f| f.1[k]) + s * (BANNER_PLACEMENT[k] + rel[k])
        });
        Some((loc, s))
    }

    // ---------- the director's calls ----------

    /// Fire.
    fn fire(&mut self, cx: &mut LevelCx<'_>, e: &SceneEvent) -> Result<()> {
        let index = || match e.args.first() {
            Some(super::format::EventArg::Num(n)) => *n as usize,
            _ => 0,
        };
        let winner = || matches!(e.args.get(1), Some(super::format::EventArg::Bool(true)));
        match e.call.as_str() {
            "ZoomToWInner" => {
                self.lerp = 0.0;
                self.lerp_from = self.cam_loc;
            }
            "ShowFighter" => {
                self.show_fighter(cx, index(), winner());
                // level Blueprint @6904: DoOnce on the first call (with or without an actor), Delay 3.0, the victory
                // VO loop
                if !self.vo_armed {
                    self.vo_armed = true;
                    self.vo_delay = VICTORY_VO_DELAY;
                }
            }
            "AnimBannerIn" => self.animate_banner_in(cx, index(), winner()),
            _ => {}
        }
        cx.events.push(MenuEvent::Podium { call: e.call.clone(), args: e.args.clone() });
        Ok(())
    }

    /// Fighters: four BP_CharacterPreview actors (tags Player1..4) on TargetPoints. The level Blueprint's
    /// DeterminePreviewActor picks one per result index, BeginPlay moves Winner2 / Loser2 by game mode, and the
    /// preview's BeginPlay turns the Player3 actor to face right in team style 0. ShowFighter (Index, IsWinner) ->
    /// BPI_ChangeFighter -> PlayVFX (the main menu's: PS_CharacterSelectLightBeam, released after 1 s,
    /// PS_CharacterSelectSwirl04 at a random yaw, and here ui_characterselect_lightbeam), and after Delay 0.2 the
    /// mesh shows and plays its montage from 0: the victory one for the Player2 actor and, outside team style 2, the
    /// Player3 actor, else the defeat one (by the actor's tag, not the result). Placement: the skin's "EOG_Winner"
    /// (Player2 / Player3 actors) or "EOG_Loser" ScaleOverrides entry replaces the root's relative location and the
    /// mesh scale; mesh yaw -90, or 90 when FacingLeft, and the AnimBP is told FacingLeft (BPI_SetIsFacingLeft
    /// @5476).
    /// Lighting: no rig on this level; the meshes take lighting channel 1 or 2 by tag and team style (`channel_of`)
    /// for the level's two sets of directional lights. At the 0.2 s the preview also runs GlowVFX(false) (@8790): the
    /// main menu's glow shell, one per actor.
    // Deviation: the shell draws before the spawn effect's particles (TranslucencySortPriority 0: the translucent
    // pass before the particles'); the build sorts all of them by the view depth of their bounds.
    // Deviation: the key light's shadows are left out; the fighter's async load and the two zero delays before the
    // effect are taken as no time.
    fn show_fighter(&mut self, cx: &mut LevelCx<'_>, index: usize, winner: bool) {
        let Some(pa) = self.preview_of(index, winner) else { return };
        if self.fighters.iter().any(|f| f.pa == pa) {
            return;
        }
        // (a big match's second list are winners too)
        let big = self.started.as_ref().is_some_and(|s| s.big);
        match self.make_fighter(cx, pa, winner || big) {
            Ok(f) => {
                let at = f.loc;
                if let Some(v) = cx.vfx.as_deref_mut() {
                    let mut f = f;
                    f.beam = v.spawn_attached(BEAM, &attachment(at, 0.0), 1.0, &[]);
                    let yaw = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.subsec_nanos()) as f64
                        / 1e9)
                        * 2.0
                        * std::f64::consts::PI;
                    f.swirl = v.spawn_attached(SWIRL, &attachment(at, yaw), 1.0, &[]);
                    self.fighters.push(f);
                } else {
                    self.fighters.push(f);
                }
                cx.events.push(MenuEvent::Sound { event: BEAM_SOUND.into(), at: Some(at) });
            }
            Err(e) => warn_once(format!("podium: the fighter of preview actor {pa}: {e}")),
        }
    }

    /// The fighter of a preview actor (preview and ShowFighter).
    fn make_fighter(&self, cx: &LevelCx<'_>, pa: usize, is_winner: bool) -> Result<Fighter> {
        let slot = &self.data.previews[pa];
        let ts = self.style_of(pa);
        let rec = self.record_of(pa).ok_or_else(|| format_error("podium", format!("no record for actor {pa}")))?;
        let (data, skin) = self.fighter_of(pa)?;
        let left = self.facing_left(pa);
        let winner_slot = slot.tag == "Player2" || slot.tag == "Player3";
        let ov = if winner_slot { &data.winner } else { &data.loser };
        // Deviation: a big match's winners on the losers' places play the victory montage (the build has no winner
        // there).
        let victory =
            slot.tag == "Player2" || (slot.tag == "Player3" && ts != 2) || self.started.as_ref().is_some_and(|s| s.big);
        let montage = if victory { &data.victory } else { &data.defeat };
        let store = cx.assets.store();
        let view = match PreviewView::new(store, &rec.key, skin) {
            Ok(v) => v,
            Err(e) if !skin.is_empty() => {
                warn_once(format!("podium: {} in skin {skin}: {e}: the default skin", rec.key));
                PreviewView::new(store, &rec.key, "")?
            }
            Err(e) => return Err(e.into()),
        };
        let view_notifies = view.notifies(&montage.name);
        let fx = NotifyVfx::new(&view);
        // ShadowChanges (ChangeFighter @25325 on a Podium level -> @34598): a skin whose SkinData's name has "wonder"
        // (Contains, any case) on the Player1 / Player4 actor moves BlobShadowCenter by (-40, -20 or 20 when
        // FacingLeft, -25).
        let mut shadow_loc = slot.shadow.loc;
        if data.skin.to_ascii_lowercase().contains("wonder") && (slot.tag == "Player1" || slot.tag == "Player4") {
            let add = [-40.0, if left { -20.0 } else { 20.0 }, -25.0];
            shadow_loc = std::array::from_fn(|k| shadow_loc[k] + add[k]);
        }
        Ok(Fighter {
            pa,
            view,
            loc: self.root_loc(pa)?,
            scale: slot.scale * ov.scale,
            montage: montage.clone(),
            channel: channel_of(&slot.tag, ts),
            left,
            shadow_loc,
            active: Vec::new(),
            age: 0.0,
            shown: false,
            pos: 0.0,
            section: section_at(montage, 0.0),
            beam: None,
            swirl: None,
            glow: None,
            notifies: view_notifies,
            meshes: Vec::new(),
            fx,
            color_index: u8::from(is_winner),
        })
    }

    /// BPI_AnimateBannerIn (level Blueprint @9127): the preview actor's banner plays Animate EOG Banner Entrance.
    /// Unverified: the sequence's events are taken to fire before the actors tick (4.26 tick manager), so the
    /// timeline advances in the event's frame.
    fn animate_banner_in(&mut self, cx: &mut LevelCx<'_>, index: usize, winner: bool) {
        let Some(k) = self.banner_of(index, winner) else { return };
        self.banners[k].view.start();
        // Start EOG Entrance @7874: Apply Banner Appearance(Banner) after Set Banner Visibility(true)
        self.apply_appearance(cx, k);
    }

    /// The banner of a sequence call's preview actor, made at its first call (hidden until AnimBannerIn): StencilValue
    /// = Banner Index (the tag's number), ColorType.
    fn banner_of(&mut self, index: usize, winner: bool) -> Option<usize> {
        let pa = self.preview_of(index, winner)?;
        if let Some(k) = self.banners.iter().position(|b| b.pa == pa) {
            return Some(k);
        }
        let b = &self.data.banner;
        let (loc, s) = self.banner_point(pa, b.mask.loc)?;
        let s_ = self.started.as_ref()?;
        let rec = if winner { s_.winners.get(index) } else { s_.losers.get(index) }?;
        let parts = BannerParts {
            mask: b.mask.clone(),
            background: b.background.clone(),
            filter: b.filter.clone(),
            trim: b.trim.clone(),
            nameplate: None,
            slant_angle: b.slant_angle,
            entrance_length: b.entrance_length,
        };
        let ber = &b.base_element_root;
        let (root_loc, _) = self.banner_point(pa, ber.loc)?;
        let root = [root_loc, ber.rot, ber.scale.map(|x| x * s)];
        let view = BannerView::new(parts, loc, s, i32::try_from(pa + 1).ok()?, rec.color + 1);
        self.banners.push(PodiumBanner {
            pa,
            view,
            slug: rec.banner.clone().unwrap_or_default(),
            tried: false,
            element: None,
            root,
        });
        Some(self.banners.len() - 1)
    }

    /// Apply Banner Appearance (BAN @179-2311): doing nothing unless the banner has a slug whose element data reads
    /// (a bad slug keeps the default banner). The default background (never drawn again) and the Banner Filter are
    /// hidden, the element is spawned at Base Element Root's world transform (@686-884), initialized with BannerIndex =
    /// Banner Index, ColorType = FTrunc(the trim's ColorType), SlantAngle = the trim's Angle (@1183-1255), then Show
    /// Banner (@2311).
    fn apply_appearance(&mut self, cx: &mut LevelCx<'_>, k: usize) {
        let slant = self.data.banner.slant_angle;
        let cam = self.cam_loc;
        let b = &mut self.banners[k];
        if b.tried || b.slug.is_empty() || !b.view.started {
            return;
        }
        b.tried = true;
        let init = BannerInit { banner_index: b.view.stencil, color_type: b.view.color, slant_angle: slant };
        match BannerElement::new(cx, &b.slug, init, b.root, cam) {
            Ok(Some(mut el)) => {
                el.show(true);
                b.view.shown[PIECE_BACKGROUND] = false;
                b.view.shown[PIECE_FILTER] = false;
                b.element = Some(el);
            }
            Ok(None) => {}
            Err(e) => warn_once(format!("podium: banner {}: {e}", b.slug)),
        }
    }

    // ---------- per tick ----------

    /// The banners' Entrance Timeline and placement; a landing shakes the camera and sounds. A slug that came after
    /// the entrance started is applied here.
    fn tick_banners(&mut self, cx: &mut LevelCx<'_>, dt: f64) {
        for k in 0..self.banners.len() {
            if self.banners[k].view.tick(dt) {
                self.shaker.play(BANNER_SHAKE);
                cx.events.push(MenuEvent::Sound { event: BANNER_SOUND.into(), at: None });
            }
            self.apply_appearance(cx, k);
        }
    }

    /// BAN ReceiveTick @11081: once the appearance is applied, each element's parallax from UI_Cursor's
    /// MousePosNormalized and the camera's location (TargetCameraSceneComponent: the level's CineCamera); the
    /// element's systems follow it.
    fn tick_banner_elements(&mut self, cx: &mut LevelCx<'_>) {
        let cam = self.cam_loc;
        let cursor = self.cursor;
        let mut scratch = Vec::new();
        for b in &mut self.banners {
            let Some(el) = &mut b.element else { continue };
            el.parallax(cursor, cam);
            // HideDefaultBorder: the owner's Banner Trim Mask is hidden
            if el.hide_trim {
                b.view.shown[PIECE_TRIM] = false;
            }
            scratch.clear();
            el.place_at(b.root, &mut scratch);
            if let Some(v) = cx.vfx.as_deref_mut() {
                el.place_systems(v);
            }
        }
    }

    /// The previews' timers and montages (Montage_Advance as the main menu's: at a section's end the position
    /// continues at the next section's start). A montage with a BlendIn (Jake_Loss_Montage, the class default 0.25 s)
    /// blends in over the idle posed while the mesh is hidden (`PreviewView::step_idle`, Unverified there).
    fn tick_fighters(&mut self, cx: &mut LevelCx<'_>, dt: f64) {
        let glow = self.glow.clone();
        for f in &mut self.fighters {
            let before = f.age;
            f.age += dt;
            if f.age >= BEAM_LIFE
                && let Some(id) = f.beam.take()
                && let Some(v) = cx.vfx.as_deref_mut()
            {
                v.kill(id);
            }
            if f.age < SHOW_DELAY {
                if f.montage.blend_in.is_some_and(|b| b > 0.0) {
                    f.view.step_idle(f.age, f.left, dt);
                }
                continue;
            }
            if !f.shown {
                f.shown = true;
                // @8790 GlowVFX(false): the timeline starts and, as the menu's, advances in the same tick
                f.glow = glow.as_ref().map(|g| {
                    let mut s = GlowShell::new(g);
                    s.start(g, false);
                    s
                });
            } else if before >= SHOW_DELAY {
                f.pos += dt;
            }
            if let (Some(sh), Some(g)) = (&mut f.glow, &glow) {
                sh.tick(g, dt);
            }
            // The meshes' ages advance before the notifies
            for e in &mut f.meshes {
                e.age += dt;
            }
            // the steps of this frame's advance, for the notifies (previous, current)
            let m = &f.montage;
            let mut steps: Vec<[f64; 2]> = Vec::new();
            let mut from = f.pos - if before >= SHOW_DELAY { dt } else { 0.0 };
            for _ in 0..8 {
                let end = if f.section + 1 < m.sections.len() { m.sections[f.section + 1].start } else { m.length };
                if f.pos < end {
                    break;
                }
                let link = &m.sections[f.section].next;
                let Some(next) = m.sections.iter().position(|s| s.name.eq_ignore_ascii_case(link)) else {
                    f.pos = end;
                    break;
                };
                steps.push([from, end]);
                from = m.sections[next].start;
                f.pos = m.sections[next].start + (f.pos - end);
                f.section = section_at(m, f.pos);
            }
            steps.push([from, f.pos]);
            fighter_notifies(f, &steps, cx.assets.store(), cx.events, cx.vfx.as_deref_mut());
            let Some(seg) = segment_at(&f.montage, f.pos) else { continue };
            let time = (seg.from + (f.pos - seg.start) * seg.rate).min(seg.to);
            let blend_in = f.montage.blend_in.unwrap_or(0.0);
            let at = PreviewAt { clip: &seg.clip, time, facing_left: f.left, montage: &f.montage.name, blend_in };
            f.view.step(&at, dt);
            for e in &f.meshes {
                f.view.pose_mesh(&e.mesh, e.age, e.looped);
            }
            if let Some(v) = cx.vfx.as_deref_mut() {
                let world: M4 = mesh_world(f.loc, [f.scale; 3], f.left);
                f.fx.follow(&mut f.view, &world, f.left, v);
            }
        }
    }

    // ---------- the frame ----------

    /// The world space widgets' quads.
    /// The winner text: BP_WinningTeam_2's WidgetComponent of WBP_3dTeam at its desired size into a transparent
    /// render target, shown on a quad in the component's YZ plane facing +X, one unit per pixel, centred (pivot 0.5),
    /// unrotated; its actor is attached to TeamNameText, which the Geo sequence moves (with the planes' shift on a
    /// page wider than the frame, see place_planes). A banner's Stat Sheet: the component on Banner Placement at its
    /// relative place, rotation and scale (times the banner actor's), drawn at the widget's desired size, one unit
    /// per pixel before the scale, around its centre.
    // Unverified: the quad's corners, the render target's colour space and the pass-through material
    // (Widget3DPassThrough_Translucent) are written from the 4.26 source. The picture is the Slate paint's RGBA8,
    // premultiplied and sRGB-encoded; the quad un-premultiplies it and sRGB-decodes it to linear (as a canvas is read
    // as sRGB), and the target's premultiplied edges are not reproduced.
    fn place_widgets(&self, t: f64, out: &mut MenuFrame) {
        let cam = &self.data.camera;
        let [w, h] = out.size.map(f64::from);
        let gain = if h > 0.0 { (w / h / cam.aspect - 1.0).max(0.0) } else { 0.0 };
        let tan_h = (cam.fov * std::f64::consts::PI / 360.0).tan();
        for v in &self.widgets {
            let Some(picture) = &v.picture else { continue };
            let [pw, ph] = [picture.desc.size[0], picture.desc.size[1]].map(f64::from);
            let place = match v.w {
                SceneWidget::WinnerText => {
                    let text = &self.data.text;
                    let ch = self.data.sequence.transforms.get("TeamNameText");
                    let at = |k: usize| ch.and_then(|c| c.get(k)).and_then(Option::as_ref);
                    let mut loc: [f64; 3] = std::array::from_fn(|k| {
                        at(k).and_then(|c| eval_podium_channel(c, t)).unwrap_or(text.actor[k])
                            + text.child[k]
                            + text.component[k]
                    });
                    if gain > 0.0 {
                        let y = loc[1] - text.child[1] - text.component[1];
                        loc[1] += (self.cam_loc[0] - loc[0]).abs() * tan_h * gain * leaving(at(1), t, y);
                    }
                    transform(loc, [0.0; 3], [1.0, pw, ph])
                }
                SceneWidget::StatSheet { index, winner } => {
                    let Some(pa) = self.preview_of(index, winner) else { continue };
                    let c = &self.data.banner.stat_sheet;
                    let Some((loc, s)) = self.banner_point(pa, c.loc) else { continue };
                    let s = c.scale[0] * s;
                    transform(loc, c.rot, [s, s * pw, s * ph])
                }
                SceneWidget::Nametag(_) => continue,
            };
            out.widgets.push(crate::scenery::WidgetQuad { place, picture: picture.clone(), first: false });
        }
    }

    /// The planes at sequence time `t`: keyed translation channels over the actor's own location.
    // Deviation: the team colour and trophy planes end in a slanted cut just past the right edge of the 16:9 frame.
    // On a wider page the view grows past that frame, so the two planes are moved to the right (-Y) by the half width
    // the view gains at their depth, which keeps the cut as far outside the picture as in the frame. The same on the
    // left when they leave: the Geo sequence's last Y key (4.5 -> 5.0 s: 5000, kept after the end) parks the two
    // planes left of the frame, the planes' cut about 300 units past its left edge at their depth, and nothing hides
    // them. The build's view is never wider than that frame there (the CineCamera's FieldOfView is horizontal; the
    // level Blueprint's only aspect rule is the camera targets' shift, Map_PodiumStage_New @4451..5097), so the
    // parked pieces are never seen; here a page wider than about 1.9:1 showed the planes at its left edge. Over that
    // last key the shift goes from the right-hand one to the left-hand one, by the half width gained at the piece's
    // depth. The level Blueprint's ultrawide shift of the camera targets (-500 / -1000 on X past 2.0 / 2.3) is left
    // out.
    fn place_planes(&self, t: f64, out: &mut MenuFrame) {
        let cam = &self.data.camera;
        let [w, h] = out.size.map(f64::from);
        let gain = if h > 0.0 { (w / h / cam.aspect - 1.0).max(0.0) } else { 0.0 };
        let tan_h = (cam.fov * std::f64::consts::PI / 360.0).tan();
        let materials = if self.you_won { "Blue" } else { "Red" };
        for p in &self.data.planes {
            let channels = self.data.sequence.transforms.get(&p.name);
            let at = |k: usize| channels.and_then(|c| c.get(k)).and_then(Option::as_ref);
            let mut loc: [f64; 3] =
                std::array::from_fn(|k| at(k).and_then(|c| eval_podium_channel(c, t)).unwrap_or(p.loc[k]));
            if gain > 0.0 {
                let wide = WIDE_PLANES.contains(&p.name.as_str());
                let side = leaving(at(1), t, loc[1]) * if wide { 2.0 } else { 1.0 } - if wide { 1.0 } else { 0.0 };
                loc[1] += (self.cam_loc[0] - loc[0]).abs() * tan_h * gain * side;
            }
            let Some(m) = p.materials.get(materials).or_else(|| p.materials.get("default")) else {
                warn_once(format!("podium: plane {} has no material", p.name));
                continue;
            };
            out.meshes.push((
                MeshPose {
                    mesh: self.plane.clone(),
                    to_world: to_world(&transform(loc, p.rot, p.scale)),
                    materials: vec![material_ref(m, None)],
                    ..MeshPose::default()
                },
                p.sort,
            ));
        }
    }
}

/// The montage's notifies over this frame's steps (queued notifies, 1428719e0: a notify is listed for a step when its
/// trigger time <= current and its end time > previous; NotifyEnd for the states no longer listed, then
/// NotifyBegin). The preview actor is a BPI_Character_Viewer with a Comp_FighterVisibilityManager, so
/// AnimNotify_SetSubmeshVisibility acts on it as on a fighter.
/// Sounds: Ak_OneShot (Received_Notify @94-434: Local Only passes on the local controller; Follow: the event on the
/// AkComponent of the owner's root) and Ak_VO_Effort (@94-201: the owner is no BP_Fighter_Character, so no IsSpeaking
/// test; @582-691 the same component) are posted at the preview actor.
// Ak_VO (Shaggy's "emote" line) posts nothing here: AliasToPlay is set, so Received_Notify @1554 casts the owner to
// Pawn, and BP_CharacterPreview is an Actor (return false @2084).
// Deviation: an Ak_OneShot without Follow posts at its socket (@687-894; no podium montage has one, posted at the
// actor here); Ak_SurfaceEffect (Shaggy's emote footsteps) is not run: its PF_Surface_Effect_Trace from the TraceFrom
// socket on the podium floor's physical material and the AkComponent spawned at the hit are not ported.
// Unverified: see PreviewView::add_mesh (the preview's skeleton without the pawn's "no mirror node" statement).
fn fighter_notifies(
    f: &mut Fighter,
    steps: &[[f64; 2]],
    store: &mvs_store::Store,
    events: &mut Vec<MenuEvent>,
    mut vfx: Option<&mut mvs_vfx::Player>,
) {
    if f.notifies.is_empty() {
        return;
    }
    for n in &f.notifies {
        let Some(ev) = n.p.get("AkEvent").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else { continue };
        if n.state || !SOUND_NOTIFIES.contains(&n.cls.trim_end_matches("_C")) {
            continue;
        }
        if steps.iter().any(|[prev, cur]| n.t <= *cur && n.t > *prev) {
            events.push(MenuEvent::Sound { event: ev.to_owned(), at: Some(f.loc) });
        }
    }
    let mut now = Vec::new();
    for (i, n) in f.notifies.iter().enumerate() {
        let end = f64::from((n.t + n.dur) as f32);
        if n.state && steps.iter().any(|[prev, cur]| n.t <= *cur && end > *prev) {
            now.push(i);
        }
    }
    let prop = |n: &Notify, k: &str| n.p.get(k).is_some_and(truthy);
    let vis = |f: &mut Fighter, n: &Notify, on: bool| {
        set_tag_visible(&mut f.view, n.p.get("SubmeshTag").and_then(|v| v.as_str()).unwrap_or("None"), on);
    };
    let world: M4 = mesh_world(f.loc, [f.scale; 3], f.left);
    let ended: Vec<usize> = f.active.iter().copied().filter(|i| !now.contains(i)).collect();
    f.active.retain(|i| now.contains(i));
    for i in ended {
        let n = f.notifies[i].clone();
        match n.cls.trim_end_matches("_C") {
            "AnimNotify_SubMeshSpawn" => {
                let mesh = n.p.get("Mesh").and_then(|v| v.as_str()).unwrap_or_default();
                if let Some(at) = f.meshes.iter().position(|e| e.mesh == mesh) {
                    f.view.remove_mesh(mesh);
                    f.meshes.remove(at);
                }
            }
            "AnimNotify_SetSubmeshVisibility" if prop(&n, "ReverseAtNotifyEnd") => vis(f, &n, !prop(&n, "Visible")),
            "AnimNotify_PlayVFX" if prop(&n, "KillOnNotifyEnd") => {
                if let Some(v) = vfx.as_deref_mut() {
                    f.fx.end(i, v);
                }
            }
            _ => {}
        }
    }
    for i in now {
        if f.active.contains(&i) {
            continue;
        }
        f.active.push(i);
        let n = f.notifies[i].clone();
        let p = &n.p;
        match n.cls.trim_end_matches("_C") {
            "AnimNotify_SubMeshSpawn" => {
                let Some(mesh) = p.get("Mesh").and_then(|v| v.as_str()).filter(|m| !m.is_empty()) else { continue };
                if f.meshes.iter().any(|e| e.mesh == mesh) {
                    continue;
                }
                match f.view.add_mesh(store, p, f.left) {
                    Ok(true) => {
                        f.meshes.push(MeshOn { mesh: mesh.to_owned(), looped: prop(&n, "LoopAnimation"), age: 0.0 })
                    }
                    Ok(false) => {}
                    Err(e) => warn_once(format!("podium: montage mesh {mesh}: {e}")),
                }
            }
            "AnimNotify_SubMeshAnimChange" => {
                let mesh = p.get("Mesh").and_then(|v| v.as_str()).unwrap_or_default();
                if let Some(e) = f.meshes.iter_mut().find(|e| e.mesh == mesh) {
                    // Deviation: the animation is resolved as a montage's too where it is no clip (mesh_clip's
                    // fallback).
                    f.view.mesh_anim(mesh, p.get("Animation").and_then(|v| v.as_str()));
                    e.looped = prop(&n, "LoopAnimation");
                    e.age = 0.0;
                }
            }
            "AnimNotify_PlayVFX" => {
                if let Some(v) = vfx.as_deref_mut() {
                    f.fx.begin(&mut f.view, &world, i, p, f.left, f.color_index, v);
                }
            }
            "AnimNotify_SetSubmeshVisibility" => {
                // Unverified: ReverseWhenAnimBlendsOut is not ported (no podium montage sets it);
                // CachedIsLocalPlayer is taken as its default (no earlier uncached GetColorIndex on the actor); the
                // sequence's transform sections are taken as KeepState (no compiled template to read it from).
                if prop(&n, "ReverseWhenAnimBlendsOut") {
                    warn_once("podium: SetSubmeshVisibility ReverseWhenAnimBlendsOut not ported".into());
                }
                vis(f, &n, prop(&n, "Visible"));
            }
            _ => {}
        }
    }
}

/// JavaScript's truthiness of a property.
fn truthy(v: &mvs_data::Value) -> bool {
    use mvs_data::Value;
    match v {
        Value::Bool(b) => *b,
        Value::Str(s) => !s.is_empty(),
        v => v.as_f64().is_none_or(|x| x != 0.0 && !x.is_nan()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev_store() -> Option<mvs_store::Store> {
        let s = mvs_store::Store::open(mvs_store::dev_store_dir()).ok();
        if s.is_none() {
            eprintln!("no developer store: skipped");
        }
        s
    }

    /// The preview actor per result index, by team style (DeterminePreviewActor).
    #[test]
    fn actors_by_team_style() {
        assert_eq!(actor_of(0, 0, true), Some(2));
        assert_eq!(actor_of(0, 1, true), None);
        assert_eq!(actor_of(1, 1, false), Some(3));
        assert_eq!(actor_of(2, 2, false), Some(3));
        assert_eq!(actor_of(2, 1, true), None);
        assert_eq!(channel_of("Player3", 0), 1);
        assert_eq!(channel_of("Player1", 2), 2);
        assert_eq!(channel_of("Player1", 0), 0);
    }

    fn started(team_style: u8, winners: usize, losers: usize) -> Started {
        Started {
            team_style,
            winners: vec![PodiumRecord::default(); winners],
            losers: vec![PodiumRecord::default(); losers],
            events: Vec::new(),
            big: false,
        }
    }

    /// A big match's winners: the first two on the 2v2 winners' actors, the rest on its losers' (0, then 3) with
    /// the 2v2's places, even one alone; free for all's on its losers' 0, 2, 3.
    #[test]
    fn a_big_match_stands_its_winners_on_every_place() {
        let mut s = started(1, 2, 1);
        s.big = true;
        assert_eq!([s.actor(0, true), s.actor(1, true), s.actor(0, false)], [Some(1), Some(2), Some(0)]);
        let mut s = started(1, 2, 2);
        s.big = true;
        assert_eq!([s.actor(0, false), s.actor(1, false)], [Some(0), Some(3)]);
        let mut s = started(2, 1, 3);
        s.big = true;
        assert_eq!([s.actor(0, true), s.actor(0, false), s.actor(2, false)], [Some(1), Some(0), Some(3)]);
    }

    /// A 2v2 match of three: the pair takes the 2v2's places, the one the 1v1's (its actor and offset), no empty
    /// place; a full 2v2 and a 1v1 as before.
    #[test]
    fn a_side_of_one_takes_the_one_v_one_place() {
        // a pair wins: winners on actors 1 and 2, the lone loser on the 1v1 loser's actor 3
        let s = started(1, 2, 1);
        assert_eq!([s.actor(0, true), s.actor(1, true)], [Some(1), Some(2)]);
        assert_eq!([s.actor(0, false), s.actor(1, false)], [Some(3), None]);
        // the lone player wins: the 1v1 winner's actor 2, the pair loses on actors 0 and 3
        let s = started(1, 1, 2);
        assert_eq!([s.actor(0, true), s.actor(1, true)], [Some(2), None]);
        assert_eq!([s.actor(0, false), s.actor(1, false)], [Some(0), Some(3)]);
        assert_eq!(side_style(1, 1), 0);
        assert_eq!(offset_of(side_style(1, 1), "Winner2"), [0.0, 325.0, 0.0]);
        // unchanged: a full 2v2, a 1v1, free for all
        let s = started(1, 2, 2);
        assert_eq!([s.actor(0, false), s.actor(1, false)], [Some(0), Some(3)]);
        assert_eq!(side_style(1, 2), 1);
        let s = started(0, 1, 1);
        assert_eq!([s.actor(0, true), s.actor(0, false)], [Some(2), Some(3)]);
        assert_eq!(side_style(2, 1), 2);
    }

    /// The Medium shake lives 0.095 s and returns to rest.
    #[test]
    fn shake_ends() {
        let mut s = mvs_view::camera::CameraShaker::default();
        s.play(BANNER_SHAKE);
        s.update(0.02);
        assert!(s.x != 0.0 || s.z != 0.0);
        for _ in 0..10 {
            s.update(0.02);
        }
        assert_eq!((s.x, s.z), (0.0, 0.0));
    }

    /// The store's Podium entry reads and the level builds on it.
    #[test]
    fn the_store_entry_builds_a_level() {
        let Some(store) = dev_store() else { return };
        let Ok(e) = super::super::format::load_level(&store, LevelName::Podium) else {
            eprintln!("no version-2 Podium entry: skipped");
            return;
        };
        let SceneData::Podium(d) = &e.data else { panic!("not a podium") };
        eprintln!("planes {:?}", d.planes.iter().map(|p| &p.name).collect::<Vec<_>>());
        // the montages' notifies are the character's data (PreviewView::notifies), not the entry's
        let mut with = 0;
        for (k, f) in &d.fighters {
            let Ok(v) = PreviewView::new(&store, k, "") else { continue };
            for m in [&f.default.victory, &f.default.defeat] {
                let n = v.notifies(&m.name);
                let classes: std::collections::BTreeSet<_> = n.iter().map(|n| n.cls.as_str()).collect();
                if !n.is_empty() {
                    with += 1;
                }
                eprintln!("{k} {}: {} notifies {classes:?}", m.name, n.len());
            }
        }
        assert!(with > 0, "no podium montage carries a notify");
    }
}
