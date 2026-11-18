//! The spawned actors' meshes: each projectile's and
//! item's view, made at its first sight and placed where the actor is between the last two ticks.
//!
//! The generic spawned-actor path's shapes, which the class rules of the character modules (crate::chars) draw with:
//! - [`ActorComponents`]: a class's template components, resolved once per class;
//! - [`ActorPlace`] and [`actor_place`]: where a spawned actor is between the last two ticks, [`actor_rot`] its rows;
//! - [`MeshRule`] and [`ActorMesh`]: a class's own drawing of its mesh components, looked up by [`mesh_rule`];
//! - [`ActorHost`]: what a rule draws with (the meshes, the draws, the colours, the clips, the fighters' sockets);
//! - the views in crate::actor_view: [`StaticView`](crate::actor_view::StaticView),
//!   [`SkinnedActor`](crate::actor_view::SkinnedActor) and the component placement (`comp_of`, `place_under`).
//!
//! A rule calls these rather than copying them: `mvs_sim::fighter::common::actor_def` (a class's data through its
//! owner, a peer or a perk part), `mvs_sim::fighter::common::actor_def_owner` (the fighter whose data has it),
//! `mvs_sim::actors::projectile::attach_parent` (a component's attach parents), `crate::skins::FighterDraws::actor`
//! (a class's mesh and materials of a variant), `crate::skins::skin_of` (the owner's skin). The particle side
//! (mvs-render fx) adds `team`, `socket_follow` and `spawn_follow`.
//!
//! ShaggySandwichProjectile: StaticMesh 'Sandwich' at relative Z -47 with RelativeScale3D under a root whose world
//! scale the mesh inherits; the actor yaws to its facing and pitches with the velocity (bRotationFollowsVelocity). No
//! custom depth: nothing in its class chain (ShaggySandwichProjectile -> ParentProjectilePhysics ->
//! SpawnedActorParent -> Actor) sets it; only Comp_Pawn_MeshManager does. Item_Sandwich: the same mesh as component
//! Geo, with the rotation it was spawned with; hidden by the death blink, and while held drawn as the holder's
//! HeldItem prop instead.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::fighter::ActorDef;
use mvs_data::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::item::{Item, ItemExt};
use mvs_sim::actors::projectile::{AttachParentOut, Projectile, attach_parent};
use mvs_sim::actors::{ActorRef, ItemId, ProjectileId};
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::elem::ElemState;
use mvs_sim::fighter::FighterId;
use mvs_sim::names::Name;
use mvs_sim::registry::entries::ActorHooks;
use mvs_vfx::math::{rotation_rows, rotator_rows};

use crate::actor_view::{SkinnedActor, StaticView, Xform, item_rows, xyz};
use crate::anim::Clips;
use crate::blend::BlendSpaces;
use crate::frame::{ActorPose, MaterialRef, MeshPose};
use crate::items::ItemAssets;
use crate::math::{self, Mat4};
use crate::mesh::MeshInfo;
use crate::pose::MeshMap;
use crate::skins::{FighterColour, FighterDraws, Variant};

/// UE rotation rows (the rows are the world directions of the local X, Y and Z).
pub type Rows = [f64; 9];

// ---- a class's components ---------------------------------------------------------------------------------------

/// One component of a class's template, as a drawing takes it (its place and the component's own fields).
#[derive(Clone, Debug)]
pub struct Component {
    /// Its name in the class's components.
    pub name: String,
    pub kind: ComponentKind,
    /// Its attach parents' transform under the actor's root (`attach_parent`).
    pub parent: AttachParentOut,
    /// RelativeLocation, RelativeRotation as rows, RelativeScale3D.
    pub loc: [f64; 3],
    pub rot: Rows,
    pub scale: [f64; 3],
    /// bVisible and not bHiddenInGame.
    pub shown: bool,
}

/// What a component draws.
#[derive(Clone, Debug)]
pub enum ComponentKind {
    /// A StaticMeshComponent: its mesh and OverrideMaterials (None: the mesh slot's own).
    StaticMesh { mesh: String, materials: Vec<Option<String>> },
    /// A SkeletalMeshComponent: its mesh, the owner's skin sub-mesh key it is (None: an actor mesh of its own), and the
    /// sequence an AnimationSingleNode component plays.
    SkeletalMesh { mesh: String, sub_mesh: Option<String>, anim: Option<String> },
    /// A ParticleSystemComponent's template.
    ParticleSystem { template: String },
    /// A DecalComponent: DecalMaterial and DecalSize (UE's default (128, 256, 256) where unset).
    Decal { material: String, size: [f64; 3] },
}

/// A class's template components that draw, in the class data's order (`actor_def(..).components`).
#[derive(Clone, Debug, Default)]
pub struct ActorComponents {
    pub components: Vec<Component>,
    /// SpawnedActorParent SetTeamColor's Materials sets: the class puts a team set on its first mesh's slot 0.
    pub team_colour: bool,
}

/// An object path's or a reference's asset name (`/Game/A/B.B` -> `B`).
fn asset_name(s: &str) -> &str {
    let s = s.rsplit('/').next().unwrap_or(s);
    s.split('.').next_back().unwrap_or(s).trim_end_matches('\'')
}

impl ActorComponents {
    /// A class's components from its data, `skin` the default skin's sub-meshes (key, mesh name) of the character
    /// whose data has the class (a SkeletalMesh component that is one of them is that sub-mesh).
    // Unverified: team_colour reads the class's `Materials` (SpawnedActorParent's SetTeamColor sets) as present and
    // not empty; the graphs that call SetTeamColor are not each read.
    pub fn resolve(def: &ActorDef, skin: &[(String, String)]) -> ActorComponents {
        let cs = &def.components;
        let mut out = Vec::new();
        for (name, c) in cs.iter() {
            let Some(c) = c.as_obj() else { continue };
            let s = |k: &str| c.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && *s != "None");
            let kind = if let Some(m) = s("StaticMesh") {
                let materials = c
                    .get("OverrideMaterials")
                    .and_then(Value::as_arr)
                    .map(|a| a.iter().map(|v| v.as_str().filter(|s| *s != "None").map(str::to_owned)).collect())
                    .unwrap_or_default();
                ComponentKind::StaticMesh { mesh: m.to_owned(), materials }
            } else if let Some(m) = s("SkeletalMesh") {
                let n = asset_name(m);
                let sub_mesh = skin.iter().find(|(_, mesh)| mesh.eq_ignore_ascii_case(n)).map(|(k, _)| k.clone());
                let anim = c
                    .get("AnimationData")
                    .and_then(Value::as_obj)
                    .and_then(|a| a.get("AnimToPlay"))
                    .and_then(Value::as_str)
                    .filter(|s| *s != "None")
                    .map(str::to_owned);
                ComponentKind::SkeletalMesh { mesh: m.to_owned(), sub_mesh, anim }
            } else if let Some(t) = s("Template") {
                ComponentKind::ParticleSystem { template: t.to_owned() }
            } else if let Some(m) = s("DecalMaterial") {
                ComponentKind::Decal { material: m.to_owned(), size: xyz(c.get("DecalSize"), f64::NAN) }
            } else {
                continue;
            };
            let kind = match kind {
                ComponentKind::Decal { material, size } => {
                    let d = [128.0, 256.0, 256.0];
                    ComponentKind::Decal {
                        material,
                        size: std::array::from_fn(|k| if size[k].is_nan() { d[k] } else { size[k] }),
                    }
                }
                k => k,
            };
            let r = c.get("RelativeRotation").and_then(Value::as_obj);
            let g = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
            out.push(Component {
                name: name.clone(),
                kind,
                parent: attach_parent(cs, name),
                loc: xyz(c.get("RelativeLocation"), 0.0),
                rot: rotator_rows([g("Pitch"), g("Yaw"), g("Roll")], None),
                scale: xyz(c.get("RelativeScale3D"), 1.0),
                shown: c.get("bVisible").and_then(Value::as_bool) != Some(false)
                    && c.get("bHiddenInGame").and_then(Value::as_bool) != Some(true),
            });
        }
        let team_colour = def.p.get("Materials").is_some_and(|m| m.as_obj().is_some_and(|o| !o.is_empty()));
        ActorComponents { components: out, team_colour }
    }

    /// The component named `name`.
    pub fn get(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}

