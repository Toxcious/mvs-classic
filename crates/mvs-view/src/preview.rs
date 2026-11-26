//! A fighter's view with no fighter: the menus' preview actors and the podium's fighters. The view of a skin with its
//! menu materials, its sub-meshes as the pawn starts them, posed from the montage segment the scene plays and its
//! character's preview hook (CharRender.pose's `preview` and `previewStep`), with no World.

use std::sync::Arc;

use mvs_data::CharData;
use mvs_sim::fighter::FighterId;
use mvs_sim::ids::Id;
use mvs_sim::names::Names;

use crate::anim::Clips;
use crate::blend::BlendSpaces;
use crate::chars::{PreviewOf, PreviewStep, render_of};
use crate::frame::{FighterPose, M4};
use crate::math::{self, Mat4};
use crate::mesh::MeshInfo;
use crate::pose::{
    MeshMap, Rec, RecMake, build_rec, meshes_of, preview_mesh_add, preview_mesh_anim, preview_mesh_pose,
    preview_mesh_remove, preview_statics_add,
};
use crate::skins::{self, FighterColour, FighterDraws, PartArmor, Variant};
use crate::view::{Loco, PoseRules, ViewKey};
use crate::{Error, Result};

/// Where the scene's montage is this frame (the segment's clip and time, the actor's
/// facing, the montage and its BlendIn).
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewAt<'a> {
    /// The segment's clip (an AnimSequence name).
    pub clip: &'a str,
    /// Seconds into the clip.
    pub time: f64,
    pub facing_left: bool,
    /// The montage's name (the view's slot key).
    pub montage: &'a str,
    pub blend_in: f64,
}

