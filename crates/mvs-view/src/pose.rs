//! The views' posing (placing and presenting the views, their materials and the events' material part): each fighter's
//! meshes posed from the World's animation state, with their materials and parameters, as UE's component transforms and
//! skinning matrices.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use mvs_data::Value;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::{instance_data, slot_uid};
use mvs_sim::fighter::anim::{AnimKey, MontageSlot};
use mvs_sim::names::Name;
use mvs_sim::{Event, World};

use crate::actors::{ActorHost, ActorViews, FighterBones, StaticModel};
use crate::anim::{Clips, HeadScaleStep, Rig};
use crate::blend::BlendSpaces;
use crate::chars::{self, CharRender, MinionView, PawnActor, PawnCx, PoseHook, PoseOf, SANDWICH, render_of};
use crate::frame::{ActorPose, FighterPose, MaterialRef, MeshPose, SecondKind};
use crate::heart_skin;
use crate::items::ItemAssets;
use crate::items::snowball_head::Heads;
use crate::math::{self, Mat4};
use crate::mesh::MeshInfo;
use crate::montage_mesh::{SocketSkin, mesh_clip, own_bone_local};
use crate::second::Seconds;
use crate::skin_intro::{SkinIntro, warn_once};
use crate::skins::{self, FighterColour, FighterDraws, Skin};
use crate::view::{CharView, Loco, MixArg, PropAttach, ViewKey};
use crate::{Error, Result};

mod warm;

/// The presentation step.
const STEP: f64 = 1.0 / 60.0;
/// PFGCamera's priority captures' render targets (Priority1..3), named as runtime textures: no object path.
pub const PRIORITY_CAPTURES: [&str; 3] = ["<PriorityCapture1>", "<PriorityCapture2>", "<PriorityCapture3>"];

pub type MeshMap = HashMap<String, Arc<MeshInfo>>;

// ---- a pawn's view ----------------------------------------------------------------------------------------------

/// Where a fighter's mesh was drawn and is.
#[derive(Clone, Copy, Debug)]
struct ShownAt {
    px: f64,
    py: f64,
    x: f64,
    y: f64,
    raw_x: f64,
    raw_y: f64,
    facing: f64,
    /// The HUD anchors' depth offset: the playerIndex along the actor's local Y, cm.
    d: f64,
}

/// A skinned mesh of a view and its slots' materials (None: no material built; a main mesh section without one is
/// left out of the picture, a part's has no mesh).
#[derive(Debug)]
pub(crate) struct ViewMesh {
    pub(crate) info: Arc<MeshInfo>,
    pub(crate) mats: Vec<Option<MaterialRef>>,
}

#[derive(Debug)]
struct PropMesh {
    info: Arc<MeshInfo>,
    mats: Vec<MaterialRef>,
}

/// A mesh a montage's AnimNotify_SubMeshSpawn put on the view (CharacterView.addMontageMesh): its holder's view bone
/// (None: the model) and matrix under it, fixed at its first show; a skinned one's own skeleton and the animation it
/// plays (montageAnims).
#[derive(Debug)]
struct MontageView {
    name: Name,
    info: Arc<MeshInfo>,
    mats: Vec<Option<MaterialRef>>,
    bone: Option<usize>,
    matrix: Mat4,
    skin: Option<SocketSkin>,
    anim: Name,
    /// A pawn without a mirror node (montageOwn): the matrix under CharacterMesh0's bone; the holder's is written from
    /// it each tick (ownBoneLocal facing left).
    own: Option<Mat4>,
    /// The view's shown bones in component space (scratch of the own case).
    cs: Vec<Mat4>,
}

/// Where a component mesh hangs (montageMeshAttach's result): the view bone (None: the model) and the matrix under
/// it in the view's space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ComponentAttach {
    pub(crate) bone: Option<usize>,
    pub(crate) matrix: Mat4,
}

/// A pawn's own mesh component on a socket of the main mesh (CharacterView.addComponentMesh): an SCS child of
/// CharacterMesh0 with a mesh of its own, keyed by its component tag in lower case (Comp_Pawn_MeshManager.AddMesh),
/// hidden until set_component_mesh shows it. A skeletal one has its own skeleton and plays what AnimNotify_Equip
/// gives it (play_component_mesh), or is master-posed on the view's bones.
#[derive(Debug)]
struct ComponentView {
    tag: String,
    info: Arc<MeshInfo>,
    mats: Vec<Option<MaterialRef>>,
    /// The skin part whose materials it draws with (`host.partMaterials`: the part's own instances, so what is set on
    /// them later, the flash and evade scalars, shows on it too); None: `mats`.
    part: Option<usize>,
    /// Facing right, facing left (K2_AttachToComponent again on the mirrored pose at each turn: the facing the view
    /// is drawn at picks one).
    at: [ComponentAttach; 2],
    /// Its own skeleton posed; None: a static mesh or a master-posed one.
    skin: Option<SocketSkin>,
    /// A master-posed one: per bone of its own skeleton, the view's bone it is drawn on.
    master: Option<Vec<usize>>,
    on: bool,
    /// The animation's position and looping (tickComponentMeshes).
    time: f64,
    looped: bool,
}

/// A part bone's draw bone (setPropMirror): its partner's main mesh bone and its BoneFlipAxis scale; None: none.
type DrawBone = Option<(usize, [f64; 3])>;

/// A minion view's own state (chars::MinionView): its entry, its creator and the minion it shows.
#[derive(Debug)]
pub(crate) struct MinionRec {
    def: &'static MinionView,
    creator: FighterId,
    /// The minion shown (None: none lives; the view hidden).
    live: Option<FighterId>,
}

/// BaseFighter's CDO stencil values a minion's SetStencilValue picks by its ForcedColorIndex: EnemyOutline (0),
/// Ally Outline (1) (Pawn_StevenUniverse @6018, Pawn_VelmaAIBotPolice SetOutline @15-314).
const MINION_STENCIL: [u8; 2] = [3, 2];

/// One pawn's view and its match state (the view, its skin, colours, place and the
/// events' clocks).
#[derive(Debug)]
pub(crate) struct Rec {
    pub(crate) fid: FighterId,
    pub(crate) id: i32,
    pub(crate) key: String,
    pub(crate) skin: Skin,
    pub(crate) view: CharView,
    pub(crate) colour: FighterColour,
    pub(crate) body: ViewMesh,
    parts: Vec<ViewMesh>,
    /// Per part, per bone of its own reference skeleton: the main mesh's bone it follows (a master pose component).
    part_bones: Vec<Vec<usize>>,
    /// Per part, per bone of its own reference skeleton: the draw bone of setPropMirror (its partner's main mesh bone
    /// and its BoneFlipAxis scale) the part is drawn on facing left; empty: none.
    part_mirror: Vec<Vec<DrawBone>>,
    /// Per part, its bones (own skeleton) on a draw bone under the followed bone, with the draw bone's scale
    /// (CharView::part_bone_scale); empty: none.
    part_scale: Vec<Vec<(usize, [f64; 3])>>,
    /// Per main mesh slot: an instance the skin overrides (the hit flash's).
    flash: Vec<bool>,
    pub(crate) armor: MaterialRef,
    pub(crate) armor_on: bool,
    /// A Disguise's copies (DisguiseArmorSubmeshes): the main mesh slots that get one; a part's is PrimaryMesh's
    /// alone, shown by the count whatever its part shows. None: a fighter's own (BaseFighter UpdateNewSkelmeshes).
    pub(crate) armor_main: Option<Vec<bool>>,
    props: Vec<PropMesh>,
    montage: Vec<MontageView>,
    /// The pawn's own mesh components (addComponentMesh), in the order they were added.
    components: Vec<ComponentView>,
    pub(crate) visible: bool,
    pub(crate) outlines_on: bool,
    shown_at: Option<ShownAt>,
    flash_left: f64,
    intro_glow: f64,
    rage_shirt: Option<usize>,
    /// The character's AnimBP nodes on this view (CharRender.pose).
    pub(crate) pose: Option<Box<dyn PoseHook>>,
    /// Its pawn actors (CharRender.pawn_actors), made at the first present.
    pawn: Option<Vec<Box<dyn PawnActor>>>,
    /// The priority stack index SetOpacityMaskTextures last gave it (FighterPose.priority).
    priority: Option<u8>,
    /// Its character's pawn components' own state between ticks (CharRender.components: `make` lays it out, `place`
    /// keeps it).
    pub(crate) own: Vec<f64>,
    /// A minion's view (None: a pawn of the match's).
    minion: Option<MinionRec>,
    /// The heart skin's opacity scalar state (heart_skin.rs); None: no heart skin.
    pub(crate) heart: Option<HeartState>,
    /// The main mesh's active morph targets (SetMorphTarget): (name, weight), in the order set.
    morphs: Vec<(Arc<str>, f32)>,
    /// Its skin's own intro over the sim's default one (skin_intro.rs; None: the default's is drawn) and the intro's
    /// part states this tick (-1: not set).
    intro: Option<SkinIntro>,
    intro_vis: Vec<i8>,
    /// The slot mix of the tick (`place_view`'s pose() mix), kept from tick to tick.
    mix: Vec<MixArg>,
}

/// HeartSkin_SetOpacity's view side: the rule, slot 0's Min and Max, the materials it writes and the state the scalar
/// was last set for (None: not yet).
#[derive(Debug)]
pub(crate) struct HeartState {
    rule: &'static heart_skin::HeartRule,
    min: f64,
    max: f64,
    /// The targets: None the main mesh's skin instances, Some(k) part k's materials.
    targets: Vec<Option<usize>>,
    moving: Option<bool>,
}

pub(crate) fn material(path: &str) -> MaterialRef {
    MaterialRef { path: Arc::from(path), ..MaterialRef::default() }
}

/// A prop's or static model's slot materials: the entry's, else the mesh slot's own.
fn prop_materials(info: &MeshInfo, mats: Vec<Option<String>>) -> Vec<MaterialRef> {
    info.slots
        .iter()
        .enumerate()
        .map(|(k, s)| match mats.get(k).cloned().flatten().or_else(|| s.material.clone()) {
            Some(p) => material(&p),
            None => MaterialRef::default(),
        })
        .collect()
}

pub(crate) fn set_texture(m: &mut MaterialRef, name: &str, path: &str) {
    match m.textures.iter_mut().find(|(n, _)| &**n == name) {
        Some(t) => t.1 = Arc::from(path),
        None => m.textures.push((Arc::from(name), Arc::from(path))),
    }
}

pub(crate) fn set_scalar(m: &mut MaterialRef, name: &str, v: f64) {
    let v = v as f32;
    match m.scalars.iter_mut().find(|(n, _)| &**n == name) {
        Some(s) => s.1 = v,
        None => m.scalars.push((Arc::from(name), v)),
    }
}

pub(crate) fn set_vector(m: &mut MaterialRef, name: &str, v: [f64; 4]) {
    let v = v.map(|x| x as f32);
    match m.vectors.iter_mut().find(|(n, _)| &**n == name) {
        Some(s) => s.1 = v,
        None => m.vectors.push((Arc::from(name), v)),
    }
}

impl Rec {
    /// HeartSkin_Initialize's reads: slot 0's instance when the skin overrides that slot,
    /// its Max and Min, and the materials of the rule's components. None: no heart skin, no rule for the character, or
    /// Max 0 (ReceiveTick's gate).
    fn heart_state(&self, draws: &FighterDraws, d: &mvs_data::fighter::FighterData, key: &str) -> Option<HeartState> {
        let path = if self.flash.first().copied().unwrap_or(false) {
            &self.body.mats.first()?.as_ref()?.path
        } else {
            return None;
        };
        let max = draws.material_scalar(path, heart_skin::MAX)?;
        let Some(rule) = heart_skin::rule_of(key) else {
            eprintln!("warning: heart skin {}: no HeartSkin entry for {key} (heart_skin.rs RULES)", self.skin.name);
            return None;
        };
        if max == 0.0 {
            return None;
        }
        let mut targets = Vec::new();
        for c in rule.components {
            let t = if *c == "Mesh" {
                None
            } else {
                let Some(tag) = heart_skin::component_tag(d, c) else {
                    eprintln!("warning: heart skin: {key} has no component {c} (heart_skin.rs RULES)");
                    continue;
                };
                // (a component the skin has no entry of keeps its template mesh and its own materials: not a heart material)
                let Some(k) = heart_skin::part_of(&self.skin, &tag) else { continue };
                Some(k)
            };
            if !targets.contains(&t) {
                targets.push(t);
            }
        }
        let min = draws.material_scalar(path, heart_skin::MIN).unwrap_or(0.0);
        Some(HeartState { rule, min, max, targets, moving: None })
    }

    /// HeartSkin_SetOpacity (the scalar): once a presentation tick, set only when the state changes.
    fn place_heart(&mut self, w: &World) {
        let Some(h) = &mut self.heart else { return };
        let mv = heart_skin::moving(w, self.fid, h.rule);
        if h.moving == Some(mv) {
            return;
        }
        h.moving = Some(mv);
        let v = if mv { h.min } else { h.max };
        let targets = h.targets.clone();
        for t in targets {
            match t {
                None => self.each_flash(|m| set_scalar(m, heart_skin::MAX, v)),
                Some(k) => {
                    for m in self.parts[k].mats.iter_mut().flatten() {
                        set_scalar(m, heart_skin::MAX, v);
                    }
                }
            }
        }
    }

    /// `addProp` with its materials: an actor entry's (mesh, slots), else the sandwich with its own slot materials.
    // Deviation: the sandwich props are drawn with the Sandwich actor entry's material (Sandwich_MAT), not a material
    // of its own on the Sandwitch texture.
    pub(crate) fn add_prop(
        &mut self,
        tag: &'static str,
        meshes: &MeshMap,
        drawn: Option<(String, Vec<Option<String>>)>,
        at: &PropAttach<'_>,
    ) {
        let (mesh, mats) = drawn.unwrap_or_else(|| (SANDWICH.to_owned(), Vec::new()));
        let info = meshes.get(&mesh).cloned().unwrap_or_else(|| panic!("prop mesh {mesh} not loaded"));
        let mats = prop_materials(&info, mats);
        self.view.add_prop(tag, at);
        self.props.push(PropMesh { info, mats });
    }

    /// `montageMeshAttach` for a pawn component's SCS attach (the build's socket `socket`, the component's relative
    /// location, rotation and scale as the notify's Offset, no mirror axes) facing `left` or right.
    // Unverified: a pawn without a mirror node (C017) would need ownBoneLocal each tick; no component of the port's
    // characters is on one (warned).
    #[allow(dead_code, reason = "the characters' component modules call it")]
    pub(crate) fn component_attach(
        &self,
        w: &World,
        socket: &str,
        loc: [f64; 3],
        quat: [f64; 4],
        scale: [f64; 3],
        left: bool,
    ) -> ComponentAttach {
        self.component_attach_on(&w.data.chars[&self.key].fighter.skeleton, socket, loc, quat, scale, left)
    }