// ---- an actor's place -------------------------------------------------------------------------------------------

/// Where a spawned actor is drawn: its sim place at the last two ticks,
/// its root's rotation and world scale, and whether its root's children are hidden.
#[derive(Clone, Copy, Debug)]
pub struct ActorPlace {
    pub x: f64,
    pub y: f64,
    pub prev_x: f64,
    pub prev_y: f64,
    /// actorRot (a projectile: a class rule's `rot`, else pitch and the yaw of its pass's side) or the item rows (yaw by
    /// facing, pitch).
    pub rot: Rows,
    pub scale: f64,
    /// The root's children hidden: a tunnel carry (carryHidden), Item_Safe's Capsule.SetVisibility(false, true).
    pub hidden: bool,
    /// The fighter an item hangs on by its hold socket (Comp_ItemManager.EquipOnSpawn: held, not holstered); its x, y
    /// are then the holder's mesh's.
    pub holder: Option<FighterId>,
}

impl ActorPlace {
    /// The place between the last tick (`alpha` 0) and the latest (1).
    pub fn at(&self, alpha: f64) -> (f64, f64) {
        (self.prev_x + (self.x - self.prev_x) * alpha, self.prev_y + (self.y - self.prev_y) * alpha)
    }
}

/// `actorRot`: a projectile's rotation rows, its class rule's `rot`, else its pitch and the yaw of its pass's side
/// (0 going straight up or down, and at rest while it looks along its velocity).
pub fn actor_rot(w: &World, pr: &Projectile) -> Rows {
    use mvs_sim::actors::projectile::{aim_down_on, yaw_x};
    if let Some(r) = mesh_rule(w.names.str(pr.cls)).and_then(|r| r.rot).and_then(|f| f(w, pr)) {
        return r;
    }
    let own_tick = w
        .registry
        .actors
        .get(w.names.str(pr.cls))
        .is_some_and(|e| matches!(&e.hooks, ActorHooks::Projectile(h) if h.own_tick));
    let at_rest = pr.vx == 0.0 && pr.vy == 0.0 && aim_down_on(w, pr) && !pr.attached && !own_tick;
    let h = std::f64::consts::FRAC_PI_2;
    let yaw = if (pr.vx == 0.0 && pr.vy != 0.0) || at_rest {
        0.0
    } else if yaw_x(pr) > 0.0 {
        -h
    } else {
        h
    };
    rotation_rows(f64::from(pr.pitch).to_radians(), yaw, 0.0)
}

/// An item's root hidden with its children: a tunnel carry, Item_Safe's Thrown.
fn item_root_hidden(it: &Item) -> bool {
    let capsule = matches!(&it.ext, ItemExt::SafeItem(s) if s.capsule_hidden);
    (it.owner.is_some() && carry_hidden(it.elem.as_ref())) || capsule
}

/// Whether item `it`'s actor is shown: held and not holstered, unless hidden (the DeathWarning
/// blink, a Gem's fade: Item::hidden); else with its root shown too and, held, holstered.
pub fn item_shown(w: &World, it: &Item) -> bool {
    let holder = (it.holder >= 0).then(|| w.by_id(it.holder)).flatten();
    match holder {
        Some(h) if !w.fighter(h).items.item_holstered => !it.hidden(),
        Some(_) | None => !it.hidden() && !item_root_hidden(it),
    }
}

/// A spawned actor's place now (None: no such actor, or one with no place of its own: a fighter, a stage actor, a
/// buff).
// Unverified: a World actor and the other families have no previous place in the sim (drawn at the latest tick's)
// and no rotation of their own here (identity rows, scale 1): their character modules place them.
pub fn actor_place(w: &World, a: ActorRef) -> Option<ActorPlace> {
    match a {
        ActorRef::Projectile(id) => {
            let pr = w.actors.projectiles.get(id)?;
            Some(ActorPlace {
                x: f64::from(pr.x),
                y: f64::from(pr.y),
                prev_x: f64::from(pr.prev_x),
                prev_y: f64::from(pr.prev_y),
                rot: actor_rot(w, pr),
                scale: f64::from(pr.scale),
                hidden: pr.owner.is_some() && carry_hidden(pr.elem.as_ref()),
                holder: None,
            })
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id)?;
            let hf = (it.holder >= 0).then(|| w.by_id(it.holder)).flatten();
            let (x, y, px, py) = match hf {
                Some(h) => {
                    let (x, y) = (mvs_sim::fighter::mesh_wx(w, h), mvs_sim::fighter::mesh_wy(w, h));
                    (x, y, x, y)
                }
                None => (it.x, it.y, it.prev_x, it.prev_y),
            };
            Some(ActorPlace {
                x: f64::from(x),
                y: f64::from(y),
                prev_x: f64::from(px),
                prev_y: f64::from(py),
                rot: item_rows(it),
                scale: f64::from(it.scale),
                hidden: item_root_hidden(it),
                holder: hf.filter(|&h| !w.fighter(h).items.item_holstered),
            })
        }
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
        _ => {
            let (x, y, _) = mvs_sim::actors::world_actor::actor_place(w, a)?;
            Some(ActorPlace {
                x: f64::from(x),
                y: f64::from(y),
                prev_x: f64::from(x),
                prev_y: f64::from(y),
                rot: crate::actor_view::IDENT,
                scale: 1.0,
                hidden: false,
                holder: None,
            })
        }
    }
}

// ---- class rules ------------------------------------------------------------------------------------------------

/// A fighter's view bones as last drawn: the model's world and each bone's component-space matrix (the view's
/// space), by the rig's bone names (ActorHost::socket).
#[derive(Debug, Default)]
pub struct FighterBones {
    pub id: i32,
    pub names: Vec<String>,
    pub model: Mat4,
    pub cs: Vec<Mat4>,
    /// Per bone: one the view keeps unmirrored (CharView::kept).
    pub kept: Vec<bool>,
    /// Its skinned montage meshes as last drawn (ActorHost::montage_bone).
    pub montage: Vec<MontageBones>,
}

