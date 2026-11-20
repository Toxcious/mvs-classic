//! The effects the sim's events start (the projectiles', ring-outs', evades', the characters' effects): systems
//! spawned, moved and ended on the particle player after each step, decals placed. What a character's or a shared
//! class's module draws is in [`chars`] (one [`chars::CharFx`] per module, its hooks called here at the presented
//! tick's points) and `shared`.

pub(crate) mod attach;
pub(crate) use attach::warn_once;
pub mod chars;
mod items;
mod ring_out;
mod second;
mod shared;
pub mod surfaces;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::projectile::AttachParentOut;
use mvs_sim::actors::{ActorRef, ItemId};
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::events::{Event, MoveFxKind, NotifyRef};
use mvs_sim::fighter::anim::montages::notify_of;
use mvs_sim::fighter::body::def as body_def;
use mvs_sim::fighter::common::{actor_def, latent_frames};
use mvs_sim::fighter::{FighterId, actor_y, cap_half, facing, heal_vfx_born};
use mvs_store::Pack;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::player::Player;
use mvs_vfx::{Attachment, SystemId};
use mvs_view::actor_view::Xform;
use mvs_view::actors::{actor_place, actor_rot};
use mvs_view::mesh::MeshInfo;
use mvs_view::skin_intro::IntroMute;
use mvs_view::skins::{FighterColour, FighterDraws, fighter_colours, skin_of};
use mvs_view::{ActorPose, FighterPose, SecondKind};

pub use attach::Body;
use attach::{
    D2R, Rig, SocketFollow, att, flag, mesh_rows, mirror_play_vfx, mul_row, play_vfx_particle, rot_rows, text,
    transpose, xyz,
};

use chars::{CharFx, ProjFxState, ProjRule};

use crate::Result;
use crate::scenery::EmitterParam;

/// One sim step (s).
const STEP: f64 = 1.0 / 60.0;
const PI: f64 = std::f64::consts::PI;
/// How many gone mode pawns' colours `Fx::gone_colours` keeps.
const GONE_KEPT: usize = 32;

/// The effects' warm list (`Fx::warm`).
#[derive(Debug, Default)]
pub struct FxWarm {
    /// The component meshes the modules may add, with each material set they may take.
    pub meshes: Vec<mvs_view::WarmMesh>,
    /// The emitters of the particle systems the match's owners hold (material, kind).
    pub emitters: Vec<(Arc<str>, mvs_vfx::draw::EmitterKind)>,
    /// The floor decals' materials (export names or object paths, as `FloorDecal::material`).
    pub decals: Vec<String>,
}

/// The floor decals' materials a match may draw: each Decal component's DecalMaterial in the match's actor classes
/// (its characters' and their perks', the stage's, the items'), and the modules' own where a component names none.
fn floor_decals(w: &World) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> =
        [chars::finn::DECAL_MAT, shared::firewall::DECAL, chars::steven::HEAL_DECAL, chars::velma::SLOW_DECAL]
            .into_iter()
            .map(String::from)
            .collect();
    let d = &w.data;
    let mut maps = vec![d.stage.actors.as_ref()];
    for c in d.chars.values() {
        maps.push(c.fighter.actors.as_ref());
        maps.push(c.perks.as_ref().and_then(|p| p.actors.as_ref()));
    }
    maps.push(d.items.as_ref().map(|i| &i.actors));
    for def in maps.into_iter().flatten().flat_map(|m| m.values()) {
        let dc = def.components.get("Decal").and_then(mvs_data::Value::as_obj);
        if let Some(m) = dc.and_then(|o| attach::text(o, "DecalMaterial")) {
            out.insert(m.to_owned());
        }
    }
    out.into_iter().collect()
}

/// A decal component the renderer's decal pass draws (Renderer::floor_decals): its material, placement and parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct FloorDecal {
    /// The spawned actor's sequence number.
    pub actor: i32,
    pub material: String,
    /// The decal's world location (UE cm), rows (local X the projection axis) and box half extents.
    pub loc: V3,
    pub rows: Rows,
    pub extent: V3,
    /// The dynamic instance's Team; None: the material's own (no graph sets it).
    pub team: Option<f64>,
    /// The dynamic instance's Alpha; None: the material's own (no graph sets it).
    pub alpha: Option<f64>,
    /// The dynamic instance's Dissolve; None: the material's own (no graph sets it).
    pub dissolve: Option<f64>,
}

/// A ring-out effect's place the camera keeps in view (RingOutVfxAnchor, a Comp_Actor_KeepOnCamera actor).
#[derive(Clone, Copy, Debug, PartialEq)]
/// What MatchCamera::ko_anchor takes, before the camera's step of the tick.
pub struct KoAnchor {
    /// The place, UE Y and Z.
    pub y: f64,
    pub z: f64,
    /// The timer that destroys it, s (max(CameraHoldTime, 0.01)).
    pub hold: f64,
}

/// How an attached component's world scale is made.
#[derive(Clone, Copy, Debug)]
enum Scale {
    /// The attachment's own (a socket's world scale, or the spawn's).
    Own,
    /// This world scale.
    Fixed(V3),
    /// This times the mesh component's world scale.
    ByMesh(V3),
    /// The socket's times the fighter's rage residual scale.
    Residual,
}

/// Where an attached component is put each step.
#[derive(Clone, Debug)]
enum Follow {
    /// On fighter `id`'s mesh component: a relative location and rotation.
    Mesh { id: i32, rel: V3, rows: Option<Rows>, scale: Scale },
    /// On a socket or bone of fighter `id`'s pose.
    Socket { id: i32, s: Box<SocketFollow>, scale: Scale },
    /// On fighter `id`'s root (the capsule: the actor location, yaw -90 / 90): `rel` under it, `up` over it.
    Root { id: i32, rel: Rows, up: f64, scale: Option<V3> },
    /// On fighter `id`'s root component as the root follow puts it: the actor's place,
    /// its yaw (-90 facing right, 90 else), `loc` and the scale times its actor scale (the sim's size scale, or a
    /// Polymorph's actor scale: XY, Z).
    RootScaled { id: i32, loc: V3 },
    /// The evade's (0, sign 200, 0) under the actor at its size scale.
    Evade { id: i32, sign: f64 },
    /// A projectile's particle component: its place under the actor's root.
    Projectile { seq: i32, rel: V3, rel_rot: Rows, scale: V3 },
    /// A particle component of a projectile whose rotation is its spawn's (an ownerless
    /// fire wall): `rel` through `rows` and the system's rotation `rows`, both times the actor's scale (`scale` its
    /// relative scale).
    Fixed { seq: i32, rel: V3, rows: Rows, scale: f64 },
    /// A class rule's particle component whose attach parent or relative scale the class's graph moves: placed anew
    /// each step (`under` of ProjRule.parent, ProjRule.scale).
    Live { seq: i32, c: Box<LiveComp> },
    /// A component of any spawned actor (mvs_view::actors::actor_place, its place of the latest tick): `rel` and
    /// `rel_rot` under the actor's root, `scale` times the actor's. `turned`: under the actor's place's rotation;
    /// else under rotation 0 (an actor the build keeps unrotated while its view turns it, as Item_BatarangV2's
    /// K2_SetActorRotation 0).
    Actor { a: ActorRef, rel: V3, rel_rot: Rows, scale: V3, turned: bool },
    /// A tunnel carry's trail: at the carry's place (carryTrail) under the actor's rotation and
    /// scale.
    Carry { a: ActorRef },
    /// On a bone of a spawned actor's view: `loc` and `rel` under the bone, `scale`
    /// times its world scale.
    Bone { a: ActorRef, bone: String, loc: V3, rel: Rows, scale: V3 },
    /// On BaseFighter's own Capsule (the actor's place, no offset): `rel` under the actor's yaw -90 / 90 when
    /// `turned`, else under rotation 0; `cap_up`: plus the Capsule's raw half
    /// height times the size scale of the tick over it.
    Capsule { id: i32, rel: V3, turned: bool, cap_up: bool },
    /// SpawnEmitterAttached on fighter `id`'s root capsule with no socket: `rel` times the actor scale (XY, XY, Z)
    /// under the actor's yaw -90 / 90; `scale` is the relative scale, the world scale is it times the actor scale.
    /// Read each step. KeepRelativeOffset passes Scale; SnapToTarget passes [`snap_scale`] at the spawn.
    CapsuleRel { id: i32, rel: V3, scale: V3 },
    /// On a socket or bone of a second view on fighter `id` (fx/second.rs), or on its mesh component (`s` None:
    /// `rel`, `rows`, `scale` times its world scale).
    Second { id: i32, kind: SecondKind, s: Option<Box<SocketFollow>>, rel: V3, rows: Rows, scale: V3 },
}

impl Follow {
    /// boneFollow's 'copy' on a socket follow (SocketFollow::copy); any other follow as it is.
    fn copy(self) -> Follow {
        match self {
            Follow::Socket { id, s, scale } => Follow::Socket { id, s: Box::new(s.copy()), scale },
            f => f,
        }
    }
}

/// Live's component: its name, rule, the template's relative location (or the rule's rel_loc), rotation rows and
/// scale, and the template's attach parents.
#[derive(Clone, Debug)]
struct LiveComp {
    cn: String,
    rule: &'static ProjRule,
    own_loc: V3,
    own_rot: Rows,
    crs: V3,
    par0: AttachParentOut,
}

/// projectileFx's `under`: a component under its attach parents `par`: its relative location, its relative scale
/// (a construction script's world scale `ws` over the template parents' scale `par0`), the parents' scale and the
/// relative rotation rows.
fn under(par: &AttachParentOut, own_loc: V3, own_rot: &Rows, crs: V3, ws: Option<f64>, par0: V3) -> (V3, V3, V3, Rows) {
    let mr: Rows = std::array::from_fn(|k| par.rot[k]).map(f64::from);
    let ms = par.scale;
    let cl = [0, 1, 2].map(|k| f64::from(ms[k]) * own_loc[k]);
    let d = mul_row(cl.map(f64::from), &mr);
    let rel = [0, 1, 2].map(|k| f64::from(par.loc[k]) + d[k]);
    let rs = match ws {
        Some(w) => [0, 1, 2].map(|k| (w * f64::from(ms[k])) / par0[k]),
        None => [0, 1, 2].map(|k| f64::from(ms[k]) * crs[k]),
    };
    (rel.map(f64::from), rs, ms.map(f64::from), compose_rows(own_rot, &mr))
}

/// A thrown item's ParticleSystem component.
#[derive(Debug, Default)]
struct ItemFx {
    h: Option<SystemId>,
    hid: bool,
    seen: bool,
}

/// A reflect's PS_Reflect systems on an actor.
#[derive(Debug, Default)]
struct Reflected {
    hs: Vec<SystemId>,
    seen: bool,
}

/// meshHidden: the fighter's mesh is hidden (dead, or invisible).
pub(crate) fn mesh_hidden(w: &World, f: mvs_sim::fighter::FighterId) -> bool {
    let fi = w.fighter(f);
    fi.life.dead != 0.0 || fi.vis.invisible
}

/// The VFX component template of Buff_SlowProjectile and of its child and its relative scale.
fn slow_vfx(c: BuffClass) -> Option<(&'static str, f64)> {
    match c {
        BuffClass::BuffSlowProjectile => Some(("PS_Garnet_ProjectileSlowTemp", 1.0)),
        BuffClass::BuffIceSlowProjectile => Some(("PS_SupermanIceBreath_IceCube", 0.3)),
        _ => None,
    }
}

/// A fighter's effects.
#[derive(Debug, Default)]
struct PawnFx {
    body: Option<Body>,
    mesh: String,
    skin: String,
    /// The skin's Slug (Comp_Pawn_SkinManager AppliedSkin.Slug, skin_c017_s04).
    slug: String,
    /// The skin's sub-mesh keys and tags (RenderChar.partKeys / partTags): a PlayVFX's tagged pose copies.
    parts: Vec<String>,
    heal: (f64, Option<SystemId>, i32),
    smoke: Option<SystemId>,
    di: (Option<SystemId>, f64),
    /// The components attached to the mesh in attach order (AttachChildren): system, socket, handle.
    mesh_psc: Vec<(String, String, SystemId)>,
    wall: [Option<SystemId>; 2],
    /// BaseFighter VFXInstances by notify.
    notify: Vec<(NotifyRef, Option<SystemId>)>,
    notify_paused: bool,
    rage: Vec<SystemId>,
    residual: (Option<SystemId>, f64),
    spawn: Option<SystemId>,
    /// Buff_Ice's IceVisual2 and IceVisual3.
    ice: (Option<SystemId>, Option<SystemId>),
    /// Buff_Fire's loop (PS_Debuff_Fire_FOE).
    fire: Option<SystemId>,
    /// The fighter wears a heart skin (FighterPose.heart).
    heart: bool,
    /// A minion's: its ForcedColorIndex, the creator's colour index (its
    /// GetColorIndex, IsLocalOnlinePlayer false). None: a pawn of the match's.
    minion: Option<u8>,
    /// A mode pawn's colour; None: the pawn has a slot of the match's `colours`.
    colour: Option<FighterColour>,
    /// What of the default intro the skin's own intro does not present (skin_intro.rs); None: the default's is drawn.
    intro: Option<IntroMute>,
}

/// A kept level's effects between two matches (`Fx::keep_level`, `Fx::resume_level`): the particle player with the
/// level's own systems alone, and the level's Emitter actors' systems among them.
#[derive(Debug)]
pub struct KeptLevel {
    player: Player,
    level: Vec<SystemId>,
}

impl PawnFx {
    /// DestroyComponent on every component the pawn holds (its pawn left the world).
    fn kill_components(&self, player: &mut Player) {
        let notify = self.notify.iter().filter_map(|n| n.1);
        let psc = self.mesh_psc.iter().map(|m| m.2);
        let all = [self.heal.1, self.smoke, self.di.0, self.residual.0, self.spawn, self.ice.0, self.ice.1, self.fire]
            .into_iter()
            .flatten()
            .chain(self.wall.iter().copied().flatten())
            .chain(self.rage.iter().copied())
            .chain(notify)
            .chain(psc);
        for h in all {
            player.kill(h);
        }
    }
}

/// The PFGCamera actor's root as the last frame placed it: its place and
/// the view's axes. What is attached to the camera actor's root follows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraRoot {
    pub pos: V3,
    pub front: V3,
    pub right: V3,
    pub up: V3,
}

impl CameraRoot {
    /// The root of `rig` as placed (the lens without the shake) and its point of view's axes.
    pub fn of(rig: &mvs_view::camera::CameraRig) -> CameraRoot {
        CameraRoot { pos: rig.root, front: rig.pov.front, right: rig.pov.right, up: rig.pov.up }
    }
}

