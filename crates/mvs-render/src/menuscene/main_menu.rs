//! The main menu's scene: New_MainMenu's camera, the trophy room's background
//! plane, PM_NewSlants' two planes, the Player1 preview and BP_CharacterPreview's glow shell; the menus' frame (the
//! levels' camera, projection and draw).
//!
//! The planes and the glow shell draw with the build's cooked materials (the menuscene kind's material references).

use std::sync::Arc;
use std::time::Instant;

use indexmap::IndexMap;
use mvs_view::camera::{look_at, perspective, turned_axes, view_of_axes};
use mvs_view::{Camera, FrameInput, FrameTime, MeshPose};

use super::format::{Glow, GlowKey, MainMenuScene, MenuSceneEntry, SceneData, ScenePlane, material_ref};
use super::{LevelCx, LevelScene, MenuFrame};
use crate::gpu::{Gpu, Texture};
use crate::material::LightMapScene;
use crate::renderer::{Owner, ViewOf};
use crate::stage::{to_world, transform};
use crate::{Assets, Error, FrameTimes, Renderer, Result};

/// The box the menus' screens are laid out in (UI_Functions.WorldLocationToScreenPosition's canvas units).
const CANVAS: [f64; 2] = [2560.0, 1440.0];
/// The menu camera's near and far planes, metres (a 30 degree camera, 16 / 9,
/// 0.1, 1000).
const NEAR: f64 = 0.1;
const FAR: f64 = 1000.0;
/// Lighting channel 0 (FLightingChannels' default): the level's meshes and every preview.
const CHANNEL_0: u8 = 1;

/// The main menu's scene (its parts and its character).
#[derive(Debug)]
pub struct MainMenuLevel {
    pub data: MainMenuScene,
    /// MainMenuBackground's plane.
    background: MeshPose,
    /// PM_NewSlants' planes with their TranslucencySortPriority.
    slants: Vec<(MeshPose, i32)>,
}

impl MainMenuLevel {
    /// The background and the slants on the engine Plane,
    /// placed once (the camera and the planes do not move on the main menu). The background draws the material
    /// UpdateBGForEvents picks for `active_events` at BeginPlay (MainMenuBackground @697 -> @777). The camera's
    /// vignette is set by `Renderer::set_menu_level`; the Player1 preview is the preview set's slot 0.
    pub fn new(_gpu: &Gpu, _assets: &Assets, entry: MenuSceneEntry, active_events: &[String]) -> Result<MainMenuLevel> {
        let SceneData::MainMenu(data) = entry.data else {
            return Err(Error::Format { what: entry.name, why: "not the MainMenu entry".into() });
        };
        let plane: Arc<str> = data.plane.as_str().into();
        let background = background_pose(&plane, &data, active_events);
        // Back to front within the priority by the view depth of the plane's origin (r.TranslucentSortPolicy 1,
        // SortByProjectedZ): the translucent pass's own order (translucency.rs).
        let slants = data.slants.iter().map(|s| (plane_pose(&plane, s), s.sort.unwrap_or(0))).collect();
        Ok(MainMenuLevel { data: *data, background, slants })
    }

    /// The background's material picked again for `active_events`, on the open level.
    // Deviation: the build picks it once, at the background's BeginPlay; the port's own background choice
    // (mvs-settings `UiSettings::menu_background`) changes it while the main menu is up.
    pub fn set_background(&mut self, active_events: &[String]) {
        let plane: Arc<str> = self.data.plane.as_str().into();
        self.background = background_pose(&plane, &self.data, active_events);
    }

    /// The main menu has no actor of its own to tick; Player1's
    /// actor and the effects tick in `Renderer::menu_tick`.
    pub fn tick(&mut self, _cx: &mut LevelCx<'_>, _dt: f64) -> Result<()> {
        Ok(())
    }

    /// The camera for an output of `size`.
    pub fn camera(&self, size: [u32; 2]) -> Camera {
        let c = &self.data.camera;
        menu_camera(c.loc, c.rot, c.fov, c.aspect, size)
    }

    /// The camera for the frame's size, the background and the slants into the
    /// frame.
    pub fn frame(&mut self, out: &mut MenuFrame) {
        out.camera = Some(self.camera(out.size));
        out.meshes.push((self.background.clone(), 0));
        out.meshes.extend(self.slants.iter().cloned());
    }
}

/// MainMenuBackground's plane with the material UpdateBGForEvents picks for `active_events`.
fn background_pose(plane: &Arc<str>, data: &MainMenuScene, active_events: &[String]) -> MeshPose {
    let bg = ScenePlane { material: data.background_material(active_events).clone(), ..data.background.clone() };
    plane_pose(plane, &bg)
}