/// A skinned montage mesh of a fighter's view as last drawn: its entry's mesh name, the mesh, its UE world matrix and
/// its bones' component-space matrices (the view's space).
#[derive(Debug)]
pub struct MontageBones {
    pub name: Name,
    pub info: Arc<MeshInfo>,
    pub world: Mat4,
    pub cs: Vec<Mat4>,
}

/// A mesh component's drawing asset of the items part: its mesh's object
/// path in the mesh kind and its header.
#[derive(Clone, Debug)]
pub struct ItemAsset {
    pub path: Arc<str>,
    pub info: Arc<MeshInfo>,
}

/// What a class rule draws with: the match's meshes, each character's draws and clips
/// by key, the fighters' colours (by pawn id) and their views' bones.
#[derive(Debug)]
pub struct ActorHost<'a> {
    pub meshes: &'a MeshMap,
    pub draws: &'a HashMap<String, std::sync::Arc<FighterDraws>>,
    pub colours: &'a [FighterColour],
    pub clips: &'a mut HashMap<String, Clips>,
    pub fighters: &'a [FighterBones],
    /// Each character's entry's blend space players bound (CharRender.blend_spaces), by key.
    pub blend: &'a HashMap<String, BlendSpaces>,
    /// The camera: its actor's root (no shake) and its up axis in the world (ViewPoser::set_camera; None: none given).
    pub camera: Option<([f64; 3], [f64; 3])>,
    /// The HUD's palette, by palette index (PaletteForIndex 0..4); empty: no HUD (no UI broker).
    pub palette: &'a [crate::skins::Rgba],
    /// MPC_CentralPalette's ColorMe as the match's world has it (the colorMe binding's Game colour,
    /// skins::PaletteRows::color_me); None: no HUD bundle to read it from.
    pub color_me: Option<crate::skins::Rgba>,
    /// A relay match (PlayingOnline).
    pub online: bool,
    /// The items part's drawing assets (a match with random items; None: a match without).
    pub items: Option<&'a mut ItemAssets>,
}