/// The match's effects: the particle player and what the events started on it.
pub struct Fx {
    pub player: Player,
    /// Get-UI-WorldShowDI (the preference, false by default).
    pub show_di: bool,
    /// world-useTeamColoredSkins as the match's start read it (MatchHost.teamSkins: the views' material sets).
    pub team_skins: bool,
    /// The step's ring-out camera anchors, for the camera (taken by the caller each step).
    pub ko_anchors: Vec<KoAnchor>,
    /// The step's ring-out entries' sound events (ally, enemy) as this machine picked them, for the sound's glue
    /// (taken by the caller each step, in the events' order).
    pub ko_picks: Vec<(String, String)>,
    /// The modules' own sounds (the event and its UE place), for the sound's glue
    /// (taken by the caller each frame).
    pub sounds: Vec<(String, [f64; 3])>,
    /// The modules' component meshes (the caller hands them to `Renderer::set_component_meshes` each frame).
    pub components: crate::cmesh::ComponentMeshes,
    /// The camera's root as the last frame placed it (the caller sets it before each `step`; None: no camera).
    pub camera: Option<CameraRoot>,
    meshes: Arc<Pack>,
    /// The fighter and material kinds, for the characters' draws a module's component meshes take materials from.
    fighter_pack: Arc<Pack>,
    material_pack: Arc<Pack>,
    /// By character key, loaded at the first ask or the warm-up (None: the fighter kind has no entry).
    draws: IndexMap<String, Option<FighterDraws>>,
    /// The spline meshes' sources by mesh path, read at the first ask or the warm-up (None: not readable).
    splines: IndexMap<String, Option<Arc<crate::cmesh::SplineSource>>>,
    rigs: IndexMap<String, Option<Rig>>,
    /// By the fighter's id.
    pawns: IndexMap<i32, PawnFx>,
    /// The colours of the last `GONE_KEPT` mode pawns that left the world (`drop_gone_pawns`), by the fighter's id:
    /// a projectile that still names one as its owner reads its team. Bounded, so an Endless run's despawns do not
    /// grow it.
    gone_colours: IndexMap<i32, FighterColour>,
    /// A step's list of pawn ids (kept from step to step).
    pawn_ids: Vec<i32>,
    followers: Vec<(SystemId, Follow)>,
    colours: Vec<FighterColour>,
    me: usize,
    online: bool,
    ring_outs: ring_out::RingOuts,
    started: bool,
    di_ended: Vec<SystemId>,
    /// By the actor's sequence number.
    projectiles: IndexMap<i32, ProjFxState>,
    /// Every character's and shared class's module (taken out while its hooks run).
    chars: Vec<CharFx>,
    /// The decals placed by the last frame.
    decals: Vec<FloorDecal>,
    /// The level's Emitter actors' systems (begin_level).
    level: Vec<SystemId>,
    /// The thrown items' ParticleSystem components.
    item_fx: IndexMap<ItemId, ItemFx>,
    /// The tunnel carries' trails, the projectiles' and the items'.
    trails: IndexMap<ActorRef, Option<SystemId>>,
    /// The items part's class-view actors' particle components (items.rs).
    class_fx: items::ClassFxs,
    /// The reflects' PS_Reflect systems, and the actors whose systems the reflect pause last slowed.
    reflects: IndexMap<ActorRef, Reflected>,
    slowed: HashSet<ActorRef>,
    /// The spawned actors' views' bones of the latest tick.
    actor_bones: HashMap<ActorRef, Vec<(Arc<str>, Xform)>>,
    /// The spawned actors a view draws at the latest tick (a class rule's view that was made: its mesh and materials
    /// loaded).
    actor_drawn: HashSet<ActorRef>,
    /// The fighters' second views (fx/second.rs), by the fighter's id and the kind.
    seconds: IndexMap<(i32, SecondKind), second::SecondFx>,
    /// Another class's DestroyComponent of every component of a fighter's Buff_ScoobySnack (JakeHorse_Actor3's
    /// ScoobySnackBuff @18046), by the fighter's id: taken by Shaggy's module before its place.
    snack_destroy: Vec<i32>,
}

impl std::fmt::Debug for Fx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fx").field("player", &self.player).field("pawns", &self.pawns.len()).finish()
    }
}

/// The DI visual's trail emitter.
const DI_TRAIL_EMITTER: usize = 1;

impl Fx {
    pub fn new(store: &mvs_store::Store, seed: u32) -> Result<Fx> {
        Self::on_player(store, seed, Player::new(store, seed)?)
    }

    /// The effects of a match whose level the match before kept (`keep_level`): its particle player, the level's
    /// systems on it going on (the scenery's blocks hold their ids), and its Emitter actors' systems; no
    /// `begin_level`. The renderer has one particle player for its life.
    pub fn resume_level(store: &mvs_store::Store, seed: u32, kept: KeptLevel) -> Result<Fx> {
        let mut fx = Self::on_player(store, seed, kept.player)?;
        fx.level = kept.level;
        Ok(fx)
    }

    /// The end of a match whose level goes on (a Lab fighter change), after `end`: every system but the level's own
    /// ends and the particle player goes to the next match's effects
    /// (`resume_level`); an empty player of `store`'s takes its place here, for the match's last reads.
    pub fn keep_level(&mut self, store: &mvs_store::Store) -> Result<KeptLevel> {
        self.player.end_all(true);
        let fresh = Player::new(store, 0)?;
        Ok(KeptLevel { player: std::mem::replace(&mut self.player, fresh), level: std::mem::take(&mut self.level) })
    }

    fn on_player(store: &mvs_store::Store, seed: u32, player: Player) -> Result<Fx> {
        Ok(Fx {
            player,
            show_di: false,
            team_skins: true,
            ko_anchors: Vec::new(),
            ko_picks: Vec::new(),
            sounds: Vec::new(),
            components: crate::cmesh::ComponentMeshes::default(),
            camera: None,
            meshes: store.kind("mesh")?,
            fighter_pack: store.kind("fighter")?,
            material_pack: store.kind("material")?,
            draws: IndexMap::new(),
            splines: IndexMap::new(),
            rigs: IndexMap::new(),
            pawns: IndexMap::new(),
            gone_colours: IndexMap::new(),
            pawn_ids: Vec::new(),
            followers: Vec::new(),
            snack_destroy: Vec::new(),
            colours: Vec::new(),
            me: 0,
            online: false,
            ring_outs: ring_out::RingOuts::new(seed),
            started: false,
            di_ended: Vec::new(),
            projectiles: IndexMap::new(),
            chars: CharFx::all(),
            decals: Vec::new(),
            level: Vec::new(),
            item_fx: IndexMap::new(),
            trails: IndexMap::new(),
            class_fx: items::ClassFxs::default(),
            reflects: IndexMap::new(),
            slowed: HashSet::new(),
            actor_bones: HashMap::new(),
            actor_drawn: HashSet::new(),
            seconds: IndexMap::new(),
        })
    }

    /// The local player and whether the match is played online (the Team values' 0.6 select); before the first step.
    pub fn set_local(&mut self, me: usize, online: bool) {
        self.me = me;
        self.online = online;
    }