    /// `component_attach` on the character's skeleton `skel` (a view with no World: the preview's).
    pub(crate) fn component_attach_on(
        &self,
        skel: &mvs_data::fighter::Skeleton,
        socket: &str,
        loc: [f64; 3],
        quat: [f64; 4],
        scale: [f64; 3],
        left: bool,
    ) -> ComponentAttach {
        let rig = &self.view.rig;
        // (montageMeshAttach negates a notify's offset facing left: a component's relative location is not, undone
        // here)
        let k = if left && socket != "None" { -1.0 } else { 1.0 };
        let loc = loc.map(|v| v * k);
        let (bone, matrix, own) = attach_core(skel, |n| rig.index(n).is_some(), socket, left, loc, quat, scale);
        if own {
            eprintln!("warning: component mesh on {socket}: a pawn without a mirror node (not followed facing left)");
        }
        ComponentAttach { bone: bone.and_then(|b| rig.index(&b)), matrix }
    }

    /// `addComponentMesh`: the component `tag` (lower case), hidden, at `at` (facing right, left); `mats` per slot
    /// (None: not drawn); `master`: it has the main mesh as its master pose (drawn on the view's bones by name).
    #[allow(dead_code, reason = "the characters' component modules call it")]
    pub(crate) fn add_component_mesh(
        &mut self,
        clips: &Clips,
        tag: &str,
        at: [ComponentAttach; 2],
        info: Arc<MeshInfo>,
        mats: Vec<Option<MaterialRef>>,
        master: bool,
    ) {
        if self.has_component_mesh(tag) {
            panic!("component mesh added twice: {tag} ({})", self.key);
        }
        let master = master.then(|| {
            let names = &self.view.rig.names;
            let used: std::collections::BTreeSet<usize> = info.used_joints.iter().flatten().copied().collect();
            info.bones()
                .iter()
                .enumerate()
                .map(|(k, n)| match names.iter().position(|b| b == n) {
                    Some(i) => i,
                    None if used.contains(&k) => {
                        panic!("master-posed mesh bone missing from the view: {n} ({})", info.path)
                    }
                    None => 0,
                })
                .collect()
        });
        let skin = (info.skeleton.is_some() && master.is_none()).then(|| SocketSkin::new(&info, clips, None));
        self.components.push(ComponentView {
            tag: tag.to_owned(),
            info,
            mats,
            part: None,
            at,
            skin,
            master,
            on: false,
            time: 0.0,
            looped: false,
        });
    }

    /// `addComponentMesh` of the skin part `key`'s mesh with `host.partMaterials(fighter, key)`, the part's instances
    /// shared; false (nothing added) when the skin has no such part or it has no material.
    pub(crate) fn add_part_component_mesh(
        &mut self,
        clips: &Clips,
        tag: &str,
        at: [ComponentAttach; 2],
        key: &str,
        master: bool,
    ) -> bool {
        let Some(k) = self.skin.parts.iter().position(|p| p.key == key) else { return false };
        let Some(p) = self.parts.get(k).filter(|p| p.mats.iter().any(Option::is_some)) else { return false };
        let info = p.info.clone();
        self.add_component_mesh(clips, tag, at, info, Vec::new(), master);
        if let Some(c) = self.components.last_mut() {
            c.part = Some(k);
        }
        true
    }

    /// `hasComponentMesh`.
    pub(crate) fn has_component_mesh(&self, tag: &str) -> bool {
        self.components.iter().any(|c| c.tag == tag)
    }

    /// `attachComponentMesh`: K2_AttachToComponent to another socket (facing right, left): the holder moves.
    #[allow(dead_code, reason = "the characters' component modules call it")]
    pub(crate) fn attach_component_mesh(&mut self, tag: &str, at: [ComponentAttach; 2]) {
        if let Some(c) = self.components.iter_mut().find(|c| c.tag == tag) {
            c.at = at;
        }
    }

    /// `setComponentMesh`: shown or hidden (SetSubMeshVisibility).
    #[allow(dead_code, reason = "the characters' component modules call it")]
    pub(crate) fn set_component_mesh(&mut self, tag: &str, visible: bool) {
        if let Some(c) = self.components.iter_mut().find(|c| c.tag == tag) {
            c.on = visible;
        }
    }

    /// `playComponentMesh`: PlayAnimation(clip, loop) on the tag's skeletal component with a skeleton of its own
    /// (None: Stop, the pose it has stays). The position starts at 0 and advances with the view's ticks.
    pub(crate) fn play_component_mesh(&mut self, clips: &mut Clips, tag: &str, clip: Option<usize>, looped: bool) {
        let Some(c) = self.components.iter_mut().find(|c| c.tag == tag) else { return };
        let Some(sk) = &mut c.skin else { return };
        sk.clip = clip.map(crate::montage_mesh::MeshClip::Clip);
        c.time = 0.0;
        c.looped = looped;
        if sk.clip.is_some() {
            let root = sk.root();
            sk.pose_locked(clips, 0.0, looped, root);
        }
    }

    /// `tickComponentMeshes`: the shown components playing an animation advance by `dt` and pose their bones (a
    /// hidden one keeps its position). The sequence's root lock holds on the component's own top bone.
    // Deviation: one step a presentation tick while the mesh is shown; the fighter's hitpause (the
    // actor's CustomTimeDilation) and a rollback's replayed ticks are not counted.
    fn tick_component_meshes(&mut self, clips: &mut Clips, dt: f64) {
        for c in &mut self.components {
            let Some(sk) = &mut c.skin else { continue };
            if sk.clip.is_none() || !c.on {
                continue;
            }
            c.time += dt;
            let root = sk.root();
            sk.pose_locked(clips, c.time, c.looped, root);
        }
    }

    /// Whether the sim shows the sub-mesh `tag` (lower case): the character's mesh list has it and SetSubMeshVisibility
    /// has not hidden it (`f.ch.subMeshes.includes(tag) && !f.subHidden.has(tag)`).
    #[allow(dead_code, reason = "the characters' component modules call it")]
    pub(crate) fn sub_mesh_shown(&self, w: &World, tag: &str) -> bool {
        let fi = w.fighter(self.fid);
        w.char_data(self.fid).def.sub_meshes.iter().any(|s| s == tag)
            && !w.names.get(tag).is_some_and(|n| fi.pose.sub_hidden.contains(&n))
    }

    /// `PoseHook::shown` of this view's pose hook: whether the AnimBP's own writes show the sub-mesh `tag` (lower
    /// case), `sim` where it has none.
    pub(crate) fn pose_shown(&self, tag: &str, sim: bool) -> bool {
        self.pose.as_ref().map_or(sim, |p| p.shown(tag, sim))
    }

    /// The fighter this view draws.
    #[allow(dead_code, reason = "the characters' component modules call it")]
    pub(crate) fn fighter(&self) -> FighterId {
        self.fid
    }

    /// `bodyMaterials(i)[n]` (view.flashMaterials): the main mesh's `n`-th skin instance, None past the last.
    pub(crate) fn flash_material(&mut self, n: usize) -> Option<&mut MaterialRef> {
        let flash = &self.flash;
        self.body
            .mats
            .iter_mut()
            .enumerate()
            .filter(|(k, _)| flash.get(*k).copied().unwrap_or(false))
            .filter_map(|(_, m)| m.as_mut())
            .nth(n)
    }

    /// `partMaterials(key)[0]`'s SetScalarParameterValue: the first drawn material of the skin part `key` (none: no
    /// such part).
    pub(crate) fn set_part_scalar(&mut self, key: &str, name: &str, v: f64) {
        let Some(k) = self.skin.parts.iter().position(|p| p.key == key) else { return };
        if let Some(m) = self.parts.get_mut(k).and_then(|p| p.mats.iter_mut().flatten().next()) {
            set_scalar(m, name, v);
        }
    }

    /// view.materials: every material of the skeletal mesh components.
    fn each_material(&mut self, mut f: impl FnMut(&mut MaterialRef)) {
        for m in self.body.mats.iter_mut().flatten() {
            f(m);
        }
        for p in &mut self.parts {
            for m in p.mats.iter_mut().flatten() {
                f(m);
            }
        }
    }

    /// USkeletalMeshComponent::SetMorphTarget on the main mesh: the morph target `name`'s weight. With `remove_zero`
    /// (bRemoveZeroWeight) a weight within ZERO_ANIMWEIGHT_THRESH (0.00001) of 0 drops it from MorphTargetCurves.
    // Unverified: the rule is 4.26's source, not read from the exe.
    pub(crate) fn set_morph_target(&mut self, name: &str, v: f64, remove_zero: bool) {
        let v = v as f32;
        let keep = !remove_zero || v.abs() > 0.00001;
        match (self.morphs.iter().position(|(n, _)| &**n == name), keep) {
            (Some(k), true) => self.morphs[k].1 = v,
            (Some(k), false) => {
                self.morphs.remove(k);
            }
            (None, true) => self.morphs.push((Arc::from(name), v)),
            (None, false) => {}
        }
    }

    /// Every material of the view (view.materials): `name` = `v` (a minion's place hook: the car's DamageAmount).
    pub(crate) fn set_scalar_all(&mut self, name: &str, v: f64) {
        self.each_material(|m| set_scalar(m, name, v));
    }

    /// view.flashMaterials: the main mesh's skin instances.
    fn each_flash(&mut self, mut f: impl FnMut(&mut MaterialRef)) {
        for (k, m) in self.body.mats.iter_mut().enumerate() {
            if let Some(m) = m
                && self.flash.get(k).copied().unwrap_or(false)
            {
                f(m);
            }
        }
    }

    /// view.instanceMaterials: the flash list and the parts' materials.
    fn each_instance(&mut self, mut f: impl FnMut(&mut MaterialRef)) {
        self.each_flash(&mut f);
        for p in &mut self.parts {
            for m in p.mats.iter_mut().flatten() {
                f(m);
            }
        }
    }

    /// IntroVFX(Glow, GlowOpacity), the timeline's two cubic keys with flat tangents.
    fn set_intro_glow(&mut self, t: f64) {
        let flat = |a: f64, t: f64, end: f64| {
            let u = (t / end).min(1.0);
            a * (1.0 - u * u * (3.0 - 2.0 * u))
        };
        let (g, o) = (flat(1.0, t, 1.0), flat(0.6, t, 0.75));
        self.each_material(|m| {
            set_scalar(m, "Glow", g);
            set_scalar(m, "GlowOpacity", o);
        });
    }
}

// ---- the poser --------------------------------------------------------------------------------------------------

/// The views of one match: the meshes, clips and skins they pose with, and each view's state kept between frames.
#[derive(Debug)]
pub struct ViewPoser {
    clips: HashMap<String, Clips>,
    /// Per character its entry's blend space players bound (CharRender.blend_spaces).
    blend: HashMap<String, BlendSpaces>,
    draws: HashMap<String, Arc<FighterDraws>>,
    meshes: MeshMap,
    recs: Vec<Rec>,
    colours: Vec<FighterColour>,
    /// The views were placed once (by `place`, or by the first `present`).
    placed: bool,
    /// SetupPlayerUI's Delay(1.0) before the materials' set-up, ticks.
    setup_left: f64,
    flash_frames: f64,
    /// PFGCamera PriorityStack, by pawn id.
    priority: Vec<i32>,
    /// The load's players (setupMaterials' pawns), by pawn id.
    players: Vec<i32>,
    /// The store the views' data is read from (a mode's pawn is made at the step that first finds it, `sync_pawns`).
    store: mvs_store::Store,
    /// Pawn ids `sync_pawns` could not make a view for (warned once; asked again when the id leaves the world).
    refused: Vec<i32>,
    me: usize,
    cs: Vec<Mat4>,
    cs_ue: Vec<Mat4>,
    actors: ActorViews,
    /// The views' bones as last drawn, by pawn (the actors' rules' sockets).
    bodies: Vec<FighterBones>,
    /// The second views on the fighters (the Disguise's copies, the Polymorph meshes).
    seconds: Seconds,
    /// The camera as `set_camera` gave it: the camera actor's root and its up axis in the world (None: none given).
    camera: Option<([f64; 3], [f64; 3])>,
    /// The HUD broker's palette by palette index; empty: the store has no HUD.
    palette: Vec<skins::Rgba>,
    /// MPC_CentralPalette's ColorMe (ActorHost::color_me).
    color_me: Option<skins::Rgba>,
    /// A relay match (PlayingOnline).
    online: bool,
    /// The items part's drawing assets (a match with random items: crate::items).
    items: Option<ItemAssets>,
    /// Buff_SnowballHead's heads (crate::items::snowball_head).
    heads: Heads,
    /// The views whose meshes are the stage scenery's, as the last `present` placed them (`scenery_places`).
    scenery_places: Vec<crate::frame::SceneryPlace>,
}

impl ViewPoser {
    /// The views of `world`'s fighters, their data read from `store`, for player 0 of an offline match (a replay, a
    /// picture, a tape).
    pub fn new(store: &mvs_store::Store, world: &World) -> Result<ViewPoser> {
        ViewPoser::new_local(store, world, 0, false, &skins::ViewPrefs::default())
    }