impl ActorHost<'_> {
    /// `brokerPlayer`'s colour for fighter `f`: GetUIBrokerForFighter's FocusColor when its IsLocal, else its
    /// TeamColor; None: no broker (no HUD).
    pub fn broker_colour(&self, w: &World, f: FighterId) -> Option<crate::skins::Rgba> {
        let c = self.colour(w, f);
        self.palette.get(if c.broker_local { c.focus_palette } else { c.team_palette }).copied()
    }

    /// SpawnedActorParent SetTeamColor's ApplyLocalUserColor branch (its
    /// graph @21071-21413 skeletal, @21590-21824 static): with the class default's ApplyLocalUserColor (`p`, the
    /// class data's properties) and a locally controlled instigator `f`, slot 0 becomes a dynamic instance of the
    /// picked material with the vector User Color = LocalUserColor, which BeginPlay set to GetPaletteEntry(2).Game
    /// (@9029-9077): the Me entry, MPC_CentralPalette's ColorMe. Called once, when the view is made.
    // Deviation: User Color is written on every slot's instance (StaticView::set_vector), where the graph makes a
    // dynamic instance of slot 0 alone (no difference on bugs-combat's tape).
    pub fn apply_local_user_colour(&self, w: &World, view: &mut StaticView, p: &Props, f: FighterId) {
        let on = p.get("ApplyLocalUserColor").and_then(Value::as_bool);
        if on == Some(true)
            && self.local_human(w, f)
            && let Some(c) = self.color_me
        {
            view.set_vector("User Color", c);
        }
    }

    /// MatchHost.localHuman: fighter `f`'s FighterColour::local_human, by its slot (a mode pawn bound to the player's
    /// slot is the player's); a pawn of no slot (a bag, an unbound demo pawn) is no human, as the build's
    /// IsLocallyControlled of the pawn's controller.
    // Unverified: a demo pawn the tutorial drives is taken as the local human's (its controller is the sim's, off the
    // snapshot).
    fn local_human(&self, w: &World, f: FighterId) -> bool {
        let i = usize::try_from(mvs_sim::world::pawns::slot_of(w, f)).ok();
        match i.and_then(|i| self.colours.get(i)) {
            Some(c) => c.local_human,
            None => false,
        }
    }

    /// Fighter `f`'s colours (its pawn id's slot; a pawn of no slot takes the second's, as the views).
    pub fn colour(&self, w: &World, f: FighterId) -> FighterColour {
        let i = usize::try_from(mvs_sim::world::pawns::slot_of(w, f)).ok();
        *i.and_then(|i| self.colours.get(i)).or(self.colours.get(1)).unwrap_or(&self.colours[0])
    }

    /// `variant`: SetTeamColor's material pick, IsLocalOnlinePlayer (the colour value 0.6) local, else the colour
    /// index 0 red, any other blue.
    // Unverified: an index past 1 (a third or fourth team) is taken as blue.
    pub fn variant(&self, w: &World, f: FighterId) -> &'static str {
        let c = self.colour(w, f);
        if c.team == 0.6 {
            "local"
        } else if c.color_index == 0 {
            "red"
        } else {
            "blue"
        }
    }

    /// `selectedVariant`: the variant fighter `f`'s own skin takes, the menu's as the local one.
    pub fn selected_variant(&self, w: &World, f: FighterId) -> &'static str {
        match self.colour(w, f).variant {
            Variant::Menu => Variant::Local.key(),
            v => v.key(),
        }
    }

    /// Whether fighter `f` draws the default skin (`look.name === d.skin.name`).
    pub fn on_default_skin(&self, w: &World, f: FighterId) -> bool {
        let pick = usize::try_from(w.fighter(f).id)
            .ok()
            .and_then(|i| w.setup.players.get(i))
            .and_then(|pl| pl.skin.as_deref());
        crate::skins::skin_of(&w.char_data(f).fighter, pick).is_ok_and(|s| s.name == w.char_data(f).fighter.skin.name)
    }

    /// `skinStatic` for fighter `f`'s skin over a pawn component's properties `p`: the skin's static
    /// entry of a tag among the component's `ComponentTags` ('<skin>.<tag>' in the draws: its key, the fighter's skin
    /// variant: ApplySubMeshes SetStaticMesh and SelectMaterialColor's pick), else the template's StaticMesh with the
    /// mesh's own materials and no team pick (the 'local' set: Iron Giant's scrap meshes on the skins without them).
    /// None: neither. A fighter on the default skin is the callers' own (their fixed keys).
    pub fn skin_static(&self, w: &World, f: FighterId, p: &Props) -> Option<(String, &'static str)> {
        let d = &w.char_data(f).fighter;
        let pick = usize::try_from(w.fighter(f).id)
            .ok()
            .and_then(|i| w.setup.players.get(i))
            .and_then(|pl| pl.skin.as_deref());
        let skin = crate::skins::skin_of(d, pick).ok()?;
        let draws = self.draws_of(w, f)?;
        if let Some(tags) = p.get("ComponentTags").and_then(Value::as_arr) {
            for t in tags.iter().filter_map(Value::as_str) {
                if let Some(k) = draws.actor_key(&format!("{}.{t}", skin.name)) {
                    return Some((k, self.selected_variant(w, f)));
                }
            }
        }
        let mesh = p.get("StaticMesh")?.as_str()?;
        let name = mesh.rsplit('/').next().unwrap_or(mesh);
        let name = name.rsplit('.').next().unwrap_or(name);
        Some((draws.actor_key(name).unwrap_or_else(|| name.to_owned()), Variant::Local.key()))
    }

    /// Fighter `f`'s character's draws.
    pub fn draws_of(&self, w: &World, f: FighterId) -> Option<&FighterDraws> {
        self.draws.get(w.names.str(w.fighter(f).ch)).map(|d| &**d)
    }

    /// Fighter `f`'s character's clips.
    pub fn clips_of(&mut self, w: &World, f: FighterId) -> Option<&mut Clips> {
        self.clips.get_mut(w.names.str(w.fighter(f).ch))
    }

    /// Fighter `f`'s character's blend space players bound (`host.assets(f).blend`).
    pub fn blend_of(&self, w: &World, f: FighterId) -> Option<&BlendSpaces> {
        self.blend.get(w.names.str(w.fighter(f).ch))
    }

    /// A mesh loaded for the match by its path.
    pub fn mesh(&self, path: &str) -> Option<Arc<MeshInfo>> {
        self.meshes.get(path).cloned()
    }

    /// The items part's mesh component `key` ('<class>.<component>', the stage kind's items entry `draws`):
    /// its mesh, loaded for a match with random items;
    /// None with no such draw or a match without items.
    pub fn item_asset(&self, key: &str) -> Option<ItemAsset> {
        let (path, info) = self.items.as_deref()?.mesh(key)?;
        Some(ItemAsset { path: Arc::from(path), info: info.clone() })
    }

    /// The items part's draws' keys ('<class>.<component>'), in the entry's order (ItemAssets.draws: the warm-up's
    /// list); empty for a match without items.
    pub fn item_keys(&self) -> Vec<&str> {
        self.items.as_deref().map_or_else(Vec::new, |i| i.keys().collect())
    }

    /// The items part's sequence for the mesh component `key` (ItemAssets.clip, MeshHost.itemClip): its AnimToPlay
    /// with no `name`, else the sequence of that name its class plays (ItemDraw.clips); the clip set and its index in
    /// it, None with no such sequence or a match without items.
    pub fn item_clip(&mut self, key: &str, name: Option<&str>) -> Option<(&mut Clips, usize)> {
        let items = self.items.as_deref_mut()?;
        let c = items.clip(key, name)?;
        Some((&mut items.clips, c))
    }

    /// `host.mats.actor(key, variant)` of the items part: the mesh component `key`'s slot materials in set `variant`
    /// ('template': its own; 'red', 'blue', 'local': its class's SetTeamColor sets where it has them, else its own);
    /// None with no such draw or a match without items.
    pub fn item_materials(&self, key: &str, variant: &str) -> Option<Vec<Option<String>>> {
        self.items.as_deref()?.materials(key, Some(variant))
    }

    /// Where a held item hangs (an item rule's socketFollow on the holder's hold socket):
    /// fighter `f`'s socket `socket` as last drawn, else (socketFollow's `missing`, a bone the view lacks) its mesh
    /// origin yawed to its facing; None with no view. A socket on a virtual bone (Bugs Bunny's VB PropPickup) follows
    /// the bone's target ([`Self::socket_follow`]).
    pub fn hold_place(&self, w: &World, f: FighterId, socket: &str) -> Option<Xform> {
        let at = Xform { loc: [0.0; 3], rot: crate::actor_view::IDENT, scale: [1.0; 3] };
        self.socket_follow(w, f, socket, &at).or_else(|| self.mesh_origin(w, f))
    }

    /// `meshAttach`: fighter `f`'s mesh origin as drawn, yawed -90 facing right and 90
    /// facing left (None: no view).
    pub fn mesh_origin(&self, w: &World, f: FighterId) -> Option<Xform> {
        let id = w.fighter(f).id;
        let m = math::to_ue(&self.fighters.iter().find(|b| b.id == id)?.model);
        let h = std::f64::consts::FRAC_PI_2;
        let yaw = if w.fighter(f).body.facing_dir > 0.0 { -h } else { h };
        Some(Xform { loc: [m[12], m[13], m[14]], rot: rotation_rows(0.0, yaw, 0.0), scale: [1.0; 3] })
    }

    /// `montageMeshBone`: bone `bone` (in any case) of fighter `f`'s skinned montage mesh `mesh` as last drawn, its
    /// UE world transform (None: no such mesh shown or no such bone).
    pub fn montage_bone(&self, w: &World, f: FighterId, mesh: &str, bone: &str) -> Option<Xform> {
        let id = w.fighter(f).id;
        let body = self.fighters.iter().find(|b| b.id == id)?;
        let m = body.montage.iter().find(|m| w.names.str(m.name).eq_ignore_ascii_case(mesh))?;
        let b = m.info.bones().iter().position(|n| n.eq_ignore_ascii_case(bone))?;
        Some(mat_xform(&math::mul(&m.world, &math::to_ue(m.cs.get(b)?))))
    }

    /// `socketFollow` on the view's main mesh: socket (or bone) `socket` of fighter `f`'s skeleton with `rel` under it
    /// (FTransform rel * socket), the view's bone as last drawn (None: no view or no such bone). Facing left
    /// the build's bone is the view's mirror partner with the bone's BoneFlipAxis in
    /// front ([F_B] * bone). A bone the view keeps unmirrored (keptBy) and every bone of a pawn without a mirror node
    /// are the view's bone of the same name reflected in depth ([`reflect_depth`]), with no partner and no flip.
    // Not read here: the virtual bones ([`Self::socket_follow`]).
    // Deviation: the copy component is taken as the main mesh. Pawn_C017's hold component is its BodyMesh copy
    // (Comp_ItemManager's UseOverrideAttachParent), which boneFollow draws as the view (asView: no reflection, the Y
    // scale negated facing left); here an item held on C017 facing left is reflected in depth.
    pub fn socket(&self, w: &World, f: FighterId, socket: &str, rel: &Xform) -> Option<Xform> {
        let id = w.fighter(f).id;
        let b = self.fighters.iter().find(|b| b.id == id)?;
        let skel = &w.char_data(f).fighter.skeleton;
        // (FName compares: any case)
        let sk = skel.sockets.iter().find(|(k, _)| k.eq_ignore_ascii_case(socket)).map(|(_, v)| v);
        let bone = sk.map_or(socket, |s| s.bone.as_str());
        let i = b.names.iter().position(|n| n.eq_ignore_ascii_case(bone))?;
        let left = w.fighter(f).body.facing_dir < 0.0;
        let kept = left && (skel.mirror_node == Some(false) || b.kept.get(i).copied().unwrap_or(false));
        let mirrored = left && !kept;
        let (i, flip) = if mirrored {
            let partner = skel.mirror.get(&b.names[i]).map_or(b.names[i].as_str(), String::as_str);
            let p = b.names.iter().position(|n| n.eq_ignore_ascii_case(partner))?;
            (p, crate::pose::bone_flip_rows(skel, &b.names[i]).unwrap_or([1.0, -1.0, 1.0]))
        } else {
            (i, [1.0; 3])
        };
        let mut m = math::to_ue(&math::mul(&b.model, b.cs.get(i)?));
        if kept {
            let mut l = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]];
            let mut loc = [m[12], m[13], m[14]];
            reflect_depth(&mut l, &mut loc);
            for c in 0..3 {
                m[c * 4..c * 4 + 3].copy_from_slice(&l[c * 3..c * 3 + 3]);
            }
            m[12..15].copy_from_slice(&loc);
        }
        if mirrored {
            m = math::mul(&m, &xform_mat(&Xform { loc: [0.0; 3], rot: crate::actor_view::IDENT, scale: flip }));
        }
        if let Some(s) = sk {
            m = math::mul(&m, &xform_mat(&Xform { loc: s.loc, rot: rotator_rows(s.rot, None), scale: s.scale }));
        }
        Some(mat_xform(&math::mul(&m, &xform_mat(rel))))
    }
}