/// A preview's fighter view.
#[derive(Debug)]
pub struct PreviewView {
    /// The roster key.
    pub key: String,
    /// The SkinData name drawn (the default skin's name for "").
    pub skin: String,
    rec: Rec,
    clips: Clips,
    /// The montage names the view's slot keys take (a preview has no match's names).
    names: Names,
    cs: Vec<Mat4>,
    cs_ue: Vec<Mat4>,
    /// The character's data, its draws and the actor meshes loaded so far (the montage meshes' models).
    cd: Arc<CharData>,
    draws: Arc<FighterDraws>,
    meshes: MeshMap,
    body_info: Arc<MeshInfo>,
    /// The skin's static SubMeshes on sockets (CharRender::socket_statics), component meshes of the view.
    statics: Vec<(&'static str, String)>,
}

/// The id a preview's view carries where a fighter's has its arena id (no fighter).
const NO_FIGHTER: FighterId = Id::new(u32::MAX, 0);

/// The menu set's colour (render/materials: `charMats.set(skin, part, 'menu')`): no Team scalar written, no outline.
const MENU_COLOUR: FighterColour = FighterColour {
    color_index: 0,
    variant: Variant::Menu,
    team: 0.0,
    stencil: 0,
    armor_local: false,
    armor_team: 0.0,
    team_palette: 0,
    focus_palette: 0,
    broker_local: false,
    local_human: false,
    outline: false,
};

impl PreviewView {
    /// The skin's mesh, clips and menu materials from `store`, its parts as the
    /// pawn starts them, its character's preview hook told it has no fighter.
    // Deviation: every clip of the character is loaded: the
    // store's clips are read in place, and a hook's clip is there whatever montage plays.
    pub fn new(store: &mvs_store::Store, key: &str, skin: &str) -> Result<PreviewView> {
        if key.is_empty() {
            return Err(Error::Format { what: "preview".into(), why: "no roster key".into() });
        }
        let fmt = |why: String| Error::Format { what: format!("preview {key}"), why };
        let cd = mvs_data::char_data(store, key).map_err(|e| fmt(e.to_string()))?;
        let render = render_of(key);
        let blend = match render.blend_spaces {
            Some(t) => BlendSpaces::bind(store, key, t)?,
            None => BlendSpaces::default(),
        };
        let mut list: indexmap::IndexMap<&str, &str> =
            cd.fighter.paths.clips.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        for (k, v) in blend.clips() {
            list.entry(k).or_insert(v);
        }
        let mut clips = Clips::load(store, list)?;
        let draws = FighterDraws::shared(store, key)?;
        let pick = (!skin.is_empty()).then_some(skin);
        let sk = skins::skin_of(&cd.fighter, pick)?;
        let body_info = MeshInfo::shared(store, &sk.mesh)?;
        let mut part_infos = Vec::new();
        for p in &sk.parts {
            let path =
                p.path.clone().ok_or_else(|| fmt(format!("part {}: no mesh {} among the paths", p.key, p.mesh)))?;
            part_infos.push(MeshInfo::shared(store, &path)?);
        }
        let body_for_fx = body_info.clone();
        let mut rec = build_rec(
            RecMake {
                fid: NO_FIGHTER,
                id: -1,
                key: key.to_owned(),
                cd: &cd,
                skin: sk,
                colour: MENU_COLOUR,
                body_info,
                part_infos,
                render,
                minion: None,
                second: None,
                // (a preview's pose hook runs slaved)
                slaved: true,
            },
            &mut clips,
            &blend,
            &draws,
        )?;
        // (setOutlinesOn(false): no custom depth)
        rec.outlines_on = false;
        // Comp_Pawn_MeshManager.SetupMeshes and Comp_FighterVisibilityManager.SetupInitialVisibility: a tagged part
        // shown when its component has the PrimaryMesh tag (skinSubMeshes' hidden set), an untagged one as the pawn
        // starts it (partInit)
        // Unverified: on the podium level BP_CharacterPreview runs SetupInitialVisibility after a Delay(0.2) (@8699,
        // @8754; other levels at once), which resets what a hook's toggle wrote before it; here it is applied at once,
        // so a toggle's write in the first 0.2 s stays.
        for k in 0..rec.skin.parts.len() {
            let on = match rec.skin.part_tags[k] {
                Some(_) => rec.skin.part_armor[k] == Some(PartArmor::Primary),
                None => rec.skin.part_init[k],
            };
            rec.view.set_part_visible(k, on);
        }
        let Rec { view, pose, .. } = &mut rec;
        if let Some(p) = pose {
            p.preview(view, &PreviewOf { fighter: &cd.fighter });
        }
        // (SetupMeshes' static SubMeshes on sockets: hidden by SetupInitialVisibility, none has the PrimaryMesh tag)
        let mut meshes = MeshMap::new();
        preview_statics_add(&mut rec, &cd, &draws, &mut meshes, store, &clips, render.socket_statics)?;
        let skin = rec.skin.name.clone();
        Ok(PreviewView {
            key: key.to_owned(),
            skin,
            rec,
            clips,
            names: Names::default(),
            cs: Vec::new(),
            cs_ue: Vec::new(),
            cd,
            draws,
            meshes,
            body_info: body_for_fx,
            statics: render.socket_statics.iter().map(|t| (*t, t.to_lowercase())).collect(),
        })
    }

    /// The preview hook's `previewStep`, then the view posed at `at` after `dt`
    /// seconds.
    pub fn step(&mut self, at: &PreviewAt<'_>, dt: f64) {
        let Rec { view, pose, .. } = &mut self.rec;
        view.place(0.0, 0.0, if at.facing_left { -1.0 } else { 1.0 });
        if let Some(p) = pose.as_deref_mut() {
            let s = PreviewStep { montage: at.montage, anim: at.clip, time: at.time, facing_left: at.facing_left };
            p.preview_step(view, &self.clips, &s);
        }
        let clip = self.clips.get(at.clip);
        if clip.is_none() {
            crate::skin_intro::warn_once(format!("preview: clip not loaded: {} ({})", at.clip, self.key));
        }
        let montage = self.names.intern(at.montage);
        let loco = Loco { dur: 0.0, hermite: false, key: ViewKey::Empty };
        let rules = pose.as_deref_mut().map(|p| p as &mut dyn PoseRules);
        view.pose(&mut self.clips, clip, at.time, Some(montage), at.blend_in, 0.0, dt, false, &loco, None, &[], rules);
        view.present(1.0);
    }