/// A plane of the scene: the engine Plane at the plane's transform with its material.
fn plane_pose(plane: &Arc<str>, p: &ScenePlane) -> MeshPose {
    MeshPose {
        mesh: plane.clone(),
        to_world: to_world(&transform(p.loc, p.rot, p.scale)),
        materials: vec![material_ref(&p.material, None)],
        ..MeshPose::default()
    }
}

/// A level's camera (render's lens): the CineCamera's actor transform (UE cm, [pitch,
/// yaw, roll]; the roll is 0 in the data and is not applied) looking along the rotator's forward vector with +Z up,
/// its horizontal field of view `fov` (degrees) kept whole for the `aspect` frame, on an output of `size` pixels.
// Deviation: the build letterboxes to 16:9; here the 16:9 frame is kept whole and the view grows past it on the
// longer side of the window (the 16:9 frame's vertical field of view on a wider window, its horizontal one on a
// taller one).
pub fn menu_camera(loc: [f64; 3], rot: [f64; 3], fov: f64, aspect: f64, size: [u32; 2]) -> Camera {
    menu_camera_turned(loc, rot, [0.0; 3], fov, aspect, size)
}

/// `menu_camera` turned by a camera shake's rotation `turn` (pitch, yaw, roll, degrees) in the camera's own space
/// (ApplyPlaySpace CameraLocal); a zero turn is `menu_camera`'s view.
pub fn menu_camera_turned(
    loc: [f64; 3],
    rot: [f64; 3],
    turn: [f64; 3],
    fov: f64,
    aspect: f64,
    size: [u32; 2],
) -> Camera {
    let (w, h) = (f64::from(size[0].max(1)), f64::from(size[1].max(1)));
    let tan_h = (fov * std::f64::consts::PI / 360.0).tan();
    let vfov = (tan_h / aspect.min(w / h)).atan() * 360.0 / std::f64::consts::PI;
    let translated_world_to_view = if turn == [0.0; 3] {
        let (p, y) = (rot[0].to_radians(), rot[1].to_radians());
        let front = [p.cos() * y.cos(), p.cos() * y.sin(), p.sin()];
        look_at(loc, [0, 1, 2].map(|i| loc[i] + front[i] * 100.0))
    } else {
        view_of_axes(turned_axes([rot[0], rot[1], 0.0], turn))
    };
    Camera { origin: loc, translated_world_to_view, view_to_clip: perspective(vfov, w / h, NEAR, FAR) }
}

/// FRichCurve::Eval for keys without tangent weights and constant extrapolation: the first
/// key's interpolation mode decides a segment; a cubic one is the engine's Bezier form (control points a third of
/// the key interval along the leave and arrive tangents). No key: 0.
pub(crate) fn rich_curve(keys: &[GlowKey], t: f64) -> f64 {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else { return 0.0 };
    if t <= first.t {
        return first.v;
    }
    if t >= last.t {
        return last.v;
    }
    let mut i = 0;
    while keys[i + 1].t <= t {
        i += 1;
    }
    let (a, b) = (&keys[i], &keys[i + 1]);
    if a.mode == "Constant" {
        return a.v;
    }
    let diff = b.t - a.t;
    let x = (t - a.t) / diff;
    let (p0, p3) = (a.v, b.v);
    if a.mode == "Linear" {
        return p0 + (p3 - p0) * x;
    }
    let p1 = p0 + (a.leave * diff) / 3.0;
    let p2 = p3 - (b.arrive * diff) / 3.0;
    let p01 = p0 + (p1 - p0) * x;
    let p12 = p1 + (p2 - p1) * x;
    let p23 = p2 + (p3 - p2) * x;
    let p012 = p01 + (p12 - p01) * x;
    let p123 = p12 + (p23 - p12) * x;
    p012 + (p123 - p012) * x
}

