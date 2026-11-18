//! Jake's effects: the WeaponMaster perk's hand emitters, the horse's
//! ScoobySnackBuff systems, and his spline meshes as component meshes (crate::cmesh): the stretch
//! body and the arc aimers' AimSpline. His actors' other meshes are mvs-view's
//! (chars/jake.rs).

use std::collections::BTreeSet;
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::jake::stretch::Stretch;
use mvs_sim::chars::jake::weapons::{WEAPON_MASTER, WEAPON_MASTER_ALLY, weapon_master_active};
use mvs_sim::combat::buffs::buff_of;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, rotation_rows};
use mvs_view::WarmMesh;
use mvs_view::meshes::{AlphaRule, SplineMesh, SplineSlice};
use mvs_view::skins::{Variant, fighter_colours, skin_of};

use super::shaggy::has_snack;
use super::{CharFxHooks, ProjRule};
use crate::cmesh::{CMeshGeometry, CMeshId, SplineSource, spline_geometry, spline_group_geometry};
use crate::fx::Fx;
use crate::fx::attach::{att, mesh_rows, warn_once, xyz};

/// What Jake's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Jake {
    /// By the fighter's id: the WeaponMaster pair (the buff's serial it belongs to and its two hands).
    own: IndexMap<i32, (u32, Vec<SystemId>)>,
    /// By the fighter's id: the WeaponMasterAlly pair.
    ally: IndexMap<i32, (u32, Vec<SystemId>)>,
    /// By the horse actor's sequence number.
    horses: IndexMap<i32, HorseSnack>,
    /// By the actor's sequence number: the stretch bodies and the aim arcs (None: not drawn).
    bodies: IndexMap<i32, Option<StretchBody>>,
    arcs: IndexMap<i32, Option<AimArc>>,
    warned: BTreeSet<String>,
}

impl CharFxHooks for Jake {
    /// jakeActorWarm's spline meshes: per Jake, the stretch body's mesh in his skin's
    /// pick in each variant and the aim arc's, their sources read and their materials as component meshes.
    fn warm(&mut self, fx: &mut Fx, w: &World, out: &mut Vec<WarmMesh>) {
        for f in w.all_ids().into_iter().filter(|&f| w.names.str(w.fighter(f).ch) == mvs_sim::chars::jake::KEY) {
            let Some(Look { key, skin, default, .. }) = look(fx, w, f, f) else { continue };
            let Some(d) = fx.draws(&key) else { continue };
            let body = if default { Some(STRETCH_MESH.to_owned()) } else { d.actor_key(&format!("{skin}.Stretch")) };
            let mut sets: Vec<(String, Option<String>)> = Vec::new();
            for v in [None, Some(Variant::Red.key()), Some(Variant::Blue.key())] {
                if let Some((path, mats)) = body.as_deref().and_then(|b| d.actor(b, v)) {
                    sets.push((path, mats.first().cloned().flatten()));
                }
            }
            if let Some((path, mats)) = d.actor(AIM_MESH, None) {
                sets.push((path, mats.first().cloned().flatten()));
            }
            for (path, slot) in sets {
                let Some(slot) = slot else { continue };
                if source(fx, &path, "the warm-up").is_some() {
                    let materials = vec![Some(Arc::from(slot.as_str()))];
                    out.push(WarmMesh { mesh: None, materials, ..WarmMesh::default() });
                }
            }
        }
    }

    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_horses(fx, w);
        self.place_splines(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_master(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];

impl Jake {
    fn clear(&mut self, fx: &mut Fx) {
        for (_, (_, hs)) in self.own.drain(..).chain(self.ally.drain(..)) {
            for h in hs {
                fx.player.kill(h);
            }
        }
        for (_, h) in self.horses.drain(..) {
            for s in h.snack.into_iter().flatten().flatten() {
                fx.player.kill(s);
            }
        }
        fx.snack_destroy.clear();
        for b in self.bodies.drain(..).filter_map(|b| b.1) {
            for id in b.ids {
                fx.components.remove(id);
            }
        }
        for a in self.arcs.drain(..).filter_map(|a| a.1) {
            fx.components.remove(a.id);
        }
    }

    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str, what: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: {what}: {ps} is not loaded");
        }
        false
    }
}