/// `boneFollow`'s kept bone facing left: a bone's UE transform (rows: its axes, `loc`: its
/// place) reflected in depth (UE X) about the view's root: 200 x the root's three x (0) - X, and each axis's X negated.
pub fn reflect_depth(l: &mut [f64; 9], loc: &mut [f64; 3]) {
    loc[0] = -loc[0];
    l[0] = -l[0];
    l[3] = -l[3];
    l[6] = -l[6];
}

/// A UE transform as its column-major matrix (UE space, cm).
pub fn xform_mat(x: &Xform) -> Mat4 {
    let mut m = math::IDENTITY;
    for c in 0..3 {
        for r in 0..3 {
            m[c * 4 + r] = x.rot[c * 3 + r] * x.scale[c];
        }
        m[12 + c] = x.loc[c];
    }
    m
}

/// A UE matrix as location, rows and scale; a mirrored one with its Y row and scale turned (boneFollow's).
pub fn mat_xform(m: &Mat4) -> Xform {
    let mut rot = [0.0; 9];
    let mut scale = [1.0; 3];
    for c in 0..3 {
        let l = (m[c * 4].powi(2) + m[c * 4 + 1].powi(2) + m[c * 4 + 2].powi(2)).sqrt();
        let l = if l > 0.0 { l } else { 1.0 };
        scale[c] = l;
        for r in 0..3 {
            rot[c * 3 + r] = m[c * 4 + r] / l;
        }
    }
    let n = &rot;
    let det =
        n[0] * (n[4] * n[8] - n[5] * n[7]) - n[1] * (n[3] * n[8] - n[5] * n[6]) + n[2] * (n[3] * n[7] - n[4] * n[6]);
    if det < 0.0 {
        for c in 0..3 {
            rot[3 + c] = -rot[3 + c];
        }
        scale[1] = -scale[1];
    }
    Xform { loc: [m[12], m[13], m[14]], rot, scale }
}

/// A class's own drawing (its mesh rule and the projectile rotation): what the shared path
/// does not.
#[derive(Clone, Copy, Default)]
pub struct MeshRule {
    /// `rot`: the actor's rotation rows where the class does not turn with its velocity (None from it: it does just
    /// now). Here and not on the particle side's ProjRule: actor_rot reads it for both.
    pub rot: Option<fn(&World, &Projectile) -> Option<Rows>>,
    /// The view of the actor's mesh components, made at its first sight (projView); None from it: not drawn (warned).
    pub mesh: Option<MakeMesh>,
}

/// MeshRule.mesh: makes the view of an actor's mesh components, from the class's components.
pub type MakeMesh = fn(&mut ActorHost<'_>, &World, ActorRef, &ActorComponents) -> Option<Box<dyn ActorMesh>>;

/// A view a class rule made: placed each frame, ended with the actor.
pub trait ActorMesh: std::fmt::Debug + Send {
    /// The actor's meshes at `at` between the last two ticks (`alpha`; spawnViews' place at the spawn tick: 1),
    /// `shown` or hidden, into `out`.
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    );
    /// The bones of its skinned mesh in the world, by name, for a spawned actor's PlayVFX on its own mesh
    /// (a view that enters its mesh; SkinnedActor::bones_world); none by default.
    fn bones(&self, _out: &mut Vec<(Arc<str>, Xform)>) {}
    /// Once a World step, after the tick's views are made (a projectile effect rule's `tick`, which a mesh
    /// reads: Stripe's saw spin); none by default.
    fn tick(&mut self, _host: &mut ActorHost<'_>, _w: &World) {}
    /// The actor's end (what the view put outside `out` goes back).
    fn end(&mut self) {}
    /// A view whose meshes are the stage scenery's (The Court's ball): where it put them at the last `place`; none by
    /// default.
    fn scenery(&self) -> Option<crate::frame::SceneryPlace> {
        None
    }
    /// Each mesh it draws is a scene child of its own (a view that adds each mesh view to the scene: an
    /// actor group per mesh); false: the actor's meshes are one group.
    fn split(&self) -> bool {
        false
    }
}

/// Every module's mesh rules by class name, resolved once (a class entered twice stops here).
fn mesh_rules() -> &'static IndexMap<&'static str, MeshRule> {
    static RULES: std::sync::OnceLock<IndexMap<&'static str, MeshRule>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        let mut t = IndexMap::new();
        let stage = [crate::court_ball::RULES];
        for (cls, rule) in crate::chars::RULES
            .iter()
            .chain(&crate::shared::RULES)
            .chain(&crate::items::RULES)
            .chain(stage.iter())
            .flat_map(|m| m.iter())
        {
            if t.insert(*cls, *rule).is_some() {
                panic!("actor mesh {cls} is registered twice");
            }
        }
        t
    })
}

/// `actorMeshRule`: the class's own rule (None: the shared path's drawing).
pub fn mesh_rule(cls: &str) -> Option<&'static MeshRule> {
    mesh_rules().get(cls)
}

/// Actor `a` is drawn by the items part's class view (an items-part actor no class rule draws: crate::items): its
/// particle components are the renderer's (mvs-render fx/items.rs).
pub fn class_view_draws(w: &World, a: ActorRef) -> bool {
    let Some(cls) = mvs_sim::actors::world_actor::actor_cls(w, a) else { return false };
    mesh_rule(w.names.str(cls)).and_then(|r| r.mesh).is_none() && crate::items::class_view::from_items(w, a)
}

/// A World actor's class has a view of its own (no shared drawing of a World actor).
fn has_mesh_rule(w: &World, cls: Name) -> bool {
    mesh_rule(w.names.str(cls)).is_some_and(|r| r.mesh.is_some())
}

/// A static model a class's mesh component names: its mesh and slot materials.
#[derive(Debug)]
pub(crate) struct StaticModel {
    pub(crate) info: Arc<MeshInfo>,
    pub(crate) mats: Vec<MaterialRef>,
}

/// A projectile's SkeletalMeshComponent the shared path draws (projView's skeletal branch): one of the owner's skin
/// sub-meshes at the component's relative Z and X scale, in its reference pose, turned by the actor's look.
#[derive(Debug)]
struct SkeletalComp {
    view: SkinnedActor,
    z: f64,
    s: f64,
}

/// A spawned actor's view as it is kept: a rule's, the shared skeletal component, the
/// shared static model path, or nothing (a rule that made no view: an empty group).
#[derive(Debug)]
enum View {
    Rule(Box<dyn ActorMesh>),
    Skeletal(Box<SkeletalComp>),
    Shared,
    Empty,
}