/// BP_CharacterPreview's glow shell on a preview's view (GlowShell):
/// CharacterDissolve_MAT on duplicates of the view's meshes, driven by the Dissolve or Reverse Dissolve timeline.
/// GlowVFX(Reverse) (@29444): SetupGlowVFXMeshes, then Dissolve.PlayFromStart (Reverse Dissolve stopped) or, with
/// Reverse, Reverse Dissolve.PlayFromStart (Dissolve stopped); both timelines' end (@21887) hides the duplicates.
// Unverified: the vertex colour alpha is taken as 1: the exported meshes carry no colours, read as a cooked mesh
// without a colour buffer, which draws with the white null buffer (4.26 source; the cooked buffer was not read).
// Deviation: the shell draws before the spawn effect's particles (TranslucencySortPriority 0: the translucent pass
// before the particles'); the build sorts all of them by the view depth of their bounds.
// Unverified: the timeline is advanced in the tick that starts it (the component ticks after its actor's latent
// actions; 4.26 source).
#[derive(Debug)]
pub struct GlowShell {
    /// Seconds into the timeline; negative when not playing.
    pub t: f64,
    pub reverse: bool,
    pub shown: bool,
    /// The timelines' values on the instances' scalars (Dissolve, Fresnel_Base, Emissive_Mult), over the material's.
    values: IndexMap<String, f64>,
}

impl GlowShell {
    /// Not playing, hidden; every instance of every duplicate takes the same
    /// values.
    pub fn new(_glow: &Glow) -> GlowShell {
        GlowShell { t: -1.0, reverse: false, shown: false, values: IndexMap::new() }
    }

    /// Its tracks at 0, the
    /// duplicates shown (each takes its source's visibility: `meshes` copies the meshes the view draws).
    pub fn start(&mut self, glow: &Glow, reverse: bool) {
        self.reverse = reverse;
        self.t = 0.0;
        self.set(glow, 0.0);
        self.shown = true;
    }

    /// The timeline's tick; its finish hides the duplicates. True in the tick it ends.
    pub fn tick(&mut self, glow: &Glow, dt: f64) -> bool {
        if self.t < 0.0 {
            return false;
        }
        self.t += dt;
        let length = if self.reverse { glow.reverse.length } else { glow.length };
        if self.t >= length {
            self.t = -1.0;
            self.shown = false;
            return true;
        }
        self.set(glow, self.t);
        false
    }

    /// Visibility.
    pub fn visible(&mut self, on: bool) {
        self.shown = on;
    }

    /// A timeline's tracks at `t` on the scalars (Reverse Dissolve writes Dissolve
    /// and Fresnel_Base alone, @23942: Emissive_Mult keeps its last value).
    fn set(&mut self, glow: &Glow, t: f64) {
        let tracks = if self.reverse { &glow.reverse.tracks } else { &glow.material.tracks };
        for tr in tracks {
            self.values.insert(tr.param.clone(), rich_curve(&tr.keys, t));
        }
    }

    /// The duplicates of `view`'s meshes (the ones the view draws, posed as
    /// they are) with the dissolve material at the timeline's place in every slot, into `out` with sort priority 0;
    /// nothing when not shown.
    pub fn meshes(&self, glow: &Glow, view: &[MeshPose], out: &mut Vec<(MeshPose, i32)>) {
        if !self.shown {
            return;
        }
        let m = material_ref(&glow.material, Some(&self.values));
        for p in view {
            let materials = vec![m.clone(); p.materials.len().max(1)];
            out.push((MeshPose { materials, custom_stencil: 0, ..p.clone() }, 0));
        }
    }
}

/// `at` (UE world)
/// projected with `camera` on an output of `output` pixels, in canvas units of the 2560 x 1440 box centred in it
/// (the menus' layout box); None outside the depth range.
pub(crate) fn project(camera: &Camera, at: [f64; 3], output: [u32; 2]) -> Option<[f64; 2]> {
    let [px, py] = project_pixels(camera, at, output)?;
    let (w, h) = (f64::from(output[0].max(1)), f64::from(output[1].max(1)));
    let s = (w / CANVAS[0]).min(h / CANVAS[1]);
    Some([(px - w / 2.0) / s + CANVAS[0] / 2.0, (py - h / 2.0) / s + CANVAS[1] / 2.0])
}

/// `at` (UE world) projected with `camera` on an output of `output` pixels: pixels from its top left, unrounded;
/// None outside the depth range.
pub(crate) fn project_pixels(camera: &Camera, at: [f64; 3], output: [u32; 2]) -> Option<[f64; 2]> {
    let v = [0, 1, 2].map(|i| at[i] - camera.origin[i]);
    let m = &camera.translated_world_to_view;
    let mut view = [0.0, 0.0, 0.0, 1.0];
    for (j, x) in view.iter_mut().enumerate().take(3) {
        *x = v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j] + m[3][j];
    }
    let p = &camera.view_to_clip;
    let clip: [f64; 4] = [0, 1, 2, 3].map(|j| (0..4).map(|k| view[k] * p[k][j]).sum());
    let [x, y, z] = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
    if !(-1.0..=1.0).contains(&z) {
        return None;
    }
    let (fx, fy) = ((x + 1.0) / 2.0, (1.0 - y) / 2.0);
    let (w, h) = (f64::from(output[0].max(1)), f64::from(output[1].max(1)));
    Some([fx * w, fy * h])
}