    /// The level's surfaces and Emitter actors on the particle player: the stage's
    /// horizontal surfaces every system's Collision modules meet (`surfaces::level_surfaces`, vfx.setSurfaces), then
    /// each Emitter actor's system at its place, its InstanceParameters and TranslucencySortPriority, active from the
    /// start; after the stage is set and before the first step.
    // Deviation: a system with a module type the particle player has no port for (mvs_vfx UNPORTED_MODULES, none
    // today) is not spawned; it is named once on stderr. The build spawns it.
    pub fn begin_level(
        &mut self,
        emitters: &[crate::scenery::LevelEmitter],
        surfaces: Vec<mvs_vfx::Surface>,
    ) -> Result<()> {
        self.player.set_surfaces(Some(surfaces));
        let mut params: Vec<(String, f64)> = Vec::new();
        let mut skipped: IndexMap<&str, String> = IndexMap::new();
        for e in emitters {
            if let Some(m) = skipped.get(e.system.as_str()).cloned().or_else(|| self.player.unported(&e.system)) {
                skipped.insert(&e.system, m);
                continue;
            }
            // The InstanceParameters as the systems read them: a vector one per axis.
            params.clear();
            for (k, v) in &e.params {
                match *v {
                    EmitterParam::Scalar(x) => params.push((k.clone(), x)),
                    EmitterParam::Vector(v) => {
                        params.extend(["X", "Y", "Z"].iter().zip(v).map(|(a, x)| (format!("{k}.{a}"), x)));
                    }
                }
            }
            let named: Vec<(&str, f64)> = params.iter().map(|(k, v)| (k.as_str(), *v)).collect();
            let r = rotation_rows(e.rot[0] * D2R, e.rot[1] * D2R, e.rot[2] * D2R);
            let at = Attachment {
                loc: e.loc,
                rot: [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]],
                scale: Some(e.scale),
            };
            // (a system the particles index lacks spawns nothing: a warning once and on)
            if let Some(h) = self.player.spawn_level(&e.system, &at, false, &named, e.sort, false) {
                self.level.push(h);
            }
        }
        for (system, m) in &skipped {
            eprintln!("level emitter {system} not spawned: its {m} module has no port");
        }
        Ok(())
    }

    /// The level's Emitter systems still live (begin_level's).
    pub fn level_live(&self) -> usize {
        self.level.iter().filter(|h| self.player.alive(**h)).count()
    }

    /// The decals placed by the last frame (the renderer's decal pass draws them: Renderer::floor_decals).
    pub fn decals(&self) -> &[FloorDecal] {
        &self.decals
    }

    /// Each drawn frame, after the views are placed and before the draw (the modules' `frame`):
    /// `alpha` the frame between the last two ticks.
    pub fn frame(&mut self, w: &World, alpha: f64) {
        self.decals.clear();
        self.each_char(|c, fx| c.frame(fx, w, alpha));
    }

    /// The match's end: the actors' effects and each module's `end`.
    pub fn end(&mut self, w: &World) {
        let ps: Vec<ProjFxState> = self.projectiles.drain(..).map(|(_, p)| p).collect();
        for mut p in ps {
            self.drop_projectile(w, &mut p);
        }
        for h in self.item_fx.drain(..).filter_map(|(_, r)| r.h) {
            self.player.kill(h);
        }
        for h in self.trails.drain(..).filter_map(|(_, h)| h) {
            self.player.kill(h);
        }
        self.end_class_items();
        for (_, r) in self.reflects.drain(..) {
            for h in r.hs {
                self.player.kill(h);
            }
        }
        self.slowed.clear();
        self.each_char(|c, fx| c.end(fx));
        // (endAll: every component mesh goes)
        self.components.clear();
    }

    /// Character `key`'s draws (the fighter kind's entry), loaded at the first ask; None, warned
    /// once, without an entry.
    pub(crate) fn draws(&mut self, key: &str) -> Option<&FighterDraws> {
        if !self.draws.contains_key(key) {
            let d = FighterDraws::read(&self.fighter_pack, self.material_pack.clone(), key)
                .map_err(|e| eprintln!("warning: the draws of {key}: {e}"))
                .ok();
            self.draws.insert(key.to_owned(), d);
        }
        self.draws.get(key).and_then(Option::as_ref)
    }

    /// The spline source of mesh `path` (crate::cmesh::SplineSource), read at the first ask.
    pub(crate) fn spline_source(&mut self, path: &str) -> std::result::Result<Arc<crate::cmesh::SplineSource>, String> {
        if !self.splines.contains_key(path) {
            let s = crate::cmesh::SplineSource::read(&self.meshes, path)
                .map_err(|e| attach::warn_once(format!("spline source {path}: {e}")))
                .ok()
                .map(Arc::new);
            self.splines.insert(path.to_owned(), s);
        }
        self.splines[path].clone().ok_or_else(|| format!("{path} is not readable as a spline source"))
    }

    /// The effects' part of the match's warm-up (crate::warm, `Renderer::prewarm_match`): the draws of every
    /// character of the match loaded, each module's list of the component meshes it may add (their sources read),
    /// the emitters of the particle systems the match's owners hold (its characters', the players' perks, taunts
    /// and ring-outs, every sticker of the characters' taunt tables, and the shared owners: base, intro, items, ffa,
    /// grow, the team owner snack and arcade, the arcade rounds' bot buffs), their templates loaded, and the floor
    /// decals' materials.
    // Unverified: the shared owners and stickers are taken in every match (intro, items, ffa, grow and team owners,
    // the stickers of the taunt slots): more is loaded than a match
    // draws, and the asset caches keep it for the session.
    pub fn warm(&mut self, w: &World) -> FxWarm {
        let mut keys: Vec<String> = w.data.chars.keys().cloned().collect();
        keys.sort();
        for k in &keys {
            self.draws(k);
        }
        let mut meshes = Vec::new();
        let mut chars = std::mem::take(&mut self.chars);
        for c in &mut chars {
            c.warm(self, w, &mut meshes);
        }
        self.chars = chars;
        let mut owners: Vec<&str> = keys.iter().map(String::as_str).collect();
        for p in &w.setup.players {
            owners.extend(p.perks.iter().chain(&p.taunts).chain(&p.ring_out).map(String::as_str));
        }
        // every sticker of each character's taunt table (a TestTaunt fills every slot)
        for c in w.data.chars.values() {
            let table = c.fighter.taunt_table.iter().flat_map(|t| t.iter());
            owners.extend(table.filter(|(_, e)| e.sticker.is_some()).map(|(n, _)| n.as_str()));
        }
        owners.extend(["base", "intro", "items", "ffa", "grow", "snack", "arcade"]);
        let emitters = self.player.warm(&owners);
        FxWarm { meshes, emitters, decals: floor_decals(w) }
    }

    /// `MatchHost.localHuman`: fighter `id` is a human this machine plays (offline every
    /// human; online the local player), by its slot: a mode pawn bound to a player's slot is that player's, one of no
    /// slot (a bag) no human.
    // Unverified: a bot is not.
    // (The build: IsLocallyControlled of the pawn's controller; a bag has none.)
    // Unverified: a demo pawn the tutorial drives is taken as the local human's (its controller is the sim's, off the
    // snapshot).
    pub(crate) fn local_human(&self, w: &World, id: i32) -> bool {
        let slot = w.by_id(id).map_or(id, |f| mvs_sim::world::pawns::slot_of(w, f));
        let human = usize::try_from(slot).ok().and_then(|i| w.setup.players.get(i)).is_some_and(|p| !p.bot);
        human && (!self.online || usize::try_from(slot).is_ok_and(|i| i == self.me))
    }

    /// `MatchHost.ownClient`: whether this machine runs fighter `id`'s client events: every
    /// fighter's offline (a standalone game), online only the local player's (by its slot, as `local_human`).
    pub(crate) fn own_client(&self, w: &World, id: i32) -> bool {
        let slot = w.by_id(id).map_or(id, |f| mvs_sim::world::pawns::slot_of(w, f));
        !self.online || usize::try_from(slot).is_ok_and(|i| i == self.me)
    }

    /// The mesh kind's pack, for geometry the modules lay from a static mesh.
    pub(crate) fn mesh_pack(&self) -> &Pack {
        &self.meshes
    }

    /// Runs `f` on each module in order, the module's state taken out of `self` for the call.
    fn each_char(&mut self, mut f: impl FnMut(&mut CharFx, &mut Fx)) {
        let mut chars = std::mem::take(&mut self.chars);
        for c in &mut chars {
            f(c, self);
        }
        self.chars = chars;
    }

    /// Runs `f` on each module in order until one returns true (the event taken).
    fn char_takes(&mut self, mut f: impl FnMut(&mut CharFx, &mut Fx) -> bool) -> bool {
        let mut chars = std::mem::take(&mut self.chars);
        let taken = chars.iter_mut().any(|c| f(c, self));
        self.chars = chars;
        taken
    }

    /// Runs a projectile rule's hook with its module's state.
    fn rule_hook(&mut self, w: &World, k: usize, hook: Option<chars::ProjHook>, st: &mut ProjFxState) {
        let Some(hook) = hook else { return };
        let mut chars = std::mem::take(&mut self.chars);
        hook(&mut chars[k], self, w, st);
        self.chars = chars;
    }

    /// After each sim step and the poser's place: the step's events spawn, move and end systems; the player ticks.
    /// `fighters` and `actors` are the poses at the latest tick (alpha 1), for the sockets and bones the systems ride.
    pub fn step(&mut self, world: &World, fighters: &[FighterPose], actors: &[ActorPose]) -> Result<()> {
        if !self.started {
            self.start(world);
        }
        self.add_pawns(world);
        self.bodies(world, fighters)?;
        self.second_bodies(world, actors)?;
        self.minion_bodies(world, actors)?;
        self.mesh_visibility(world);
        self.actor_bones.clear();
        self.actor_drawn.clear();
        for a in actors {
            if let (Some(r), false) = (a.actor, a.bones.is_empty()) {
                self.actor_bones.insert(r, a.bones.clone());
            }
            if let Some(r) = a.actor {
                self.actor_drawn.insert(r);
            }
        }
        // Each presented tick: the events (the DI visuals' countdown first), the
        // smoke trails, the modules' actors, the heal, the modules' places, the projectiles' components, the PlayVFX
        // systems' hitpause dilation, the particle tick, then the modules' after-tick.
        self.tick_di_visuals();
        // (ahead of the events: a new projectile's view placed, its class view's components made)
        self.place_class_items(world, true);
        for e in &world.events {
            self.event(world, e);
        }
        // (after the events: a despawned pawn's last events are read with its entry)
        self.drop_gone_pawns(world);
        self.place_smoke_trails(world);
        self.each_char(|c, fx| c.actors(fx, world));
        self.place_heal(world);
        self.each_char(|c, fx| c.place(fx, world));
        self.place_projectiles(world);
        self.dilate_notify_fx(world);
        self.follow(world);
        self.player.tick(STEP);
        self.each_char(|c, fx| c.after_tick(fx, world));
        // (the frame's placeProjectiles, after the tick: the items' thrown components and tunnel trails)
        self.place_items(world);
        self.place_class_items(world, false);
        Ok(())
    }

    /// A Lab reset: every one-shot system ends; the kept components (the smoke trails)
    /// stay, inactive and empty.
    pub fn clear(&mut self) {
        self.player.clear();
        for p in self.pawns.values_mut() {
            let (smoke, minion, colour, intro) = (p.smoke, p.minion, p.colour, p.intro.take());
            let (body, mesh, skin) = (p.body.take(), std::mem::take(&mut p.mesh), std::mem::take(&mut p.skin));
            let slug = std::mem::take(&mut p.slug);
            *p = PawnFx { body, mesh, skin, slug, smoke, minion, colour, intro, ..PawnFx::default() };
            p.heal.0 = -1.0;
        }
        let player = &self.player;
        self.followers.retain(|(h, _)| player.alive(*h));
        self.di_ended.clear();
        self.projectiles.clear();
        self.seconds.clear();
        self.ko_anchors.clear();
        self.each_char(|c, fx| c.reset(fx));
    }

    fn start(&mut self, w: &World) {
        self.started = true;
        // (without the team-coloured skins, as the effects do: a skin material picked from these is not the
        // variant the fighter's view draws; take the view's variant for that, as jake.rs's stretch body)
        self.colours = fighter_colours(&w.setup, self.me, self.online, false);
        // PandaGameState's preload (@12548-12712): GetParticleDataToPlay once per player's ring-out.
        for p in &w.setup.players {
            let a = mvs_sim::world::blast::ring_out_asset_name(w, p.ring_out.as_deref());
            self.ring_outs.particle_data_to_play(&w.data.ring_outs, a);
        }
        for fid in w.all_ids() {
            self.pawn_entry(w, fid);
        }
    }

    /// The pawn entry of fighter `fid`: a player's at the start, a mode pawn's at the step that first finds it.
    /// A mode pawn's colour is its slot's, the second player's without one (the view's rule,
    /// `colours`).
    fn pawn_entry(&mut self, w: &World, fid: FighterId) {
        let f = w.fighter(fid);
        let id = f.id;
        let d = &w.char_data(fid).fighter;
        let pick = w.setup.players.get(id as usize).and_then(|p| p.skin.as_deref());
        let look = skin_of(d, pick).ok();
        // (skin_intro.rs mutedByIntro: what of the sim's default intro a skin's own intro does not present)
        let intro = look.as_ref().and_then(|s| IntroMute::of(d, s));
        let parts = look
            .as_ref()
            .map(|s| s.parts.iter().map(|p| p.key.clone()).chain(s.part_tags.iter().flatten().cloned()).collect())
            .unwrap_or_default();
        let (skin, slug) = look.map(|s| (s.name, s.slug)).unwrap_or_default();
        // Unverified: a pawn bound to no slot takes the second player's colours, the first's without one.
        let colour = if self.colours.get(id as usize).is_some() {
            None
        } else {
            let slot = usize::try_from(mvs_sim::world::pawns::slot_of(w, fid)).ok();
            slot.and_then(|i| self.colours.get(i)).or(self.colours.get(1)).or(self.colours.first()).copied()
        };
        // DamageManager BeginPlay: HitSmokeTrail = SpawnEmitterAttached(PS_CharacterSmokeTrail, the root,
        // SnapToTarget, scale 1), deactivated at once.
        let smoke = self.player.attach_component("PS_CharacterSmokeTrail", [0.0; 3], [0.0; 3], 1.0, &[]);
        self.pawns
            .insert(id, PawnFx { skin, slug, parts, smoke, intro, colour, heal: (-1.0, None, 0), ..PawnFx::default() });
    }

    /// A mode pawn that appeared gets its entry (the players' are made at the start).
    fn add_pawns(&mut self, w: &World) {
        let Some(ps) = &w.pawns else { return };
        for &fid in ps {
            let fi = w.fighter(fid);
            if !fi.life.is_minion && !self.pawns.contains_key(&fi.id) {
                self.pawn_entry(w, fid);
            }
        }
    }

    /// A pawn that left the world has its components killed and its entry dropped (its smoke trail
    /// would stay in the player's list otherwise).
    fn drop_gone_pawns(&mut self, w: &World) {
        if w.pawns.is_none() {
            return;
        }
        while let Some(id) =
            self.pawns.iter().find(|(id, p)| p.minion.is_none() && w.by_id(**id).is_none()).map(|x| *x.0)
        {
            if let Some(p) = self.pawns.shift_remove(&id) {
                // (the port's own) its colour kept for the projectiles that still name it as their owner, in flight
                // or spawned later (a Waves run's rung-out enemy): the last GONE_KEPT pawns' only
                if let Some(c) = p.colour {
                    self.gone_colours.insert(id, c);
                    if self.gone_colours.len() > GONE_KEPT {
                        self.gone_colours.shift_remove_index(0);
                    }
                }
                p.kill_components(&mut self.player);
            }
        }
    }

    /// The fighters' bodies from this tick's poses (kept from the last pose for a fighter not posed now).
    fn bodies(&mut self, w: &World, fighters: &[FighterPose]) -> Result<()> {
        for fp in fighters {
            let Some(fid) = w.by_id(fp.fighter as i32) else { continue };
            let Some(mesh) = fp.meshes.first() else { continue };
            let id = w.fighter(fid).id;
            let path = &*mesh.mesh;
            if !self.rigs.contains_key(path) {
                let info = MeshInfo::read(&self.meshes.bytes(path)?, path)?;
                let key = w.names.str(w.fighter(fid).ch);
                let rig = info.skeleton.is_some().then(|| {
                    Rig::new(&info, &w.char_data(fid).fighter.skeleton)
                        .with_kept(mvs_view::view::kept_bones(key, &info))
                });
                self.rigs.insert(path.to_owned(), rig);
            }
            let Some(Some(rig)) = self.rigs.get(path) else { continue };
            let left = facing(w, fid) < 0.0;
            if let Some(p) = self.pawns.get_mut(&id) {
                p.body.get_or_insert_with(Body::default).set(&mesh.to_world, &mesh.bones, rig, left);
                if p.mesh != path {
                    p.mesh = path.to_owned();
                }
                p.heart = fp.heart;
            }
        }
        Ok(())
    }

    /// The systems the fighter's mesh hides with it (Comp_FighterVisibilityManager
    /// AddInvisibility: each component of its VFX array, which BaseFighter AddVFX fills with the PlayVFX notifies'
    /// emitters, @1576..1957; the rage effects and the residual, attached to the mesh) shown while
    /// `mesh_hidden` is false. Before the step's events, as the views are placed before the presented tick. A notify's
    /// or an emote's system is hidden at its spawn as well (AddVFX @68..183).
    // Deviation: the rage effects and the residual started while the fighter is hidden are drawn until
    // the next step hides them (AddVFX is not modelled for them; AddVFX hides them at once).
    // Deviation: the smoke's AddInvisibility for the viewer (occlusionHides) is not ported, as the views'.
    fn mesh_visibility(&mut self, w: &World) {
        for (&id, p) in &self.pawns {
            let Some(fid) = w.by_id(id).filter(|_| p.minion.is_none()) else { continue };
            let shown = !mesh_hidden(w, fid);
            let notify = p.notify.iter().filter_map(|n| n.1);
            for h in p.rage.iter().copied().chain(p.residual.0).chain(notify) {
                self.player.set_visible(h, shown);
            }
        }
    }

    /// Comp_FighterVisibilityManager AddVFX (@68..183): a component added while the fighter is hidden is hidden at
    /// once.
    fn hide_if_hidden(&mut self, w: &World, id: i32, h: Option<SystemId>) {
        if let (Some(h), Some(fid)) = (h, w.by_id(id))
            && mesh_hidden(w, fid)
        {
            self.player.set_visible(h, false);
        }
    }

    /// The minions' views' bodies from this tick's actor poses (an ActorPose of a minion: its view's meshes, the
    /// main mesh first), on a pawn entry of the minion's own (`minion_pawn`).
    fn minion_bodies(&mut self, w: &World, actors: &[ActorPose]) -> Result<()> {
        for a in actors {
            let Some(ActorRef::Fighter(fid)) = a.actor else { continue };
            if a.second.is_some() || !w.fighter(fid).life.is_minion {
                continue;
            }
            let Some(mesh) = a.meshes.first() else { continue };
            let id = w.fighter(fid).id;
            self.minion_pawn(w, id);
            let path = mesh.mesh.to_string();
            if !self.rigs.contains_key(&path) {
                let info = MeshInfo::read(&self.meshes.bytes(&path)?, &path)?;
                let rig = info.skeleton.is_some().then(|| Rig::new(&info, &w.char_data(fid).fighter.skeleton));
                self.rigs.insert(path.clone(), rig);
            }
            let Some(Some(rig)) = self.rigs.get(&path) else { continue };
            let body = Body::of(mesh, rig, facing(w, fid) < 0.0);
            if let Some(p) = self.pawns.get_mut(&id) {
                p.body = Some(body);
                p.mesh = path;
            }
        }
        Ok(())
    }

    /// The pawn entry of minion `id`, made at its first sight with its creator's
    /// colour index; a pawn of the match's or an id no fighter has is left as it is.
    fn minion_pawn(&mut self, w: &World, id: i32) {
        if self.pawns.contains_key(&id) {
            return;
        }
        let Some(fid) = w.by_id(id).filter(|&f| w.fighter(f).life.is_minion) else { return };
        let creator = w.fighter(fid).life.minion_creator.map(|c| w.fighter(c).id);
        let ci = creator.and_then(|c| self.colours.get(c as usize)).map_or(0, |c| c.color_index);
        self.pawns.insert(id, PawnFx { minion: Some(ci), heal: (-1.0, None, 0), ..PawnFx::default() });
    }

    /// Whether `id` is a pawn of the match's with effects of its own (no minion's entry).
    fn is_player_pawn(&self, id: i32) -> bool {
        self.pawns.get(&id).is_some_and(|p| p.minion.is_none())
    }

    /// `team`: SelectFloat(0.6, ColorIndex, IsLocalOnlinePlayer); a minion's (MinionFx.team): its ForcedColorIndex,
    /// never the local 0.6.
    fn team(&self, id: i32) -> f64 {
        match self.colours.get(id as usize) {
            Some(c) => c.team,
            None => match self.pawns.get(&id) {
                Some(PawnFx { minion: Some(ci), .. }) => f64::from(*ci),
                Some(PawnFx { colour: Some(c), .. }) => c.team,
                _ => match self.gone_colours.get(&id) {
                    Some(c) => c.team,
                    None => panic!("fx: no pawn entry for fighter {id} (a step makes one for each pawn it finds)"),
                },
            },
        }
    }

    fn color_index(&self, id: i32) -> u8 {
        match self.colours.get(id as usize) {
            Some(c) => c.color_index,
            None => match self.pawns.get(&id) {
                Some(p) => p.minion.or(p.colour.map(|c| c.color_index)).unwrap_or(0),
                None => self.gone_colours.get(&id).map_or(0, |c| c.color_index),
            },
        }
    }

    /// Whether a view draws spawned actor `a` at the latest tick (its class rule's view was made).
    pub(crate) fn actor_drawn(&self, a: ActorRef) -> bool {
        self.actor_drawn.contains(&a)
    }

    fn rig(&self, id: i32) -> Option<(&Body, &Rig)> {
        let p = self.pawns.get(&id)?;
        Some((p.body.as_ref()?, self.rigs.get(&p.mesh)?.as_ref()?))
    }

    /// `skelName`.
    fn skel_name(&self, id: i32, n: &str) -> String {
        self.rig(id).map_or_else(|| n.to_string(), |(_, r)| r.name(n))
    }

    // ---- places

    /// An attached component's world transform now.
    fn place(&self, w: &World, f: &Follow) -> Option<Attachment> {
        match f {
            Follow::Mesh { id, rel, rows, scale } => {
                let b = self.pawns.get(id)?.body.as_ref()?;
                let (loc, rot) = b.mesh_attach(*rel, rows.as_ref());
                let s = match scale {
                    Scale::Fixed(s) => Some(*s),
                    Scale::ByMesh(s) => Some(s.map(|v| v * b.scale)),
                    Scale::Own | Scale::Residual => None,
                };
                Some(att(loc, &rot, s))
            }
            Follow::Socket { id, s, scale } => {
                let (body, rig) = self.rig(*id)?;
                let Some((loc, rot, sc)) = s.eval(body, rig) else {
                    // (the bone is not in the pose: the mesh component's origin)
                    let (loc, rot) = body.mesh_attach([0.0; 3], None);
                    return Some(att(loc, &rot, None));
                };
                let sc = match scale {
                    Scale::Own => sc,
                    Scale::Fixed(f) => *f,
                    Scale::ByMesh(f) => f.map(|v| v * body.scale),
                    Scale::Residual => sc.map(|v| v * self.pawns.get(id).map_or(1.0, |p| p.residual.1)),
                };
                Some(att(loc, &rot, Some(sc)))
            }
            Follow::Root { id, rel, up, scale } => {
                let fid = w.by_id(*id)?;
                let fm = w.fighter(fid);
                let rows = compose_rows(rel, &mesh_rows(facing(w, fid) < 0.0));
                Some(att([0.0, f64::from(-fm.body.x), f64::from(actor_y(w, fid)) + up], &rows, *scale))
            }
            Follow::RootScaled { id, loc } => {
                let fid = w.by_id(*id)?;
                let fm = w.fighter(fid);
                let (xy, z) = fm.body.actor_scale.map_or((fm.body.size_scale, fm.body.size_scale), |a| (a[0], a[1]));
                let right = facing(w, fid) > 0.0;
                let rot = mesh_rows(!right);
                let d = mul_row([loc[0] * f64::from(xy), loc[1] * f64::from(xy), loc[2] * f64::from(z)], &rot);
                Some(att(
                    [d[0], -f64::from(fm.body.x) + d[1], f64::from(actor_y(w, fid)) + d[2]],
                    &rot,
                    Some([f64::from(xy), f64::from(xy), f64::from(z)]),
                ))
            }
            Follow::Evade { id, sign } => {
                let fid = w.by_id(*id)?;
                let fm = w.fighter(fid);
                let rot = mesh_rows(facing(w, fid) < 0.0);
                let k = fm.body.size_scale;
                let y = sign * 200.0 * f64::from(k);
                let loc = [y * rot[3], -f64::from(fm.body.x) + y * rot[4], f64::from(actor_y(w, fid)) + y * rot[5]];
                Some(att(loc, &rot, Some([f64::from(k); 3])))
            }
            Follow::Capsule { id, rel, turned, cap_up } => {
                let fid = w.by_id(*id)?;
                let fm = w.fighter(fid);
                let rot = if *turned { mesh_rows(facing(w, fid) < 0.0) } else { IDENTITY };
                let d = mul_row(*rel, &rot);
                let up = if *cap_up { body_def(w, fid).inner_cap_half * f64::from(fm.body.size_scale) } else { 0.0 };
                Some(att([d[0], -f64::from(fm.body.x) + d[1], f64::from(actor_y(w, fid)) + d[2] + up], &rot, None))
            }
            Follow::CapsuleRel { id, rel, scale } => {
                let fid = w.by_id(*id)?;
                let fm = w.fighter(fid);
                let s = capsule_scale(w, fid);
                let rot = mesh_rows(facing(w, fid) < 0.0);
                let d = mul_row([rel[0] * s[0], rel[1] * s[1], rel[2] * s[2]], &rot);
                let ws = [s[0] * scale[0], s[1] * scale[1], s[2] * scale[2]];
                Some(att([d[0], -f64::from(fm.body.x) + d[1], f64::from(actor_y(w, fid)) + d[2]], &rot, Some(ws)))
            }
            Follow::Projectile { seq, rel, rel_rot, scale } => {
                let (_, pr) = w.actors.projectiles.iter().find(|(_, p)| p.seq == *seq)?;
                let rot = actor_rot(w, pr);
                let mut loc = [0.0, -f64::from(pr.x), f64::from(pr.y)];
                let d = mul_row(*rel, &rot);
                for k in 0..3 {
                    loc[k] += f64::from(pr.scale) * d[k];
                }
                Some(att(loc, &compose_rows(rel_rot, &rot), Some(scale.map(|v| v * f64::from(pr.scale)))))
            }
            Follow::Fixed { seq, rel, rows, scale } => {
                let (_, pr) = w.actors.projectiles.iter().find(|(_, p)| p.seq == *seq)?;
                let s = f64::from(pr.scale);
                let d = mul_row(*rel, rows);
                let loc = [s * d[0], -f64::from(pr.x) + s * d[1], f64::from(pr.y) + s * d[2]];
                Some(att(loc, rows, Some([s * scale; 3])))
            }
            Follow::Live { seq, c } => {
                let (_, pr) = w.actors.projectiles.iter().find(|(_, p)| p.seq == *seq)?;
                let cs = pr.owner.and_then(|o| actor_def(w, ActorRef::Fighter(o), pr.cls)).map(|d| &d.components);
                let par = match (c.rule.parent, cs) {
                    (Some(p), Some(cs)) => p(w, pr, cs, &c.cn),
                    _ => c.par0.clone(),
                };
                let (rel, rs, ms, rel_rot) =
                    under(&par, c.own_loc, &c.own_rot, c.crs, c.rule.world_scale, c.par0.scale.map(f64::from));
                let rot = actor_rot(w, pr);
                let mut loc = [0.0, -f64::from(pr.x), f64::from(pr.y)];
                let d = mul_row(rel, &rot);
                for k in 0..3 {
                    loc[k] += f64::from(pr.scale) * d[k];
                }
                // (a relative scale the class's graph sets in place of the component's own: the rule's scale)
                let s = c.rule.scale.and_then(|f| f(w, pr, ms)).unwrap_or(rs);
                Some(att(loc.map(f64::from), &compose_rows(&rel_rot, &rot), Some(s.map(|v| v * f64::from(pr.scale)))))
            }
            Follow::Actor { a, rel, rel_rot, scale, turned } => {
                let p = actor_place(w, *a)?;
                let rot = if *turned { p.rot } else { IDENTITY };
                let d = mul_row(*rel, &rot);
                let loc = [p.scale * d[0], -p.x + p.scale * d[1], p.y + p.scale * d[2]];
                Some(att(loc, &compose_rows(rel_rot, &rot), Some(scale.map(|v| v * p.scale))))
            }
            Follow::Carry { a } => {
                let t = mvs_sim::actors::tunnel_carry::carry_trail(w, *a)?;
                let p = actor_place(w, *a)?;
                Some(att([0.0, f64::from(-t.x), f64::from(t.y)], &p.rot, Some([p.scale; 3])))
            }
            Follow::Second { id, kind, s, rel, rows, scale } => {
                self.place_second(*id, *kind, s.as_deref(), *rel, rows, *scale)
            }
            Follow::Bone { a, bone, loc, rel, scale } => {
                let bones = self.actor_bones.get(a)?;
                let (_, b) = bones.iter().find(|(n, _)| n.eq_ignore_ascii_case(bone))?;
                let off = [0, 1, 2].map(|c| (0..3).map(|r| loc[r] * b.rot[3 * r + c] * b.scale[r]).sum::<f64>());
                let at = [0, 1, 2].map(|k| b.loc[k] + off[k]);
                Some(att(at, &compose_rows(rel, &b.rot), Some([0, 1, 2].map(|k| scale[k] * b.scale[k]))))
            }
        }
    }

    /// Every attached component to its place before the player's tick; the ended ones dropped.
    fn follow(&mut self, w: &World) {
        let mut followers = std::mem::take(&mut self.followers);
        followers.retain(|(h, _)| self.player.alive(*h));
        for (h, f) in &followers {
            if let Some(a) = self.place(w, f) {
                self.player.attach(*h, &a);
            }
        }
        self.followers = followers;
        for p in self.pawns.values_mut() {
            p.mesh_psc.retain(|(_, _, h)| self.player.alive(*h));
        }
    }

    /// SpawnEmitterAttached with its follower (`vfx.spawnAttached`); None where it has no place yet (no pose).
    fn spawn_follow(&mut self, w: &World, ps: &str, f: Follow, scale: f64, params: &[(&str, f64)]) -> Option<SystemId> {
        let a = self.place(w, &f)?;
        let h = self.player.spawn_attached(ps, &a, scale, params)?;
        self.followers.push((h, f));
        Some(h)
    }

    /// `attachToMesh`: a component on fighter `id`'s mesh, in its AttachChildren.
    fn attach_to_mesh(
        &mut self,
        w: &World,
        id: i32,
        ps: &str,
        socket: &str,
        f: Follow,
        params: &[(&str, f64)],
    ) -> Option<SystemId> {
        let h = self.spawn_follow(w, ps, f, 1.0, params)?;
        if let Some(p) = self.pawns.get_mut(&id) {
            p.mesh_psc.push((ps.to_string(), socket.to_string(), h));
        }
        Some(h)
    }

    /// `socketFollow`: a socket or bone (in any case) of fighter `id`, or its mesh component for None.
    fn socket_follow(&self, w: &World, id: i32, socket: Option<&str>, loc: V3, rot: &Rows, scale: V3) -> Follow {
        match socket {
            None | Some("None") | Some("") => {
                Follow::Mesh { id, rel: loc, rows: Some(*rot), scale: Scale::ByMesh(scale) }
            }
            Some(s) => {
                let name = self.skel_name(id, s);
                let fid = w.by_id(id).expect("a pawn's fighter");
                let skel = &w.char_data(fid).fighter.skeleton;
                Follow::Socket { id, s: Box::new(SocketFollow::new(skel, &name, loc, rot, scale)), scale: Scale::Own }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn attach_socket(
        &mut self,
        w: &World,
        id: i32,
        ps: &str,
        socket: &str,
        loc: V3,
        rot: &Rows,
        scale: V3,
        params: &[(&str, f64)],
    ) -> Option<SystemId> {
        let f = self.socket_follow(w, id, Some(socket), loc, rot, scale);
        self.attach_to_mesh(w, id, ps, socket, f, params)
    }

    /// The mesh component's RelativeScale3D (the character's mesh scale).
    fn mesh_relative_scale(&self, w: &World, id: i32) -> f64 {
        w.by_id(id).map_or(1.0, |f| w.char_data(f).boxes.mesh_scale)
    }

    // ---- the events

    /// `mutedByIntro`: a sim event of a pawn's default intro whose presentation its skin's own
    /// intro replaces (a sound or an effect not shown, not played).
    pub fn muted_by_intro(&self, w: &World, e: &Event) -> bool {
        let id = match *e {
            Event::Sound { fighter, .. }
            | Event::SoundStart { fighter, .. }
            | Event::NotifyVfx { fighter, .. }
            | Event::ParticleNotify { fighter, .. } => fighter,
            _ => return false,
        };
        let Some(s) = self.pawns.get(&id).and_then(|p| p.intro.as_ref()) else { return false };
        w.by_id(id).is_some_and(|f| s.muted(w, f, e))
    }

    fn event(&mut self, w: &World, e: &Event) {
        if self.muted_by_intro(w, e) {
            return;
        }
        // (an event that names a minion and is no sound goes to the modules that draw minions, and no further: untaken,
        // it is dropped; one only a module's graphs raise goes to that module; the rest are the shared path's below)
        if self.names_minion(w, e) {
            self.char_takes(|c, fx| c.minion_event(fx, w, e));
            return;
        }
        if self.char_takes(|c, fx| c.event(fx, w, e)) {
            return;
        }
        self.shared_event(w, e);
    }

    /// handleEvents' own cases (the drawing of a pawn's events); a minion module hands its minion's
    /// events of the same rules here (notifyVfx, particleNotify, hitVfx, moveFx,
    /// projectileDeath), on the minion's own pawn entry.
    fn shared_event(&mut self, w: &World, e: &Event) {
        match *e {
            Event::BounceVfx { x, y, roll } => {
                // BFBounceEffects: SpawnEmitterAtLocation(PS_HitBounceImpact, ImpactPoint, (roll, 0, 0), 1.35).
                self.player.spawn(
                    "PS_HitBounceImpact",
                    [0.0, f64::from(-x), f64::from(y)],
                    [0.0, 0.0, f64::from(roll)],
                    [1.35; 3],
                    &[],
                );
            }
            Event::KoVfx { fighter, asset, hold, p, tl, tr, bl, center } => self.ko_vfx(
                w,
                fighter,
                w.names.str(asset),
                f64::from(hold),
                ring_out::KoInputs {
                    p: p.map(f64::from),
                    tl: tl.map(f64::from),
                    tr: tr.map(f64::from),
                    bl: bl.map(f64::from),
                    center: center.map(f64::from),
                },
            ),
            Event::HitVfx { victim, x, y, z_floor, vx, vy, dist, decay_stacks, override_vfx, force_small } => {
                // DamageManager PlayHitVFX: at the victim's nearest bone.
                let decayed = decay_stacks >= 3.0;
                let large = dist >= 3500.0 && !force_small;
                let sys = match override_vfx {
                    Some(o) => w.names.str(o),
                    None if large => "PS_HitImpact_Directional_01",
                    None if decayed => "PS_HitImpact_TEMPLATE_Grayscale",
                    None => "PS_HitImpact_TEMPLATE",
                };
                let (uy, uz) = (-vx, vy);
                let speed = uy.hypot(uz);
                let (pitch, yaw) = if speed > 0.0 { (uz.atan2(uy.abs()), uy.atan2(0.0)) } else { (0.0, 0.0) };
                let scale = 1.0 + (speed / 6000.0).clamp(0.0, 1.0) * 0.8;
                let at = self.nearest_bone(victim, [0.0, f64::from(-x), f64::from(y.max(z_floor))]);
                let team = if decayed { 0.5 } else { self.team(victim) };
                self.player.spawn(
                    sys,
                    at,
                    [f64::from(pitch) / D2R, f64::from(yaw) / D2R, 0.0],
                    [f64::from(scale); 3],
                    &[("Team", team)],
                );
            }
            Event::SmokeTrail { fighter, on } => {
                // EnableTrailEffect: Activate(false), Team.
                let Some(h) = self.pawns.get(&fighter).and_then(|p| p.smoke) else { return };
                if !on {
                    return self.player.deactivate(h);
                }
                self.player.activate(h);
                let team = self.team(fighter);
                self.player.set_param(h, "Team", team);
            }
            Event::DiVisual { fighter, attacker, vx, vy, x, y } => {
                self.di_visual(fighter, attacker, f64::from(vx), f64::from(vy), f64::from(x), f64::from(y))
            }
            Event::MoveFx { fighter, kind, x, y } => self.move_fx(w, fighter, kind, f64::from(x), f64::from(y)),
            Event::NotifyVfx { fighter, n, on, left, copy, poly } => {
                // (a second mesh's or a Polymorph mesh's: its module's event; no module drew it here)
                if copy || poly {
                    return warn_undrawn(on, "notify VFX of a second or Polymorph mesh");
                }
                self.notify_vfx(w, fighter, n, on, left);
            }
            Event::EmoteVfx { fighter, n, on, ps, scale } => {
                if !on {
                    return self.end_notify(fighter, n);
                }
                // AnimNotify_PlayEmoteVFX -> AddVFX on the mesh: relative location Offset, world scale Scale.
                let off = notify_of(w, w.by_id(fighter).expect("a pawn"), n).map(|x| xyz(x.p.get("Offset"), 0.0));
                let f = Follow::Mesh {
                    id: fighter,
                    rel: off.unwrap_or([0.0; 3]),
                    rows: None,
                    scale: Scale::Fixed(scale.map(f64::from)),
                };
                let h = self.attach_to_mesh(w, fighter, w.names.str(ps), "None", f, &[]);
                // (AddVFX @130229 is a Map_Add: a second begin overwrites the entry, the earlier plays out)
                if let Some(p) = self.pawns.get_mut(&fighter) {
                    p.notify.retain(|(k, _)| *k != n);
                    p.notify.push((n, h));
                }
                self.hide_if_hidden(w, fighter, h);
            }
            Event::ParticleNotify { fighter, n, on, left, copy, poly } => {
                if copy || poly {
                    return warn_undrawn(on, "particle notify of a second or Polymorph mesh");
                }
                self.particle_notify(w, fighter, n, on, left);
            }
            Event::RageVfx { fighter, on } => self.rage_vfx(w, fighter, on),
            Event::RageResidual { fighter, scale } => self.rage_residual(w, fighter, scale.map(f64::from)),
            Event::SpawnFx { fighter, on } => {
                // BaseFighter SpawnInvulnerabilityVisuals: PS_Invulnerability on the mesh, scale 1, Team;
                // DestroySpawnVFX destroys it.
                let Some(p) = self.pawns.get_mut(&fighter) else { return };
                if !on {
                    if let Some(h) = p.spawn.take() {
                        self.player.kill(h);
                    }
                    return;
                }
                if p.spawn.is_some_and(|h| self.player.alive(h)) {
                    return;
                }
                let f = Follow::Mesh { id: fighter, rel: [0.0; 3], rows: None, scale: Scale::ByMesh([1.0; 3]) };
                let team = self.team(fighter);
                let h = self.attach_to_mesh(w, fighter, "PS_Invulnerability", "None", f, &[("Team", team)]);
                if let Some(p) = self.pawns.get_mut(&fighter) {
                    p.spawn = h;
                }
            }
            Event::ArmorHit { fighter, x, y } => {
                // CallArmorHitMulticast @1451: PS_ArmorImpact1 on the root, FindLookAtRotation(actor, (X, hit Y,
                // hit Z)), KeepWorldPosition.
                let Some(fid) = w.by_id(fighter) else { return };
                let fm = w.fighter(fid);
                let (dy, dz) = (-(x - fm.body.x), y - actor_y(w, fid));
                let yaw = if dy == 0.0 { 0.0 } else { dy.atan2(0.0) };
                let pitch = dz.atan2(dy.abs());
                let root = mesh_rows(facing(w, fid) < 0.0);
                let rel = compose_rows(&rotation_rows(f64::from(pitch), f64::from(yaw), 0.0), &transpose(&root));
                self.spawn_follow(
                    w,
                    "PS_ArmorImpact1",
                    Follow::Root { id: fighter, rel, up: 0.0, scale: None },
                    1.0,
                    &[],
                );
            }
            Event::ArmorBreak { fighter } => {
                // ArmorBreakMulticast @2267: PS_ArmorBreak01 at the actor + (0, 0, CapsuleHalfHeight), Y mirrored
                // facing right, Team.
                let Some(fid) = w.by_id(fighter) else { return };
                let fm = w.fighter(fid);
                let left = facing(w, fid) < 0.0;
                let half = if fm.capsule.cmc_crouched {
                    let z = fm.body.actor_scale.map_or(fm.body.size_scale, |a| a[1]);
                    mvs_sim::fighter::capsule_half(w, fid) / z
                } else {
                    fm.capsule.cap_half_u
                };
                let scale = Some(if left { [1.0, 1.0, 1.0] } else { [1.0, -1.0, 1.0] });
                let team = self.team(fighter);
                let f = Follow::Root { id: fighter, rel: IDENTITY, up: f64::from(half), scale };
                self.spawn_follow(w, "PS_ArmorBreak01", f, 1.0, &[("Team", team)]);
            }
            Event::EvadeFx { fighter } => {
                // The evade: PS_EvadeConfirm at Y = SelectFloat(-200, 200, FacingLeft) @314.
                let Some(fid) = w.by_id(fighter) else { return };
                let sign = if facing(w, fid) < 0.0 { -1.0 } else { 1.0 };
                let team = self.team(fighter);
                self.spawn_follow(w, "PS_EvadeConfirm", Follow::Evade { id: fighter, sign }, 1.0, &[("Team", team)]);
            }
            Event::ProjectileDeath { cls, fighter, vfx, x, y, ox, rot, scale, .. } => {
                // (an emitter of the class's own graph that its rule draws)
                if let Some((k, chars::ProjRule { own_emitter: Some(own), .. })) = chars::proj_rule(w.names.str(cls)) {
                    let mut chars = std::mem::take(&mut self.chars);
                    let taken = own(&mut chars[k], self, w, e);
                    self.chars = chars;
                    if taken {
                        return;
                    }
                }
                // SpawnedActorParent EndPlay DeathVFX, Team = 0.6 with no instigator.
                // Unverified: with no instigator (fighter -1) the select's LocalColorIndex 0.6.
                if let Some(v) = vfx {
                    let team = if fighter < 0 { 0.6 } else { self.team(fighter) };
                    self.player.spawn(
                        w.names.str(v),
                        [f64::from(ox), f64::from(-x), f64::from(y)],
                        rot.map(f64::from),
                        [f64::from(scale); 3],
                        &[("Team", team)],
                    );
                }
            }
            Event::ItemFx { vfx, x, y, scale, .. } => {
                // MapRandomActorSpawnArea MulticastPreSpawnVFX @4473: at the spawn point, rotation 0.
                self.player.spawn(
                    w.names.str(vfx),
                    [0.0, f64::from(-x), f64::from(y)],
                    [0.0; 3],
                    [f64::from(scale); 3],
                    &[],
                );
            }
            Event::IntroFx { fighter } => {
                // PlayIntroAnimation (@161953): VFX Scaling(PS_CharacterSelectSwirl) at the mesh, its scale.
                let Some(b) = self.pawns.get(&fighter).and_then(|p| p.body.as_ref().map(Body::place)) else { return };
                let yaw = if b.left { 90.0 } else { -90.0 };
                self.player.spawn("PS_CharacterSelectSwirl", b.loc, [0.0, yaw, 0.0], [b.scale; 3], &[]);
            }
            Event::Abduct { fighter } => {
                // AbductPlayer: PS_EndgameLosers on the Capsule at (0, 0, its raw half height), Team = ColorIndex;
                // it follows the capsule.
                let ci = f64::from(self.color_index(fighter));
                let f = Follow::Capsule { id: fighter, rel: [0.0; 3], turned: false, cap_up: true };
                self.spawn_follow(w, "PS_EndgameLosers", f, 1.0, &[("Team", ci)]);
            }
            Event::EnlargeFx { fighter, begin } => self.enlarge_fx(w, fighter, begin),
            Event::GrowFx { fighter } => self.grow_fx(w, fighter),
            Event::ReflectFx { actor, item, owner } => self.reflect_fx(w, actor, item, owner),
            Event::Sound { fighter, ev, .. } => self.buff_sound(w, fighter, ev),
            _ => {}
        }
    }

    /// `socketOf(i, 'VFX_Root')`: the skeleton's socket or bone of that name in its spelling, None (the mesh component)
    /// where it has neither.
    fn vfx_root(&self, w: &World, id: i32) -> Option<String> {
        let fid = w.by_id(id)?;
        let skel = &w.char_data(fid).fighter.skeleton;
        let n = skel.sockets.keys().find(|k| k.eq_ignore_ascii_case("VFX_Root")).map_or("VFX_Root", |k| k.as_str());
        let name = self.skel_name(id, n);
        let has = skel.sockets.contains_key(&name) || self.rig(id).is_some_and(|(_, r)| r.has_bone(&name));
        has.then_some(name)
    }

    /// Buff_Enlarge's SpawnVFX (begin) and EndPlay: PS_ShadowClone_Impact attached to VFX_Root at
    /// (0, 0, 80), SnapToTarget: the relative scale is 1 / the socket's world scale at the spawn, Team. The sound is
    /// not posted (PF_Post_Event with a null actor returns). The HUD's enlargeText (begin) is the HUD's.
    // Deviation: the view has this tick's size when the events run (the views are placed before the events), so at
    // the begin the effect's world scale is 1; the build's BeginPlay SpawnVFX may run before
    // UpdatePlayerStatus sets the size (not read).
    fn enlarge_fx(&mut self, w: &World, id: i32, begin: bool) {
        let Some(fid) = w.by_id(id) else { return };
        let socket = self.vfx_root(w, id);
        let probe = self.socket_follow(w, id, socket.as_deref(), [0.0; 3], &IDENTITY, [1.0; 3]);
        let Some(a) = self.place(w, &probe) else { return };
        let Some(p) = self.pawns.get(&id) else { return };
        let cur = p.body.as_ref().map_or(1.0, |b| b.scale);
        let sc = a.scale.unwrap_or([1.0; 3]);
        let k = if begin {
            1.0
        } else {
            f64::from(w.fighter(fid).body.size_scale) / (cur / self.mesh_relative_scale(w, id))
        };
        let rel = [0, 1, 2].map(|j| 1.0 / (sc[j] * k));
        let team = self.team(id);
        let f = self.socket_follow(w, id, socket.as_deref(), [0.0, 0.0, 80.0], &IDENTITY, rel);
        self.attach_to_mesh(w, id, "PS_ShadowClone_Impact", "VFX_Root", f, &[("Team", team)]);
    }

    /// Mutator_GrowWithDamage MulticastSizeChange @2522: SnapToTargetIncludingScale (EAttachLocation 3)
    /// at VFX_Root, relative scale 1, so the emitter takes the socket's world scale (the grown size).
    fn grow_fx(&mut self, w: &World, id: i32) {
        let socket = self.vfx_root(w, id);
        let team = self.team(id);
        let f = self.socket_follow(w, id, socket.as_deref(), [0.0; 3], &IDENTITY, [1.0; 3]);
        self.attach_to_mesh(w, id, "PS_ShadowClone_Impact", "VFX_Root", f, &[("Team", team)]);
    }

    /// Buff_Fire's and Buff_Ice's emitters, keyed off their sound events.
    fn buff_sound(&mut self, w: &World, fighter: i32, ev: mvs_sim::names::Name) {
        self.fire_vfx(w, fighter, w.names.str(ev));
        self.ice_vfx(w, fighter, w.names.str(ev));
    }

    /// fireVfx: Buff_Fire's visuals on the burning fighter, read from the buff's sound events (SFX_Loop_Start =
    /// ReceiveBeginPlay, SFX_Ignite = InitialVisuals, SFX_Loop_Stop = the actor's end). LoopVisualsOn -> SpawnVFX:
    /// SpawnEmitterAttached(PS_Debuff_Fire_FOE, the active mesh, VFX_Root, the root's socket location - (0, 0, 200),
    /// rotation 0, scale 1, KeepWorldPosition) with Team: the component keeps that world transform relative to the
    /// socket's (a skeleton with neither socket nor bone VFX_Root: the mesh component's). InitialVisuals:
    /// SpawnEmitterAtLocation(PS_HitImpact_TEMPLATE, the actor location, rotation 0, scale 1) with Team. The actor's
    /// end destroys the loop.
    // Deviation: OnActiveMeshChanged (RemoveVFX and SpawnVFX on the new mesh) is not modelled; the socket
    // is taken from the last pose.
    fn fire_vfx(&mut self, w: &World, id: i32, ev: &str) {
        if !self.is_player_pawn(id) {
            return;
        }
        // (Buff_Fire's class: the data of a pawn's character that brings it)
        let def = w.all_ids().into_iter().find_map(|f| w.char_data(f).fighter.buffs.as_ref()?.get("Buff_Fire_C"));
        let Some(def) = def else { return };
        let is = |k: &str| def.get(k).and_then(|v| v.as_str()) == Some(ev);
        let Some(fid) = w.by_id(id) else { return };
        let team = self.team(id);
        if is("SFX_Loop_Start") {
            if let Some(h) = self.pawns.get_mut(&id).and_then(|p| p.fire.take()) {
                self.player.kill(h);
            }
            let socket = self.vfx_root(w, id);
            let f0 = self.socket_follow(w, id, socket.as_deref(), [0.0; 3], &IDENTITY, [1.0; 3]);
            let Some(a) = self.place(w, &f0) else { return };
            let sc = a.scale.unwrap_or([1.0; 3]);
            let fm = w.fighter(fid);
            let d = [-a.loc[0], -f64::from(fm.body.x) - a.loc[1], f64::from(actor_y(w, fid)) - 200.0 - a.loc[2]];
            let r = a.rot;
            let rel = [0, 1, 2].map(|k| (d[0] * r[k][0] + d[1] * r[k][1] + d[2] * r[k][2]) / sc[k]);
            let rows: Rows = [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]];
            let inv = [1.0 / sc[0], 1.0 / sc[1], 1.0 / sc[2]];
            let f = self.socket_follow(w, id, socket.as_deref(), rel, &transpose(&rows), inv);
            let h = self.attach_to_mesh(w, id, "PS_Debuff_Fire_FOE", "VFX_Root", f, &[("Team", team)]);
            if let Some(p) = self.pawns.get_mut(&id) {
                p.fire = h;
            }
        } else if is("SFX_Ignite") {
            let fm = w.fighter(fid);
            self.player.spawn(
                "PS_HitImpact_TEMPLATE",
                [0.0, f64::from(-fm.body.x), f64::from(actor_y(w, fid))],
                [0.0; 3],
                [1.0; 3],
                &[("Team", team)],
            );
        } else if is("SFX_Loop_Stop")
            && let Some(h) = self.pawns.get_mut(&id).and_then(|p| p.fire.take())
        {
            self.player.kill(h);
        }
    }

    // ---- a reflect's visuals

    /// SpawnedActorParent MulticastReflect with ShouldPause, its gate open (the sim's reflectFx event):
    /// SpawnEmitterAttached(PS_Reflect, RootComponent, no socket, location 0, rotation 0, scale 1, KeepRelativeOffset,
    /// auto destroy) @19131 with Team = the new owner's colour value @19326-19503 (team skins on). It rides the
    /// actor's root (its place of each tick, its rotation and scale as the actor's components) until the actor ends.
    /// Each pause makes one.
    // Deviation: the Team of UseTeamColors off (the actor's LocalColorIndex) is not read: the port's default UI
    // preferences have team skins on.
    fn reflect_fx(&mut self, w: &World, seq: i32, item: bool, owner: i32) {
        let a = if item {
            w.actors.items.iter().find(|(_, it)| it.seq == seq).map(|(id, _)| ActorRef::Item(id))
        } else {
            w.actors
                .projectiles
                .iter()
                .find(|(_, p)| p.seq == seq && p.owner.is_some())
                .map(|(id, _)| ActorRef::Projectile(id))
        };
        let Some(a) = a else { return };
        let f = Follow::Actor { turned: true, a, rel: [0.0; 3], rel_rot: IDENTITY, scale: [1.0; 3] };
        let team = self.team(owner);
        let h = self.spawn_follow(w, "PS_Reflect", f, 1.0, &[("Team", team)]);
        let r = self.reflects.entry(a).or_default();
        r.seen = true;
        r.hs.extend(h);
    }

    /// reflectVisuals, once a step after the projectiles' components: the PS_Reflect systems end with their actor, and
    /// the reflect pause's dilation. The pause's Delay(0.0) @1782 sets every ParticleSystemComponent of the actor to
    /// CustomTimeDilation 0.5 @2277, and 1 again behind Delay(0.375) @969: while ReflectState.pause is 2 the actor's own
    /// components (the class's ParticleSystem, its rule's `more` and `systems`; an item's thrown ParticleSystem) and
    /// its PS_Reflect tick at 0.5.
    // Unverified: PS_Reflect, made by SpawnEmitterAttached, is taken as among the actor's components.
    fn reflect_visuals(&mut self, w: &World) {
        for r in self.reflects.values_mut() {
            r.seen = false;
        }
        for (pid, pr) in w.actors.projectiles.iter() {
            if pr.owner.is_none() {
                continue;
            }
            let a = ActorRef::Projectile(pid);
            if let Some(rf) = self.reflects.get_mut(&a) {
                rf.seen = !pr.dead;
            }
            let Some(rs) = &pr.reflect else { continue };
            let mut hs = Vec::new();
            if let Some(st) = self.projectiles.get(&pr.seq) {
                hs.extend(st.h);
                hs.extend(st.more.iter().flatten().copied());
                if let Some((k, r)) = st.rule
                    && let Some(f) = r.systems
                {
                    hs.extend(f(&self.chars[k], st));
                }
            }
            self.pause_dilation(a, rs.pause == 2, hs);
        }
        for (iid, it) in w.actors.items.iter() {
            let a = ActorRef::Item(iid);
            if let Some(rf) = self.reflects.get_mut(&a) {
                rf.seen = !it.dead;
            }
            let Some(rs) = &it.reflect else { continue };
            let hs = self.item_fx.get(&iid).and_then(|r| r.h).into_iter().collect();
            self.pause_dilation(a, rs.pause == 2, hs);
        }
        let gone: Vec<ActorRef> = self.reflects.iter().filter(|(_, r)| !r.seen).map(|(a, _)| *a).collect();
        for a in gone {
            for h in self.reflects.shift_remove(&a).map(|r| r.hs).unwrap_or_default() {
                self.player.kill(h);
            }
            self.slowed.remove(&a);
        }
    }

    /// pauseDilation: an actor's systems `hs` and its PS_Reflect at 0.5 while `slow`, at 1 once at its end.
    fn pause_dilation(&mut self, a: ActorRef, slow: bool, mut hs: Vec<SystemId>) {
        if !slow && !self.slowed.remove(&a) {
            return;
        }
        if slow {
            self.slowed.insert(a);
        }
        let d = if slow { 0.5 } else { 1.0 };
        hs.extend(self.reflects.get(&a).map(|r| r.hs.clone()).unwrap_or_default());
        for h in hs {
            self.player.set_time_dilation(h, d);
        }
    }

    // ---- the items' components and the tunnel trails

    /// placeProjectiles' items, each frame after the tick: an item's thrown ParticleSystem (itemTrail) and its tunnel
    /// trail; the ended items' go.
    fn place_items(&mut self, w: &World) {
        for r in self.item_fx.values_mut() {
            r.seen = false;
        }
        for (id, it) in w.actors.items.iter() {
            let carried = it.owner.is_some() && mvs_sim::actors::tunnel_carry::carry_hidden(w, ActorRef::Item(id));
            let capsule = matches!(&it.ext, mvs_sim::actors::item::ItemExt::SafeItem(s) if s.capsule_hidden);
            self.item_trail(w, id, carried || capsule);
            self.tunnel_trail(w, ActorRef::Item(id));
        }
        let gone: Vec<ItemId> = self.item_fx.iter().filter(|(_, r)| !r.seen).map(|(k, _)| *k).collect();
        for id in gone {
            if let Some(h) = self.item_fx.shift_remove(&id).and_then(|r| r.h) {
                self.player.kill(h);
            }
        }
        self.drop_trails(w);
    }

    /// itemTrail: a thrown item's ParticleSystem component (Item_Sandwich: PS_ShaggySandwichProjectile at (48, 0, 16)
    /// on the Capsule, bVisible false): ReleaseFromFighterMulticast shows it with IsThrown, ItemThrowHitMulticast and
    /// PickUp hide it; SetTeamColor gives it Team (ItemActorParent @11539: with no owner 0, not local). A tunnel carry
    /// or a hidden root hides it.
    // Unverified: the component is started when it is shown and killed when it is hidden.
    fn item_trail(&mut self, w: &World, id: ItemId, hidden: bool) {
        let it = w.actors.items.get(id).expect("a live item");
        if !it.thrown || it.holder >= 0 {
            if let Some(h) = self.item_fx.shift_remove(&id).and_then(|r| r.h) {
                self.player.kill(h);
            }
            return;
        }
        if let Some(r) = self.item_fx.get_mut(&id) {
            r.seen = true;
            if hidden || r.hid {
                if let Some(h) = r.h {
                    self.player.set_visible(h, !hidden);
                }
                r.hid = hidden;
            }
            return;
        }
        self.item_fx.insert(id, ItemFx { h: None, hid: false, seen: true });
        let def = it.def(&w.data);
        let Some(c) = def.components.get("ParticleSystem").and_then(|c| c.as_obj()) else { return };
        let Some(template) = text(c, "Template").map(str::to_owned) else { return };
        let rel = xyz(c.get("RelativeLocation"), 0.0);
        let team = it.owner.map_or(0.0, |o| self.team(w.fighter(o).id));
        let f = Follow::Actor { turned: true, a: ActorRef::Item(id), rel, rel_rot: IDENTITY, scale: [1.0; 3] };
        let h = self.spawn_follow(w, &template, f, 1.0, &[("Team", team)]);
        if hidden && let Some(h) = h {
            self.player.set_visible(h, false);
        }
        self.item_fx.insert(id, ItemFx { h, hid: hidden, seen: true });
    }

    /// tunnelTrail: Buff_ProjectileThroughTunnel's StartMove @1469 SpawnEmitterAttached(PS_BugsBunnyTunnel_Trail, the
    /// owner's root, no socket, (0, 0, 0), (0, 0, 0), scale 1, KeepRelativeOffset), no Team; the timeline puts it at the
    /// carry's place (carryTrail) by K2_SetWorldLocation, the root's rotation and scale; EndPlay @930 destroys it.
    fn tunnel_trail(&mut self, w: &World, a: ActorRef) {
        let owned = match a {
            ActorRef::Item(id) => w.actors.items.get(id).is_some_and(|it| it.owner.is_some()),
            _ => true,
        };
        let on = owned && mvs_sim::actors::tunnel_carry::carry_trail(w, a).is_some();
        match (on, self.trails.contains_key(&a)) {
            (false, true) => {
                if let Some(Some(h)) = self.trails.shift_remove(&a) {
                    self.player.kill(h);
                }
            }
            (true, false) => {
                let h = self.spawn_follow(w, "PS_BugsBunnyTunnel_Trail", Follow::Carry { a }, 1.0, &[]);
                self.trails.insert(a, h);
            }
            _ => {}
        }
    }

    /// The trails of the actors gone from the world end with them.
    fn drop_trails(&mut self, w: &World) {
        let live = |a: &ActorRef| match *a {
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some(),
            ActorRef::Item(id) => w.actors.items.get(id).is_some(),
            _ => false,
        };
        let gone: Vec<ActorRef> = self.trails.keys().filter(|a| !live(a)).copied().collect();
        for a in gone {
            if let Some(Some(h)) = self.trails.shift_remove(&a) {
                self.player.kill(h);
            }
        }
    }

    /// iceVfx: Buff_Ice FreezeVisuals (P_Impact_Ice on the Capsule at (0, -50, 0) @212, PS_SupermanIceBreath_IceCube
    /// at (0, CapsuleHalfHeight, 0) @848, both under the actor's yaw, no Team) and EndFreezeVisuals (P_Impact_Ice
    /// once more @4547, both kept ones destroyed @4791 / @5263). Both are attached to the Capsule with relative scale
    /// 1, KeepRelativeOffset (@212, @848): the offsets and the emitters' scale are the actor's scale times theirs
    /// (CapsuleHalfHeight is the raw one, 96, a relative location).
    // Unverified: the capsule's place is the sim's of this tick.
    fn ice_vfx(&mut self, w: &World, id: i32, ev: &str) {
        const FREEZE: &str = "sfx_cha_superman_icebreath_freeze";
        const ENDS: [&str; 2] = ["sfx_cha_superman_icebreath_freeze_break", "sfx_cha_superman_icebreath_debuff"];
        if ev != FREEZE && !ENDS.contains(&ev) {
            return;
        }
        let Some(fid) = w.by_id(id) else { return };
        if !self.is_player_pawn(id) {
            return;
        }
        let cap = |rel: V3| Follow::CapsuleRel { id, rel, scale: [1.0; 3] };
        let hit = self.spawn_follow(w, "P_Impact_Ice", cap([0.0, -50.0, 0.0]), 1.0, &[]);
        let (old_hit, old_cube) = std::mem::take(&mut self.pawns[&id].ice);
        for h in [old_hit, old_cube].into_iter().flatten() {
            self.player.kill(h);
        }
        if ev == FREEZE {
            let y = body_def(w, fid).inner_cap_half;
            let cube = self.spawn_follow(w, "PS_SupermanIceBreath_IceCube", cap([0.0, y, 0.0]), 1.0, &[]);
            self.pawns[&id].ice = (hit, cube);
        }
    }

    /// FindClosestBone_K2 on the victim's mesh (DamageAndLaunch @9876): the bone location nearest to a UE location.
    // Deviation: from the latest tick's pose, as the last rendered pose.
    pub fn nearest_bone(&self, victim: i32, hit: V3) -> V3 {
        let Some(b) = self.pawns.get(&victim).and_then(|p| p.body.as_ref()) else { return hit };
        let mut best = (f64::INFINITY, hit);
        for (_, l) in &b.bones {
            let d = (l[0] - hit[0]).powi(2) + (l[1] - hit[1]).powi(2) + (l[2] - hit[2]).powi(2);
            if d < best.0 {
                best = (d, *l);
            }
        }
        best.1
    }

    /// GetSocketRotation on fighter `id`'s mesh: the world rows (UE axes, row vectors) of its socket or bone `socket`
    /// (in any case) as the latest tick posed it; None with no body or no such bone.
    pub fn socket_rows(&self, w: &World, id: i32, socket: &str) -> Option<Rows> {
        let (body, rig) = self.rig(id)?;
        let fid = w.by_id(id)?;
        let skel = &w.char_data(fid).fighter.skeleton;
        let s = SocketFollow::new(skel, &rig.name(socket), [0.0; 3], &IDENTITY, [1.0; 3]);
        s.eval(body, rig).map(|(_, rows, _)| rows)
    }

    /// HandleBlastBoxDeathFX (@1232-2545): the entry this machine picks, its transform, the camera anchor; the
    /// emitter with Team. The entry's audio events are queued for the sound's glue (`ko_picks`).
    fn ko_vfx(&mut self, w: &World, fighter: i32, asset: &str, hold: f64, k: ring_out::KoInputs) {
        let Some(entry) = self.ring_outs.emitter_data_to_play(&w.data.ring_outs, asset) else { return };
        let t = ring_out::ko_transform(entry, &k);
        self.ko_picks.push((entry.ally.clone(), entry.enemy.clone()));
        // RingOutVfxAnchor, destroyed by a timer after hold s.
        self.ko_anchors.push(KoAnchor { y: t.loc[1], z: t.loc[2], hold });
        let team = self.team(fighter);
        self.player.spawn(&entry.ps, t.loc, t.rot, t.scale, &[("Team", team)]);
    }

    /// Comp_Pawn_DamageManager DI_Visual_Multicast (@6526-8346).
    // Unverified: online, the attacker is the MostRecentAttacker fighter.
    fn di_visual(&mut self, fighter: i32, attacker: i32, vx: f64, vy: f64, x: f64, y: f64) {
        if !self.pawns.contains_key(&fighter) {
            return;
        }
        if self.online && fighter != self.me as i32 && attacker != self.me as i32 {
            return;
        }
        if !self.show_di {
            return;
        }
        if let Some(h) = self.pawns[&fighter].di.0 {
            self.end_di(fighter, h);
        }
        if vx.hypot(vy) < 2000.0 {
            return;
        }
        let g = -(mvs_sim::consts::GRAVITY_SCALE * 9.8 * 150.0);
        let ci = f64::from(self.color_index(fighter));
        let params = [
            ("Velocity.X", 0.0),
            ("Velocity.Y", -vx),
            ("Velocity.Z", vy),
            ("Acceleration.X", 0.0),
            ("Acceleration.Y", 0.0),
            ("Acceleration.Z", f64::from(g)),
            ("Team", ci),
            ("Lifetime", 1.0),
        ];
        let h = self.player.spawn("PS_PlayerRibbon_DI", [0.0, -x, y], [0.0; 3], [1.0; 3], &params);
        if let Some(p) = self.pawns.get_mut(&fighter) {
            p.di = (h, f64::from(latent_frames(0.3)));
        }
    }

    // Deviation: the deactivated ribbon keeps one invisible guide particle; it is killed once the trail is empty.
    fn end_di(&mut self, fighter: i32, h: SystemId) {
        self.player.deactivate(h);
        self.di_ended.push(h);
        if let Some(p) = self.pawns.get_mut(&fighter) {
            p.di = (None, 0.0);
        }
    }

    fn tick_di_visuals(&mut self) {
        let mut ids = std::mem::take(&mut self.pawn_ids);
        ids.clear();
        ids.extend(self.pawns.keys().copied());
        for &id in &ids {
            let p = self.pawns.get_mut(&id).expect("a pawn");
            if let Some(h) = p.di.0 {
                p.di.1 -= 1.0;
                if p.di.1 <= 0.0 {
                    self.end_di(id, h);
                }
            }
        }
        self.pawn_ids = ids;
        let player = &mut self.player;
        self.di_ended.retain(|&h| {
            if player.alive(h) && player.particles(h, DI_TRAIL_EMITTER) > 0 {
                return true;
            }
            player.kill(h);
            false
        });
    }

    /// BaseFighter movement FX.
    fn move_fx(&mut self, w: &World, id: i32, kind: MoveFxKind, x: f64, y: f64) {
        let Some(fid) = w.by_id(id) else { return };
        let ch = cap_half(w, fid);
        let fm = w.fighter(fid);
        let Some(body) = self.pawns.get(&id).and_then(|p| p.body.as_ref().map(Body::place)) else { return };
        let world_z = body.loc[2] - f64::from(fm.body.y);
        match kind {
            // SFX_Jump: PS_DustUp at the capsule bottom.
            MoveFxKind::Jump => {
                self.player.spawn_before_manager("PS_DustUp", [0.0, -x, y - f64::from(ch)], [0.0; 3], [1.0; 3], &[]);
            }
            // SFX_Jump_Double: PS_Dust_DoubleJump at the mesh location.
            MoveFxKind::DoubleJump => {
                self.player.spawn_before_manager("PS_Dust_DoubleJump", [0.0, -x, y + world_z], [0.0; 3], [1.0; 3], &[]);
            }
            // VFX_SFX_Land: PS_JumpLand at the capsule bottom, scale 1.7.
            MoveFxKind::Land => {
                self.player.spawn("PS_JumpLand", [0.0, -x, y - f64::from(ch)], [0.0; 3], [1.7; 3], &[]);
            }
            // SFX_FastFall: on the mesh at (0, 0, 125), scale 1.5 x the mesh's.
            MoveFxKind::FastFall => {
                let f = Follow::Mesh { id, rel: [0.0, 0.0, 125.0], rows: None, scale: Scale::Own };
                if let Some(a) = self.place(w, &f)
                    && let Some(h) = self.player.spawn_attached("PS_FastFall", &a, 1.5 * body.scale, &[])
                {
                    self.followers.push((h, f));
                    self.pawns[&id].mesh_psc.push(("PS_FastFall".into(), "None".into(), h));
                }
            }
            // SFX_Wall_Enter: PS_WallTrail on the leading foot, PS_DustWallGrab on the mesh at (0, 0, CapsuleHalfHeight)
            // pitch 180, unless the earlier ones live.
            MoveFxKind::WallOn => {
                let wall = self.pawns[&id].wall;
                if wall[0].is_some_and(|h| self.player.alive(h)) {
                    return;
                }
                let sock = if facing(w, fid) < 0.0 { "L_Foot_end" } else { "R_Foot_end" };
                let h0 = self.attach_socket(w, id, "PS_WallTrail", sock, [0.0; 3], &IDENTITY, [1.0; 3], &[]);
                let mut h1 = wall[1];
                if !wall[1].is_some_and(|h| self.player.alive(h)) {
                    let rows = rotation_rows(PI, 0.0, 0.0);
                    let f = Follow::Mesh { id, rel: [0.0, 0.0, f64::from(ch)], rows: Some(rows), scale: Scale::Own };
                    h1 = self.attach_to_mesh(w, id, "PS_DustWallGrab", "None", f, &[]);
                }
                self.pawns[&id].wall = [h0, h1];
            }
            // SFX_Wall_Exit: DestroyComponent on both.
            MoveFxKind::WallOff => {
                for h in std::mem::take(&mut self.pawns[&id].wall).into_iter().flatten() {
                    self.player.kill(h);
                }
            }
        }
    }

    fn end_notify(&mut self, id: i32, n: NotifyRef) {
        let Some(p) = self.pawns.get_mut(&id) else { return };
        if let Some(k) = p.notify.iter().position(|(m, _)| *m == n)
            && let (_, Some(h)) = p.notify.remove(k)
        {
            self.player.kill(h);
        }
    }

    /// AnimNotify_PlayVFX -> BaseFighter AddVFX: SpawnEmitterAttached(PS, Mesh, AttachPoint, Location, Rotation,
    /// Scale, KeepRelativeOffset), in VFXInstances by notify; NotifyEnd with KillOnNotifyEnd destroys it.
    fn notify_vfx(&mut self, w: &World, id: i32, n: NotifyRef, on: bool, left: bool) {
        if !on {
            return self.end_notify(id, n);
        }
        let Some(fid) = w.by_id(id) else { return };
        let Some(note) = notify_of(w, fid, n) else { return };
        let p = &note.p;
        let skin = self.pawns.get(&id).map_or("", |p| p.skin.as_str());
        let Some(ps) = play_vfx_particle(p, skin, self.color_index(id)) else { return };
        let (socket, loc, rot, scale) = mirror_play_vfx(p, left);
        let socket = socket.filter(|s| s != "None" && !s.is_empty());
        let Some(socket) = socket else {
            // BPI_VFXScaling (@119399-119839): at the mesh, not attached, no Team.
            let Some(b) = self.pawns.get(&id).and_then(|p| p.body.as_ref().map(Body::place)) else { return };
            let mr = mesh_rows(b.left);
            let d = mul_row(loc, &mr);
            let at = [0, 1, 2].map(|k| b.loc[k] + d[k]);
            let a = att(at, &compose_rows(&mr, &rot), Some(scale.map(|s| s * b.scale)));
            self.player.spawn_attached(&ps, &a, 1.0, &[]);
            return;
        };
        // UseOverrideAttachParent (@751..960): the owner's first SkeletalMeshComponent tagged OverrideAttachParentTag,
        // else the notify's mesh. A tagged sub-mesh is skinned to the view's one skeleton by bone name, so the socket
        // is the same bone's whichever component is named; the component only decides the bone facing left: the
        // notify's mesh is CharacterMesh0 (main), a tagged sub-mesh of the skin a pose copy (SocketFollow::copy). A
        // tag that is no sub-mesh of the skin falls back to the notify's mesh, as the graph does without such a
        // component. (C017's 44 untagged uses are main, his 30 on BodyMesh and 2 on WeaponMeshTop copies.)
        let tag = p.get("OverrideAttachParentTag").and_then(|v| v.as_str()).filter(|t| !t.is_empty() && *t != "None");
        let copy = match tag.filter(|_| flag(p, "UseOverrideAttachParent")) {
            Some(t) => {
                let has = self.pawns.get(&id).is_some_and(|p| p.parts.iter().any(|k| k == t));
                if !has {
                    attach::warn_once(format!(
                        "notify VFX: no component tagged {t} ({})",
                        w.names.str(w.fighter(fid).ch)
                    ));
                }
                has
            }
            None => false,
        };
        let team = if flag(p, "UseTeamColorParam?") {
            let local = p.get("UseLocalOnlineColoring").and_then(|v| v.as_bool()).unwrap_or(true);
            Some(if local { self.team(id) } else { f64::from(self.color_index(id)) })
        } else {
            None
        };
        let team = team.map(|t| [("Team", t)]);
        let params = team.as_ref().map_or(&[][..], |t| &t[..]);
        let f = self.socket_follow(w, id, Some(&socket), loc, &rot, scale);
        let f = if copy { f.copy() } else { f };
        let h = self.attach_to_mesh(w, id, &ps, &socket, f, params);
        if let Some(p) = self.pawns.get_mut(&id) {
            p.notify.retain(|(k, _)| *k != n);
            p.notify.push((n, h));
        }
        self.hide_if_hidden(w, id, h);
    }

    /// Comp_HitPause StartPause -> HandleVFXTimeDilation: CustomTimeDilation 0.005 on VFXInstances, back to 1 at the end.
    // Deviation: switched on the sim's hitpause after the step, not inside the hit's own tick.
    fn dilate_notify_fx(&mut self, w: &World) {
        for (id, p) in self.pawns.iter_mut() {
            let Some(fid) = w.by_id(*id) else { continue };
            let on = w.fighter(fid).hit.hitpause > 0.0;
            if on == p.notify_paused {
                continue;
            }
            p.notify_paused = on;
            for h in p.notify.iter().filter_map(|(_, h)| *h) {
                self.player.set_time_dilation(h, if on { 0.005 } else { 1.0 });
            }
        }
    }

    /// The engine's PlayParticleEffect / TimedParticleEffect and the game's PlaySingleShotVFX / PlayTimedVFX notifies.
    fn particle_notify(&mut self, w: &World, id: i32, n: NotifyRef, on: bool, left: bool) {
        let Some(fid) = w.by_id(id) else { return };
        let Some(note) = notify_of(w, fid, n) else { return };
        let p = &note.p;
        let ps = text(p, "PSTemplate").map(str::to_string);
        let game = note.cls == "AnimNotify_PlaySingleShotVFX" || note.cls == "AnimNotifyState_PlayTimedVFX";
        let timed = note.cls == "AnimNotifyState_TimedParticleEffect" || note.cls == "AnimNotifyState_PlayTimedVFX";
        let name = text(p, "SocketName").unwrap_or("None").to_string();
        if !on {
            // NotifyEnd: the first child PSC with that socket (or the swapped one) and template, not deactivated.
            let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
            let swapped = attach::swap_lr(&name);
            let Some(pf) = self.pawns.get(&id) else { return };
            let c = pf.mesh_psc.iter().find(|(s, k, h)| {
                (same(k, &name) || (game && same(k, &swapped)))
                    && Some(s.as_str()) == ps.as_deref()
                    && self.player.alive(*h)
                    && !self.player.deactivated(*h)
            });
            if let Some(&(_, _, h)) = c {
                if flag(p, "bDestroyAtEnd") {
                    self.player.kill(h);
                } else {
                    self.player.deactivate(h);
                }
            }
            return;
        }
        let Some(ps) = ps else { return };
        if !timed && self.player.immortal(&ps) {
            return;
        }
        if timed {
            let sk = self.skel_name(id, &name);
            let skel = &w.char_data(fid).fighter.skeleton;
            if !skel.sockets.contains_key(&sk) && !self.rig(id).is_some_and(|(_, r)| r.has_bone(&sk)) {
                return;
            }
        }
        let socket = if game && left { attach::swap_lr(&name) } else { name };
        let scale = if timed { [1.0; 3] } else { xyz(p.get("Scale"), 1.0) };
        let (loc, rot) = (xyz(p.get("LocationOffset"), 0.0), rot_rows(p.get("RotationOffset")));
        if timed || p.get("Attached").and_then(|v| v.as_bool()).unwrap_or(true) {
            let f = self.socket_follow(w, id, Some(&socket), loc, &rot, scale);
            self.attach_to_mesh(w, id, &ps, &socket, f, &[]);
            return;
        }
        let f = self.socket_follow(w, id, Some(&socket), loc, &rot, [1.0; 3]);
        let Some(a) = self.place(w, &f) else { return };
        let s = a.scale.unwrap_or([1.0; 3]);
        let sign = |v: f64| {
            if v < 0.0 {
                -1.0
            } else if v > 0.0 {
                1.0
            } else {
                0.0
            }
        };
        let fixed = Attachment { scale: Some([0, 1, 2].map(|k| sign(s[k]) * scale[k])), ..a };
        self.player.spawn_attached(&ps, &fixed, 1.0, &[]);
    }

    /// Comp_Pawn_Rage EnableVFX: the eyes (KeepWorldPosition, EyeVFXScale) and the aura on fml_un_C_spine_a.
    fn rage_vfx(&mut self, w: &World, id: i32, on: bool) {
        let Some(fid) = w.by_id(id) else { return };
        for h in std::mem::take(&mut self.pawns[&id].rage) {
            self.player.kill(h);
        }
        if !on {
            return;
        }
        let team = self.team(id);
        let comps = &w.char_data(fid).fighter.components;
        let key = "Comp_Pawn_Rage_GEN_VARIABLE";
        let rage_p = comps.pawn.get(key).or_else(|| comps.base.get(key)).and_then(|c| c.p.as_obj());
        let eyes = rage_p.and_then(|r| text(r, "RedEyes")).map(str::to_string);
        let aura = rage_p.and_then(|r| text(r, "RedAura")).map(str::to_string);
        let es = xyz(rage_p.and_then(|r| r.get("EyeVFXScale")), 1.0);
        let mut made = Vec::new();
        if let Some(eyes) = eyes {
            for n in ["L_Eye", "R_Eye"] {
                let socket = self.skel_name(id, n);
                let probe = self.socket_follow(w, id, Some(&socket), [0.0; 3], &IDENTITY, [1.0; 3]);
                let Some(a) = self.place(w, &probe) else { continue };
                let sc = a.scale.unwrap_or([1.0; 3]);
                let r = a.rot;
                let rows = [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]];
                let rel_scale = [es[0] / sc[0], es[1] / sc[1], es[2] / sc[2]];
                let params = [("Team", team)];
                made.extend(self.attach_socket(w, id, &eyes, &socket, [0.0; 3], &transpose(&rows), rel_scale, &params));
            }
        }
        if let Some(aura) = aura {
            let s = "fml_un_C_spine_a";
            made.extend(self.attach_socket(w, id, &aura, s, [0.0; 3], &IDENTITY, [1.5, 1.0, 2.25], &[("Team", team)]));
        }
        self.pawns[&id].rage = made;
    }

    /// Create Rage Residual Multicast: PS_BruteRageAura_Hold on Spine3, SetRelativeScale3D each tick.
    fn rage_residual(&mut self, w: &World, id: i32, scale: Option<f64>) {
        let Some(p) = self.pawns.get_mut(&id) else { return };
        let Some(scale) = scale else {
            if let Some(h) = p.residual.0.take() {
                self.player.kill(h);
            }
            return;
        };
        p.residual.1 = scale;
        if p.residual.0.is_some_and(|h| self.player.alive(h)) {
            return;
        }
        let mut f = self.socket_follow(w, id, Some("Spine3"), [0.0; 3], &IDENTITY, [1.0; 3]);
        if let Follow::Socket { scale, .. } = &mut f {
            *scale = Scale::Residual;
        }
        let team = self.team(id);
        let h = self.attach_to_mesh(w, id, "PS_BruteRageAura_Hold", "Spine3", f, &[("Team", team)]);
        self.pawns[&id].residual.0 = h;
    }

    /// Buff_HealOverTime FireVFXOn: PS_Heal on Root at scale 1.4, Team; every Delay(1.1) Deactivate and Activate(reset).
    // Unverified: the reset is drawn as a new component; the scale is taken as the world scale.
    fn place_heal(&mut self, w: &World) {
        let heal_loop = latent_frames(1.1) as i32;
        let mut ids = std::mem::take(&mut self.pawn_ids);
        ids.clear();
        ids.extend(self.pawns.iter().filter(|(_, p)| p.minion.is_none()).map(|(id, _)| *id));
        for &id in &ids {
            let Some(fid) = w.by_id(id) else { continue };
            let born = heal_vfx_born(w, fid);
            let tick = w.fighter(fid).clock.tick;
            let (b, h, at) = self.pawns[&id].heal;
            if f64::from(born) == b && !(born >= 0.0 && tick - at >= heal_loop) {
                continue;
            }
            if let Some(h) = h {
                self.player.kill(h);
            }
            self.pawns[&id].heal = (f64::from(born), None, tick);
            if born < 0.0 {
                continue;
            }
            let mut f = self.socket_follow(w, id, Some("Root"), [0.0; 3], &IDENTITY, [1.0; 3]);
            if let Follow::Socket { scale, .. } = &mut f {
                *scale = Scale::Fixed([1.4; 3]);
            }
            let team = self.team(id);
            let h = self.attach_to_mesh(w, id, "PS_Heal", "Root", f, &[("Team", team)]);
            self.pawns[&id].heal.1 = h;
        }
        self.pawn_ids = ids;
    }

    /// HitSmokeTrail snapped to the capsule centre.
    fn place_smoke_trails(&mut self, w: &World) {
        for (id, p) in &self.pawns {
            let (Some(h), Some(fid)) = (p.smoke, w.by_id(*id)) else { continue };
            let fm = w.fighter(fid);
            self.player.set_location(h, [0.0, f64::from(-fm.body.x), f64::from(fm.body.y)]);
        }
    }

    /// An owned projectile's ParticleSystem component (under its attach
    /// parents), with Team (ownedViews: a projectile with no owner has no view), and its class rule's hooks: made
    /// once after the component, tick and after each tick, end with the actor.
    fn place_projectiles(&mut self, w: &World) {
        for p in self.projectiles.values_mut() {
            p.seen = false;
        }
        for (pid, pr) in w.actors.projectiles.iter() {
            if pr.dead || !mvs_sim::actors::projectile::is_owned_projectile(w, pid) {
                continue;
            }
            let own = ActorRef::Projectile(pid);
            let mut st = match self.projectiles.shift_remove(&pr.seq) {
                Some(st) => st,
                None => {
                    let rule = chars::proj_rule(w.names.str(pr.cls));
                    let cs = actor_def(w, own, pr.cls).map(|d| &d.components);
                    let cn = match cs {
                        Some(cs) if cs.contains_key("ParticleSystem") => "ParticleSystem",
                        Some(cs) if cs.contains_key("PS_Explosion_SatchelAura") => "PS_Explosion_SatchelAura",
                        _ => "PS_TetherProjectile",
                    };
                    let r = rule.map(|r| r.1);
                    let h = cs.and_then(|cs| self.projectile_component(w, pr, cs, cn, r));
                    let mut st = ProjFxState::new(pr.seq, pid, h, cn, f64::from(pr.x), f64::from(pr.y), rule);
                    if let (Some(r), Some(cs)) = (r, cs) {
                        st.more = r.more.iter().map(|n| self.projectile_component(w, pr, cs, n, Some(r))).collect();
                    }
                    if let Some((k, r)) = rule {
                        self.rule_hook(w, k, r.made, &mut st);
                    }
                    st
                }
            };
            st.seen = true;
            st.id = pid;
            if let (Some((_, r)), Some(h)) = (st.rule, st.h)
                && let Some(shown) = r.shown
            {
                self.player.set_visible(h, shown(w, pr));
            }
            st.x = f64::from(pr.x);
            st.y = f64::from(pr.y);
            if let Some((k, r)) = st.rule {
                self.rule_hook(w, k, r.tick, &mut st);
            }
            self.buff_components(w, pr, &mut st);
            if let Some((k, r)) = st.rule {
                self.rule_hook(w, k, r.after, &mut st);
            }
            // carryFx: Buff_ProjectileThroughTunnel's BeginPlay @165 hides every visible child of the root until EndPlay
            // @863 shows them (the class's particle components here, the mesh view in the poser); the trail
            let hidden = mvs_sim::actors::tunnel_carry::carry_hidden(w, own);
            if hidden || st.hid {
                let on = !hidden && st.rule.and_then(|(_, r)| r.shown).is_none_or(|f| f(w, pr));
                for h in st.h.into_iter().chain(st.more.iter().flatten().copied()) {
                    self.player.set_visible(h, on);
                }
                st.hid = hidden;
            }
            self.projectiles.insert(pr.seq, st);
            self.tunnel_trail(w, own);
        }
        let gone: Vec<i32> = self.projectiles.iter().filter(|(_, p)| !p.seen).map(|(k, _)| *k).collect();
        for seq in gone {
            if let Some(mut st) = self.projectiles.shift_remove(&seq) {
                self.drop_projectile(w, &mut st);
            }
        }
        self.reflect_visuals(w);
        self.drop_trails(w);
    }

    /// projectileFx's buff components: Buff_SpeedUpProjectile (a Buff_SlowProjectile, from ProjectileAccelerator): its
    /// VFX component PS_Garnet_ProjectileSlowTemp with Team = SelectFloat(0, the instigator's colour index,
    /// IsLocalOnlinePlayer), attached to the projectile's root (snap to target, the relative rotation and scale kept);
    /// Buff_SlowProjectile and its child Buff_IceSlowProjectile (ubergraph @1205-1572): the same, the child's template
    /// PS_SupermanIceBreath_IceCube at a relative scale of 0.3. The buff's EndPlay destroys it (@10).
    // Unverified: the component's ticks before the buff's BeginPlay are not counted.
    fn buff_components(&mut self, w: &World, pr: &mvs_sim::actors::projectile::Projectile, st: &mut ProjFxState) {
        let own = ActorRef::Projectile(st.id);
        let c = self.colours.get(pr.owner_id as usize);
        let t = c.map_or(0.0, |c| if c.team == 0.6 { 0.0 } else { f64::from(c.color_index) });
        if pr.sped_up && st.speed.is_none() {
            let f = Follow::Actor { turned: true, a: own, rel: [0.0; 3], rel_rot: IDENTITY, scale: [1.0; 3] };
            st.speed = self.spawn_follow(w, "PS_Garnet_ProjectileSlowTemp", f, 1.0, &[("Team", t)]);
        }
        let slow = pr.elem.as_ref().and_then(|_| mvs_sim::combat::elem::slow_of(w, own));
        if slow != st.slow_cls {
            if let Some(h) = st.slow.take() {
                self.player.kill(h);
            }
            st.slow_cls = slow;
            if let Some((template, k)) = slow.and_then(slow_vfx) {
                let f = Follow::Actor { turned: true, a: own, rel: [0.0; 3], rel_rot: IDENTITY, scale: [k; 3] };
                st.slow = self.spawn_follow(w, template, f, 1.0, &[("Team", t)]);
            }
        }
    }

    /// `remake`: ParticleSystem.Activate(true) on the record's first component: killed and made anew, with the owner's
    /// Team of now.
    #[allow(dead_code, reason = "the character modules call it (a ProjRule hook re-activating its component)")]
    pub(crate) fn remake(&mut self, w: &World, st: &mut ProjFxState) {
        if let Some(h) = st.h.take() {
            self.player.kill(h);
        }
        let Some(pr) = w.actors.projectiles.get(st.id) else { return };
        let Some(cs) = actor_def(w, ActorRef::Projectile(st.id), pr.cls).map(|d| &d.components) else { return };
        st.h = self.projectile_component(w, pr, cs, st.cn, st.rule.map(|r| r.1));
    }

    /// An ended actor's components and its rule's end.
    fn drop_projectile(&mut self, w: &World, st: &mut ProjFxState) {
        for h in st.h.into_iter().chain(st.more.iter().flatten().copied()) {
            self.player.kill(h);
        }
        if let Some((k, r)) = st.rule {
            self.rule_hook(w, k, r.end, st);
        }
        for h in st.speed.take().into_iter().chain(st.slow.take()) {
            self.player.kill(h);
        }
    }

    /// A class's particle component `cn`: its relative transform under the attach parents (FTransform composition, sim
    /// actors/projectile.rs attach_parent), the actor's rotation and scale on top; what the class's rule does to it on
    /// top of the template (ProjRule: rel_loc, world_scale, parent, scale, no_team).
    fn projectile_component(
        &mut self,
        w: &World,
        pr: &mvs_sim::actors::projectile::Projectile,
        cs: &mvs_data::value::Props,
        cn: &str,
        rule: Option<&'static ProjRule>,
    ) -> Option<SystemId> {
        let c = cs.get(cn)?.as_obj()?;
        let template = text(c, "Template")?.to_string();
        let par0 = mvs_sim::actors::projectile::attach_parent(cs, cn);
        let own_loc =
            rule.and_then(|r| r.rel_loc).and_then(|f| f(w, pr)).unwrap_or_else(|| xyz(c.get("RelativeLocation"), 0.0));
        let own_rot = rot_rows(c.get("RelativeRotation"));
        let crs = xyz(c.get("RelativeScale3D"), 1.0);
        let ws = rule.and_then(|r| r.world_scale);
        let team = [("Team", self.team(pr.owner_id))];
        let params: &[(&str, f64)] = if rule.is_some_and(|r| r.no_team) { &[] } else { &team };
        let f = match rule {
            Some(r) if r.parent.is_some() || r.scale.is_some() => {
                let c = LiveComp { cn: cn.to_owned(), rule: r, own_loc, own_rot, crs, par0 };
                Follow::Live { seq: pr.seq, c: Box::new(c) }
            }
            _ => {
                let (rel, rs, _, rel_rot) = under(&par0, own_loc, &own_rot, crs, ws, par0.scale.map(f64::from));
                Follow::Projectile { seq: pr.seq, rel, rel_rot, scale: rs }
            }
        };
        self.spawn_follow(w, &template, f, 1.0, params)
    }
}