    /// The views of `world`'s fighters for local player `me`, `online` a relay match (PlayingOnline), with the UI
    /// preferences `prefs` as the match's start has them (team skins, the outlines, the colour bindings).
    // Deviation: the character occlusion captures are always on.
    // Deviation: the outline toggles are read at the match's start, where every pawn binds OnUIPrefsChanged to
    // SetOutline (a change in the pause menu's settings shows from the next match).
    pub fn new_local(
        store: &mvs_store::Store,
        world: &World,
        me: usize,
        online: bool,
        prefs: &skins::ViewPrefs,
    ) -> Result<ViewPoser> {
        let mut colours = skins::fighter_colours(&world.setup, me, online, prefs.team_skins);
        for c in &mut colours {
            c.outline = skins::outline_shown(c.stencil, prefs.outlines);
        }
        let latent = mvs_sim::fighter::common::latent_frames;
        let mut p = ViewPoser {
            clips: HashMap::new(),
            blend: HashMap::new(),
            draws: HashMap::new(),
            meshes: HashMap::new(),
            recs: Vec::new(),
            colours,
            placed: false,
            camera: None,
            setup_left: f64::from(latent(1.0)),
            flash_frames: f64::from(latent(0.15)),
            priority: Vec::new(),
            players: Vec::new(),
            store: store.clone(),
            refused: Vec::new(),
            me,
            cs: Vec::new(),
            cs_ue: Vec::new(),
            actors: ActorViews::default(),
            bodies: Vec::new(),
            seconds: Seconds::default(),
            palette: Vec::new(),
            color_me: None,
            online,
            items: None,
            heads: Heads::default(),
            scenery_places: Vec::new(),
        };
        // (the items part's assets, for a match with random items)
        if mvs_sim::world::flow::items_data_of(world).is_some() {
            p.items = Some(ItemAssets::load(store)?);
        }
        if let Some(rows) = skins::hud_palette(store)? {
            p.palette = (0..5).map(|i| skins::palette_for_index_in(&rows.ui, i, prefs.bound())).collect();
            p.color_me = rows.color_me_in(prefs.bound());
        }
        p.mesh(store, SANDWICH)?;
        for f in world.all_ids() {
            p.add(store, world, f, None)?;
        }
        p.players = p.recs.iter().map(|r| r.id).collect();
        // (a mode's cast, held with the match as a fighter's is: the characters whose assets its
        // actors draw with, Rick's for Tutorial_Meeseeks, are never a pawn's)
        if let Some(mvs_sim::setup::MatchMode::Tutorial { cast, .. }) = &world.setup.mode {
            for k in cast.iter().filter(|k| world.data.chars.contains_key(k.as_str())) {
                p.load_char(store, world, k)?;
            }
        }
        p.seconds = Seconds::new(&mut p, store, world)?;
        // the minion views of the fighters' modules (made with the match, hidden until a minion lives)
        for f in world.all_ids() {
            for m in render_of(world.names.str(world.fighter(f).ch)).minions {
                if !world.data.chars.contains_key(m.key) {
                    eprintln!("warning: the match holds no {} (a minion's character): its view is not drawn", m.key);
                    continue;
                }
                p.add(store, world, f, Some(m))?;
            }
        }
        // the static models: Shaggy's sandwich, with the Sandwich actor entry's
        // materials (as the HeldItem prop's)
        // (the characters by key, so the first one holding a Sandwich entry does not depend on the map's order)
        let mut keys: Vec<&String> = p.draws.keys().collect();
        keys.sort();
        let drawn = keys.into_iter().find_map(|k| p.draws[k].actor("Sandwich", None));
        let info = p.meshes[SANDWICH].clone();
        let mats = prop_materials(&info, drawn.map(|d| d.1).unwrap_or_default());
        p.actors.models.insert("Sandwich", StaticModel { info, mats });
        Ok(p)
    }

    fn mesh(&mut self, store: &mvs_store::Store, path: &str) -> Result<Arc<MeshInfo>> {
        if let Some(m) = self.meshes.get(path) {
            return Ok(m.clone());
        }
        let m = MeshInfo::shared(store, path)?;
        self.meshes.insert(path.to_owned(), m.clone());
        Ok(m)
    }

    /// Fighter `f`'s own view.
    pub(crate) fn rec_for(&self, f: FighterId) -> Option<&Rec> {
        self.recs.iter().find(|r| r.fid == f)
    }

    /// Character `key`'s clips, once loaded.
    pub(crate) fn clips_of(&self, key: &str) -> Option<&Clips> {
        self.clips.get(key)
    }

    /// The main mesh slots skin `skin` of character `key` overrides (FighterDraws::skinned), once loaded.
    pub(crate) fn skinned(&self, key: &str, skin: &str) -> Vec<bool> {
        self.draws.get(key).map(|d| d.skinned(skin)).unwrap_or_default()
    }

    fn load_draws(&mut self, store: &mvs_store::Store, key: &str) -> Result<()> {
        if !self.draws.contains_key(key) {
            self.draws.insert(key.to_owned(), FighterDraws::shared(store, key)?);
        }
        Ok(())
    }

    /// `makeView` and `dressView` for fighter `f`, and the
    /// materials' values at its add (Team and Emissive_Mult 0 on every material of the view). With `minion`: the
    /// view of that entry of creator `f`'s module (a minion's or a car's slot): its own skin, the creator's colour
    /// index on the stencil and the skin set, none of the pawn's own (head scale, pose hook, components, held items),
    /// the materials as the skin set makes them.
    fn add(
        &mut self,
        store: &mvs_store::Store,
        w: &World,
        f: FighterId,
        minion: Option<&'static MinionView>,
    ) -> Result<()> {
        let fi = w.fighter(f);
        let key = minion.map_or_else(|| w.names.str(fi.ch).to_owned(), |m| m.key.to_owned());
        let cd = &w.data.chars[&key];
        // the held items' materials: the actor entries of the first character whose data has the class
        let mut item_keys = Vec::new();
        for h in chars::held_items() {
            let k = w
                .data
                .chars
                .iter()
                .find(|(_, d)| d.fighter.actors.as_ref().is_some_and(|a| a.contains_key(h.cls)))
                .map(|(k, _)| k.clone());
            if let Some(k) = &k {
                self.load_draws(store, k)?;
            }
            item_keys.push(k);
        }
        let pick = usize::try_from(fi.id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
        let skin = match minion {
            Some(m) => (m.skin)(&cd.fighter)?,
            None => skins::skin_of(&cd.fighter, pick)?,
        };
        // Unverified: a pawn bound to no slot takes the second player's colours (an opponent's), the first's without
        // one.
        let slot = usize::try_from(mvs_sim::world::pawns::slot_of(w, f)).ok().and_then(|i| self.colours.get(i));
        let mut colour = *slot.or(self.colours.get(1)).unwrap_or(&self.colours[0]);
        if let Some(m) = minion {
            // ForcedColorIndex = the creator's GetColorIndex; GetColorIndex then reports IsLocalOnlinePlayer false
            let ci = usize::from(colour.color_index == 1);
            colour.variant = if m.local_set && colour.team == 0.6 {
                skins::Variant::Local
            } else if ci == 1 {
                skins::Variant::Blue
            } else {
                skins::Variant::Red
            };
            colour.stencil = MINION_STENCIL[ci];
            let mut rec = self.make_view(store, w, f, &key, skin, colour, None, Some(m))?;
            rec.id = -1;
            rec.visible = false;
            self.recs.push(rec);
            return Ok(());
        }
        let mut rec = self.make_rec(store, w, f, &key, skin, colour, None)?;
        if let Some(c) = render_of(&key).components {
            (c.make)(&mut rec, w, &self.clips[&key], &self.draws[&key], &self.meshes);
        }
        for (h, k) in chars::held_items().zip(&item_keys) {
            (h.prop)(&mut rec, w, k.as_ref().map(|k| &*self.draws[k]), &self.meshes);
        }
        rec.each_material(|m| {
            set_scalar(m, "Team", colour.team);
            set_scalar(m, "Emissive_Mult", 0.0);
        });
        rec.heart = rec.heart_state(&self.draws[&key], &cd.fighter, &key);
        // (the load's fighters whose skin has an intro of its own: skin_intro.rs; a minion's view, returned above, has none)
        rec.intro = SkinIntro::new(w, &cd.fighter, &rec.skin);
        if rec.intro.is_some() {
            rec.intro_vis = vec![-1; rec.skin.parts.len()];
        }
        self.recs.push(rec);
        Ok(())
    }

    /// Character `key`'s clips (with its entry's blend spaces' clips), draws and actor meshes, once a match.
    fn load_char(&mut self, store: &mvs_store::Store, w: &World, key: &str) -> Result<()> {
        if !self.clips.contains_key(key) {
            let render = render_of(key);
            let blend = match render.blend_spaces {
                Some(t) => BlendSpaces::bind(store, key, t)?,
                None => BlendSpaces::default(),
            };
            // (the clips the pose hook takes: its blend spaces', loaded with the character's)
            let paths = &w.data.chars[key].fighter.paths.clips;
            let mut list: indexmap::IndexMap<&str, &str> =
                paths.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            for (k, v) in blend.clips() {
                list.entry(k).or_insert(v);
            }
            let clips = Clips::load(store, list)?;
            self.clips.insert(key.to_owned(), clips);
            self.blend.insert(key.to_owned(), blend);
        }
        self.load_draws(store, key)?;
        // (the montage meshes' models, RenderChar.actorMeshes: loaded with the view)
        let cd = &w.data.chars[key];
        for path in self.draws[key].actor_meshes() {
            if self.meshes.contains_key(&path) {
                continue;
            }
            let m = self.mesh(store, &path)?;
            self.meshes.insert(path, m);
        }
        // (the spawned actors' skeletal meshes that are no skin sub-mesh: an AddComponent
        // node's template, NODE_Add..., is no component of the actor; kept by the mesh's path)
        let subs = &cd.fighter.skin.sub_meshes;
        for def in cd.fighter.actors.iter().flat_map(|a| a.values()) {
            for (cn, c) in def.components.iter() {
                let n = c.as_obj().and_then(|c| c.get("SkeletalMesh")).and_then(Value::as_str);
                let Some(n) = n.filter(|n| !cn.starts_with("NODE_Add") && !n.is_empty() && *n != "None") else {
                    continue;
                };
                if subs.iter().any(|m| m.mesh.eq_ignore_ascii_case(n)) {
                    continue;
                }
                let Some(path) = cd.fighter.paths.meshes.get(n) else { continue };
                if !self.meshes.contains_key(path) {
                    let m = self.mesh(store, path)?;
                    self.meshes.insert(path.clone(), m);
                }
            }
        }
        // (the meshes its actors' graphs set that no SCS component holds: the entry's actor_meshes)
        for n in render_of(key).actor_meshes {
            let Some(path) = cd.fighter.paths.meshes.get(*n) else {
                eprintln!("warning: {key}: actor mesh {n} is not among the character's mesh paths");
                continue;
            };
            if !self.meshes.contains_key(path) {
                let m = self.mesh(store, path)?;
                self.meshes.insert(path.clone(), m);
            }
        }
        Ok(())
    }

    /// A view of character `key`'s actor mesh `cls` (an actors entry of its draws, in `colour`'s variant) on fighter
    /// `f` in `colour` (its stencil), posed with `key`'s clips: no parts, no pose hook, mesh scale 1 (a kit's
    /// Polymorph prop: crate::second). None: the draws have no such entry.
    pub(crate) fn prop_rec(
        &mut self,
        store: &mvs_store::Store,
        w: &World,
        f: FighterId,
        key: &str,
        cls: &str,
        colour: FighterColour,
    ) -> Result<Option<Rec>> {
        let variant = Some(colour.variant.key());
        self.load_char(store, w, key)?;
        let Some(cls) = self.draws[key].actor_key(cls) else { return Ok(None) };
        let Some((path, mats)) = self.draws[key].actor(&cls, variant) else { return Ok(None) };
        let info = self.mesh(store, &path)?;
        let clips = self.clips.get_mut(key).expect("loaded above");
        let rig = Rig::bind(clips, info.bones().to_vec());
        Ok(Some(Self::actor_rec(w, f, key, path, info, &mats, rig, colour)))
    }

    /// A view of the items part's draw `key` (an items entry's `<buff>.Polymorph`, in `colour`'s variant) on fighter
    /// `f` in `colour` (its stencil), posed with the items part's clips: as [`ViewPoser::prop_rec`] (the items branch
    /// of the polymorph build). None: a match without the items part, or no such draw.
    pub(crate) fn items_rec(&self, w: &World, f: FighterId, key: &str, colour: FighterColour) -> Option<Rec> {
        let items = self.items.as_ref()?;
        let (path, info) = items.mesh(key)?;
        let mats = items.materials(key, Some(colour.variant.key()))?;
        let rig = Rig::bind(&items.clips, info.bones().to_vec());
        Some(Self::actor_rec(w, f, key, path.to_owned(), info.clone(), &mats, rig, colour))
    }

    /// The items part's draw `key`'s sequence `name` (ItemAssets::clip), in a match with items.
    pub(crate) fn item_clip(&self, key: &str, name: &str) -> Option<usize> {
        self.items.as_ref()?.clip(key, Some(name))
    }

    /// The Rec of an actor mesh `path` (`info`, slot materials `mats`) drawn as a view of its own on fighter `f`:
    /// no parts, no pose hook, mesh scale 1.
    #[allow(clippy::too_many_arguments)]
    fn actor_rec(
        w: &World,
        f: FighterId,
        key: &str,
        path: String,
        info: Arc<MeshInfo>,
        mats: &[Option<String>],
        rig: Rig,
        colour: FighterColour,
    ) -> Rec {
        let view = CharView::new(&info, rig, 1.0, 0.0);
        let skin = Skin {
            name: String::new(),
            slug: String::new(),
            mesh: path,
            parts: Vec::new(),
            part_tags: Vec::new(),
            part_init: Vec::new(),
            part_armor: Vec::new(),
            intro: None,
        };
        let mats = mats.iter().map(|m| m.as_deref().map(material)).collect();
        Rec {
            fid: f,
            id: w.fighter(f).id,
            key: key.to_owned(),
            flash: Vec::new(),
            skin,
            view,
            colour,
            body: ViewMesh { info, mats },
            parts: Vec::new(),
            part_bones: Vec::new(),
            part_mirror: Vec::new(),
            part_scale: Vec::new(),
            armor: MaterialRef::default(),
            armor_on: false,
            armor_main: None,
            props: Vec::new(),
            montage: Vec::new(),
            components: Vec::new(),
            visible: true,
            outlines_on: true,
            shown_at: None,
            flash_left: 0.0,
            intro_glow: -1.0,
            rage_shirt: None,
            pose: None,
            pawn: None,
            priority: None,
            own: Vec::new(),
            minion: None,
            heart: None,
            morphs: Vec::new(),
            intro: None,
            intro_vis: Vec::new(),
            mix: Vec::new(),
        }
    }

    /// `makeView` and `viewMaterials` without the match's own writes: a view of character `key` in `skin` and `colour`
    /// (its variant and stencil) for fighter `f`, with its entry's pose hook (`second`: the kind of second view,
    /// PoseOf::slaved for a Disguise's copy, PoseOf::stand_in for a Polymorph mesh) and pawn components. A fighter's own view (add) and a second view on a fighter
    /// (crate::second: a Disguise's copy, a Polymorph mesh of a character's skin).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn make_rec(
        &mut self,
        store: &mvs_store::Store,
        w: &World,
        f: FighterId,
        key: &str,
        skin: Skin,
        colour: FighterColour,
        second: Option<SecondKind>,
    ) -> Result<Rec> {
        self.make_view(store, w, f, key, skin, colour, second, None)
    }

    /// The prop meshes of skin `skin` (its look's propMeshes, by their own path, else the character's path of the
    /// mesh), with the view (the skin's props): an actor draws a skin's prop by its path
    /// (Jake's house and horse: S01_House, Origins_Horse, ...), which load_char's character-wide lists hold only for
    /// the default skin.
    fn load_skin_props(&mut self, store: &mvs_store::Store, cd: &mvs_data::CharData, skin: &str) -> Result<()> {
        let fd = &cd.fighter;
        let look = fd.skins.as_ref().and_then(|m| m.get(skin));
        let props = look.map_or(fd.skin.prop_meshes.as_deref(), |l| l.prop_meshes.as_deref()).unwrap_or(&[]);
        for p in props {
            let Some(path) = p.path.as_ref().or_else(|| fd.paths.meshes.get(&p.mesh)) else {
                eprintln!("warning: skin {skin}: prop mesh {} is not among the character's mesh paths", p.mesh);
                continue;
            };
            self.mesh(store, path)?;
        }
        Ok(())
    }