/// The shown level's MenuFrame (its `frame`, the previews') drawn through
/// the renderer's passes and post (the fade and the vignette), the picture to `out`. The level's meshes and the
/// previews are on lighting channel 0, each preview on its rig's channel as well; the custom depth meshes draw into
/// custom depth alone.
// Not ported: the CineCamera's depth of field.
pub(crate) fn draw(r: &mut Renderer, vfx: Option<&mut mvs_vfx::Player>, out: Option<&Texture>) -> Result<FrameTimes> {
    let start = Instant::now();
    let mut frame = MenuFrame { size: r.output, ..MenuFrame::default() };
    match r.menu.scene.as_mut() {
        Some(LevelScene::MainMenu(l)) => l.frame(&mut frame),
        Some(LevelScene::PreMatch(l)) => l.frame(&mut frame),
        Some(LevelScene::Podium(l)) => l.frame(&mut frame),
        None => return Err(Error::Format { what: "menu frame".into(), why: "no level shown".into() }),
    }
    r.menu.previews.frame(&mut frame);
    let Some(camera) = frame.camera.clone() else {
        return Err(Error::Format { what: "menu frame".into(), why: "the level set no camera".into() });
    };
    r.menu.camera = Some(camera.clone());
    r.menu.quads = std::mem::take(&mut frame.widgets);
    r.menu.set_lights(&frame);
    r.clear_items();
    // (the menus' levels have a volumetric lightmap whatever the stage's)
    let stage_scene = std::mem::replace(&mut r.lightmap_scene, LightMapScene::STAGE);
    let added = add_items(r, &frame);
    r.lightmap_scene = stage_scene;
    added?;
    let time = FrameTime { game: r.menu.time as f32, real: r.menu.time as f32, delta: r.menu.last_dt as f32 };
    let f = FrameInput { tick: 0, alpha: 1.0, time, camera: &camera, fighters: &[], actors: &[], events: &[] };
    r.draw_items(&f, vfx, out, start, ViewOf::Menu)
}