// ---- the WeaponMaster perk ----
//
// Buff_Jake_WeaponMaster on Jake, Buff_Jake_WeaponMasterAlly on a team mate (mvs-sim chars/jake/weapons.rs), drawn
// over every fighter of the match.
// Buff_Jake_WeaponMaster BeginPlay @3560 (not on a dedicated server): SpawnEmitterAttached(PS_Jake_WeaponMaster, the
//   owner's Mesh, R_Hand @3729 and L_Hand @3949, location 0, rot(0, 0, 0), scale (2, 2, 2), KeepRelativeOffset, auto
//   destroy), both kept in VFX and SetVisibility(false). SetVFXVisibility(v) @223-324: each SetVisibility(v, false);
//   true at ActivateBuff @3065, false at DeactivateBuff @3544. No Team is set, and the two are not added to the
//   visibility tracking: an invisible Jake (the house, the horse, the stretch) keeps them as they are.
// Buff_Jake_WeaponMasterAlly BeginPlay @15: the same pair at scale (2.5, 2.5, 2.5) @457 / @643, each added to the
//   ally's Comp_FighterVisibilityManager (AddVFX @1214); ReceiveEndPlay @1349: RemoveVFX and K2_DestroyComponent each.
// The sim has no event for either: the owner's pair is seen as the buff on the fighter (made hidden at its first
// tick, shown by weapon_master_active), the ally's as its buff's life.
// Unverified: a hidden emitter goes on ticking.
// Deviation: the owner's components outlive the buff in the build (nothing destroys them); here they go with it.
// Deviation: a fighter is hidden here when it is dead or invisible; the smoke's AddInvisibility for the viewer
// is not ported, as mvs-view's place_view.

const MASTER_PS: &str = "PS_Jake_WeaponMaster";
const HANDS: [&str; 2] = ["R_Hand", "L_Hand"];
const OWN_SCALE: f64 = 2.0;
const ALLY_SCALE: f64 = 2.5;

impl Jake {
    fn spawn_master(&mut self, fx: &mut Fx, w: &World, id: i32, scale: f64) -> Vec<SystemId> {
        if !self.has(fx, MASTER_PS, "Jake WeaponMaster") {
            return Vec::new();
        }
        HANDS
            .iter()
            .filter_map(|hand| {
                let f = fx.socket_follow(w, id, Some(hand), [0.0; 3], &IDENTITY, [scale; 3]);
                fx.attach_to_mesh(w, id, MASTER_PS, hand, f, &[])
            })
            .collect()
    }

    fn place_master(&mut self, fx: &mut Fx, w: &World) {
        let (master, ally) = (w.names.get(WEAPON_MASTER), w.names.get(WEAPON_MASTER_ALLY));
        for f in w.all_ids() {
            let fi = w.fighter(f);
            let id = fi.id;
            let b = master.and_then(|c| buff_of(w, f, c)).map(|b| b.serial);
            if b != self.own.get(&id).map(|o| o.0) {
                for h in self.own.shift_remove(&id).map(|o| o.1).unwrap_or_default() {
                    fx.player.kill(h);
                }
                if let Some(serial) = b {
                    let hs = self.spawn_master(fx, w, id, OWN_SCALE);
                    self.own.insert(id, (serial, hs));
                }
            }
            if let Some((_, hs)) = self.own.get(&id) {
                let on = weapon_master_active(w, f);
                for &h in hs {
                    fx.player.set_visible(h, on);
                }
            }
            let a = ally.and_then(|c| buff_of(w, f, c)).map(|b| b.serial);
            if a != self.ally.get(&id).map(|o| o.0) {
                for h in self.ally.shift_remove(&id).map(|o| o.1).unwrap_or_default() {
                    fx.player.kill(h);
                }
                if let Some(serial) = a {
                    let hs = self.spawn_master(fx, w, id, ALLY_SCALE);
                    self.ally.insert(id, (serial, hs));
                }
            }
            // (AddVFX: the fighter's visibility tracking)
            if let Some((_, hs)) = self.ally.get(&id) {
                let on = !(fi.life.dead != 0.0 || fi.vis.invisible);
                for &h in hs {
                    fx.player.set_visible(h, on);
                }
            }
        }
    }
}