    /// `make_rec`, or with `minion` a minion's view of creator `f` (add): the generic entry's rules (no head scale,
    /// components or view hook of the character's), the entry's mesh scale and pose hook.
    #[allow(clippy::too_many_arguments)]
    fn make_view(
        &mut self,
        store: &mvs_store::Store,
        w: &World,
        f: FighterId,
        key: &str,
        skin: Skin,
        colour: FighterColour,
        second: Option<SecondKind>,
        minion: Option<&'static MinionView>,
    ) -> Result<Rec> {
        let fi = w.fighter(f);
        let key = key.to_owned();
        let render: &CharRender = if minion.is_some() { &CharRender::GENERIC } else { render_of(&key) };
        let cd = &w.data.chars[&key];
        self.load_char(store, w, &key)?;
        if minion.is_none() {
            self.load_skin_props(store, cd, &skin.name)?;
        }
        let body_info = self.mesh(store, &skin.mesh)?;
        let mut part_infos = Vec::new();
        for p in &skin.parts {
            let path = p.path.clone().ok_or_else(|| Error::Format {
                what: format!("{key} part {}", p.key),
                why: format!("no mesh {} among the character's paths", p.mesh),
            })?;
            part_infos.push(self.mesh(store, &path)?);
        }
        let ViewPoser { clips, blend, draws, meshes, .. } = self;
        let clips = clips.get_mut(&key).expect("loaded above");
        let draws = &draws[&key];
        let id = fi.id;
        let mut rec = build_rec(
            RecMake {
                fid: f,
                id,
                key: key.clone(),
                cd,
                skin,
                colour,
                body_info,
                part_infos,
                render,
                minion,
                second,
                slaved: false,
            },
            clips,
            &blend[&key],
            draws,
        )?;
        if let Some(view) = render.view {
            view(&mut rec, clips, draws, meshes);
        }
        Ok(rec)
    }

    /// The camera before `present` (an actor that follows the camera's centre reads it: Velma's Jenkins pin): the
    /// PFGCamera actor's root as placed (CameraRig.root: the lens without the shake) and the view's up axis
    /// (CameraRig.pov.up), in the world.
    pub fn set_camera(&mut self, root: [f64; 3], up: [f64; 3]) {
        self.camera = Some((root, up));
    }

    /// One presentation step (the views, events and montage meshes), after every
    /// World step: each view placed at the World's tick (every view clock one STEP), the step's events read (so
    /// before the caller drains them), the montage meshes put on or taken off.
    pub fn place(&mut self, world: &World) {
        self.sync_pawns(world);
        self.place_views(world);
        self.handle_events(world);
        self.place_montage_meshes(world);
        self.place_components(world);
        self.seconds.place(world, &mut self.clips, self.items.as_mut().map(|i| &mut i.clips));
        let ViewPoser {
            meshes,
            draws,
            colours,
            clips,
            bodies,
            actors,
            blend,
            camera,
            palette,
            color_me,
            online,
            items,
            ..
        } = self;
        let mut host = ActorHost {
            meshes,
            draws,
            colours,
            clips,
            fighters: bodies,
            blend,
            camera: *camera,
            palette,
            color_me: *color_me,
            online: *online,
            items: items.as_mut(),
        };
        actors.spawn_views(&mut host, world);
        self.placed = true;
    }

    /// The Lab's reset (new pawns on the same views): each character's pose hook anew
    /// (PoseHook::reset), and its pawn components' state (PawnComponents::reset).
    pub fn reset(&mut self) {
        self.seconds.reset();
        self.heads.clear();
        for r in &mut self.recs {
            let Rec { pose, parts, .. } = r;
            if let Some(h) = pose {
                h.reset(&mut |k, n, v| {
                    for m in parts[k].mats.iter_mut().flatten() {
                        set_scalar(m, n, v);
                    }
                });
            }
            if r.minion.is_some() {
                hide_minion(r);
                continue;
            }
            if let Some(reset) = render_of(&r.key).components.and_then(|c| c.reset) {
                reset(r);
            }
        }
    }

    /// One frame: every view drawn between the last tick (`alpha` 0) and the latest (`alpha`
    /// 1), blending only. `fighters` and `actors` are cleared and filled: the vectors keep their capacity, the poses
    /// in them are new each frame. Before any `place`, the views are placed at the World's tick first, with no events
    /// (the match's first frame).
    pub fn present(&mut self, world: &World, alpha: f32, fighters: &mut Vec<FighterPose>, actors: &mut Vec<ActorPose>) {
        actors.clear();
        if !self.placed {
            self.sync_pawns(world);
            self.place_views(world);
            self.place_montage_meshes(world);
            self.placed = true;
        }
        let mut minions = Vec::new();
        self.present_views(f64::from(alpha), fighters, &mut minions);
        self.seconds.present(f64::from(alpha), actors);
        let ViewPoser {
            meshes,
            draws,
            colours,
            clips,
            bodies,
            actors: views,
            recs,
            blend,
            camera,
            palette,
            color_me,
            online,
            items,
            heads,
            scenery_places,
            ..
        } = self;
        let mut host = ActorHost {
            meshes,
            draws,
            colours,
            clips,
            fighters: bodies,
            blend,
            camera: *camera,
            palette,
            color_me: *color_me,
            online: *online,
            items: items.as_mut(),
        };
        views.present(&mut host, world, f64::from(alpha), actors);
        // (the snowman heads: headFx's frame, on the bones just presented)
        let shown = |f: FighterId| recs.iter().any(|r| r.fid == f && r.minion.is_none() && r.visible);
        heads.present(&host, world, shown, actors);
        scenery_places.clear();
        views.scenery_places(scenery_places);
        actors.append(&mut minions);
        // the pawns' own actors, on the bones just presented (a minion's view has none)
        for r in recs.iter_mut().filter(|r| r.minion.is_none()) {
            let Some(make) = render_of(&r.key).pawn_actors else { continue };
            let list = r.pawn.get_or_insert_with(|| make(&host, world, r.fid));
            let fi = world.fighter(r.fid);
            let cx = PawnCx {
                fid: r.fid,
                hidden: fi.life.dead != 0.0 || fi.vis.invisible,
                pose: r.pose.as_deref(),
                stencil: if r.outlines_on { r.colour.stencil } else { 0 },
            };
            for a in list.iter_mut() {
                a.present(&host, world, &cx, f64::from(alpha), actors);
            }
        }
    }

    /// The HUD's point of fighter `f` between the last tick (0) and the latest (1): its
    /// sim place with the mesh's Z, the mesh's scale and its depth from the camera; None for a fighter not shown.
    // (hudAt is null for a dead fighter and the presented point otherwise, the view's own `visible` aside)
    pub fn hud_point(&self, w: &World, f: FighterId, alpha: f32) -> Option<HudPoint> {
        let id = w.fighter(f).id;
        let r = self.recs.iter().find(|r| r.id == id)?;
        let s = r.shown_at?;
        let a = f64::from(alpha);
        Some(HudPoint {
            x: s.px + (s.x - s.px) * a,
            y: s.py + (s.y - s.py) * a,
            d: s.d,
            mesh_z: r.view.mesh_z,
            mesh_scale: r.view.mesh_scale,
        })
    }

    /// The skins' own intros' sounds whose times the views' clocks passed since the last call (skin_intro.rs
    /// `introSounds`): (the fighter's id, the AkEvent), the pawns in order. Once a tick, after
    /// `place`.
    pub fn intro_sounds(&mut self, out: &mut Vec<(i32, String)>) {
        out.clear();
        let mut heard = Vec::new();
        for r in &mut self.recs {
            let Some(s) = &mut r.intro else { continue };
            s.sounds(&mut heard);
            out.extend(heard.drain(..).map(|ev| (r.id, ev)));
        }
    }

    /// The views whose meshes are the stage scenery's (The Court's basketball), as the last `present` placed them:
    /// the caller hands each to `Renderer::scenery_place` before the frame draws.
    pub fn scenery_places(&self) -> &[crate::frame::SceneryPlace] {
        &self.scenery_places
    }

    /// The actor classes met so far whose meshes no ported rule draws.
    pub fn undrawn_actors(&self) -> &BTreeSet<String> {
        &self.actors.undrawn
    }

    // ---- the materials and the events' material part --------------------------------------------------

    /// A pawn's view of the match's (a minion's event reaches none: its view's effects are the minion modules').
    fn rec_of(&mut self, id: i32) -> Option<&mut Rec> {
        self.recs.iter_mut().find(|r| r.id == id && r.minion.is_none())
    }

    /// SetOpacityMaskTextures for stack indexes 0..3: OpacityToggle 1 at index 0, else 0 and LocalPlayerSceneDepth
    /// the camera's capture of index - 1 (never taken off).
    fn set_opacity_masks(&mut self) {
        for index in 0..self.priority.len().min(4) {
            let id = self.priority[index];
            if let Some(r) = self.rec_of(id) {
                r.priority = Some(index as u8);
                let v = if index == 0 { 1.0 } else { 0.0 };
                r.each_instance(|m| {
                    set_scalar(m, "OpacityToggle", v);
                    if index > 0 {
                        set_texture(m, "LocalPlayerSceneDepth", PRIORITY_CAPTURES[index - 1]);
                    }
                });
            }
        }
    }

    /// SetFighterToHighPriority: a fighter that is not at index 0 goes to index 1.
    fn set_high_priority(&mut self, id: i32) {
        if self.priority.first().is_none_or(|&f| f == id) {
            return;
        }
        if let Some(at) = self.priority.iter().position(|&f| f == id) {
            self.priority.remove(at);
        }
        self.priority.insert(1, id);
        self.set_opacity_masks();
    }

    /// `setupMaterials`: each load player's MaterialInstances loop and HandleMaterialOffset, then the masks.
    // Unverified: the fighters enter the stack in player order (all the Delays end in one frame); in Local Play each
    // human goes to index 0 in player order, so the last human is in front (the Delays' completion order).
    fn setup_materials(&mut self, w: &World) {
        for k in 0..self.players.len() {
            let id = self.players[k];
            let i = usize::try_from(id).unwrap_or(usize::MAX);
            let local = if w.setup.local_play { w.setup.players.get(i).is_some_and(|p| !p.bot) } else { i == self.me };
            self.enter_priority(id, local);
        }
        self.set_opacity_masks();
    }

    /// One pawn's MaterialInstances loop and HandleMaterialOffset: a locally controlled human
    /// goes to the front of the stack, any other pawn is desaturated and goes to the end.
    fn enter_priority(&mut self, id: i32, local: bool) {
        // (a pawn with no view takes no index)
        let Some(r) = self.rec_of(id) else { return };
        if local {
            self.priority.insert(0, id);
            return;
        }
        r.each_material(|m| set_scalar(m, "Desaturation", 0.025));
        self.priority.push(id);
    }

    /// A mode pawn's ReceivePossessed (the sim's PriorityAdd event): SetupPlayerUI's loop for
    /// that pawn alone.
    // Unverified: the pawn bound to the local player's slot is taken as the locally controlled human's (the
    // controller is the sim's, off the snapshot).
    fn priority_add(&mut self, w: &World, id: i32) {
        let Some(f) = w.by_id(id) else { return };
        let local = usize::try_from(mvs_sim::world::pawns::slot_of(w, f)).is_ok_and(|s| s == self.me);
        self.enter_priority(id, local);
        self.set_opacity_masks();
    }

    /// A mode's pawn gets its view at the step that first finds it, its character's data loaded with
    /// the match, and a pawn that left the world has its view dropped with the view's
    /// materials (`recs` owns them). The pawn stays in the priority stack, as the build's destroy paths leave it.
    fn sync_pawns(&mut self, w: &World) {
        // (the players' views are made with the poser: only a mode's pawns come and go)
        let Some(ps) = &w.pawns else { return };
        let here = |id: i32| ps.iter().any(|&f| !w.fighter(f).life.is_minion && w.fighter(f).id == id);
        while let Some((fid, id)) = self.recs.iter().find(|r| r.minion.is_none() && !here(r.id)).map(|r| (r.fid, r.id))
        {
            self.recs.retain(|r| r.id != id && !(r.minion.is_some() && r.fid == fid));
            // (its bones go with it, or an id taken by another character keeps the old names)
            self.bodies.retain(|b| b.id != id);
        }
        self.refused.retain(|&id| here(id));
        for &f in ps {
            let id = w.fighter(f).id;
            if w.fighter(f).life.is_minion
                || self.recs.iter().any(|r| r.id == id && r.minion.is_none())
                || self.refused.contains(&id)
            {
                continue;
            }
            let store = self.store.clone();
            let made = self.add(&store, w, f, None).and_then(|()| {
                for m in render_of(w.names.str(w.fighter(f).ch)).minions {
                    if w.data.chars.contains_key(m.key) {
                        self.add(&store, w, f, Some(m))?;
                    }
                }
                Ok(())
            });
            if let Err(e) = made {
                eprintln!("warning: pawns: no view for pawn {id}: {e}");
                self.refused.push(id);
            }
        }
    }

    /// BPI_PulseHitVFX: ColorMap = NormalizeToRange(CurrentDamage, 0, 75) on the flash materials, the 0.15 s clock
    /// (not retriggered), and the priority stack.
    fn pulse_hit(&mut self, w: &World, id: i32) {
        let frames = self.flash_frames;
        let Some(r) = self.recs.iter_mut().find(|r| r.id == id) else { return };
        let v = w.fighter(r.fid).stats.damage / 75.0;
        r.each_flash(|m| set_scalar(m, "ColorMap", f64::from(v)));
        if r.flash_left == 0.0 {
            r.flash_left = frames;
        }
        // (a minion's flash (pulseHit): its SetFighterToHighPriority branch is not ported, the
        // priority stack holds the players' views; its Delay counted as a player's)
        if r.minion.is_some() {
            return;
        }
        self.set_high_priority(id);
    }