/// The actors' views: the static models by the name the class data gives them, and what is kept per view.
#[derive(Debug, Default)]
pub(crate) struct ActorViews {
    pub(crate) models: HashMap<&'static str, StaticModel>,
    /// A projectile's root scale at its view's first draw.
    scales: HashMap<ProjectileId, f64>,
    /// The views by actor (projViews; a rollback's actors keep their ids).
    views: HashMap<ActorRef, View>,
    /// The classes' components, resolved at a class's first view.
    comps: HashMap<Name, Arc<ActorComponents>>,
    /// The classes met whose mesh components no ported rule draws.
    pub(crate) undrawn: BTreeSet<String>,
    /// The tutorial's Meeseeks (stage actors).
    meeseeks: crate::tutorial_meeseeks::MeeseeksViews,
}

/// `carryHidden`: Buff_ProjectileThroughTunnel on the actor hides its root's children.
fn carry_hidden(elem: Option<&ElemState>) -> bool {
    elem.is_some_and(|e| e.buffs.iter().any(|b| b.class == BuffClass::BuffProjectileThroughTunnel))
}

fn comp<'a>(cs: &'a Props, name: &str) -> Option<&'a Props> {
    cs.get(name).and_then(Value::as_obj)
}

fn vec_x(c: &Props, key: &str, axis: &str) -> Option<f64> {
    c.get(key)?.as_obj()?.get(axis)?.as_f64()
}

/// A quaternion about the view's Y axis by `a` times one about its Z axis by `b` (Quaternion.setFromAxisAngle).
fn yaw_pitch(a: f64, b: f64) -> [f64; 4] {
    let qa = [0.0, (a / 2.0).sin(), 0.0, (a / 2.0).cos()];
    let qb = [0.0, 0.0, (b / 2.0).sin(), (b / 2.0).cos()];
    math::qmul(qa, qb)
}

fn warn_once(seen: &mut BTreeSet<String>, msg: String) {
    if seen.insert(msg.clone()) {
        eprintln!("{msg}");
    }
}

/// `itemDefOf`: an item's class data, its spawner's, else the stage's, else the items part's.
pub fn item_def<'a>(w: &'a World, it: &Item) -> Option<&'a ActorDef> {
    match it.spawner {
        Some(f) => mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(f), it.cls),
        None => {
            let cls = w.names.str(it.cls);
            w.data
                .stage
                .actors
                .as_ref()
                .and_then(|a| a.get(cls))
                .or_else(|| mvs_sim::world::flow::items_data_of(w).and_then(|d| d.actors.get(cls)))
        }
    }
}

/// The class data of a spawned actor's view (projView's `cs`; a rule reads its raw components here): an item's
/// itemDefOf, a projectile's owner's, an ownerless one's items part's, else the stage's; a World actor's owner's (the
/// character modules' `host.data(fi).actors[cls]`).
pub fn class_def(w: &World, a: ActorRef) -> Option<&ActorDef> {
    match a {
        ActorRef::Item(id) => item_def(w, w.actors.items.get(id)?),
        ActorRef::Projectile(id) => {
            let pr = w.actors.projectiles.get(id)?;
            match pr.owner {
                Some(o) => mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(o), pr.cls),
                None => {
                    let cls = w.names.str(pr.cls);
                    mvs_sim::world::flow::items_data_of(w)
                        .and_then(|d| d.actors.get(cls))
                        .or_else(|| w.data.stage.actors.as_ref().and_then(|a| a.get(cls)))
                }
            }
        }
        ActorRef::Actor(id) => mvs_sim::fighter::common::actor_def(w, a, w.actors.actors.get(id)?.cls),
        ActorRef::Ship(id) => mvs_sim::fighter::common::actor_def(w, a, w.actors.ships.get(id)?.cls),
        _ => None,
    }
}

/// The live actors of the World's own lists a class rule draws (C019_Ship_C: Morty's ships),
/// with their classes.
fn typed_world_actors(w: &World) -> impl Iterator<Item = (ActorRef, Name)> + '_ {
    w.actors.ships.iter().filter(|(_, s)| !s.dead && has_mesh_rule(w, s.cls)).map(|(id, s)| (ActorRef::Ship(id), s.cls))
}

/// The default skin's sub-meshes (key, mesh name) of the character whose data has class `cls` of a projectile's owner.
fn skin_sub_meshes(w: &World, owner: Option<FighterId>, cls: Name) -> Vec<(String, String)> {
    let Some(o) = owner else { return Vec::new() };
    let from = mvs_sim::fighter::common::actor_def_owner(w, o, cls);
    match crate::skins::skin_of(&w.char_data(from).fighter, None) {
        Ok(s) => s.parts.into_iter().map(|p| (p.key, p.mesh)).collect(),
        Err(_) => Vec::new(),
    }
}

impl ActorViews {
    /// The mesh a class's component draws: its static model, else the class goes in the undrawn list.
    fn model(&mut self, cls: &str, c: Option<&Props>) -> Option<&StaticModel> {
        let name = c.and_then(|c| c.get("StaticMesh")).and_then(Value::as_str);
        match name.and_then(|n| self.models.get(n)) {
            Some(m) => Some(m),
            None => {
                warn_once(&mut self.undrawn, format!("{cls}: actor class has no StaticMesh model, not drawn"));
                None
            }
        }
    }