// ---- the spline meshes ----
//
// A view is made at the first tick the effects see its actor (a live JakeStretch_C, ProjectileAimer_SpitJake_C or
// JakeEatAlly_Aim_C) and goes with it (K2_DestroyActor takes its components). The class data, meshes and materials
// are the owner's character's, a Jake's (actor_def_owner). Each is drawn by mvs-view meshes::SplineMesh (a
// SplineMeshView: Alpha by the vertices' own bounds, the normals and tangents turned by the slices) as one component
// mesh per draw group, a dynamic instance of the actors entry's slot 0 each.

const STRETCH_CLS: &str = "JakeStretch_C";
const SPIT_AIMER_CLS: &str = "ProjectileAimer_SpitJake_C";
const ALLY_AIM_CLS: &str = "JakeEatAlly_Aim_C";

impl Jake {
    fn place_splines(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (_, x) in w.actors.actors.iter() {
            if x.dead {
                continue;
            }
            match &x.state {
                WorldActorState::Stretch(s) if w.names.str(x.cls) == STRETCH_CLS => {
                    seen.push(x.seq);
                    if !self.bodies.contains_key(&x.seq) {
                        let b = StretchBody::new(fx, w, x.owner, x.cls);
                        self.bodies.insert(x.seq, b);
                    }
                    if let Some(b) = self.bodies.get_mut(&x.seq).expect("entered above") {
                        b.place(fx, s, f64::from(x.size_scale.unwrap_or(1.0)));
                    }
                }
                WorldActorState::SpitAimer(a) if w.names.str(x.cls) == SPIT_AIMER_CLS => {
                    seen.push(x.seq);
                    let arc: Vec<f64> = a.arc.iter().map(|&v| f64::from(v)).collect();
                    let from = (f64::from(a.x), f64::from(a.y));
                    self.place_arc(fx, w, x.seq, x.owner, x.cls, false, |s| s.walk(from, &arc, a.count));
                }
                WorldActorState::JakeEatAllyAim(a) if w.names.str(x.cls) == ALLY_AIM_CLS => {
                    seen.push(x.seq);
                    let seg = a.spline.map(f64::from);
                    self.place_arc(fx, w, x.seq, x.owner, x.cls, true, |_| Some(seg));
                }
                _ => {}
            }
        }
        let gone: Vec<i32> =
            self.bodies.keys().chain(self.arcs.keys()).copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(b) = self.bodies.shift_remove(&k).flatten() {
                for id in b.ids {
                    fx.components.remove(id);
                }
            }
            if let Some(a) = self.arcs.shift_remove(&k).flatten() {
                fx.components.remove(a.id);
            }
        }
    }

    /// `seg`: the aimer's SetStartAndEnd (start, start tangent, end, end tangent; sim x, y each) from its view's state.
    #[allow(clippy::too_many_arguments)] // (the aimer's fields: two classes of one shape)
    fn place_arc(
        &mut self,
        fx: &mut Fx,
        w: &World,
        seq: i32,
        owner: FighterId,
        cls: mvs_sim::names::Name,
        ally: bool,
        seg: impl FnOnce(&mut AimArc) -> Option<[f64; 8]>,
    ) {
        if !self.arcs.contains_key(&seq) {
            let a = AimArc::new(fx, w, owner, cls, ally);
            self.arcs.insert(seq, a);
        }
        if let Some(a) = self.arcs.get_mut(&seq).expect("entered above") {
            let s = seg(a);
            a.place(fx, s);
        }
    }
}

/// The character key, skin name, whether it is the character's default skin, and the actors variant key (None:
/// local) the owner's skin draws a class of `from`'s data with (the view's look_of).
struct Look {
    key: String,
    skin: String,
    default: bool,
    variant: Option<&'static str>,
}