    fn handle_events(&mut self, w: &World) {
        if self.setup_left > 0.0 {
            self.setup_left -= 1.0;
            if self.setup_left == 0.0 {
                self.setup_materials(w);
            }
        }
        for r in &mut self.recs {
            if r.flash_left > 0.0 {
                r.flash_left -= 1.0;
                if r.flash_left == 0.0 {
                    r.each_flash(|m| set_scalar(m, "ColorMap", 0.0));
                }
            }
        }
        for r in &mut self.recs {
            r.place_heart(w);
        }
        for r in &mut self.recs {
            if r.intro_glow < 0.0 {
                continue;
            }
            let now = (r.intro_glow + STEP).min(1.0);
            r.set_intro_glow(now);
            r.intro_glow = if now < 1.0 { now } else { -1.0 };
        }
        for e in &w.events {
            // the characters' pose hooks (CharRender.pose) that answer an event
            for r in self.recs.iter_mut().filter(|r| r.minion.is_none()) {
                let Rec { pose, parts, id, .. } = r;
                if let Some(h) = pose {
                    h.event(w, *id, e, &mut |k, n, v| {
                        for m in parts[k].mats.iter_mut().flatten() {
                            set_scalar(m, n, v);
                        }
                    });
                }
            }
            match e {
                Event::Hit { victim, .. } => self.pulse_hit(w, *victim),
                Event::PulseHit { fighter, .. } => self.pulse_hit(w, *fighter),
                Event::RageGlow { fighter, on, shirt } => {
                    let Some(r) = self.rec_of(*fighter) else { continue };
                    let v = if *on { 1.0 } else { 0.0 };
                    // (a skin with no Shirt sub-mesh has no shirt to set: nothing glows)
                    if *shirt {
                        if let Some(p) = r.parts.first_mut() {
                            for m in p.mats.iter_mut().flatten() {
                                set_scalar(m, "Emissive_Mult", v);
                            }
                        }
                    } else {
                        r.each_material(|m| set_scalar(m, "Emissive_Mult", v));
                    }
                }
                Event::MatScalar { fighter, name, value } => {
                    let n = w.names.str(*name);
                    if let Some(r) = self.rec_of(*fighter) {
                        r.each_instance(|m| set_scalar(m, n, f64::from(*value)));
                    }
                }
                Event::IntroReveal { fighter } => {
                    if let Some(r) = self.rec_of(*fighter) {
                        r.intro_glow = 0.0;
                        r.set_intro_glow(0.0);
                    }
                }
                // AnimNotify_Equip's animation on the equipped mesh: PlayAnimation(Animation,
                // LoopAnimation) at Begin with EquipAtStart, PlayAnimation(EndAnimation, EndLoopAnimation) at End, Stop()
                // where the mesh is unequipped, each behind a cast to SkeletalMeshComponent: a component with a skeleton
                // of its own on the view plays it. A skin sub-mesh is master-posed: nothing drawn changes.
                Event::SubMeshAnim { fighter, tag, anim, looping } => {
                    let Some(r) = self.recs.iter_mut().find(|r| r.id == *fighter && r.minion.is_none()) else {
                        continue;
                    };
                    let clips = self.clips.get_mut(&r.key).expect("the view's clips");
                    let tag = w.names.str(*tag).to_lowercase();
                    let anim = anim.map(|a| w.names.str(a));
                    if r.has_component_mesh(&tag) {
                        let clip = anim.and_then(|a| clips.get(a));
                        if let (Some(a), None) = (anim, clip) {
                            eprintln!("warning: sub mesh animation not loaded: {a} ({tag})");
                        }
                        r.play_component_mesh(clips, &tag, clip, *looping);
                    } else if anim.is_some() && !r.skin.part_tags.iter().flatten().any(|t| t.eq_ignore_ascii_case(&tag))
                    {
                        eprintln!("warning: sub mesh animation: no mesh component drawn for the tag {tag} ({})", r.key);
                    }
                }
                Event::PriorityAdd { fighter } => self.priority_add(w, *fighter),
                _ => {}
            }
        }
    }

    // ---- placing and presenting the views -------------------------------------------------------------------

    fn place_views(&mut self, w: &World) {
        let ViewPoser { clips, recs, .. } = self;
        for r in recs.iter_mut() {
            let clips = clips.get_mut(&r.key).expect("the view's clips");
            if r.minion.is_some() {
                place_minion(r, clips, w);
            } else {
                place_view(r, clips, w);
            }
        }
    }

    fn place_montage_meshes(&mut self, w: &World) {
        let ViewPoser { recs, draws, meshes, clips, .. } = self;
        for r in recs.iter_mut() {
            let clips = clips.get_mut(&r.key).expect("the view's clips");
            place_montage_meshes(r, w, &draws[&r.key], meshes, clips);
        }
    }

    /// The characters' pawn components (CharRender.components' place, after the montage
    /// meshes).
    fn place_components(&mut self, w: &World) {
        for r in &mut self.recs {
            if let Some(c) = render_of(&r.key).components {
                (c.place)(r, w);
            }
        }
    }

    fn present_views(&mut self, a: f64, fighters: &mut Vec<FighterPose>, actors: &mut Vec<ActorPose>) {
        // (the poses of the last frame written over, their buffers kept)
        let mut n = 0;
        let ViewPoser { recs, cs, cs_ue, bodies, .. } = self;
        for r in recs.iter_mut() {
            let Some(s) = r.shown_at else { continue };
            let x = s.px + (s.x - s.px) * a;
            let y = s.py + (s.y - s.py) * a;
            r.view.place(x, y, s.facing);
            r.view.present(a);
            // (a pawn's view hidden whole is posed all the same, as presentViews does, for the effects' and the
            // actors' sockets; a minion's hidden view is no actor of the frame)
            if !r.visible && r.minion.is_some() {
                continue;
            }
            r.view.bones_cs(cs);
            // (a minion's view: an actor of the frame, the minion its actor; no bones of the actors' sockets)
            if let Some(m) = r.minion.as_ref().and_then(|m| m.live) {
                cs_ue.clear();
                cs_ue.extend(cs.iter().map(math::to_ue));
                let meshes = meshes_of(r, cs, cs_ue);
                actors.push(ActorPose {
                    meshes,
                    actor: Some(mvs_sim::actors::ActorRef::Fighter(m)),
                    ..ActorPose::default()
                });
                continue;
            }
            let body = match bodies.iter().position(|b| b.id == r.id) {
                Some(i) => &mut bodies[i],
                None => {
                    bodies.push(FighterBones { id: r.id, names: r.view.rig.names.clone(), ..FighterBones::default() });
                    bodies.last_mut().expect("the body just pushed")
                }
            };
            body.model = r.view.model_world();
            body.cs.clone_from(cs);
            if body.kept.len() != cs.len() {
                body.kept = (0..cs.len()).map(|k| r.view.kept(k)).collect();
            }
            // the skinned montage meshes' bones (ActorHost::montage_bone), placed as meshes_of draws them
            body.montage.clear();
            if !r.view.parts_only {
                for mv in &r.montage {
                    let Some(sk) = &mv.skin else { continue };
                    let m = match mv.bone {
                        Some(b) => math::mul(&math::mul(&body.model, &cs[b]), &mv.matrix),
                        None => math::mul(&body.model, &mv.matrix),
                    };
                    body.montage.push(crate::actors::MontageBones {
                        name: mv.name,
                        info: mv.info.clone(),
                        world: math::to_ue(&m),
                        cs: sk.bones_cs(),
                    });
                }
            }
            cs_ue.clear();
            cs_ue.extend(cs.iter().map(math::to_ue));
            present_vectors(r, cs);
            if n == fighters.len() {
                fighters.push(FighterPose::default());
            }
            let f = &mut fighters[n];
            n += 1;
            meshes_into(r, cs, cs_ue, &mut f.meshes);
            (f.fighter, f.priority, f.hidden, f.heart) = (r.id as u32, r.priority, !r.visible, r.heart.is_some());
        }
        fighters.truncate(n);
    }
}

/// PoseHook::vectors on the pose just presented (`cs`: its bones in the model's space), onto the view's materials.
fn present_vectors(r: &mut Rec, cs: &[Mat4]) {
    let model = r.view.model_world();
    let Rec { pose, body, parts, .. } = r;
    let Some(p) = pose else { return };
    p.vectors(&|b| math::to_ue(&math::mul(&model, &cs[b])), &mut |part, n, v| {
        let mats = match part {
            None => &mut body.mats,
            Some(k) => &mut parts[k].mats,
        };
        for m in mats.iter_mut().flatten() {
            set_vector(m, n, v);
        }
    });
}

/// A fighter's point for the HUD's anchors (ViewPoser::hud_point).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HudPoint {
    /// The sim's x, y between the two ticks.
    pub x: f64,
    pub y: f64,
    /// The anchors' offset along UE X (the playerIndex on the widget components' local Y), cm.
    pub d: f64,
    /// The mesh's Z (UE cm).
    pub mesh_z: f64,
    pub mesh_scale: f64,
}

/// A minion view's tick: the live minion of its entry that its creator made, placed as a
/// pawn's view (placeOne); none: hidden, its AnimBP anew.
fn place_minion(r: &mut Rec, clips: &mut Clips, w: &World) {
    let m = r.minion.as_ref().expect("a minion's view");
    let (def, creator, was) = (m.def, m.creator, m.live);
    let live = w.pawns.as_deref().unwrap_or(&[]).iter().copied().find(|&x| {
        let fi = w.fighter(x);
        fi.life.is_minion
            && fi.life.minion_creator == Some(creator)
            && fi.life.dead == 0.0
            && w.names.str(fi.ch) == def.key
    });
    let Some(f) = live else {
        if was.is_some() {
            hide_minion(r);
        }
        return;
    };
    // (a new car is a new pawn: a new anim instance)
    if def.renew && was.is_some_and(|x| x != f) {
        hide_minion(r);
    }
    r.minion.as_mut().expect("a minion's view").live = Some(f);
    r.fid = f;
    r.id = w.fighter(f).id;
    place_view(r, clips, w);
    if let Some(place) = def.place {
        place(r, w, f);
    }
}

/// A minion view's minion gone (or the Lab's reset): hidden, its AnimBP anew, the flash's wait ended (MinionFx.end).
fn hide_minion(r: &mut Rec) {
    if let Some(m) = &mut r.minion {
        m.live = None;
    }
    r.id = -1;
    r.visible = false;
    r.shown_at = None;
    let Rec { pose, parts, .. } = r;
    if let Some(h) = pose {
        h.reset(&mut |k, n, v| {
            for m in parts[k].mats.iter_mut().flatten() {
                set_scalar(m, n, v);
            }
        });
    }
    if r.flash_left > 0.0 {
        r.each_flash(|m| set_scalar(m, "ColorMap", 0.0));
    }
    r.flash_left = 0.0;
}

/// A minion view's loco key under MinionView::nav: one constant, the machine's own crossfades in its pose.
const NAV_KEY: ViewKey = ViewKey::Gen(i32::MIN);