    /// A class's components (resolved once).
    fn comps(&mut self, w: &World, a: ActorRef, cls: Name) -> Arc<ActorComponents> {
        if let Some(c) = self.comps.get(&cls) {
            return c.clone();
        }
        let owner = match a {
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| p.owner),
            _ => None,
        };
        let c = Arc::new(match class_def(w, a) {
            Some(d) => ActorComponents::resolve(d, &skin_sub_meshes(w, owner, cls)),
            None => ActorComponents::default(),
        });
        self.comps.insert(cls, c.clone());
        c
    }

    /// `projView`: the actor's view, made at its first sight.
    fn view(&mut self, host: &mut ActorHost<'_>, w: &World, a: ActorRef, cls: Name) -> &mut View {
        if !self.views.contains_key(&a) {
            let v = self.make_view(host, w, a, cls);
            self.views.insert(a, v);
        }
        self.views.get_mut(&a).expect("the view just made")
    }

    fn make_view(&mut self, host: &mut ActorHost<'_>, w: &World, a: ActorRef, cls: Name) -> View {
        let name = w.names.str(cls);
        if let Some(make) = mesh_rule(name).and_then(|r| r.mesh) {
            let cs = self.comps(w, a, cls);
            return match make(host, w, a, &cs) {
                Some(m) => View::Rule(m),
                None => {
                    warn_once(&mut self.undrawn, format!("{name}: actor mesh not drawn (its mesh or materials)"));
                    View::Empty
                }
            };
        }
        // (an items-part class no rule draws)
        if crate::items::class_view::from_items(w, a) {
            let cs = self.comps(w, a, cls);
            return match crate::items::class_view::class_view(host, w, a, &cs) {
                Some(m) => View::Rule(m),
                None => {
                    warn_once(&mut self.undrawn, format!("{name}: actor mesh not drawn (its mesh or materials)"));
                    View::Empty
                }
            };
        }
        let ActorRef::Projectile(id) = a else { return View::Shared };
        let Some(pr) = w.actors.projectiles.get(id) else { return View::Shared };
        let Some(owner) = pr.owner else { return View::Shared };
        let cs = self.comps(w, a, cls);
        let Some(sk) = cs.components.iter().find(|c| matches!(c.kind, ComponentKind::SkeletalMesh { .. })) else {
            return View::Shared;
        };
        let ComponentKind::SkeletalMesh { sub_mesh, mesh, .. } = &sk.kind else { unreachable!() };
        // (the skin of the character whose data has the class: the owner's, else a peer's, as actor_def; the owner's
        // skin's part of the default skin's key, its own entry or the template's mesh it keeps)
        let from = mvs_sim::fighter::common::actor_def_owner(w, owner, cls);
        let fd = &w.char_data(from).fighter;
        let pick = usize::try_from(w.fighter(from).id)
            .ok()
            .and_then(|i| w.setup.players.get(i))
            .and_then(|p| p.skin.as_deref());
        let skin = crate::skins::skin_of(fd, pick).ok();
        let part = sub_mesh.as_ref().and_then(|k| skin.as_ref()?.parts.iter().find(|p| p.key.eq_ignore_ascii_case(k)));
        let (Some(skin), Some(part)) = (skin.as_ref(), part) else {
            warn_once(&mut self.undrawn, format!("{name}: actor mesh {mesh} is not a skin sub-mesh"));
            return View::Shared;
        };
        let info = part.path.as_deref().and_then(|p| host.mesh(p));
        let keys = host.draws_of(w, from).and_then(|d| {
            if part.own {
                d.actor(&part.mesh, None).map(|a| a.1)
            } else {
                d.set(&skin.name, &part.key, Variant::Local).ok()
            }
        });
        let (Some(info), Some(keys)) = (info, keys) else {
            warn_once(&mut self.undrawn, format!("{name}: actor mesh {mesh} not loaded"));
            return View::Shared;
        };
        let scale = pr.scale;
        View::Skeletal(Box::new(SkeletalComp {
            view: SkinnedActor::new(info, &keys),
            z: sk.loc[2] * f64::from(scale),
            s: sk.scale[0] * f64::from(scale),
        }))
    }

    /// The views whose meshes are the stage scenery's, as the last `present` placed them.
    pub(crate) fn scenery_places(&self, out: &mut Vec<crate::frame::SceneryPlace>) {
        for v in self.views.values() {
            if let View::Rule(m) = v
                && let Some(p) = m.scenery()
            {
                out.push(p);
            }
        }
    }

    /// The views of the actors gone from the world end (placeProjectiles' drop).
    fn drop_gone(&mut self, w: &World) {
        self.scales.retain(|id, _| w.actors.projectiles.get(*id).is_some());
        self.views.retain(|a, v| {
            let live = match *a {
                ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some(),
                ActorRef::Item(id) => w.actors.items.get(id).is_some(),
                ActorRef::Actor(id) => w.actors.actors.get(id).is_some_and(|x| !x.dead),
                ActorRef::Ship(id) => w.actors.ships.get(id).is_some_and(|x| !x.dead),
                _ => false,
            };
            if !live && let View::Rule(m) = v {
                m.end();
            }
            live
        });
    }

    /// `spawnViews`: the views a class rule draws of the actors spawned in the tick, placed at the actor's place of
    /// the tick at once (their bones there for a PlayVFX of the tick's events).
    pub(crate) fn spawn_views(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let mut scratch = Vec::new();
        for (id, pr) in w.actors.projectiles.iter() {
            let a = ActorRef::Projectile(id);
            if self.views.contains_key(&a) {
                continue;
            }
            let shown = pr.owner.is_none() || !carry_hidden(pr.elem.as_ref());
            let place = actor_place(w, a).expect("a live projectile's place");
            if let View::Rule(m) = self.view(host, w, a, pr.cls) {
                m.place(host, w, &place, 1.0, shown, &mut scratch);
            }
            scratch.clear();
        }
        for (id, x) in w.actors.actors.iter() {
            let a = ActorRef::Actor(id);
            if x.dead || self.views.contains_key(&a) || !has_mesh_rule(w, x.cls) {
                continue;
            }
            let Some(place) = actor_place(w, a) else { continue };
            if let View::Rule(m) = self.view(host, w, a, x.cls) {
                m.place(host, w, &place, 1.0, true, &mut scratch);
            }
            scratch.clear();
        }
        for (a, cls) in typed_world_actors(w) {
            if self.views.contains_key(&a) {
                continue;
            }
            let Some(place) = actor_place(w, a) else { continue };
            if let View::Rule(m) = self.view(host, w, a, cls) {
                m.place(host, w, &place, 1.0, true, &mut scratch);
            }
            scratch.clear();
        }
        for v in self.views.values_mut() {
            if let View::Rule(m) = v {
                m.tick(host, w);
            }
        }
    }

    /// placeProjectiles: every projectile's and item's mesh at `alpha` between the last two ticks.
    pub(crate) fn present(&mut self, host: &mut ActorHost<'_>, w: &World, alpha: f64, out: &mut Vec<ActorPose>) {
        self.drop_gone(w);
        let lerp = |p: f64, c: f64| p + (c - p) * alpha;
        for (id, pr) in w.actors.projectiles.iter() {
            let a = ActorRef::Projectile(id);
            let cls = w.names.str(pr.cls);
            // (the world's own, no owner: placed by their rule, not hidden by a carry, which needs an owner)
            let Some(owner) = pr.owner else {
                if let View::Rule(m) = self.view(host, w, a, pr.cls) {
                    let place = actor_place(w, a).expect("a live projectile's place");
                    let mut meshes = Vec::new();
                    m.place(host, w, &place, alpha, true, &mut meshes);
                    push_rule(out, a, meshes, &**m);
                }
                continue;
            };
            // Deviation: the view keeps the root scale of its first draw (a later SetActorScale3D is
            // not followed).
            let scale = *self.scales.entry(id).or_insert(f64::from(pr.scale));
            let hidden = carry_hidden(pr.elem.as_ref());
            match self.view(host, w, a, pr.cls) {
                View::Rule(m) => {
                    let place = actor_place(w, a).expect("a live projectile's place");
                    let mut meshes = Vec::new();
                    m.place(host, w, &place, alpha, !hidden, &mut meshes);
                    push_rule(out, a, meshes, &**m);
                    continue;
                }
                View::Skeletal(sc) => {
                    if hidden {
                        continue;
                    }
                    // UE yaw to the view: the model at rotation.y 0 faces UE +X (yaw -90 is rotation.y +90)
                    let q = yaw_pitch(f64::from(-pr.look_yaw.to_radians()), f64::from(pr.look_pitch.to_radians()));
                    let root = math::compose(
                        [
                            0.0,
                            lerp(f64::from(pr.prev_y), f64::from(pr.y)) / 100.0,
                            -lerp(f64::from(pr.prev_x), f64::from(pr.x)) / 100.0,
                        ],
                        [0.0, 0.0, 0.0, 1.0],
                        [1.0; 3],
                    );
                    let local = math::compose([0.0, sc.z / 100.0, 0.0], q, [sc.s; 3]);
                    let m = math::to_ue(&math::mul(&root, &local));
                    sc.view.set(&mat_xform(&m));
                    let mut meshes = Vec::new();
                    sc.view.draw(&mut meshes);
                    out.push(ActorPose { meshes, actor: Some(a), bones: Vec::new(), second: None });
                    continue;
                }
                View::Empty => continue,
                View::Shared => {}
            }
            if hidden {
                continue;
            }
            let Some(def) = mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(owner), pr.cls) else { continue };
            let sm = comp(&def.components, "StaticMesh");
            let Some(model) = self.model(cls, sm) else { continue };
            let sm = sm.expect("a model's component");
            let (x, y) = (lerp(f64::from(pr.prev_x), f64::from(pr.x)), lerp(f64::from(pr.prev_y), f64::from(pr.y)));
            let v = if pr.vx == 0.0 || pr.vx.is_nan() { pr.facing } else { pr.vx };
            let fwd = if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            };
            let q = yaw_pitch(fwd * std::f64::consts::FRAC_PI_2, f64::from(pr.pitch).to_radians());
            let z = vec_x(sm, "RelativeLocation", "Z").unwrap_or(0.0) * scale;
            let s = vec_x(sm, "RelativeScale3D", "X").unwrap_or(1.0) * scale;
            out.push(ActorPose { meshes: vec![static_pose(model, x, y, q, z, s)], ..ActorPose::default() });
        }
        for (id, it) in w.actors.items.iter() {
            self.present_item(host, w, id, it, alpha, out);
        }
        // (the World actors a class rule draws: the character modules' own walks of world.actors, each view
        // placed every frame and gone with its actor)
        for (id, x) in w.actors.actors.iter() {
            let a = ActorRef::Actor(id);
            if x.dead || !has_mesh_rule(w, x.cls) {
                continue;
            }
            let Some(place) = actor_place(w, a) else { continue };
            if let View::Rule(m) = self.view(host, w, a, x.cls) {
                let mut meshes = Vec::new();
                m.place(host, w, &place, alpha, true, &mut meshes);
                push_rule(out, a, meshes, &**m);
            }
        }
        for (a, cls) in typed_world_actors(w) {
            let Some(place) = actor_place(w, a) else { continue };
            if let View::Rule(m) = self.view(host, w, a, cls) {
                let mut meshes = Vec::new();
                m.place(host, w, &place, alpha, true, &mut meshes);
                push_rule(out, a, meshes, &**m);
            }
        }
        self.meeseeks.present(host, w, alpha, out);
    }

    fn present_item(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        id: ItemId,
        it: &Item,
        alpha: f64,
        out: &mut Vec<ActorPose>,
    ) {
        let a = ActorRef::Item(id);
        let lerp = |p: f64, c: f64| p + (c - p) * alpha;
        let holder = (it.holder >= 0).then(|| w.by_id(it.holder)).flatten();
        // held, it is the holder's HeldItem prop; holstered (no hold socket) the actor sits at the mesh
        // Deviation: a holstered item is drawn at the mesh's origin with the rotation it had, not the
        // mesh component's.
        let holstered = holder.is_none_or(|h| w.fighter(h).items.item_holstered);
        let visible = !it.hidden() && !item_root_hidden(it) && holstered;
        let v = self.view(host, w, a, it.cls);
        if matches!(v, View::Empty) {
            return;
        }
        if let View::Rule(m) = v {
            let place = actor_place(w, a).expect("a live item's place");
            // (held and not holstered: the rule puts it on the holder's hold socket, Comp_ItemManager.EquipOnSpawn)
            let shown = item_shown(w, it);
            let mut meshes = Vec::new();
            m.place(host, w, &place, alpha, shown, &mut meshes);
            push_rule(out, a, meshes, &**m);
            return;
        }
        if !visible {
            return;
        }
        let cls = w.names.str(it.cls);
        let def = it.def(&w.data);
        let geo = comp(def.components, "Geo");
        let Some(model) = self.model(cls, geo) else { return };
        let geo = geo.expect("a model's component");
        let (x, y) = match holder {
            Some(h) => (f64::from(mvs_sim::fighter::mesh_wx(w, h)), f64::from(mvs_sim::fighter::mesh_wy(w, h))),
            None => (lerp(f64::from(it.prev_x), f64::from(it.x)), lerp(f64::from(it.prev_y), f64::from(it.y))),
        };
        let q = yaw_pitch(f64::from(it.facing) * std::f64::consts::FRAC_PI_2, f64::from(it.pitch).to_radians());
        // the root Capsule's scale carries the Geo's relative location and scale; a thrown sandwich's Geo is at
        // world scale 2 from then on (SetVisualSize)
        let z = vec_x(geo, "RelativeLocation", "Z").unwrap_or(0.0) * f64::from(it.scale);
        let s = if it.big_geo { 2.0 } else { vec_x(geo, "RelativeScale3D", "X").unwrap_or(1.0) * f64::from(it.scale) };
        out.push(ActorPose { meshes: vec![static_pose(model, x, y, q, z, s)], ..ActorPose::default() });
    }
}