impl Fx {
    /// An event that is no sound and whose fighter, victim, owner or juggler is
    /// an id with no pawn record (offRecord: a minion's, or a gone pawn's).
    fn names_minion(&self, w: &World, e: &Event) -> bool {
        event_ids(e).into_iter().flatten().any(|id| {
            id >= 0
                && match w.by_id(id) {
                    Some(f) => w.fighter(f).life.is_minion,
                    None => !self.is_player_pawn(id),
                }
        })
    }
}

/// An event's victim, fighter, owner and juggler ids (in the slot order); none for a sound
/// (sound, soundStart, soundStop, rtpc, switch) and an event naming no pawn.
pub(super) fn event_ids(e: &Event) -> [Option<i32>; 4] {
    match e {
        Event::ComboCounts { fighter, juggler, .. } => [None, Some(*fighter), None, Some(*juggler)],
        Event::CombatInput { fighter, .. }
        | Event::MarkerBurst { fighter, .. }
        | Event::ItemFx { fighter, .. }
        | Event::MortarBoom { fighter, .. }
        | Event::AcceleratorBuff { fighter, .. }
        | Event::AcceleratorEnd { fighter, .. }
        | Event::ProjectileDeath { fighter, .. }
        | Event::ShockwaveVfx { fighter, .. }
        | Event::Ko { fighter, .. }
        | Event::ArmorHit { fighter, .. }
        | Event::ArmorBreak { fighter, .. }
        | Event::Respawn { fighter, .. }
        | Event::SlotBound { fighter, .. }
        | Event::PriorityAdd { fighter, .. }
        | Event::Rumble { fighter, .. }
        | Event::RumbleStop { fighter, .. }
        | Event::KoVfx { fighter, .. }
        | Event::BlastBoxSound { fighter, .. }
        | Event::SmokeTrail { fighter, .. }
        | Event::DiVisual { fighter, .. }
        | Event::RageVfx { fighter, .. }
        | Event::SpawnFx { fighter, .. }
        | Event::RageGlow { fighter, .. }
        | Event::SwordGlow { fighter, .. }
        | Event::MatScalar { fighter, .. }
        | Event::EvadeFx { fighter, .. }
        | Event::PopText { fighter, .. }
        | Event::ItemUiCreate { fighter, .. }
        | Event::ItemUi { fighter, .. }
        | Event::RageResidual { fighter, .. }
        | Event::SizeScale { fighter, .. }
        | Event::EnlargeFx { fighter, .. }
        | Event::GrowFx { fighter, .. }
        | Event::PolymorphFx { fighter, .. }
        | Event::ElectricFx { fighter, .. }
        | Event::TazHealFx { fighter, .. }
        | Event::TjPoof { fighter, .. }
        | Event::Stocks { fighter, .. }
        | Event::Abduct { fighter, .. }
        | Event::IntroFx { fighter, .. }
        | Event::IntroReveal { fighter, .. }
        | Event::UiState { fighter, .. }
        | Event::EatUi { fighter, .. }
        | Event::StruggleUi { fighter, .. }
        | Event::UiAbility { fighter, .. }
        | Event::AbilityAmmo { fighter, .. }
        | Event::MoveFx { fighter, .. }
        | Event::NotifyVfx { fighter, .. }
        | Event::EmoteVfx { fighter, .. }
        | Event::ActorVfx { fighter, .. }
        | Event::ParticleNotify { fighter, .. }
        | Event::SubMeshAnim { fighter, .. }
        | Event::PulseHit { fighter, .. }
        | Event::AbilityUiCreate { fighter, .. }
        | Event::DisguiseFx { fighter, .. } => [None, Some(*fighter), None, None],
        Event::Hit { victim, .. } | Event::HitVfx { victim, .. } => [Some(*victim), None, None, None],
        Event::Backstab { fighter, victim, .. }
        | Event::AryaKnife { fighter, victim, .. }
        | Event::GauntletHit { fighter, victim, .. } => [Some(*victim), Some(*fighter), None, None],
        Event::ReflectFx { owner, .. } => [None, None, Some(*owner), None],
        Event::ItemUiExit { fighter, .. } => [None, *fighter, None, None],
        Event::CloudLabel { fighter, owner, .. } => [None, Some(*fighter), Some(*owner), None],
        _ => [None; 4],
    }
}