/// One tick's placement of a view (placeViews' loop body).
fn place_view(r: &mut Rec, clips: &mut Clips, w: &World) {
    let f = r.fid;
    let fi = w.fighter(f);
    let dead = fi.life.dead != 0.0;
    // Deviation: the smoke's AddInvisibility for the viewer is not ported.
    let shown = !fi.vis.invisible;
    // (a minion's view: AddInvisibility hides it whole, the AlwaysVisibleMeshes split not taken)
    let kept = if dead || shown || r.minion.is_some() {
        None
    } else if fi.vis.invisible {
        fi.vis.invis_shown.as_ref()
    } else {
        fi.vis.always_visible.as_ref()
    };
    let parts_only = kept.is_some_and(|k| !k.is_empty());
    r.visible = !dead && (shown || parts_only);
    r.view.parts_only = parts_only;
    r.view.size_scale = f64::from(fi.body.size_scale);
    // (a skin's own intro over the sim's default one: skin_intro.rs; once a tick, its clock)
    let si = r.intro.as_mut().and_then(|s| s.pose(w, f));
    if let Some(s) = &r.intro {
        s.parts(&mut r.intro_vis);
    }
    let has = |set: &indexmap::IndexSet<Name>, t: &str| w.names.get(t).is_some_and(|n| set.contains(&n));
    for (k, t) in r.skin.part_tags.iter().enumerate() {
        r.view.part_on[k] = match (kept.filter(|_| parts_only), t) {
            (Some(kept), Some(t)) => has(kept, t),
            (Some(_), None) => false,
            (None, Some(t)) => !has(&fi.pose.sub_hidden, t),
            // (a part the sim keeps no tag of: its skin's initial visibility, or the skin intro's state)
            (None, None) if si.is_some() && r.intro_vis[k] >= 0 => r.intro_vis[k] == 1,
            (None, None) => r.skin.part_init[k],
        };
    }
    r.armor_on = fi.armor.armor_vis.unwrap_or(0.0) > 0.0 && !fi.armor.armor_hid && !parts_only && r.minion.is_none();
    let was = r.shown_at.take();
    if dead {
        return;
    }
    let nav = r.minion.as_ref().and_then(|m| m.def.nav);
    let v = &mut r.view;
    let (x, y, facing) = (fi.body.x, fi.body.y, fi.body.facing_dir);
    // Mesh RelativeLocation.X (hit wobble) runs along the actor's forward; Z carries the bounce offset.
    let hud = (x + facing * fi.hit.mesh_x, y + fi.hit.mesh_z);
    v.place(f64::from(hud.0), f64::from(hud.1), f64::from(facing));
    let sh = mvs_sim::chars::shaggy::state::shaggy_of(w, f);
    v.set_prop("RockMesh", sh.is_some() && mvs_sim::chars::shaggy::state::rock(w, f));
    let held =
        w.actors.items.iter().any(|(_, it)| it.held && it.holder == fi.id && chars::is_held_item(w.names.str(it.cls)));
    v.set_prop("HeldItem", !fi.items.item_holstered && held);
    // BaseFighter SetOutline writes OutlineCount = the preference test (0 or 1) at set-up and on each
    // OnUIPrefsChanged; TurnOffOutline counts from there.
    // Deviation: the sim keeps its count from 1 and the view ANDs the preference test in (MatchHost.outlineShown): a
    // preference turned on inside a TurnOffOutline window shows the outline at the window's end, where the build's
    // reset to 1 shows it at once and leaves the count 2.
    // (the skin's intro has no TurnOffOutline of the default's: those the sim has open are not counted)
    r.outlines_on = r.colour.outline
        && match &si {
            Some(si) => fi.life.outline_count + si.outline_fix > 0,
            None => mvs_sim::fighter::outlines_on(w, f),
        };
    v.hit_pitch = mvs_sim::fighter::anim::hit_pitch(w, f).map(f64::from);
    v.layer_pitch = mvs_sim::fighter::anim::layer_pitch(w, f).map(f64::from);
    // (a minion's view: its entry's rule, a headScale of 1; the Watermelon's its fighter's)
    v.head_scale = if r.minion.as_ref().is_some_and(|m| !m.def.head_scale) {
        1.0
    } else {
        f64::from(fi.pose.head_scale.unwrap_or(1.0))
    };
    if let Some(p) = &mut r.pose {
        p.set(w, f, v, clips);
    }
    if let Some(p) = &mut r.pose {
        let (body, parts, ci) = (&mut r.body, &mut r.parts, r.colour.color_index);
        p.scalars(ci, &mut |part, n, val| {
            let mats = match part {
                None => &mut body.mats,
                Some(k) => &mut parts[k].mats,
            };
            for m in mats.iter_mut().flatten() {
                set_scalar(m, n, val);
            }
        });
    }
    let hold = sh.is_some_and(|s| s.hold_pose);
    let add = sh.filter(|s| s.rage_shirt >= 0.0).and_then(|s| Some((r.rage_shirt?, f64::from(s.rage_shirt))));
    // the kick slot's mix first (Shaggy's), then the AnimBP's slot mix (CharHooks.slotMix): mixArg's entries
    let slot_mix = fi.hooks.char.slot_mix.and_then(|h| h(w, f)).map_or(&[][..], |m| &m.slot_mix[..]);
    let entries = sh
        .and_then(|s| s.kick_mix.as_ref())
        .map(|k| (k.anim, k.time, k.w))
        .into_iter()
        .chain(slot_mix.iter().map(|x| (x.anim, x.time, x.w)));
    let key = match fi.anim.anim_key {
        AnimKey::None => ViewKey::Empty,
        AnimKey::Montage(n) => ViewKey::Anim(n),
        AnimKey::Gen(g) => ViewKey::Gen(g),
    };
    if let (Some(si), Some(s)) = (&si, &r.intro) {
        // The skin's intro (skin_intro.rs): its pose at its own blend-out weight over the state machines' pose, under
        // the sim's intro name (the view's crossfade key); once its blend-out is over, the machines' pose alone, as
        // after a montage's drawn blend-out.
        let (la, lt) = mvs_sim::fighter::anim::loco_clip(w, f);
        let src = clips.get(w.names.str(la));
        if src.is_none() {
            warn_once(format!("skin intro: source clip not loaded: {} ({})", w.names.str(la), r.key));
        }
        let loco_key = ViewKey::Gen(fi.anim.loco_gen);
        if si.w <= 0.0 {
            let loco = Loco { dur: f64::from(fi.anim.loco_blend.0), hermite: fi.anim.loco_blend.1, key: loco_key };
            let rules = r.pose.as_deref_mut().map(|p| p as &mut dyn crate::view::PoseRules);
            v.pose(clips, src, f64::from(lt), None, 0.0, 0.0, STEP, hold, &loco, add, &[], rules);
        } else {
            if si.w < 1.0 {
                v.slot_under(src, f64::from(lt), si.w, loco_key);
            }
            let anim = s.anim(si);
            let clip = clips.get(anim);
            if clip.is_none() {
                warn_once(format!("skin intro clip not loaded: {anim} ({})", r.key));
            }
            // (pickIntro: an entry of the default intro's tracks takes the skin intro's clip and time)
            let mix = &mut r.mix;
            mix.clear();
            mix.extend(entries.map(|(a, t, wt)| {
                let a = w.names.str(a);
                if s.def_anim(a) {
                    MixArg { clip, time: si.time, w: f64::from(wt) }
                } else {
                    MixArg { clip: clips.get(a), time: f64::from(t), w: f64::from(wt) }
                }
            }));
            let loco = Loco { dur: f64::from(fi.anim.loco_blend.0), hermite: fi.anim.loco_blend.1, key };
            let rules = r.pose.as_deref_mut().map(|p| p as &mut dyn crate::view::PoseRules);
            v.pose(clips, clip, si.time, s.def_name(), si.blend_in, si.blend_out, STEP, hold, &loco, add, mix, rules);
        }
    } else {
        // The montage's own blend-out as the Slot node draws it, or a stopped montage's crossfade.
        let sm = mvs_sim::fighter::shown_montage(w, f).and_then(|dash| {
            let st = if dash { fi.montages.dash_montage.as_ref() } else { fi.montages.montage.as_ref() }?;
            let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main });
            Some((st, uid.and_then(|u| instance_data(&w.data, w, f, u))))
        });
        match sm {
            Some((st, m)) => {
                let wt = m.map_or(1.0, |m| st.shown_weight(m, &|s| eprintln!("warning: {s}")));
                if wt < 1.0 {
                    match nav {
                        Some(n) => v.slot_under(clips.get(n), 0.0, wt.max(0.0), NAV_KEY),
                        None => {
                            let (la, lt) = mvs_sim::fighter::anim::loco_clip(w, f);
                            v.slot_under(clips.get(w.names.str(la)), f64::from(lt), wt, ViewKey::Gen(fi.anim.loco_gen));
                        }
                    }
                }
            }
            // A montage stopped with a blend time stays the view's crossfade from the last shown pose.
            // Deviation: its pose is held, where the build's interrupted instance goes on advancing; a stop whose
            // entry is already gone (0 s) and a montage played over another are the view's crossfade.
            None => {
                if let Some(gone) = v.shown_montage()
                    && let Some(fd) = fi.montages.fading.iter().rev().find(|fd| fd.m.montage.name == gone)
                {
                    v.montage_gone(f64::from(fd.left + 1.0) * STEP);
                }
            }
        }
        let loco = match nav {
            Some(_) => Loco { dur: 0.0, hermite: false, key: NAV_KEY },
            None => Loco { dur: f64::from(fi.anim.loco_blend.0), hermite: fi.anim.loco_blend.1, key },
        };
        let mix = &mut r.mix;
        mix.clear();
        mix.extend(entries.map(|(a, t, wt)| MixArg {
            clip: clips.get(w.names.str(a)),
            time: f64::from(t),
            w: f64::from(wt),
        }));
        let (name, blend_in, blend_out) = match sm {
            Some((st, m)) => (Some(st.montage.name), m.map_or(0.0, |m| m.blend_in), m.map_or(0.0, |m| m.blend_out)),
            None => (None, 0.0, 0.0),
        };
        let anim = r.pose.as_ref().and_then(|p| p.anim(w, f)).unwrap_or(fi.anim.anim);
        let (clip, time) = match nav {
            Some(n) if sm.is_none() => (clips.get(n), 0.0),
            _ => (clips.get(w.names.str(anim)), f64::from(fi.anim.anim_time)),
        };
        let rules = r.pose.as_deref_mut().map(|p| p as &mut dyn crate::view::PoseRules);
        v.pose(clips, clip, time, name, blend_in, blend_out, STEP, hold, &loco, add, mix, rules);
    }
    // A fighter the step did not carry here from where the last tick left it (a respawn, a placement) is not drawn
    // on the way: the step's own start point is then not the last tick's end point.
    // BaseFighter's UI init: K2_AddLocalOffset(0, playerIndex, 0) on both widget components puts playerIndex into
    // RelativeLocation.Y under CharacterMesh0 (its scale applies); the actor's yaw -90 (right) / +90 (left) turns
    // local Y into UE +X / -X. A mode pawn's playerIndex is its slot's. Unverified: an unbound one's is taken as 0.
    let d = f64::from(facing) * f64::from(mvs_sim::world::pawns::slot_of(w, f).max(0)) * v.mesh_scale;
    let mut s = ShownAt {
        px: f64::from(hud.0),
        py: f64::from(hud.1),
        x: f64::from(hud.0),
        y: f64::from(hud.1),
        raw_x: f64::from(x),
        raw_y: f64::from(y),
        facing: f64::from(facing),
        d,
    };
    match was {
        Some(p) if r.visible && p.raw_x == f64::from(fi.body.prev_x) && p.raw_y == f64::from(fi.body.prev_y) => {
            s.px = p.x;
            s.py = p.y;
        }
        _ => v.cut(),
    }
    r.shown_at = Some(s);
    r.tick_component_meshes(clips, STEP);
}

/// `placeMontageMeshes`: a view's montage meshes follow the fighter's list; a mesh new to
/// the list is put on at the attach its notify gives while the fighter faces as it does now.
// Deviation: the transform and the view's bone are those of the facing at the first draw; the build's component stays
// on its socket through a turn, and a turn while a mesh is out is not followed here.
fn place_montage_meshes(r: &mut Rec, w: &World, draws: &FighterDraws, meshes: &MeshMap, clips: &mut Clips) {
    let fi = w.fighter(r.fid);
    let list = &fi.montages.montage_meshes;
    let data = &w.data.chars[&r.key].fighter;
    let anim_of = |a: Name| Some(w.names.str(a)).filter(|s| !s.is_empty());
    r.montage.retain(|m| list.iter().any(|e| e.mesh == m.name));
    for e in list {
        if let Some(mv) = r.montage.iter_mut().find(|m| m.name == e.mesh) {
            // AnimNotify_SubMeshAnimChange's PlayAnimation shows as another animation name on the entry
            if mv.anim != e.anim {
                if let Some(sk) = &mut mv.skin {
                    sk.clip = mesh_clip(clips, data, anim_of(e.anim));
                }
                mv.anim = e.anim;
            }
            pose_montage_mesh(&r.view, mv, f64::from(e.age), e.looping, clips);
            continue;
        }
        let name = w.names.str(e.mesh);
        let spawn = mvs_sim::registry::notifies::montage_mesh_spawn(w, &w.data, r.fid, name);
        // (a mesh the character's entry has no actor for, or no materials, is not drawn). A notify with a Materials
        // entry: BaseFighter Add Montage Mesh's SelectMaterialColor, the entry's team set for the colour (Harley's
        // jack-in-the-box); none: the mesh's own set for every team.
        // Deviation: the entry's sets are the actor table's, not read from the notify: Rick's montage Meeseeks takes
        // the actor's Meeseeks_MAT for the local player where its notify names Meeseeks_Blue_MIC, and its
        // M_GundamRick entry (C020_S03) is not applied.
        let teamed = spawn.and_then(|p| p.get("Materials")).and_then(Value::as_arr).is_some_and(|m| !m.is_empty());
        let Some((path, mats)) = draws.actor(name, teamed.then(|| r.colour.variant.key())) else { continue };
        let Some(info) = meshes.get(&path).cloned() else { continue };
        let skel = &w.data.chars[&r.key].fighter.skeleton;
        let socket = spawn.and_then(|p| p.get("Socket")).and_then(Value::as_str).unwrap_or(w.names.str(e.socket));
        let rig = &r.view.rig;
        let (bone, matrix, own) =
            montage_mesh_attach(skel, |n| rig.index(n).is_some(), spawn, socket, fi.body.facing_dir < 0.0);
        let bone = bone.and_then(|b| rig.index(&b));
        let mats = mats.iter().map(|m| m.as_deref().map(material)).collect();
        let skin =
            info.skeleton.is_some().then(|| SocketSkin::new(&info, clips, mesh_clip(clips, data, anim_of(e.anim))));
        let own = own.then_some(matrix);
        let mut mv = MontageView { name: e.mesh, info, mats, bone, matrix, skin, anim: e.anim, own, cs: Vec::new() };
        pose_montage_mesh(&r.view, &mut mv, f64::from(e.age), e.looping, clips);
        r.montage.push(mv);
    }
}

/// The tick's pose of a montage mesh (poseMontageMesh at the entry's age) and, for a pawn without a mirror node, its
/// holder's matrix (ownBoneLocal facing left: the attach parent follows a turn, the offset stays the spawn's).
// Unverified: B of ownBoneLocal is the bone's world matrix of the tick's pose; a bone object of the view holds the pose
// its last frame drew.
fn pose_montage_mesh(view: &CharView, mv: &mut MontageView, age: f64, looped: bool, clips: &mut Clips) {
    if let Some(sk) = &mut mv.skin {
        sk.pose_at(clips, age * STEP, looped);
    }
    // (a bone the view lacks: the holder hangs on the model, whose matrix is then B)
    if let Some(rel) = mv.own {
        mv.matrix = if view.facing_left {
            let bw = match mv.bone {
                Some(b) => {
                    view.shown_bones_cs(&mut mv.cs);
                    math::mul(&view.model_world(), &mv.cs[b])
                }
                None => view.model_world(),
            };
            // (the view root's depth: three x, always 0 here)
            own_bone_local(&bw, 0.0, &rel)
        } else {
            rel
        };
    }
}

/// A preview's AnimNotify_SubMeshSpawn (the montage meshes' building over a preview's
/// own state): the actor `mesh_name` named by the notify's properties `p` put on the view at the attach the notify
/// gives while the view faces `left`. False: already on, or the character's entry has no actor or materials for it.
// Unverified: the preview's skeleton is read without the pawn's "no mirror node" statement (C017): which component
// BP_CharacterPreview attaches to is not read.
#[allow(clippy::too_many_arguments)]
pub(crate) fn preview_mesh_add(
    r: &mut Rec,
    cd: &mvs_data::CharData,
    draws: &FighterDraws,
    meshes: &mut MeshMap,
    store: &mvs_store::Store,
    clips: &mut Clips,
    mesh: Name,
    mesh_name: &str,
    p: &mvs_data::Props,
    left: bool,
) -> Result<bool> {
    if r.montage.iter().any(|m| m.name == mesh) {
        return Ok(false);
    }
    let Some((path, mats)) = draws.actor(mesh_name, None) else { return Ok(false) };
    let info = match meshes.get(&path) {
        Some(i) => i.clone(),
        None => {
            let i = MeshInfo::shared(store, &path)?;
            meshes.insert(path, i.clone());
            i
        }
    };
    let mut skel = cd.fighter.skeleton.clone();
    skel.mirror_node = None;
    let rig = &r.view.rig;
    let socket = p.get("Socket").and_then(Value::as_str).unwrap_or("None");
    let (bone, matrix, own) = montage_mesh_attach(&skel, |n| rig.index(n).is_some(), Some(p), socket, left);
    let bone = bone.and_then(|b| rig.index(&b));
    let mats = mats.iter().map(|m| m.as_deref().map(material)).collect();
    let anim = p.get("Animation").and_then(Value::as_str).filter(|s| !s.is_empty());
    let skin = info.skeleton.is_some().then(|| SocketSkin::new(&info, clips, mesh_clip(clips, &cd.fighter, anim)));
    let own = own.then_some(matrix);
    r.montage.push(MontageView { name: mesh, info, mats, bone, matrix, skin, anim: mesh, own, cs: Vec::new() });
    Ok(true)
}