impl Drop for ActorViews {
    /// The match's end (endActors): every rule's view ends.
    fn drop(&mut self) {
        for v in self.views.values_mut() {
            if let View::Rule(m) = v {
                m.end();
            }
        }
    }
}

/// The bones a rule's view enters for the particle side.
/// A rule's view's meshes as the actor's group, or a group per mesh (ActorMesh::split; its bones on the first).
fn push_rule(out: &mut Vec<ActorPose>, a: ActorRef, meshes: Vec<MeshPose>, m: &dyn ActorMesh) {
    let mut bones = Some(actor_bones(m));
    if !m.split() || meshes.len() < 2 {
        out.push(ActorPose { meshes, actor: Some(a), bones: bones.take().unwrap_or_default(), ..ActorPose::default() });
        return;
    }
    for mesh in meshes {
        out.push(ActorPose {
            meshes: vec![mesh],
            actor: Some(a),
            bones: bones.take().unwrap_or_default(),
            ..ActorPose::default()
        });
    }
}

fn actor_bones(m: &dyn ActorMesh) -> Vec<(Arc<str>, Xform)> {
    let mut out = Vec::new();
    m.bones(&mut out);
    out
}

/// A static model under an actor root at sim (`x`, `y`) turned by `q` (the view's space), the model at relative
/// height `z` cm and uniform scale `s`.
fn static_pose(m: &StaticModel, x: f64, y: f64, q: [f64; 4], z: f64, s: f64) -> MeshPose {
    let root: Mat4 = math::compose([0.0, y / 100.0, -x / 100.0], q, [1.0; 3]);
    let local = math::compose([0.0, z / 100.0, 0.0], [0.0, 0.0, 0.0, 1.0], [s; 3]);
    MeshPose {
        mesh: Arc::from(m.info.path.as_str()),
        lod: 0,
        to_world: math::rows3x4(&math::to_ue(&math::mul(&root, &local))),
        bones: Vec::new(),
        shown: m.info.sections.iter().map(|s| !s.disabled).collect(),
        materials: m.mats.clone(),
        custom_stencil: 0,
        capture_skip: false,
        no_decals: false,
        morphs: Vec::new(),
    }
}