fn look(fx: &Fx, w: &World, owner: FighterId, from: FighterId) -> Option<Look> {
    let pick =
        usize::try_from(w.fighter(from).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref());
    let fd = &w.char_data(from).fighter;
    let skin = skin_of(fd, pick).ok()?;
    // (the instigator's pick of the owner's applied skin's material sets; the views'
    // colours, with team-coloured skins (mvs-view pose.rs), not the effects' own)
    let colours = fighter_colours(&w.setup, fx.me, fx.online, fx.team_skins);
    let id = usize::try_from(w.fighter(owner).id).ok();
    let c = id.and_then(|i| colours.get(i)).or(colours.get(1)).or(colours.first());
    let variant = match c.map_or(Variant::Local, |c| c.variant) {
        Variant::Menu | Variant::Local => None,
        v => Some(v.key()),
    };
    Some(Look {
        key: w.names.str(w.fighter(from).ch).to_owned(),
        default: skin.name == fd.skin.name,
        skin: skin.name.clone(),
        variant,
    })
}

/// The spline source of mesh `path` (read once a match: Fx::spline_source), else a warning once.
fn source(fx: &mut Fx, path: &str, what: &str) -> Option<Arc<SplineSource>> {
    fx.spline_source(path).map_err(|e| warn_once(format!("Jake: {what}: {e}"))).ok()
}

/// `sim (x, y) -> UE (0, -x, y)`.
fn ue(x: f64, y: f64) -> V3 {
    [0.0, -x, y]
}

// ---- JakeStretch_C's body ----
//
// One USplineMeshComponent per added mesh (AddSplineMesh @2632: AddComponent of NODE_AddSplineMeshComponent-5,
// StaticMesh Jake_StretchMesh, ForwardAxis X, SplineUpDir (0, 1, 0), at the actor's origin: its curve is in actor
// space, which the sim's world values stand for); on a cake skin (IsCake @5198-5616: the owner's AppliedSkin C005_S03
// or Jake_S01) the mesh S01_Stretch (@2677-2691) and StartRoll / EndRoll 1.7 when the owner faced left at the add
// (@2796-2870, inside the IsCake branch, which jumps to @2908: radians, the actor's FacingLeft for all of them: the
// add comes after @4051 sets it and the owner's facing does not change it); its start / end scale CalculateTaper or
// CalculateTaperReturn (MakeVector2D(s, s)), each SetStartAndEnd the sim's `ends`; its material a dynamic instance of
// Comp_Pawn_SkinManager.GetMaterialForSkin(0) (@2976-3048, on every skin: it replaces the cake branch's SetMaterial
// of OwnerMat @2746; the actors entry Jake_StretchMesh for the default skin, '<skin>.Stretch' for another, in the
// skin pick's variant) with Diffuse_Mult 0.7 or 1 (@3490, the sim's `dims`): draw group 1 or 0. The wiggle (@10515)
// is the sim's: it moves the points the ends come from. The root's scale (the sim's size scale) is on the meshes'
// widths.
// The skin is the render's: IsCake is read from the drawn skin's name (CAKE_SKINS).
// The body is drawn while its actor lives (no visibility of its own).
// Unverified: the body's instances set Diffuse_Mult alone. GetMaterialForSkin(0) is the skin's instance,
//   which also holds what the spawn and the match write on the body's slots (BPI_AddMaterialInstance's Team,
//   Comp_Pawn_Rage's Emissive_Mult, Desaturation, OpacityToggle: mvs-view pose.rs); whether a dynamic instance of it
//   keeps them is not read.

const STRETCH_MESH: &str = "Jake_StretchMesh";
/// @2832 / @2870.
const ROLL_LEFT: f64 = 1.7;
/// @3443.
const DIM: f64 = 0.7;
/// @5198-5362.
const CAKE_SKINS: [&str; 2] = ["C005_S03", "Jake_S01"];

fn right_rows() -> Rows {
    rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0)
}

fn left_rows() -> Rows {
    rotation_rows(0.0, std::f64::consts::FRAC_PI_2, 0.0)
}

#[derive(Debug)]
struct StretchBody {
    src: Arc<SplineSource>,
    mesh: SplineMesh,
    /// The template's SplineUpDir (actor space).
    up: V3,
    cake: bool,
    /// Per draw group (Diffuse_Mult 1, 0.7) its component mesh.
    ids: [CMeshId; 2],
    slices: Vec<SplineSlice>,
}