/// The frame's meshes, custom depth meshes and previews into the renderer's draw list.
fn add_items(r: &mut Renderer, frame: &MenuFrame) -> Result<()> {
    for (k, (pose, sort)) in frame.meshes.iter().enumerate() {
        let from = r.list.translucent.len();
        r.add_pose(Owner::MenuMesh(k as u32), pose, None, CHANNEL_0, false, None)?;
        for it in &mut r.list.translucent[from..] {
            it.sort_key = *sort as f32;
        }
    }
    let base = frame.meshes.len();
    for (k, pose) in frame.custom_depth.iter().enumerate() {
        r.add_custom_depth_pose(Owner::MenuMesh((base + k) as u32), pose)?;
    }
    for (fp, channel) in frame.fighters.iter().filter(|(fp, _)| !fp.hidden) {
        let from = r.list.opaque.len();
        for (k, p) in fp.meshes.iter().enumerate() {
            r.add_pose(Owner::MenuFighter(fp.fighter, k as u32), p, None, CHANNEL_0 | (1 << channel), false, None)?;
        }
        r.cast_shadows_from(from);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::format::{GlowTimeline, ParamTrack, SceneMaterial};
    use super::*;

    fn key(t: f64, v: f64, mode: &str, arrive: f64, leave: f64) -> GlowKey {
        GlowKey { t, v, mode: mode.into(), arrive, leave }
    }

    #[test]
    fn rich_curve_segments() {
        let keys =
            [key(0.0, 1.0, "Linear", 0.0, 0.0), key(1.0, 3.0, "Constant", 0.0, 0.0), key(2.0, 0.0, "Cubic", 0.0, 0.0)];
        assert_eq!(rich_curve(&keys, -1.0), 1.0);
        assert_eq!(rich_curve(&keys, 0.5), 2.0);
        assert_eq!(rich_curve(&keys, 1.5), 3.0);
        assert_eq!(rich_curve(&keys, 5.0), 0.0);
        // a flat cubic segment: the Bezier's middle half way, its first quarter below the line
        let cubic = [key(0.0, 0.0, "Cubic", 0.0, 0.0), key(2.0, 4.0, "Cubic", 0.0, 0.0)];
        assert!((rich_curve(&cubic, 1.0) - 2.0).abs() < 1e-12);
        assert!(rich_curve(&cubic, 0.5) < 1.0);
        // tangents of the line's slope: the line itself
        let line = [key(0.0, 0.0, "Cubic", 2.0, 2.0), key(2.0, 4.0, "Cubic", 2.0, 2.0)];
        assert!((rich_curve(&line, 0.5) - 1.0).abs() < 1e-12);
        assert_eq!(rich_curve(&[], 1.0), 0.0);
    }

    #[test]
    fn camera_projects_into_the_box() {
        // a lens at the origin looking along +X (yaw 0), a 16:9 output: a point ahead lands at the box's centre
        let c = menu_camera([0.0; 3], [0.0; 3], 30.0, 16.0 / 9.0, [1920, 1080]);
        let p = project(&c, [500.0, 0.0, 0.0], [1920, 1080]).unwrap();
        assert!((p[0] - 1280.0).abs() < 1e-9 && (p[1] - 720.0).abs() < 1e-9, "{p:?}");
        // UE +Y is to the right of a lens looking along +X with +Z up
        let r = project(&c, [500.0, 50.0, 0.0], [1920, 1080]).unwrap();
        let u = project(&c, [500.0, 0.0, 50.0], [1920, 1080]).unwrap();
        assert!(r[0] > 1280.0 && u[1] < 720.0, "{r:?} {u:?}");
        // the horizontal field of view is the lens's on a 16:9 output: the frame's edge at tan(15 deg) x depth
        let edge = project(&c, [1000.0, 1000.0 * 15f64.to_radians().tan(), 0.0], [1920, 1080]).unwrap();
        assert!((edge[0] - 2560.0).abs() < 1e-6, "{edge:?}");
        // just behind the lens: off the depth range
        assert!(project(&c, [-5.0, 0.0, 0.0], [1920, 1080]).is_none());
    }

    #[test]
    fn camera_keeps_the_frame() {
        // 4:3: the 16:9 frame's horizontal field of view kept, its edge at the box's edge
        let c = menu_camera([0.0; 3], [0.0; 3], 30.0, 16.0 / 9.0, [1600, 1200]);
        let edge = project(&c, [1000.0, 1000.0 * 15f64.to_radians().tan(), 0.0], [1600, 1200]).unwrap();
        assert!((edge[0] - 2560.0).abs() < 1e-6, "{edge:?}");
        // 21:9: the 16:9 frame's vertical field of view kept, its top at the box's top
        let c = menu_camera([0.0; 3], [0.0; 3], 30.0, 16.0 / 9.0, [2520, 1080]);
        let tan_v = 15f64.to_radians().tan() * 9.0 / 16.0;
        let top = project(&c, [1000.0, 0.0, 1000.0 * tan_v], [2520, 1080]).unwrap();
        assert!(top[1].abs() < 1e-6, "{top:?}");
    }

    #[test]
    fn glow_timelines() {
        let track = |param: &str, keys: Vec<GlowKey>| ParamTrack { param: param.into(), keys };
        let glow = Glow {
            length: 1.25,
            reverse: GlowTimeline {
                length: 0.5,
                tracks: vec![track(
                    "Dissolve",
                    vec![key(0.0, 1.0, "Linear", 0.0, 0.0), key(0.5, 0.0, "Linear", 0.0, 0.0)],
                )],
            },
            material: SceneMaterial {
                tracks: vec![
                    track("Dissolve", vec![key(0.0, 0.0, "Linear", 0.0, 0.0), key(1.25, 1.0, "Linear", 0.0, 0.0)]),
                    track("Emissive_Mult", vec![key(0.0, 5.0, "Constant", 0.0, 0.0)]),
                ],
                ..SceneMaterial::default()
            },
        };
        let mut g = GlowShell::new(&glow);
        assert!(!g.tick(&glow, 0.1), "not playing");
        g.start(&glow, false);
        assert!(g.shown && g.values["Dissolve"] == 0.0 && g.values["Emissive_Mult"] == 5.0);
        assert!(!g.tick(&glow, 0.625));
        assert!((g.values["Dissolve"] - 0.5).abs() < 1e-12);
        assert!(g.tick(&glow, 0.625), "ends at the length");
        assert!(!g.shown && g.t < 0.0);
        // Reverse Dissolve writes Dissolve alone: Emissive_Mult keeps its value
        g.values.insert("Emissive_Mult".into(), 7.0);
        g.start(&glow, true);
        assert_eq!(g.values["Dissolve"], 1.0);
        assert_eq!(g.values["Emissive_Mult"], 7.0);
        assert!(g.tick(&glow, 0.5));
    }
}