    /// The pose under a montage that blends in, before it plays: the character's anim set idle at `time`, no montage
    /// (the podium's preview between ChangeFighter and its montage's Delay(0.2), BP_CharacterPreview @5837 / @8950).
    /// False: the character's data has no idle clip loaded.
    // Unverified: the preview's AnimClass is the skin's (Comp_Pawn_SkinManager DetermineAnimInstance @145), with no
    // fighter for its graph to read; its pose with no montage is taken as the anim set's idle.
    pub fn step_idle(&mut self, time: f64, facing_left: bool, dt: f64) -> bool {
        let Some(idle) = self.cd.def.anim.idle.as_deref() else { return false };
        let Some(clip) = self.clips.get(idle) else { return false };
        let key = ViewKey::Anim(self.names.intern(idle));
        let view = &mut self.rec.view;
        view.place(0.0, 0.0, if facing_left { -1.0 } else { 1.0 });
        let loco = Loco { dur: 0.0, hermite: false, key };
        view.pose(&mut self.clips, Some(clip), time, None, 0.0, 0.0, dt, false, &loco, None, &[], None);
        view.present(1.0);
        true
    }

    /// The notifies of one of the character's montages, or of the skin's own (its
    /// podium montages the character's lack: the gamedata's skin `montages`).
    // Deviation: the skin's own montages are looked up before the character's, so a skin's own victory or defeat
    // montage runs its notifies (the character's montages alone would run none).
    pub fn notifies(&self, montage: &str) -> Vec<mvs_data::fighter::Notify> {
        let d = &self.cd.fighter;
        let own = d.skins.as_ref().and_then(|m| m.get(&self.skin)).and_then(|l| l.montages.as_ref());
        own.and_then(|m| m.get(montage))
            .or_else(|| d.montages.get(montage))
            .map(|m| m.notifies.clone())
            .unwrap_or_default()
    }

    /// The body mesh's model and the character's skeleton (the effects' rig).
    pub fn body(&self) -> (&Arc<MeshInfo>, &mvs_data::fighter::Skeleton) {
        (&self.body_info, &self.cd.fighter.skeleton)
    }

    /// AnimNotify_SubMeshSpawn: the notify's mesh put on the view. False: the character's
    /// entry has no model for it, or it is on already.
    pub fn add_mesh(&mut self, store: &mvs_store::Store, p: &mvs_data::Props, left: bool) -> Result<bool> {
        let Some(mesh) = p.get("Mesh").and_then(mvs_data::Value::as_str) else { return Ok(false) };
        let name = self.names.intern(mesh);
        preview_mesh_add(
            &mut self.rec,
            &self.cd,
            &self.draws,
            &mut self.meshes,
            store,
            &mut self.clips,
            name,
            mesh,
            p,
            left,
        )
    }

    /// RemoveMontageMesh.
    pub fn remove_mesh(&mut self, mesh: &str) {
        let name = self.names.intern(mesh);
        preview_mesh_remove(&mut self.rec, name);
    }

    /// AnimNotify_SubMeshAnimChange: the mesh plays `anim` (setMontageMeshClip).
    pub fn mesh_anim(&mut self, mesh: &str, anim: Option<&str>) {
        let name = self.names.intern(mesh);
        preview_mesh_anim(&mut self.rec, &self.cd, &mut self.clips, name, anim);
    }

    /// poseMontageMesh: the mesh's animation at `age` seconds.
    pub fn pose_mesh(&mut self, mesh: &str, age: f64, looped: bool) {
        let name = self.names.intern(mesh);
        preview_mesh_pose(&mut self.rec, &mut self.clips, name, age, looped);
    }

    /// The part indices a sub-mesh tag names (in any case): a part's sim tag, else its
    /// SubMeshes key (the tag Comp_Pawn_MeshManager.AddMesh files its component under).
    // Deviation: the SubMeshes key is the fallback tag: the sim tags alone find no part the default skin lacks
    // (C019_S02's EyepatchMesh, which its defeat montage hides).
    pub fn parts_of(&self, tag: &str) -> Vec<usize> {
        let sk = &self.rec.skin;
        (0..sk.part_tags.len())
            .filter(|&k| sk.part_tags[k].as_deref().unwrap_or(&sk.parts[k].key).eq_ignore_ascii_case(tag))
            .collect()
    }

    /// setPartVisible: part `k` shown or hidden (a notify's SetSubmeshVisibility).
    pub fn set_part_visible(&mut self, k: usize, on: bool) {
        if k < self.rec.view.part_on.len() {
            self.rec.view.set_part_visible(k, on);
        }
    }