/// BP_CharacterPreview's Add Mesh Component for the skin's static SubMeshes on a socket (`CharRender::socket_statics`):
/// a StaticMeshComponent snapped to the main mesh's socket its Socket_ tag names (K2_AttachToComponent SnapToTarget,
/// @296-446), filed under its key (MainTag); each hidden until the preview shows it. False per tag the pawn has no
/// static component, mesh or materials of.
// Deviation: the store's skin looks keep the skeletal SubMeshes alone; the pawn's StaticMeshComponent of the same tag
// (its socket, its mesh, an identity relative transform, as the skins' entries: Finn's four skins list the sixteen
// teeth on their own sockets) stands in for the SubMeshes entry.
pub(crate) fn preview_statics_add(
    r: &mut Rec,
    cd: &mvs_data::CharData,
    draws: &FighterDraws,
    meshes: &mut MeshMap,
    store: &mvs_store::Store,
    clips: &Clips,
    tags: &[&str],
) -> Result<()> {
    for tag in tags {
        let comp = cd.fighter.components.pawn.values().find(|c| {
            c.ty == "StaticMeshComponent"
                && c.p
                    .as_obj()
                    .and_then(|p| p.get("ComponentTags"))
                    .and_then(Value::as_arr)
                    .is_some_and(|t| t.iter().any(|v| v.as_str().is_some_and(|v| v.eq_ignore_ascii_case(tag))))
        });
        let mesh = comp.and_then(|c| c.p.as_obj()?.get("StaticMesh")?.as_str());
        let socket = comp.and_then(|c| c.socket.as_deref()).filter(|s| *s != "None");
        let actor = mesh.and_then(|m| draws.actor(m, None));
        let (Some(socket), Some((path, keys))) = (socket, actor) else {
            eprintln!(
                "warning: preview {}: the static sub-mesh {tag} is not drawn (no component, socket or mesh)",
                r.key
            );
            continue;
        };
        let info = match meshes.get(&path) {
            Some(i) => i.clone(),
            None => {
                let i = match MeshInfo::shared(store, &path) {
                    Ok(i) => i,
                    Err(e) => {
                        eprintln!(
                            "warning: preview {}: the static sub-mesh {tag}'s mesh {path} is not read: {e}",
                            r.key
                        );
                        continue;
                    }
                };
                meshes.insert(path, i.clone());
                i
            }
        };
        let skel = &cd.fighter.skeleton;
        let at = [false, true]
            .map(|left| r.component_attach_on(skel, socket, [0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3], left));
        let mats = keys.iter().map(|m| m.as_deref().map(material)).collect();
        r.add_component_mesh(clips, &tag.to_lowercase(), at, info, mats, false);
    }
    Ok(())
}

/// RemoveMontageMesh: the mesh off the view.
pub(crate) fn preview_mesh_remove(r: &mut Rec, mesh: Name) {
    r.montage.retain(|m| m.name != mesh);
}

/// AnimNotify_SubMeshAnimChange on a preview: the mesh plays another animation.
pub(crate) fn preview_mesh_anim(
    r: &mut Rec,
    cd: &mvs_data::CharData,
    clips: &mut Clips,
    mesh: Name,
    anim: Option<&str>,
) {
    if let Some(mv) = r.montage.iter_mut().find(|m| m.name == mesh)
        && let Some(sk) = &mut mv.skin
    {
        sk.clip = mesh_clip(clips, &cd.fighter, anim.filter(|s| !s.is_empty()));
    }
}

/// poseMontageMesh of a preview's mesh at `age` seconds.
pub(crate) fn preview_mesh_pose(r: &mut Rec, clips: &mut Clips, mesh: Name, age: f64, looped: bool) {
    let Rec { view, montage, .. } = r;
    if let Some(mv) = montage.iter_mut().find(|m| m.name == mesh) {
        pose_montage_mesh(view, mv, age, looped, clips);
    }
}

/// `boneFlipRows`: a bone's BoneFlipAxis in the mirror table as the diagonal of its flip in UE
/// axes (None: the node writes the bone back unchanged).
pub(crate) fn bone_flip_rows(skel: &mvs_data::fighter::Skeleton, bone: &str) -> Option<[f64; 3]> {
    // Unverified: a bone the table does not list is taken as Y, self-partnered.
    let axis = skel.flip.as_ref().and_then(|f| f.get(bone)).map_or("Y", String::as_str);
    match axis {
        "X" => Some([-1.0, 1.0, 1.0]),
        "Y" => Some([1.0, -1.0, 1.0]),
        "Z" => Some([1.0, 1.0, -1.0]),
        "XYZ" => Some([-1.0, -1.0, -1.0]),
        "None" => None,
        _ => panic!("mirror table: BoneFlipAxis {axis} ({bone})"),
    }
}

/// `name` with every `from`_ (either case) made `to`_ (a case-insensitive replace of l_ and r_).
fn swap_side(name: &str, from: u8, to: char) -> String {
    let b = name.as_bytes();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].eq_ignore_ascii_case(&from) && b.get(i + 1) == Some(&b'_') {
            out.push(to);
            out.push('_');
            i += 2;
        } else {
            let c = name[i..].chars().next().expect("in bounds");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn has_side(name: &str, side: u8) -> bool {
    name.as_bytes().windows(2).any(|w| w[0].eq_ignore_ascii_case(&side) && w[1] == b'_')
}

/// `montageMeshAttach`: an AnimNotify_SubMeshSpawn's mesh on its socket (the mirrored side's
/// facing left), its Offset under the socket, reflected for the view's mirrored pose; the view bone it hangs from
/// (None: the model) and the matrix under it in the view's space.
fn montage_mesh_attach(
    skel: &mvs_data::fighter::Skeleton,
    has_bone: impl Fn(&str) -> bool,
    p: Option<&mvs_data::Props>,
    name: &str,
    left: bool,
) -> (Option<String>, Mat4, bool) {
    let obj = |k: &str| p.and_then(|p| p.get(k)).and_then(Value::as_obj);
    let num =
        |o: Option<&mvs_data::Props>, k: &str, d: f64| o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(d);
    let o = obj("Offset");
    let part = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_obj);
    let mirror = |a: Option<&mvs_data::Props>, k: &str| {
        let on = a.and_then(|a| a.get(&format!("Mirror{k}"))).is_some_and(|v| v.as_bool().unwrap_or(false));
        if left && on { -1.0 } else { 1.0 }
    };
    let (lm, rm, sm) = (obj("MirrorLocationAxis"), obj("MirrorRotationAxis"), obj("MirrorScaleAxis"));
    let t = part("Translation");
    let loc = ["X", "Y", "Z"].map(|k| num(t, k, 0.0) * mirror(lm, k));
    let r = part("Rotation");
    let quat = [num(r, "X", 0.0), num(r, "Y", 0.0), num(r, "Z", 0.0), num(r, "W", 1.0)];
    if left && ["X", "Y", "Z"].iter().any(|k| mirror(rm, k) < 0.0) && quat[..3].iter().any(|&q| q != 0.0) {
        eprintln!("warning: montage mesh: MirrorRotationAxis not ported ({name})");
    }
    let sc = part("Scale3D");
    let scale = ["X", "Y", "Z"].map(|k| num(sc, k, 1.0) * mirror(sm, k));
    attach_core(skel, has_bone, name, left, loc, quat, scale)
}

/// montageMeshAttach after its Offset is read (its mirror axes applied): the socket of `name` (its mirrored side's
/// facing left), the offset under it (its translation negated facing left), reflected for the view's mirrored pose.
fn attach_core(
    skel: &mvs_data::fighter::Skeleton,
    has_bone: impl Fn(&str) -> bool,
    name: &str,
    left: bool,
    loc: [f64; 3],
    quat: [f64; 4],
    scale: [f64; 3],
) -> (Option<String>, Mat4, bool) {
    let exists = |n: &str| n != "None" && (skel.sockets.contains_key(n) || has_bone(n));
    let (has_l, has_r) = (has_side(name, b'l'), has_side(name, b'r'));
    let swapped = if has_l { swap_side(name, b'l', 'R') } else { swap_side(name, b'r', 'L') };
    let cand = if left { swapped.as_str() } else { name };
    let socket = if exists(cand) && (has_l || has_r) {
        cand
    } else if exists(name) {
        name
    } else {
        "None"
    };
    let neg = if left && name != "None" { -1.0 } else { 1.0 };
    let mut m = math::compose(loc.map(|v| v * neg), quat, scale);
    let sk = if socket == "None" { None } else { skel.sockets.get(socket) };
    if let Some(sk) = sk {
        let q = math::rotator_quat(sk.rot[0], sk.rot[1], sk.rot[2]);
        m = math::mul(&math::compose(sk.loc, q, sk.scale), &m);
    }
    let bone = if socket == "None" { None } else { Some(sk.map_or(socket, |s| s.bone.as_str())) };
    // A pawn without a mirror node (the skeleton's mirror_node false, C017): the attach parent is CharacterMesh0's
    // bone of the rule's name, the view's bone of the same name reflected in depth. The matrix stays the socket and
    // the offset under that bone, with no flip and no mirrored bone; `own` tells the caller to put the reflection in
    // front each tick the view faces left (own_bone_local).
    let own = skel.mirror_node == Some(false) && bone.is_some();
    if left && !own {
        let f = bone.and_then(|b| bone_flip_rows(skel, b)).unwrap_or([1.0, -1.0, 1.0]);
        m = math::mul(&math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], f), &m);
    }
    const SWAP_YZ: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    m = math::mul(&math::mul(&SWAP_YZ, &m), &SWAP_YZ);
    for e in &mut m[12..15] {
        *e /= 100.0;
    }
    let bone = bone.map(|b| if left && !own { skel.mirror.get(b).map_or(b, String::as_str) } else { b });
    (bone.map(str::to_owned), m, own)
}

/// A skinned mesh's bones: per bone of its own reference skeleton, the followed bone's component-space transform
/// (UE cm) times the bone's inverse reference matrix, as FMatrix3x4, after `out`'s.
fn skin_bones(info: &MeshInfo, map: Option<&[usize]>, cs_ue: &[Mat4], out: &mut Vec<[f32; 12]>) {
    out.extend((0..info.inv_ref.len()).map(|k| {
        let b = map.map_or(k, |m| m[k]);
        math::rows3x4(&math::mul(&cs_ue[b], &info.inv_ref[k]))
    }));
}

/// A part's bones on its draw bones facing left (showPropMirror): bone k at its partner's bone times its flip scale.
fn mirror_bones(info: &MeshInfo, mirror: &[DrawBone], bones: &mut [[f32; 12]], cs: &[Mat4]) {
    for (k, m) in mirror.iter().enumerate() {
        let Some((partner, flip)) = m else { continue };
        let draw = math::mul(&cs[*partner], &math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], *flip));
        bones[k] = math::rows3x4(&math::mul(&math::to_ue(&draw), &info.inv_ref[k]));
    }
}

/// `setPropMirror`, with `turns` and no copied props: per part, the draw bone of each mirrored bone
/// it is skinned to. The mirror node's mirrored prop bones B draw at F_B x the view's bone partner(B) facing left (the
/// prop meshes, VisMesh children of BodyMesh with CharacterMesh0 as master pose, take two scales and so show the
/// node's mirrored bones unscaled), at B facing right. A section whose joints are all kept is drawn on the view's
/// bones; one weighted to mirrored and kept bones is refused.
// Unverified: not checked against the running game; a prop mesh on kept bones (the Trident: the body's picture
// reflected in depth) is not handled.
// Deviation: a section's mesh is bound to the draw bones when its own joints are mirrored and a kept-only
// section stays on the view's bones; a MeshPose has one bone list, so a part with both kinds of section draws every
// section on the draw bones (warned; the draw bones are only the mirrored joints, which a kept section does not use).
fn prop_mirror(
    parts: &[ViewMesh],
    part_bones: &[Vec<usize>],
    names: &[String],
    skel: &mvs_data::fighter::Skeleton,
) -> Result<Vec<Vec<DrawBone>>> {
    let axis_of = |n: &str| skel.flip.as_ref().and_then(|f| f.get(n)).map_or("Y", String::as_str);
    let mut out = Vec::with_capacity(parts.len());
    for (p, map) in parts.iter().zip(part_bones) {
        let info = &p.info;
        let own = |j: usize| names[map[j]].as_str();
        let mut draws = vec![None; map.len()];
        let mut any = false;
        let mut kept_only = false;
        for (k, used) in info.used_joints.iter().enumerate() {
            let mirrored = used.iter().filter(|&&j| axis_of(own(j)) != "None").count();
            if mirrored == 0 {
                kept_only |= !used.is_empty();
                continue;
            }
            if mirrored != used.len() {
                return Err(Error::Format {
                    what: format!("prop mirror: {} section {k}", info.path),
                    why: "weighted to mirrored and kept bones".into(),
                });
            }
            any = true;
            for &j in used {
                let n = own(j);
                let flip = crate::view::flip_scale(axis_of(n)).expect("a mirrored bone");
                let partner = skel.mirror.get(n).and_then(|m| names.iter().position(|x| x == m)).unwrap_or(map[j]);
                draws[j] = Some((partner, flip));
            }
        }
        if any && kept_only {
            eprintln!("warning: prop mirror: {} has sections on kept bones and on mirrored ones", info.path);
        }
        out.push(if any { draws } else { Vec::new() });
    }
    Ok(out)
}

/// What [`build_rec`] makes a view of: the fighter (FighterId, pawn id), its character's data, skin and colour, the
/// meshes loaded, its entry and pose hook maker, and the kind of view (a second view, a minion's).
pub(crate) struct RecMake<'a> {
    pub(crate) fid: FighterId,
    pub(crate) id: i32,
    pub(crate) key: String,
    pub(crate) cd: &'a mvs_data::CharData,
    pub(crate) skin: Skin,
    pub(crate) colour: FighterColour,
    pub(crate) body_info: Arc<MeshInfo>,
    pub(crate) part_infos: Vec<Arc<MeshInfo>>,
    pub(crate) render: &'static CharRender,
    pub(crate) minion: Option<&'static MinionView>,
    pub(crate) second: Option<SecondKind>,
    /// The pose hook made with PoseOf::slaved (a preview actor's view); a
    /// Disguise's copy is slaved in any case.
    pub(crate) slaved: bool,
}