impl StretchBody {
    /// stretchLook and the view's make (None, warned once: the template, mesh or materials are missing).
    fn new(fx: &mut Fx, w: &World, owner: FighterId, cls: mvs_sim::names::Name) -> Option<StretchBody> {
        let made = (|| {
            let from = actor_def_owner(w, owner, cls);
            let Look { key, skin, default, variant: vk, .. } = look(fx, w, owner, from)?;
            let cs = &w.char_data(from).fighter.actors.as_ref()?.get(STRETCH_CLS)?.components;
            // (the AddSplineMeshComponent node whose StaticMesh is the body's)
            let t = cs.iter().find_map(|(n, c)| {
                let c = c.as_obj()?;
                (n.starts_with("NODE_AddSplineMeshComponent")
                    && c.get("StaticMesh").and_then(Value::as_str) == Some(STRETCH_MESH))
                .then_some(c)
            })?;
            let axis = SplineMesh::axis_of(t.get("ForwardAxis").and_then(Value::as_str));
            let ud = t.get("SplineUpDir");
            let up = if ud.is_some() { xyz(ud, 0.0) } else { [0.0, 0.0, 1.0] };
            let d = fx.draws(&key)?;
            let body_key =
                if default { Some(STRETCH_MESH.to_owned()) } else { d.actor_key(&format!("{skin}.Stretch")) };
            let (path, mats) = d.actor(&body_key?, vk)?;
            let slot = mats.first().cloned().flatten()?;
            let src = source(fx, &path, "the stretch body")?;
            let mesh = SplineMesh::new(
                src.positions.clone(),
                src.normals.clone(),
                src.tangents.clone(),
                src.indices.clone(),
                axis,
                AlphaRule::Bounds,
            );
            let ids =
                [1.0, DIM].map(|k| fx.components.add_material(&slot, &[("Diffuse_Mult", k)], CMeshGeometry::empty()));
            Some(StretchBody { src, mesh, up, cake: CAKE_SKINS.contains(&skin.as_str()), ids, slices: Vec::new() })
        })();
        if made.is_none() {
            warn_once("Jake: the stretch body is not drawn (its template, mesh or materials are missing)".into());
        }
        made
    }

    fn place(&mut self, fx: &mut Fx, s: &Stretch, size: f64) {
        let rows = if s.facing_left { left_rows() } else { right_rows() };
        // SplineUpDir in the world: the template's, turned by the actor
        let u = self.up;
        let up: V3 = std::array::from_fn(|k| u[0] * rows[k] + u[1] * rows[3 + k] + u[2] * rows[6 + k]);
        let roll = if self.cake && s.facing_left { ROLL_LEFT } else { 0.0 };
        // the meshes' slices from the sim (fill)
        let n = (s.scales.len() / 2).min(s.ends.len() / 8);
        self.slices.clear();
        for i in 0..n {
            let e = &s.ends[i * 8..i * 8 + 8];
            let (s0, s1) = (f64::from(s.scales[i * 2]) * size, f64::from(s.scales[i * 2 + 1]) * size);
            self.slices.push(SplineSlice {
                scale0: [s0; 2],
                scale1: [s1; 2],
                roll0: roll,
                roll1: roll,
                group: usize::from(s.dims.get(i).is_some_and(|&d| d < 1.0)),
                ..SplineSlice::curve(
                    ue(f64::from(e[0]), f64::from(e[1])),
                    ue(f64::from(e[2]), f64::from(e[3])),
                    ue(f64::from(e[4]), f64::from(e[5])),
                    ue(f64::from(e[6]), f64::from(e[7])),
                )
            });
        }
        self.mesh.update(&self.slices, up);
        for (g, &id) in self.ids.iter().enumerate() {
            let geometry = spline_group_geometry(&self.src, &self.mesh, g);
            if let Some(m) = fx.components.get_mut(id) {
                m.shown = geometry.is_some();
                m.geometry = geometry.unwrap_or_else(CMeshGeometry::empty);
            }
        }
    }
}