/// A copy or Polymorph mesh's notify no module drew (a warning at its begin).
fn warn_undrawn(on: bool, what: &str) {
    if on {
        eprintln!("{what}: no module draws it");
    }
}

#[cfg(test)]
mod tests;

/// The world scale of fighter `fid`'s root capsule: the actor scale (XY, XY, Z), else the size scale.
pub(crate) fn capsule_scale(w: &World, fid: FighterId) -> V3 {
    let b = &w.fighter(fid).body;
    let (xy, z) = b.actor_scale.map_or((b.size_scale, b.size_scale), |a| (a[0], a[1]));
    [f64::from(xy), f64::from(xy), f64::from(z)]
}

/// SpawnEmitterAttached's SnapToTarget relative scale on fighter `fid`'s capsule: Scale over the capsule's world
/// scale at the spawn, component-wise, 0 for a near-zero component (exe 142a948b0); fixed from then on.
pub(crate) fn snap_scale(w: &World, fid: FighterId, scale: f64) -> V3 {
    capsule_scale(w, fid).map(|p| if p.abs() <= SNAP_SMALL { 0.0 } else { scale / p })
}

/// The near-zero threshold of SnapToTarget's divide (DAT_143b46c24, UE's SMALL_NUMBER).
// Unverified: the constant's value is read as UE's SMALL_NUMBER (1e-8), not from the exe's data.
const SNAP_SMALL: f64 = 1e-8;