/// `makeView`'s view of a character's skin from its loaded parts (ViewPoser::make_view's, and a preview's with no
/// World: crate::preview): the main mesh and parts with `draws`' materials in the colour's variant, the parts'
/// bone maps, the view on `clips` with the entry's pose hook, the armor overlay. The entry's `view` hook is the
/// caller's.
pub(crate) fn build_rec(m: RecMake<'_>, clips: &mut Clips, blend: &BlendSpaces, draws: &FighterDraws) -> Result<Rec> {
    let RecMake { fid, id, key, cd, skin, colour, body_info, part_infos, render, minion, second, slaved } = m;
    let variant = colour.variant;
    let mats = |l: Vec<Option<String>>| l.iter().map(|m| m.as_deref().map(material)).collect::<Vec<_>>();
    let body_mats = mats(draws.set(&skin.name, "", variant)?);
    let body_bones = body_info.bones().to_vec();
    let mut parts = Vec::new();
    let mut part_bones = Vec::new();
    for (k, p) in skin.parts.iter().enumerate() {
        let l = if p.own {
            draws.actor(&p.mesh, None).map(|a| a.1).unwrap_or_default()
        } else {
            draws.set(&skin.name, &p.key, variant)?
        };
        let info = part_infos[k].clone();
        // Deviation: a part's bones are matched to the main mesh's by name in any case, and one the main mesh lacks
        // follows its root bone; matching exactly would refuse a missing bone a vertex is weighted to.
        let map = info
            .bones()
            .iter()
            .map(|n| body_bones.iter().position(|b| b.eq_ignore_ascii_case(n)).unwrap_or(0))
            .collect();
        part_bones.push(map);
        parts.push(ViewMesh { mats: mats(l), info });
    }
    let rig = Rig::bind(clips, body_bones);
    let mesh_scale = minion.and_then(|m| m.mesh_scale).unwrap_or(cd.boxes.mesh_scale);
    let mut view = CharView::new(&body_info, rig, mesh_scale, cd.boxes.mesh_z);
    if render.turns {
        view.set_turns();
    }
    let part_keys: Vec<String> = skin.parts.iter().map(|p| p.key.clone()).collect();
    view.part_on = skin.part_init.clone();
    let make = minion.map_or(render.pose, |m| m.pose);
    let pose = make.map(|make| {
        let slaved = slaved || second == Some(SecondKind::Disguise);
        let stand_in = second == Some(SecondKind::Polymorph);
        make(&mut view, clips, &PoseOf { skin: &skin, part_keys: &part_keys, blend, slaved, stand_in })
    });
    view.head_step = HeadScaleStep::new(&view.rig.names, render.head_scale);
    let part_mirror = if render.turns && !render.copied_props {
        prop_mirror(&parts, &part_bones, &view.rig.names, &cd.fighter.skeleton)?
    } else {
        vec![Vec::new(); parts.len()]
    };
    let mut part_scale = vec![Vec::new(); parts.len()];
    for (k, list) in &view.part_bone_scale {
        let bones = parts[*k].info.bones();
        part_scale[*k] = list.iter().filter_map(|(n, s)| Some((bones.iter().position(|b| b == n)?, *s))).collect();
    }
    let mut armor = material(if colour.armor_local { skins::ARMOR_OVERLAY_LOCAL } else { skins::ARMOR_OVERLAY });
    set_scalar(&mut armor, "Team", colour.armor_team);
    Ok(Rec {
        fid,
        id,
        key: key.clone(),
        flash: draws.skinned(&skin.name),
        skin,
        view,
        colour,
        body: ViewMesh { info: body_info, mats: body_mats },
        parts,
        part_bones,
        part_mirror,
        part_scale,
        armor,
        armor_on: false,
        armor_main: None,
        props: Vec::new(),
        montage: Vec::new(),
        components: Vec::new(),
        visible: true,
        outlines_on: true,
        shown_at: None,
        flash_left: 0.0,
        intro_glow: -1.0,
        rage_shirt: render.rage_shirt.and_then(|n| clips.get(n)),
        pose,
        pawn: None,
        priority: None,
        own: Vec::new(),
        minion: minion.map(|def| MinionRec { def, creator: fid, live: None }),
        heart: None,
        morphs: Vec::new(),
        intro: None,
        intro_vis: Vec::new(),
        mix: Vec::new(),
    })
}

/// The drawn meshes of a view: the main mesh (unless only parts are kept), its shown parts, the armor overlay
/// copies, and the props shown.
pub(crate) fn meshes_of(r: &Rec, cs: &[Mat4], cs_ue: &[Mat4]) -> Vec<MeshPose> {
    let mut out = Vec::new();
    meshes_into(r, cs, cs_ue, &mut out);
    out
}

/// A list of mesh poses written over in place: each `next` the following one, made when the list is shorter.
struct PoseList<'a> {
    v: &'a mut Vec<MeshPose>,
    n: usize,
}

impl PoseList<'_> {
    /// The next pose, its earlier values still in it (each caller sets every field).
    fn next(&mut self) -> &mut MeshPose {
        if self.n == self.v.len() {
            self.v.push(MeshPose::default());
        }
        self.n += 1;
        &mut self.v[self.n - 1]
    }
}

/// `p` set to a mesh's pose of no bones yet, no morphs, LOD 0: the path (made again only when it changes), the
/// transform, the stencil and the capture flag.
fn pose_head(p: &mut MeshPose, path: &str, to_world: [f32; 12], stencil: u8, capture_skip: bool) {
    if *p.mesh != *path {
        p.mesh = Arc::from(path);
    }
    p.lod = 0;
    p.to_world = to_world;
    p.custom_stencil = stencil;
    p.capture_skip = capture_skip;
    p.bones.clear();
    p.shown.clear();
    p.morphs.clear();
}

/// `ms` set to `n` material refs, slot k's by `f` on the ref already there.
fn set_materials(ms: &mut Vec<MaterialRef>, n: usize, mut f: impl FnMut(usize, &mut MaterialRef)) {
    ms.truncate(n);
    ms.resize_with(n, MaterialRef::default);
    for (k, m) in ms.iter_mut().enumerate() {
        f(k, m);
    }
}

/// `d` a copy of `s`, in `d`'s room.
fn copy_ref(d: &mut MaterialRef, s: &MaterialRef) {
    d.path.clone_from(&s.path);
    d.scalars.clone_from(&s.scalars);
    d.vectors.clone_from(&s.vectors);
    d.textures.clone_from(&s.textures);
}

/// `d` the material at `path` with no parameters (`material`; "" the default ref).
fn path_ref(d: &mut MaterialRef, path: &str) {
    if *d.path != *path {
        d.path = Arc::from(path);
    }
    d.scalars.clear();
    d.vectors.clear();
    d.textures.clear();
}

/// `meshes_of` into `out`, its poses' buffers kept.
pub(crate) fn meshes_into(r: &Rec, cs: &[Mat4], cs_ue: &[Mat4], out: &mut Vec<MeshPose>) {
    let model = r.view.model_world();
    let to_world = math::rows3x4(&math::to_ue(&model));
    let z_flip = math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, -1.0]);
    let flipped_y = math::rows3x4(&math::to_ue(&math::mul(&model, &z_flip)));
    let stencil = if r.outlines_on { r.colour.stencil } else { 0 };
    let mut out = PoseList { v: out, n: 0 };
    // (a skinned mesh's pose: its bones on the followed bones `map`)
    let skinned = |p: &mut MeshPose, m: &ViewMesh, map: Option<&[usize]>, stencil: u8| {
        pose_head(p, &m.info.path, to_world, stencil, false);
        skin_bones(&m.info, map, cs_ue, &mut p.bones);
    };
    // (a slot with no material built, Superman_Cornea_Mat's, is not drawn: it names the mesh slot's own material,
    // as the pose tape and the renderer's fallback do)
    let own = |p: &mut MeshPose, m: &ViewMesh| {
        set_materials(&mut p.materials, m.mats.len(), |k, d| match &m.mats[k] {
            Some(x) => copy_ref(d, x),
            None => path_ref(d, m.info.slots.get(k).and_then(|s| s.material.as_deref()).unwrap_or("")),
        });
    };
    // (a section is drawn when its slot has a material)
    let drawn = |p: &mut MeshPose, m: &ViewMesh| {
        p.shown.extend(m.info.sections.iter().map(|s| !s.disabled && m.mats.get(s.slot).is_some_and(Option::is_some)));
    };
    if !r.view.parts_only {
        let body = out.next();
        skinned(body, &r.body, None, stencil);
        drawn(body, &r.body);
        own(body, &r.body);
        // Unverified: the morphs move the main mesh alone, not its armor overlay copy (no character with a morph has
        // armor).
        body.morphs.clone_from(&r.morphs);
    }
    // (a part on draw bones, setPropMirror: on its partners' facing left)
    let part = |m: &mut MeshPose, k: usize, p: &ViewMesh, stencil: u8| {
        skinned(m, p, Some(&r.part_bones[k]), stencil);
        // (setPartBoneScale: the bone's draw bone, scaled in its own axes, with the same inverse bind matrix)
        for &(j, s) in &r.part_scale[k] {
            let draw = math::mul(&cs[r.part_bones[k][j]], &math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], s));
            m.bones[j] = math::rows3x4(&math::mul(&math::to_ue(&draw), &p.info.inv_ref[j]));
        }
        if r.view.facing_left && !r.part_mirror[k].is_empty() {
            mirror_bones(&p.info, &r.part_mirror[k], &mut m.bones, cs);
            // (the prop mesh scaled across the screen axis, `p.mesh.scale.z = -z`: the view's z is UE's Y, so the
            // world matrix takes the reflection and each bone its inverse, the bone matrix's second row negated; the
            // vertices stay, the winding and the tangent sign are then those of a mesh that is not reflected)
            m.to_world = flipped_y;
            for b in &mut m.bones {
                for v in &mut b[4..8] {
                    *v = -*v;
                }
            }
        }
    };
    for (k, p) in r.parts.iter().enumerate() {
        if r.view.part_on[k] {
            let m = out.next();
            part(m, k, p, stencil);
            drawn(m, p);
            own(m, p);
        }
    }
    // Deviation: the main mesh's overlay copy leaves out a disabled section, where the view draws it (no Shaggy mesh
    // has one).
    // The armor overlay copies (BaseFighter UpdateNewSkelmeshes): the overlay on every slot, every main mesh section
    // and each drawn section of a part that receives armor, with its part; translucent, so no custom depth.
    if r.armor_on {
        let all = |d: &mut MeshPose, m: &ViewMesh| {
            set_materials(&mut d.materials, m.info.slots.len(), |_, x| copy_ref(x, &r.armor));
        };
        let main = |slot: usize| r.armor_main.as_ref().is_none_or(|m| m.get(slot).copied().unwrap_or(false));
        if r.body.info.sections.iter().any(|s| !s.disabled && main(s.slot)) {
            let body = out.next();
            skinned(body, &r.body, None, 0);
            body.shown.extend(r.body.info.sections.iter().map(|s| !s.disabled && main(s.slot)));
            // (a slot the copy leaves out, Superman_Cornea's: no section of it is copied, so the copy names the mesh
            // slot's own material, as the pose tape writes a slot with no mesh in the copy)
            let copied = |k: usize| r.body.info.sections.iter().any(|s| s.slot == k && main(k));
            let slot = |k: usize| r.body.info.slots.get(k).and_then(|s| s.material.as_deref()).unwrap_or("");
            set_materials(&mut body.materials, r.body.info.slots.len(), |k, d| {
                if copied(k) {
                    copy_ref(d, &r.armor);
                } else {
                    path_ref(d, slot(k));
                }
            });
        }
        for (k, p) in r.parts.iter().enumerate() {
            // Unverified: a part's armor copy on its draw bones (none of the turns characters' parts takes armor)
            let on = match r.armor_main {
                None => r.view.part_on[k] && r.skin.part_armor[k].is_some(),
                Some(_) => r.skin.part_armor[k] == Some(skins::PartArmor::Primary),
            };
            if on {
                let m = out.next();
                part(m, k, p, 0);
                drawn(m, p);
                all(m, p);
            }
        }
    }
    // props: the right attach, or the left one facing left; hidden with the main mesh when only parts are kept
    if !r.view.parts_only {
        for (p, pm) in r.view.props.iter().zip(&r.props) {
            if !p.on {
                continue;
            }
            let local = match (&p.left, r.view.facing_left) {
                (Some(l), true) => l,
                _ => &p.right,
            };
            let m = math::mul(&math::mul(&model, &cs[p.bone]), local);
            // (a held item is its own actor: not in the fighter's priority capture)
            let d = out.next();
            pose_head(d, &pm.info.path, math::rows3x4(&math::to_ue(&m)), stencil, p.tag == "HeldItem");
            d.shown.extend(pm.info.sections.iter().map(|s| !s.disabled));
            set_materials(&mut d.materials, pm.mats.len(), |k, x| copy_ref(x, &pm.mats[k]));
        }
    }
    // (a mesh's slot materials, a slot with none the default ref; its sections drawn where the slot has one)
    let slot_mats = |d: &mut MeshPose, info: &MeshInfo, mats: &[Option<MaterialRef>]| {
        d.shown.extend(info.sections.iter().map(|s| !s.disabled && mats.get(s.slot).is_some_and(Option::is_some)));
        set_materials(&mut d.materials, mats.len(), |k, x| match &mats[k] {
            Some(m) => copy_ref(x, m),
            None => path_ref(x, ""),
        });
    };
    // (an instance's own bones under the holder, the instance root (identity))
    let own_bones = |d: &mut MeshPose, bones: &[Mat4], info: &MeshInfo| {
        d.bones.extend(bones.iter().zip(&info.inv_ref).map(|(b, inv)| math::rows3x4(&math::mul(&math::to_ue(b), inv))));
    };
    // the montage meshes: a static one has no custom depth copy, a skinned one has the fighter's (addMontageMesh's
    // `!asset.skinned` noOutline); hidden with the main mesh when only parts are kept
    if !r.view.parts_only {
        for mv in &r.montage {
            let m = match mv.bone {
                Some(b) => math::mul(&math::mul(&model, &cs[b]), &mv.matrix),
                None => math::mul(&model, &mv.matrix),
            };
            let d = out.next();
            let stencil = if mv.skin.is_some() { stencil } else { 0 };
            pose_head(d, &mv.info.path, math::rows3x4(&math::to_ue(&m)), stencil, false);
            // a skinned one: its own bones under the holder (the instance root, identity)
            if let Some(sk) = &mv.skin {
                own_bones(d, &sk.bones_cs(), &mv.info);
            }
            slot_mats(d, &mv.info, &mv.mats);
        }
    }
    // the component meshes shown, each with the fighter's stencil, the static ones too (Comp_Pawn_MeshManager AddMesh
    // @1291-1332 gives every tagged mesh child of the main mesh SetCustomDepthStencilValue and SetRenderCustomDepth;
    // no component the views add is tagged NoOutline); hidden with the main mesh when only parts are kept
    if !r.view.parts_only {
        for c in r.components.iter().filter(|c| c.on) {
            let at = &c.at[usize::from(r.view.facing_left)];
            let mats = c.part.and_then(|k| r.parts.get(k)).map_or(&c.mats, |p| &p.mats);
            let d = out.next();
            match &c.master {
                Some(map) => {
                    pose_head(d, &c.info.path, to_world, stencil, false);
                    skin_bones(&c.info, Some(map), cs_ue, &mut d.bones);
                }
                None => {
                    let m = match at.bone {
                        Some(b) => math::mul(&math::mul(&model, &cs[b]), &at.matrix),
                        None => math::mul(&model, &at.matrix),
                    };
                    pose_head(d, &c.info.path, math::rows3x4(&math::to_ue(&m)), stencil, false);
                    if let Some(sk) = &c.skin {
                        own_bones(d, &sk.bones_cs(), &c.info);
                    }
                }
            }
            slot_mats(d, &c.info, mats);
        }
    }
    let n = out.n;
    out.v.truncate(n);
}