// ---- the arc aimers' AimSpline ----
//
// AimSpline is one USplineMeshComponent (StaticMesh SpitAimer, ForwardAxis Y, the template's SplineUpDir (0, 0, 1):
// the actor's yaw leaves it; scale 1, no roll) with the actors entry SpitAimer's slot 0 (JakeSpitAim_MAT).
// - The spit's (ProjectileAimer_SpitJake_C, BeginPlay @6446-7236): a dynamic instance of JakeSpitAim_MAT; shown when
//   the owner is locally controlled; else, when the owner's team mate is a locally controlled fighter, shown with
//   Team 0 (@7151); else hidden (SetHiddenInGame @7202). Its curve each tick @4876-5236: SetStartAndEnd(the actor's
//   location, unit(-> the arc's point 2) x 900, LastVector, unit(LastVector -> the arc's next point) x 900).
// - The ally aim's (JakeEatAlly_Aim_C): bVisible false in the template, SetVisibility(true) when GetOwner's fighter is
//   locally controlled; the mesh's own material. Its curve @5606-6062: SetStartAndEnd(the location + unit(-> point 2)
//   x 100, ...).
// The sim's aimers keep no SetStartAndEnd: the arc's first point to its last is drawn, the tangents along its first
// and last legs, 900 long; under two points it is hidden.
// Deviation: SetStartAndEnd is handed world positions while AimSpline is no absolute component (under Sphere or
// SkillShotIndicator under the Capsule), so the exe bends the mesh in the component's space under ComponentToWorld
// (spec Components/specs/39): the build draws the arc offset by the actor's place. The port keeps drawing it at the
// world positions the graph names, the cleaner line.
// "Locally controlled" is Fx::local_human (unverified: a bot is not).

const AIM_MESH: &str = "SpitAimer";
/// @5110 / @5178.
const TANGENT: f64 = 900.0;
/// ProjectileAimer_SpitJake's AimDistance class default.
const AIM_DISTANCE: f64 = 900.0;

// ProjectileAimer_SpitJake's AimSpline, every tick: ArcLength summed from the actor's location along ArcPositions
// (@3706-4112) up to the first point i at or past AimDistance (@4139); LastVector = arc[i-1] pulled back along
// arc[i-1] -> arc[i] by the overshoot (@4187-4558); SetStartAndEnd(location, unit(location -> arc[2]) x 900,
// LastVector, unit(LastVector -> arc[i+1]) x 900) (@4876-5236). JakeEatAlly_Aim's is the sim's own (ally_spit.rs
// `spline`: start 100 out, tangents x its AimDistance).
// Unverified: a tick whose arc never reaches AimDistance keeps the last LastVector and its index (the BP's variables
// keep their values); before the first one reaches it, nothing is drawn.

#[derive(Debug)]
struct AimArc {
    src: Arc<SplineSource>,
    mesh: SplineMesh,
    id: CMeshId,
    /// AimDistance (the class data's, else the class default).
    dist: f64,
    /// LastVector and LastVectorIndex, once set.
    last: Option<(f64, f64, usize)>,
}