    /// The posed view's meshes in UE world space: `world` is the mesh component's LocalToWorld (UE's row-vector
    /// matrix: the mesh's relative scale, its yaw of -90 and the actor's place; facing left the reflection across the
    /// actor's Y the preview's root takes). The pose's `fighter` is the caller's to
    /// set (an id for the renderer's buffers).
    pub fn pose(&mut self, world: &M4) -> FighterPose {
        // the static SubMeshes as the AnimBP's toggles leave them (hidden with no hook)
        for (tag, lower) in &self.statics {
            let on = self.rec.pose.as_deref().is_some_and(|p| p.shown(tag, false));
            self.rec.set_component_mesh(lower, on);
        }
        let r = &self.rec;
        r.view.bones_cs(&mut self.cs);
        self.cs_ue.clear();
        self.cs_ue.extend(self.cs.iter().map(math::to_ue));
        let mut meshes = meshes_of(r, &self.cs, &self.cs_ue);
        // The meshes are made under the view's own model matrix M0 (at its root, scale 1); W = world x M0^-1 (column
        // form, UE) takes each to `world`.
        let d: Mat4 = std::array::from_fn(|i| world[i / 4][i % 4]);
        let w = math::mul(&d, &math::invert(&math::to_ue(&r.view.model_world())));
        for m in &mut meshes {
            let mut c = math::IDENTITY;
            for row in 0..3 {
                for col in 0..4 {
                    c[col * 4 + row] = f64::from(m.to_world[row * 4 + col]);
                }
            }
            m.to_world = math::rows3x4(&math::mul(&w, &c));
        }
        FighterPose { fighter: u32::MAX, meshes, priority: None, hidden: false, outline: false, heart: false }
    }

    /// The body mesh as `pose` places it, without the meshes made: its skinning matrices into `bones` (the buffer
    /// kept by the caller) and its LocalToWorld returned (`world`, as FMatrix3x4). The effects' sockets follow these.
    pub fn body_bones(&mut self, world: &M4, bones: &mut Vec<[f32; 12]>) -> [f32; 12] {
        let r = &self.rec;
        r.view.bones_cs(&mut self.cs);
        self.cs_ue.clear();
        self.cs_ue.extend(self.cs.iter().map(math::to_ue));
        let info = &r.body.info;
        bones.clear();
        bones.extend(info.inv_ref.iter().enumerate().map(|(k, inv)| math::rows3x4(&math::mul(&self.cs_ue[k], inv))));
        let d: Mat4 = std::array::from_fn(|i| world[i / 4][i % 4]);
        math::rows3x4(&d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A preview's pose hook is made slaved: Arya's sword has no
    /// AryaSword_AnimBP, so none of its bones is scaled.
    #[test]
    fn an_arya_preview_has_no_sword_scales() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no dev store: skipped");
            return;
        };
        let v = PreviewView::new(&store, "arya", "").unwrap();
        assert!(v.rec.skin.parts.iter().any(|p| p.key == "AryaSword"), "the default skin has the sword part");
        assert!(v.rec.view.part_bone_scale.is_empty(), "{:?}", v.rec.view.part_bone_scale);
    }

    /// Finn's teeth on a preview: sixteen Finn_Tooth components on their sockets, hidden until Finn_AnimBP's toggles
    /// show them on the curves of his victory intro (the podium's).
    #[test]
    fn a_finn_preview_shows_teeth_on_his_victory() {
        const TOOTHS: [&str; 2] = ["toptooth1", "bottooth8"];
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no dev store: skipped");
            return;
        };
        let mut v = PreviewView::new(&store, "finn", "").unwrap();
        assert!(TOOTHS.iter().all(|t| v.rec.has_component_mesh(t)), "the teeth as component meshes");
        let world: M4 = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 }));
        let teeth =
            |v: &mut PreviewView| v.pose(&world).meshes.iter().filter(|m| m.mesh.ends_with("Finn_Tooth")).count();
        assert_eq!(teeth(&mut v), 0, "hidden by SetupInitialVisibility");
        let mut most = 0;
        for k in 0..240 {
            let at = PreviewAt {
                clip: "Finn_Emote_Victory_Intro",
                time: f64::from(k) / 60.0,
                facing_left: false,
                montage: "Finn_Emote_Victory_Montage",
                blend_in: 0.0,
            };
            v.step(&at, 1.0 / 60.0);
            most = most.max(teeth(&mut v));
        }
        assert!(most > 0, "no tooth shown on the victory intro");
    }
}