impl AimArc {
    /// The view's make (None: not shown to this machine, or its template, mesh or material missing: warned once).
    fn new(fx: &mut Fx, w: &World, owner: FighterId, cls: mvs_sim::names::Name, ally: bool) -> Option<AimArc> {
        let oid = w.fighter(owner).id;
        let mut shown = fx.local_human(w, oid);
        let mut team = None;
        if !ally && !shown {
            // (the owner's team mate that this machine controls: PlayerDataManager.GetTeamMate)
            let me = w.fighter(owner);
            let mate = w.all_ids().into_iter().any(|f| {
                let o = w.fighter(f);
                o.id != oid && o.team.team == me.team.team && fx.local_human(w, o.id)
            });
            if mate {
                shown = true;
                team = Some(0.0);
            }
        }
        if !shown {
            return None;
        }
        let made = (|| {
            let from = actor_def_owner(w, owner, cls);
            let fd = &w.char_data(from).fighter;
            let def = fd.actors.as_ref()?.get(w.names.str(cls))?;
            let dist = def.p.get("AimDistance").and_then(Value::as_f64).unwrap_or(AIM_DISTANCE);
            let t = def.components.get("AimSpline")?.as_obj()?;
            let axis = SplineMesh::axis_of(t.get("ForwardAxis").and_then(Value::as_str));
            let key = w.names.str(w.fighter(from).ch).to_owned();
            let (path, mats) = fx.draws(&key)?.actor(AIM_MESH, None)?;
            let slot = mats.first().cloned().flatten()?;
            let src = source(fx, &path, "the aim arc")?;
            let mesh = SplineMesh::new(
                src.positions.clone(),
                src.normals.clone(),
                src.tangents.clone(),
                src.indices.clone(),
                axis,
                AlphaRule::Bounds,
            );
            let scalars: &[(&str, f64)] = match team {
                Some(t) => &[("Team", t)],
                None => &[],
            };
            let id = fx.components.add_material(&slot, scalars, CMeshGeometry::empty());
            // (it is added to the scene on its own: no render order, sorted by depth)
            if let Some(m) = fx.components.get_mut(id) {
                m.by_depth = true;
            }
            Some(AimArc { src, mesh, id, dist, last: None })
        })();
        if made.is_none() {
            warn_once("Jake: the aim arc is not drawn (its template, mesh or material is missing)".into());
        }
        made
    }

    /// The spit aimer's SetStartAndEnd from its arc (`p`: sim x, y per point, `count` in use) and location.
    fn walk(&mut self, from: (f64, f64), p: &[f64], count: i32) -> Option<[f64; 8]> {
        let n = usize::try_from(count).unwrap_or(0).min(p.len() / 2);
        let pt = |i: usize| (p[2 * i], p[2 * i + 1]);
        let mut len = 0.0;
        for i in 0..n {
            let prev = if i == 0 { from } else { pt(i - 1) };
            let c = pt(i);
            len += (c.0 - prev.0).hypot(c.1 - prev.1);
            if len < self.dist {
                continue;
            }
            // (at i 0 the Array_Get(-1) gives an empty vector, as the sim's ally aim)
            let q = if i > 0 { pt(i - 1) } else { (0.0, 0.0) };
            let (ux, uy) = unit(c.0 - q.0, c.1 - q.1);
            let over = len - self.dist;
            self.last = Some((q.0 - ux * over, q.1 - uy * over, i));
            break;
        }
        let (lx, ly, i) = self.last?;
        let (sx, sy) = if n > 2 { unit(p[4] - from.0, p[5] - from.1) } else { (0.0, 0.0) };
        let (ex, ey) = if i + 1 < n { unit(p[2 * i + 2] - lx, p[2 * i + 3] - ly) } else { (0.0, 0.0) };
        Some([from.0, from.1, sx * TANGENT, sy * TANGENT, lx, ly, ex * TANGENT, ey * TANGENT])
    }

    fn place(&mut self, fx: &mut Fx, seg: Option<[f64; 8]>) {
        let Some(m) = fx.components.get_mut(self.id) else { return };
        let Some(s) = seg else {
            m.shown = false;
            return;
        };
        let g = SplineSlice::curve(ue(s[0], s[1]), ue(s[2], s[3]), ue(s[4], s[5]), ue(s[6], s[7]));
        self.mesh.update(&[g], [0.0, 0.0, 1.0]);
        m.shown = true;
        m.geometry = spline_geometry(&self.src, &self.mesh);
    }
}

/// The unit vector of (x, y), (0, 0) for a zero one (the stock GetSafeNormal).
fn unit(x: f64, y: f64) -> (f64, f64) {
    let l = x.hypot(y);
    if l <= 1e-8 { (0.0, 0.0) } else { (x / l, y / l) }
}

// ---- the horse's ScoobySnackBuff ----
//
// JakeHorse_Actor3's ScoobySnackBuff (Tick @10261 -> @18328 -> @18046): while the owner has Buff_ScoobySnack_C, once
// (a DoOnce) the buff's ParticleSystems are destroyed (Shaggy's module, Fx::snack_destroy) and
// PS_ShaggyScoobySnackBuff is attached to JakeHouse at L_Hand and R_Hand, scale 2, Team = the owner's colour index
// (Conv_IntToFloat, no local 0.6); without the buff @15308 destroys them and resets the DoOnce (@2318). Jake_Horse
// has no L_Hand / R_Hand bone: both sit at the component's origin (SpawnEmitterAttached on a missing socket takes the
// component): the actor's place, its yaw by OldFacing (as mvs-view's horse: the facing of the tick before, the
// first seen at first), the component's relative scale times 2 and the root's size scale (the rows unreflected: a
// system's scale stays positive).
// Deviation: the yaw's OldFacing runs once a presented tick of the world, as the view's.
// The horse's place (and its snack) runs only while its mesh is drawn (it returns without
// the skeleton): here, while the view entered the horse's bones. The modules' actors run before their places (the
// order of the match's step), so the destroy asked here reaches Shaggy's place_snack before its spawn of the same tick:
// when the buff starts while the horse is out, the destroy finds nothing and Shaggy's systems spawn and stay.
// Deviation: Shaggy's systems on the owner are not hidden with the hidden Jake (the snack's visibility tracking is not
// ported, shaggy/mod.rs place_snack).

const HORSE_CLS: &str = "JakeHorse_Actor3_C";
const HORSE_COMP: &str = "JakeHouse";
const SNACK_PS: &str = "PS_ShaggyScoobySnackBuff";
const SNACK_SCALE: f64 = 2.0;

#[derive(Debug, Default)]
struct HorseSnack {
    /// FacingLeft at the last tick run (None: not seen yet).
    old_left: Option<bool>,
    last_tick: Option<i32>,
    yaw_left: bool,
    /// The DoOnce's two hand systems (None: not made).
    snack: Option<Vec<Option<SystemId>>>,
}

impl Jake {
    fn place_horses(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (aid, x) in w.actors.actors.iter() {
            let WorldActorState::JakeHorse(h) = &x.state else { continue };
            if x.dead || w.names.str(x.cls) != HORSE_CLS {
                continue;
            }
            // (the horse's mesh drawn)
            if !fx.actor_bones.contains_key(&ActorRef::Actor(aid)) {
                continue;
            }
            seen.push(x.seq);
            let tick = w.flow.tick;
            let st = self.horses.entry(x.seq).or_default();
            if st.last_tick != Some(tick) {
                // (the yaw of the tick before this one's OldFacing update)
                st.yaw_left = st.old_left.unwrap_or(h.facing_left);
                st.old_left = Some(h.facing_left);
                st.last_tick = Some(tick);
            }
            let rows = mesh_rows(st.yaw_left);
            let comp = actor_def(w, ActorRef::Actor(aid), x.cls)
                .and_then(|d| d.components.get(HORSE_COMP))
                .and_then(Value::as_obj)
                .and_then(|c| c.get("RelativeScale3D"));
            let cs = xyz(comp, 1.0);
            let r = x.size_scale.unwrap_or(1.0);
            let at =
                att([0.0, f64::from(-h.x), f64::from(h.y)], &rows, Some(cs.map(|v| v * SNACK_SCALE * f64::from(r))));
            let owner = x.owner_id;
            let has = has_snack(w, owner);
            let st = self.horses.get_mut(&x.seq).expect("entered above");
            match (st.snack.is_some(), has) {
                (false, true) => {
                    fx.snack_destroy.push(owner);
                    let mut on = Vec::new();
                    if fx.player.emitter_count(SNACK_PS).is_some() {
                        let team = f64::from(fx.color_index(owner));
                        for _ in 0..2 {
                            on.push(fx.player.spawn_attached(SNACK_PS, &at, 1.0, &[("Team", team)]));
                        }
                    }
                    st.snack = Some(on);
                }
                (true, true) => {
                    for &s in st.snack.iter().flatten().flatten() {
                        fx.player.attach(s, &at);
                    }
                }
                (true, false) => {
                    for s in st.snack.take().into_iter().flatten().flatten() {
                        fx.player.kill(s);
                    }
                }
                (false, false) => {}
            }
        }
        let gone: Vec<i32> = self.horses.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(h) = self.horses.shift_remove(&k) {
                for s in h.snack.into_iter().flatten().flatten() {
                    fx.player.kill(s);
                }
            }
        }
    }
}
