//! Reindog's effects: the classes' projectile rules of TetherProjectile_C,
//! SeismicReactor_C and Firebolt_C / Firebolt_Child_C. The rules read the shared Projectile fields and the sim's own;
//! offsets are each class's ubergraph's. The shared electric classes his reactor gives are `fx/shared/electric.rs`.
//!
//! PS_Creature_ElectricityEyes_L / _R: the pawn's components on L_EyeSocket / R_EyeSocket, bVisible false in the
//! template; Pawn_Reindog shows them only for skin_c009_s09 (@4020-4565, with its Armor_Dynamics): on the default skin
//! nothing draws them.
//!
//! CreatureSpit_C: its ParticleSystem (PS_Creature_F_Projectile, scale 2 under StaticMesh) is the shared path's
//! projectile component, with SetTeamColor's Team (@732: SelectFloat(0.6, the owner's colour value, IsLocalOnline));
//! MulticastSetProjectileScale's Capsule.SetWorldScale3D (@123) is the sim's Projectile.scale. No rule here.
//!
//! Creature_WreckingBall_C's sphere (an actor mesh rule there): a component mesh
//! placed each drawn frame (`ball_frame`).

use std::collections::{BTreeMap, HashMap};

use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt, attach_parent, class_p};
use mvs_sim::chars::creature::fire::bolt_scale;
use mvs_sim::chars::creature::seismic::{at_boom, at_warn, complex_down, trace_down};
use mvs_sim::events::Event;
use mvs_sim::fighter::common::{actor_def, latent_frames};
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, V3};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule, component_warm, fighter_of};
use crate::cmesh::{CMeshGeometry, CMeshId, object_path};
use crate::fx::attach::{rot_rows, xyz};
use crate::fx::{Follow, Fx, Scale, under};

// Deviation: PS_Creature_ElectricityEyes: the port does not draw a pawn's particle components (skin only); the
// build shows them for skin_c009_s09 only (nothing on the default skin).

/// What Reindog's effects keep between steps: per actor (its sequence number) the rule's own record; the systems
/// warned of.
#[derive(Debug, Default)]
pub(crate) struct Creature {
    tethers: HashMap<i32, Tether>,
    reactors: HashMap<i32, Reactor>,
    bolts: HashMap<i32, Bolt>,
    /// Per wrecking ball: its sphere (None: its colour index has no material).
    balls: BTreeMap<i32, Option<CMeshId>>,
    warned: std::collections::BTreeSet<String>,
}

impl CharFxHooks for Creature {
    /// The wrecking ball's sphere with each of its materials (the warm-up, `Fx::warm`).
    fn warm(&mut self, fx: &mut Fx, w: &World, out: &mut Vec<mvs_view::WarmMesh>) {
        let mesh = object_path(SPHERE);
        if fighter_of(w, mvs_sim::chars::creature::KEY).is_none() || !fx.mesh_pack().has(&mesh) {
            return;
        }
        for k in 0..2 {
            if let Some(mat) = ball_material(k) {
                out.push(component_warm(Some(&mesh), mat));
            }
        }
    }

    fn frame(&mut self, fx: &mut Fx, w: &World, alpha: f64) {
        self.ball_frame(fx, w, alpha);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

const SPHERE: &str = "Engine/Content/BasicShapes/Sphere";

/// The sphere's material by the instigator's colour index.
fn ball_material(color_index: u8) -> Option<&'static str> {
    match color_index {
        0 => Some("Panda_Main/VFX/Materials/Mat_Shield_P2#local"),
        1 => Some("Panda_Main/VFX/Materials/Mat_Shield_P1#local"),
        _ => None,
    }
}

impl Creature {
    /// Creature_WreckingBall_C's Visual component, the engine's Sphere (radius 50) at RelativeScale3D 4.5 under the
    /// Hitbox under the Capsule, all at the actor's origin, so a sphere of radius 225 around the ally's actor location.
    /// Its material: the class's BeginPlay (@173-314, not on a dedicated server) does Visual.SetMaterial(0,
    /// switch(the instigator's GetColorIndex) {0: Mat_Shield_P2, 1: Mat_Shield_P1, default: null}). The instance is
    /// the material's own (SetMaterial of a Material, no dynamic instance): its defaults (Intensity 4, Color (0, 22,
    /// 22)); Mat_Shield_P1 / P2 are Translucent Unlit, built as "#local" component materials. The default branch (an
    /// index past 1: GetColorIndex gives 0 or 1 offline) would put the Sphere's own BasicShapeMaterial back, which is
    /// not built: nothing is drawn there. Made at the actor's first drawn frame, gone with it.
    // Deviation: the sim keeps the ball one tick behind the ally (the pull moves the ally after the projectile pass;
    // the hit reads the ally's live place): the view is shifted by that lag so the sphere sits on the ally.
    fn ball_frame(&mut self, fx: &mut Fx, w: &World, alpha: f64) {
        let live: Vec<_> = w
            .actors
            .projectiles
            .iter()
            .filter(|(_, pr)| matches!(pr.ext, ProjectileExt::WreckingBall(_)))
            .map(|(id, pr)| (id, pr.seq))
            .collect();
        self.balls.retain(|seq, m| {
            if live.iter().any(|(_, s)| s == seq) {
                return true;
            }
            if let Some(id) = m {
                fx.components.remove(*id);
            }
            false
        });
        for (id, seq) in live {
            let Some(pr) = w.actors.projectiles.get(id) else { continue };
            let m = match self.balls.get(&seq) {
                Some(m) => *m,
                None => {
                    let m = ball_mesh(fx, w, id, pr);
                    self.balls.insert(seq, m);
                    m
                }
            };
            let (Some(m), Some(at)) = (m, mvs_view::actors::actor_place(w, ActorRef::Projectile(id))) else { continue };
            let (x, y) = at.at(alpha);
            let (hx, hy) = mvs_sim::chars::creature::perks_ball::hit_xy(w, pr);
            let (px, py) = (x + f64::from(hx - pr.x), y + f64::from(hy - pr.y));
            if let Some(c) = fx.components.get_mut(m) {
                c.shown = !at.hidden;
                if let CMeshGeometry::Static { to_world, .. } = &mut c.geometry {
                    to_world[3] = 0.0;
                    to_world[7] = -px as f32;
                    to_world[11] = py as f32;
                }
            }
        }
    }
}

/// The ball's sphere at the world scale Visual's x the Hitbox's (the Capsule's is absolute, 1), no rotation.
fn ball_mesh(fx: &mut Fx, w: &World, id: mvs_sim::actors::ProjectileId, pr: &Projectile) -> Option<CMeshId> {
    let cs = &actor_def(w, ActorRef::Projectile(id), pr.cls)?.components;
    let mat = ball_material(fx.color_index(pr.owner_id))?;
    let v = cs.get("Visual")?.as_obj()?;
    v.get("StaticMesh")?;
    let sx =
        |c: Option<&mvs_data::Value>| xyz(c.and_then(|c| c.as_obj()).and_then(|c| c.get("RelativeScale3D")), 1.0)[0];
    let s = (sx(cs.get("Visual")) * sx(cs.get("Hitbox"))) as f32;
    let mesh = object_path(SPHERE);
    if !fx.mesh_pack().has(&mesh) {
        return None;
    }
    let to_world = [s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0];
    Some(fx.components.add(mat, &[], CMeshGeometry::Static { mesh: mesh.into(), to_world }))
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (
        "TetherProjectile_C",
        ProjRule {
            made: Some(tether_made),
            tick: Some(tether_tick),
            end: Some(tether_end),
            systems: Some(tether_systems),
            ..ProjRule::NONE
        },
    ),
    (
        "SeismicReactor_C",
        ProjRule {
            made: Some(reactor_made),
            tick: Some(reactor_tick),
            end: Some(reactor_end),
            systems: Some(reactor_systems),
            own_emitter: Some(reactor_destroy),
            ..ProjRule::NONE
        },
    ),
    ("Firebolt_C", BOLT),
    ("Firebolt_Child_C", BOLT),
];

const BOLT: ProjRule = ProjRule {
    made: Some(firebolt),
    end: Some(bolt_end),
    tick: Some(bolt_tick),
    systems: Some(bolt_systems),
    ..ProjRule::NONE
};

fn me(c: &mut CharFx) -> &mut Creature {
    match c {
        CharFx::Creature(s) => s,
        _ => unreachable!("a Reindog rule entered by another module"),
    }
}

fn me_ref(c: &CharFx) -> &Creature {
    match c {
        CharFx::Creature(s) => s,
        _ => unreachable!("a Reindog rule entered by another module"),
    }
}

impl Creature {
    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Reindog: {ps} is not loaded");
        }
        false
    }

    /// Every actor's record and its systems (the ally's ribbon included).
    fn clear(&mut self, fx: &mut Fx) {
        let tethers = self.tethers.drain().flat_map(|(_, o)| [o.tip, o.beam, o.source, o.pin, o.connected, o.ribbon]);
        let reactors = self.reactors.drain().flat_map(|(_, o)| [o.crystal, o.warn, o.beam]);
        let bolts = self.bolts.drain().map(|(_, b)| b.h);
        for m in std::mem::take(&mut self.balls).into_values().flatten() {
            fx.components.remove(m);
        }
        let hs: Vec<SystemId> = tethers.chain(reactors).chain(bolts).flatten().collect();
        for h in hs {
            fx.player.kill(h);
        }
    }
}

/// Comp_Pawn_SkinManager.GetColorIndex's Team (0.6 for the local player online, as Fx::team).
fn team_index(fx: &Fx, id: i32) -> f64 {
    let t = fx.team(id);
    if t == 0.6 { 0.6 } else { f64::from(fx.color_index(id)) }
}

fn components<'a>(w: &'a World, st: &ProjFxState, pr: &Projectile) -> Option<&'a Props> {
    actor_def(w, ActorRef::Projectile(st.id), pr.cls).map(|d| &d.components)
}

/// A particle component of the class's template at its relative transform under its attach parents, under the
/// actor's rotation and root scale (as the shared path's own component), with the Team given (None: no Team).
fn component(fx: &mut Fx, w: &World, st: &ProjFxState, name: &str, team: Option<f64>) -> Option<SystemId> {
    let pr = w.actors.projectiles.get(st.id)?;
    let cs = components(w, st, pr)?;
    let h = fx.projectile_component(w, pr, cs, name, Some(&NO_TEAM))?;
    if let Some(t) = team {
        fx.player.set_param(h, "Team", t);
    }
    Some(h)
}

/// The rule `component` passes: the Team is its caller's.
static NO_TEAM: ProjRule = ProjRule { no_team: true, ..ProjRule::NONE };

/// SpawnEmitterAttached(ps, the actor's component `on`, None, location 0, rotation 0, scale 1, SnapToTarget, auto
/// destroy): at the component's place and rotation, world scale 1 (the actor's root scale taken as the spawn's).
fn attached_to(
    c: &mut Creature,
    fx: &mut Fx,
    w: &World,
    st: &ProjFxState,
    ps: &str,
    on: &str,
    team: f64,
) -> Option<SystemId> {
    let pr = w.actors.projectiles.get(st.id)?;
    let cs = components(w, st, pr)?;
    let Some(comp) = cs.get(on).and_then(|v| v.as_obj()) else {
        if c.warned.insert(format!("{}.{on}", w.names.str(pr.cls))) {
            eprintln!("warning: Reindog: {} has no component {on}", w.names.str(pr.cls));
        }
        return None;
    };
    if !c.has(fx, ps) {
        return None;
    }
    let par = attach_parent(cs, on);
    let own_loc = xyz(comp.get("RelativeLocation"), 0.0);
    let own_rot = rot_rows(comp.get("RelativeRotation"));
    let (rel, _, _, rel_rot) = under(&par, own_loc, &own_rot, [1.0; 3], None, par.scale.map(f64::from));
    let f = Follow::Projectile { seq: st.seq, rel, rel_rot, scale: [f64::from(1.0 / pr.scale); 3] };
    fx.spawn_follow(w, ps, f, 1.0, &[("Team", team)])
}

/// SpawnEmitterAttached(ps, the root, None, (x, 0, 0), rotation 0, scale 1, KeepRelativeOffset): at the offset in the
/// actor's rotation, under its scale.
fn on_root(
    c: &mut Creature,
    fx: &mut Fx,
    w: &World,
    st: &ProjFxState,
    ps: &str,
    x: f64,
    team: f64,
) -> Option<SystemId> {
    if !c.has(fx, ps) {
        return None;
    }
    let f = Follow::Projectile { seq: st.seq, rel: [x, 0.0, 0.0], rel_rot: IDENTITY, scale: [1.0; 3] };
    fx.spawn_follow(w, ps, f, 1.0, &[("Team", team)])
}

// ---- Firebolt_C / Firebolt_Child_C

// Firebolt_C / Firebolt_Child_C: StaticMesh and SM_Projectile are bVisible false and have no mesh; ParticleSystem has
// no template. SetTeamColor @3664: ProjectileParticleSystem = SpawnEmitterAttached(BaseVFX
// (PS_Creature_FireWall_Projectile01), SM_Projectile, None, 0, 0, scale FacingLeft ? (1, -1, 1) : (1, 1, 1),
// SnapToTargetIncludingScale, no auto destroy) @4267 with Team = SelectFloat(0.6, the owner's colour value,
// IsLocalOnline) @4127; InitFromPool sets SM_Projectile's world scale to 1 (@4968) and the component's relative scale
// to (1, FacingLeft ? -1 : 1, 1) (@5386); MulticastReflect and MulticastVelocity with a Y velocity set FacingLeft =
// Velocity.Y > 0 and the scale again (@4618-4576). OnReleasedToPool hides and deactivates it (@1152). Its impacts
// (PS_Creature_FireWall_Impact @2256 / @2548) are the sim's death events.
// Unverified: Firebolt's FacingLeft is the owner's facing at the first sight and the sign of the velocity after (the
// reflect and velocity multicasts' rule). Its scale is SM_Projectile's world scale (SnapToTargetIncludingScale): 1 at
// InitFromPool, then + 0.02 a tick below MaxSize 7 (ReceiveTick @1840-2186), the sim's bolt_scale (the growth times the
// actor's scale), times the graph's relative (1, +-1, 1). The Instigator of each is taken as the actor's owner.

const FIREBOLT_VFX: &str = "PS_Creature_FireWall_Projectile01";

#[derive(Debug)]
struct Bolt {
    h: Option<SystemId>,
    left: bool,
}

/// The bolt's relative scale under the actor's (Follow::Projectile multiplies the actor's scale in).
fn bolt_rel(w: &World, pr: &Projectile, left: bool) -> V3 {
    let max = class_p(w, pr).get("MaxSize").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let g = bolt_scale(pr.age, mvs_sim::math::narrow(max));
    [f64::from(g), if left { f64::from(-g) } else { f64::from(g) }, f64::from(g)]
}

fn firebolt(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let left = pr.owner.is_some_and(|o| mvs_sim::fighter::facing(w, o) < 0.0);
    let mut b = Bolt { h: None, left };
    if s.has(fx, FIREBOLT_VFX) {
        let f = Follow::Projectile { seq: st.seq, rel: [0.0; 3], rel_rot: IDENTITY, scale: bolt_rel(w, pr, left) };
        let team = fx.team(pr.owner_id);
        b.h = fx.spawn_follow(w, FIREBOLT_VFX, f, 1.0, &[("Team", team)]);
    }
    s.bolts.insert(st.seq, b);
}

/// The facing and the growth of this tick (the follow reads them each place).
fn bolt_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(b) = me(c).bolts.get_mut(&st.seq) else { return };
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if pr.vx != 0.0 {
        b.left = pr.vx < 0.0; // (Velocity.Y > 0: UE Y is -x)
    }
    let Some(h) = b.h else { return };
    let rel = bolt_rel(w, pr, b.left);
    if let Some((_, Follow::Projectile { scale, .. })) = fx.followers.iter_mut().find(|(k, _)| *k == h) {
        *scale = rel;
    }
}

fn bolt_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(Bolt { h: Some(h), .. }) = me(c).bolts.remove(&st.seq) {
        fx.player.kill(h);
    }
}

fn bolt_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).bolts.get(&st.seq).and_then(|b| b.h).into_iter().collect()
}

// ---- SeismicReactor_C

// SeismicReactor_C (the sim: mvs-sim chars/creature/seismic.rs):
//   SM_SeismicReactor (SM_crystals, MAT_Crystals_Inst, under DamageBox at Z -75) is bVisible false and bHiddenInGame in
//   the template, and the mesh, its materials and MAT_SeismicReactor are in neither data export: it stays undrawn
//   (mvs-view chars/creature.rs's rule draws nothing for it); its dynamic material's Team (@11001) and Timeline_0's
//   EmissiveStrength (@17313, played by nothing) go with it. What the player sees of the reactor is its particles:
//   PS_CreatureCrystal's mesh emitters (CrystalBig / CrystalSmall) are the crystal.
//   PS_CreatureCrystal (Z 85 under the mesh, bStartWithTickEnabled false: drawn while the actor lives, as the other
//   actors' components of that kind) gets Team = SelectFloat(0.6, the Instigator's GetColorIndex,
//   IsLocalOnlinePlayer) in Begin Play Visuals @10655. Every Team below is that value.
//   The actor's rotation (mvs-view's rule rot) turns the crystal and the warning with it.
//   Charge Up Visuals @11111 spawns no emitter: the charge is the spin.
//   The events, at the sim's ages after the charge's start (SeismicReactor.charge_at), each seen once:
//   - WarningVisualsMulticast (@503, 0.25 s) @14496: SpawnEmitterAttached(PS_CreatureCrystal_Warning,
//     SM_SeismicReactor, None, 0, 0, scale 1, SnapToTarget, auto destroy) @14501: on the mesh's place, world scale 1;
//     SeismicSwayOn false (@14915), so LineTraceSingle(TraceTypeQuery 3) straight down 20000 @15008 and
//     SpawnEmitterAtLocation(PS_CreatureCrystal_WarningGround, the hit's Location or the trace's end) @15440, then
//     @30-444: PS_CreatureCrystal_LightingBeam_Warning at the actor @58 (its beam ends UserSet with nothing set: the
//     distributions' (0, 0, 0) to (0, 0, -800)).
//   - Seismic Boom Visuals(P) (SeismicBoom @9545, 1.2 s) @11432: SpawnEmitterAtLocation(PS_CreatureCrystal_LightingBeam,
//     (0, 0, 0)) @11647 kept as BeamParticleComponentHackUE420Broken, Team as the vector (1, 1, Team) @12045, beam
//     source the actor + (0, -20, 0) @12181, target P @12236; PS_CreatureCrystal_Activate at the actor + (0, 0, -100)
//     @12429; Delay(0.1) @12296 -> @1188: PS_CreatureCrystal_Impact at P. P is the sim's trace (SeismicBoom
//     @9113-9490), taken again here.
//   - ReceiveDestroyed @13066 (DestroyVisuals): PS_CreatureCrystal_Destroy at the actor @13370, the sim's
//     projectileDeath event (DeathVFX none, offset 0) taken by own_emitter. ReceiveEndPlay @10341 destroys the kept
//     beam; the attached warning and the crystal go with the actor; the ones at a location stay.
//   Every spawn rotation is (0, 0, 0).
// Deviation: the trace's boxes are the stage's ground and platforms: World::blockers (the ships', spawned platforms'
// and solids' boxes the sim's trace adds) takes a `&mut World` the view does not have.

const REACTOR_CRYSTAL: &str = "PS_CreatureCrystal";
const REACTOR_WARNING: &str = "PS_CreatureCrystal_Warning";
const REACTOR_GROUND: &str = "PS_CreatureCrystal_WarningGround";
const REACTOR_BOLT_WARNING: &str = "PS_CreatureCrystal_LightingBeam_Warning";
const REACTOR_BEAM: &str = "PS_CreatureCrystal_LightingBeam";
const REACTOR_ACTIVATE: &str = "PS_CreatureCrystal_Activate";
const REACTOR_IMPACT_VFX: &str = "PS_CreatureCrystal_Impact";
const REACTOR_DESTROY: &str = "PS_CreatureCrystal_Destroy";

#[derive(Debug)]
struct Reactor {
    crystal: Option<SystemId>,
    /// The actor's age the rule last looked at.
    seen: i32,
    /// The last WarningVisualsMulticast's attached warning (the ones before it have completed).
    warn: Option<SystemId>,
    /// BeamParticleComponentHackUE420Broken.
    beam: Option<SystemId>,
    /// The boom's P (the impact's K2Node_CustomEvent_Hit_Location).
    at: V3,
}

/// SpawnEmitterAtLocation(ps, loc, rotation 0, scale 1, auto destroy) with the params given.
fn spawn_at(c: &mut Creature, fx: &mut Fx, ps: &str, loc: V3, params: &[(&str, f64)]) -> Option<SystemId> {
    if !c.has(fx, ps) {
        return None;
    }
    fx.player.spawn(ps, loc, [0.0; 3], [1.0; 3], params)
}

/// LineTraceSingle(TraceTypeQuery 3) from (x, y) straight down 20000 (the sim's trace_down over the stage block and
/// what blocks a trace, boxes and polygon solids, and the stage meshes' complex segments as the sim's seismic_boom):
/// the hit's Location, else the trace's end.
fn trace_to(w: &World, x: f64, y: f64) -> f64 {
    let mut shapes = w.blocker_shapes_read(mvs_sim::world::spawned_platform::BlockKind::Trace);
    shapes.boxes.insert(0, w.stage.ground);
    let (nx, ny) = (mvs_sim::math::narrow(x), mvs_sim::math::narrow(y));
    let hit = match (
        trace_down(shapes.view(), nx, ny),
        complex_down(w.data.stage.complex_segments.as_deref().unwrap_or(&[]), nx, ny),
    ) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    f64::from(hit.unwrap_or(mvs_sim::math::narrow(y - 20000.0)))
}

fn reactor_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let team = team_index(fx, pr.owner_id);
    let crystal = component(fx, w, st, REACTOR_CRYSTAL, Some(team));
    me(c).reactors.insert(st.seq, Reactor { crystal, seen: pr.age, warn: None, beam: None, at: [0.0; 3] });
}

/// The reactor's events since the rule last looked.
fn reactor_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(mut o) = s.reactors.remove(&st.seq) else { return };
    reactor_events(s, &mut o, fx, w, st);
    s.reactors.insert(st.seq, o);
}

fn reactor_events(s: &mut Creature, o: &mut Reactor, fx: &mut Fx, w: &World, st: &ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let ProjectileExt::SeismicReactor(r) = &pr.ext else { return };
    let (from, a) = (o.seen, pr.age);
    if a <= from {
        return;
    }
    o.seen = a;
    let charge = f64::from(r.charge_at);
    let due = |k: f64| f64::from(from) < charge + k && charge + k <= f64::from(a);
    let id = pr.owner_id;
    let team = team_index(fx, id);
    let (x, y) = (st.x, st.y);
    if due(f64::from(at_warn())) {
        // WarningVisualsMulticast @14496-15509
        o.warn = attached_to(s, fx, w, st, REACTOR_WARNING, "SM_SeismicReactor", team);
        // SeismicSwayOn (Perk_Creature_AirSupport, SeismicReactor.sway) pops the flow at @14915: neither of the two.
        if !r.sway {
            spawn_at(s, fx, REACTOR_GROUND, [0.0, -x, trace_to(w, x, y)], &[("Team", team)]);
            spawn_at(s, fx, REACTOR_BOLT_WARNING, [0.0, -x, y], &[("Team", team)]);
        }
    }
    if due(f64::from(at_boom())) {
        // Seismic Boom Visuals(P) @11432-12757
        o.at = [0.0, -x, trace_to(w, x, y)];
        // (SetVectorParameter: no FColor round trip)
        let params = [("Team.X", 1.0), ("Team.Y", 1.0), ("Team.Z", team)];
        let beam = spawn_at(s, fx, REACTOR_BEAM, [0.0; 3], &params);
        if let Some(h) = beam {
            fx.player.set_beam_source_point(h, 0, [0.0, -x - 20.0, y], 0);
            fx.player.set_beam_target_point(h, 0, o.at, 0);
        }
        o.beam = beam;
        spawn_at(s, fx, REACTOR_ACTIVATE, [0.0, -x, y - 100.0], &[("Team", team)]);
    }
    if due(f64::from(at_boom() + latent_frames(0.1))) {
        spawn_at(s, fx, REACTOR_IMPACT_VFX, o.at, &[("Team", team)]); // @1188
    }
}

fn reactor_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).reactors.remove(&st.seq) else { return };
    // (the crystal and the attached warning go with the actor; ReceiveEndPlay @10341 the beam)
    for h in [o.crystal, o.warn, o.beam].into_iter().flatten() {
        fx.player.kill(h);
    }
}

fn reactor_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).reactors.get(&st.seq).map_or(Vec::new(), |o| [o.crystal, o.beam].into_iter().flatten().collect())
}

/// DestroyVisuals @13370: PS_CreatureCrystal_Destroy at the actor.
fn reactor_destroy(c: &mut CharFx, fx: &mut Fx, _w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, x, y, .. } = *e else { return false };
    let team = team_index(fx, fighter);
    spawn_at(me(c), fx, REACTOR_DESTROY, [0.0, f64::from(-x), f64::from(y)], &[("Team", team)]);
    true
}

// ---- TetherProjectile_C

// TetherProjectile_C (the sim: mvs-sim chars/creature/tether.rs): its ParticleSystem has no template.
//   PS_TetherProjectile and BeamParticleSystem (PS_TetherBeam) are under DamageBox with no relative transform and
//   bAutoManageAttachment, AutoAttachSocketName Gem, AutoAttachLocationRule SnapToTarget: by the auto-attachment rule
//   they are attached again at activation to DamageBox at Gem, a socket it does not have: they stay at the DamageBox,
//   the actor's place and rotation. Both are auto activated.
//   BeginPlay (@11423 -> 9805-10729): AttachBeam(self), TetherSourceVFX = SpawnEmitterAttached(PS_TetherPoints, the
//   owner's Mesh, Gem, 0, 0, scale 1, SnapToTarget, auto destroy) @9915, then Team Value = SelectFloat(0.6, the
//   Instigator's GetColorIndex, IsLocalOnlinePlayer), BeamParticleSystem.SetColorParameter(Color, MakeColor(0, 0,
//   Team Value, 1)) @10555, Team = Team Value on PS_TetherProjectile @10613 and TetherSourceVFX @10671.
//   AttachBeam (its Target argument is read nowhere in its bytecode): SetBeamSourcePoint(1 and 2, the Instigator's
//   Mesh.GetSocketLocation(Gem), 0) @228 / @372; ReceiveTick @12718 / @12862 the same. PS_TetherBeam has two emitters,
//   BeamGlow (0) and Elec (1): emitter 2 takes nothing and BeamGlow's UserSet source array stays empty, so its source
//   is its distribution (0, 0, 0) through the component, the tether's own place. Both BeamTarget ends are the Actor
//   method on 'BeamTarget', whose InstanceParameters entry has no actor and nothing sets one: the Target distribution
//   (0, 0, 0) through the component, the tether. So the drawn beam is Elec's, from the Gem to the tether.
//   The owner's HitStunStart / HitStunEnded (bound at BeginPlay @12148 / @12234) -> Color (1, 0, Team Value, 1)
//   @14600 / (0, 0, Team Value, 1) @14711. Timeline_0 (PlayFromStart @1513, from the 5 s link @1412; length 1, the
//   track EmissiveStrength = CurveFloat_0_1) -> Alpha @15143 each update.
//   SpawnAttachPoint @234: PS_TetherPoints on the root at (10, 0, 0), rotation 0, scale 1, KeepRelativeOffset, auto
//   destroy, Team = Conv_IntToFloat(GetColorIndex's index) @565 (no 0.6 select); kept by nothing: it goes with the
//   actor.
//   AttachFXMulticast @12922 (the attach to an ally): ConnectedVFX = PS_CreatureTetherAlly on the root, location 0,
//   KeepRelativeOffset, auto destroy @13375, Team = Team Value @13490; PS_TetherProjectile destroyed @13548.
//   PullAudioMulticast(ally, T) @13582: PS_CreatureTetherRibbon on the ally's Mesh, None, location 0,
//   KeepRelativeOffset, scale 1, auto destroy @13710, Team = Team Value @13828; Delay(T) @13886 -> @5161 destroys it.
//   ReceiveDestroyed @14924 -> @12431: TetherSourceVFX and ConnectedVFX destroyed; the actor's own components with it.
//   Cylinder (bVisible false) and TetherObject (SM_heart, bHiddenInGame) are not drawn.

const TETHER_TIP: &str = "PS_TetherProjectile";
const TETHER_POINTS: &str = "PS_TetherPoints";
const TETHER_ALLY: &str = "PS_CreatureTetherAlly";
const TETHER_RIBBON: &str = "PS_CreatureTetherRibbon";
const GEM: &str = "Gem";
/// SetBeamSourcePoint's emitter 1 (Elec); its emitter 2 does not exist.
const BEAM_ELEC: i32 = 1;
/// SpawnAttachPoint's location (10, 0, 0).
const POINT_X: f64 = 10.0;
/// ReceiveTick's TickInterval 0.033 at the fixed step: the ages 1, 3, 5, ... (as mvs-sim chars/creature/tether.rs
/// TICK_FRAMES: change both).
const TICK_FRAMES: i32 = 2;
/// Timeline_0's track EmissiveStrength: CurveFloat_0_1 of the TetherProjectile class export (every key RCIM_Linear),
/// length 1.
const BLINK_T: [f64; 10] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 1.0];
const BLINK_V: [f64; 10] = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
const BLINK_LEN: f64 = 1.0;

fn blink(t: f64) -> f64 {
    if t <= BLINK_T[0] {
        return BLINK_V[0];
    }
    for k in 1..BLINK_T.len() {
        if t > BLINK_T[k] {
            continue;
        }
        let (a, b) = (BLINK_T[k - 1], BLINK_T[k]);
        return BLINK_V[k - 1] + ((BLINK_V[k] - BLINK_V[k - 1]) * (t - a)) / (b - a);
    }
    BLINK_V[BLINK_V.len() - 1]
}

#[derive(Debug)]
struct Tether {
    /// Team Value.
    team: f64,
    tip: Option<SystemId>,
    beam: Option<SystemId>,
    /// TetherSourceVFX.
    source: Option<SystemId>,
    /// The age of the last ReceiveTick taken.
    tick_at: i32,
    /// The owner's hitstun as last seen (the two delegates' edges).
    stun: bool,
    /// The beam's Alpha as last set (1: the parameter's default, not set).
    alpha: f64,
    /// SpawnAttachPoint's PS_TetherPoints.
    pin: Option<SystemId>,
    pinned: bool,
    /// ConnectedVFX.
    connected: Option<SystemId>,
    attached: bool,
    ribbon: Option<SystemId>,
    /// 0 not spawned, 1 spawned, 2 its Delay(T) has destroyed it.
    ribbon_state: u8,
    /// The actor's age the ribbon's Delay(T) fires at (the sim's ribbon_at when the ribbon spawned).
    ribbon_due: f64,
    /// The actor's age as last seen.
    age: i32,
}

/// The owner's Gem socket's place now.
fn gem_at(fx: &Fx, w: &World, id: i32) -> Option<V3> {
    let f = fx.socket_follow(w, id, Some(GEM), [0.0; 3], &IDENTITY, [1.0; 3]);
    fx.place(w, &f).map(|a| a.loc)
}

fn beam_color(fx: &mut Fx, h: Option<SystemId>, stun: bool, team: f64) {
    let Some(h) = h else { return };
    let c = color_param([if stun { 1.0 } else { 0.0 }, 0.0, team]);
    fx.player.set_param(h, "Color.X", c[0]);
    fx.player.set_param(h, "Color.Y", c[1]);
    fx.player.set_param(h, "Color.Z", c[2]);
}

fn tether_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let i = pr.owner_id;
    let team = team_index(fx, i);
    let stun = pr.owner.is_some_and(|o| mvs_sim::fighter::in_hitstun(w, o));
    let tip = component(fx, w, st, TETHER_TIP, Some(team));
    let beam = component(fx, w, st, "BeamParticleSystem", None); // (BeginPlay sets Team on the tip and the source only)
    beam_color(fx, beam, false, team);
    // AttachBeam(self) @9805
    if let (Some(h), Some(g)) = (beam, gem_at(fx, w, i)) {
        fx.player.set_beam_source_point(h, BEAM_ELEC, g, 0);
    }
    // TetherSourceVFX @9915. SnapToTarget: the relative scale is Scale / the mesh's world scale, world scale 1.
    let mut source = None;
    if s.has(fx, TETHER_POINTS) {
        let mut f = fx.socket_follow(w, i, Some(GEM), [0.0; 3], &IDENTITY, [1.0; 3]);
        if let Follow::Socket { scale, .. } = &mut f {
            *scale = Scale::Fixed([1.0; 3]);
        }
        source = fx.attach_to_mesh(w, i, TETHER_POINTS, GEM, f, &[("Team", team)]);
    }
    let o = Tether {
        team,
        tip,
        beam,
        source,
        tick_at: 0,
        stun,
        alpha: 1.0,
        pin: None,
        pinned: false,
        connected: None,
        attached: false,
        ribbon: None,
        ribbon_state: 0,
        ribbon_due: -1.0,
        age: pr.age,
    };
    s.tethers.insert(st.seq, o);
    // (the shared path calls the rule's tick right after)
}

/// What the sim's state of this tick shows.
fn tether_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(mut o) = s.tethers.remove(&st.seq) else { return };
    tether_events(s, &mut o, fx, w, st);
    s.tethers.insert(st.seq, o);
}

fn tether_events(s: &mut Creature, o: &mut Tether, fx: &mut Fx, w: &World, st: &ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let ProjectileExt::TetherProjectile(t) = &pr.ext else { return };
    let a = pr.age;
    o.age = a;
    // ReceiveTick @12718: the source at the Gem, at the last tick's age
    let tick = if a >= 1 { a - ((a - 1) % TICK_FRAMES) } else { 0 };
    if tick > o.tick_at {
        o.tick_at = tick;
        if let (Some(h), Some(g)) = (o.beam, gem_at(fx, w, pr.owner_id)) {
            fx.player.set_beam_source_point(h, BEAM_ELEC, g, 0);
        }
    }
    // HitStunEffectOn / Off
    let stun = pr.owner.is_some_and(|f| mvs_sim::fighter::in_hitstun(w, f));
    if stun != o.stun {
        o.stun = stun;
        beam_color(fx, o.beam, stun, o.team);
    }
    // Timeline_0: from the frame after PlayFromStart one update a frame, position + delta, the last at its length
    // (the Delay(1.0)'s link comes after it: past that the timeline is over)
    if t.break_warning {
        let k = if t.at1 >= 0.0 { f64::from(a) - f64::from(t.at1 - latent_frames(1.0)) } else { f64::INFINITY };
        if k >= 1.0 {
            let v = blink((k * f64::from(mvs_sim::consts::DELTA_SECONDS)).min(BLINK_LEN));
            if v != o.alpha {
                o.alpha = v;
                if let Some(h) = o.beam {
                    fx.player.set_param(h, "Alpha", v);
                }
            }
        }
    }
    // SpawnAttachPoint (the 0.225 s link, Blocked, or the attach's)
    if t.pinned && !o.pinned {
        o.pinned = true;
        let team = f64::from(fx.color_index(pr.owner_id));
        o.pin = on_root(s, fx, w, st, TETHER_POINTS, POINT_X, team);
    }
    // AttachFXMulticast
    if t.ally.is_some() && !o.attached {
        o.attached = true;
        o.connected = on_root(s, fx, w, st, TETHER_ALLY, 0.0, o.team);
        if let Some(h) = o.tip.take() {
            fx.player.kill(h);
        }
    }
    // PullAudioMulticast's ribbon on the ally and its Delay(T)
    if o.ribbon_state == 0
        && t.pulled
        && t.ribbon_at >= 0.0
        && let Some(ally) = t.ally
    {
        o.ribbon_state = 1;
        o.ribbon_due = f64::from(t.ribbon_at);
        let v = w.fighter(ally).id;
        if s.has(fx, TETHER_RIBBON) {
            let f = fx.socket_follow(w, v, None, [0.0; 3], &IDENTITY, [1.0; 3]);
            o.ribbon = fx.attach_to_mesh(w, v, TETHER_RIBBON, "None", f, &[("Team", o.team)]);
        }
    }
    if o.ribbon_state == 1 && t.ribbon_at < 0.0 {
        o.ribbon_state = 2;
        if let Some(h) = o.ribbon.take() {
            fx.player.kill(h);
        }
    }
}

/// The actor's end: its own components with it (PS_TetherProjectile, the beam, the pinned tether's points),
/// TetherSourceVFX and ConnectedVFX destroyed (@12467 / @12500). The ribbon is the ally's: it goes when its Delay(T)
/// has fired (in the actor's last step, the pull's end); a tether destroyed ahead of it drops the Delay, and the ribbon
/// runs out on the ally (auto destroy). The actor has left the World here: the Delay has fired when the last step's
/// age (one past the age last seen, the sim's step adding it first) reached the due age.
// Unverified: the gone actor's last ribbon_at is not read. Here a destroy outside the actor's own step (one that
// does not add to the age) is taken as one step later, and a step whose destroy comes before its ribbon check (the
// ally's death) as one that fired it.
fn tether_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).tethers.remove(&st.seq) else { return };
    let fired = f64::from(o.age + 1) >= o.ribbon_due;
    if o.ribbon_state == 1
        && fired
        && let Some(h) = o.ribbon
    {
        fx.player.kill(h);
    }
    for h in [o.tip, o.beam, o.source, o.pin, o.connected].into_iter().flatten() {
        fx.player.kill(h);
    }
}

fn tether_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).tethers.get(&st.seq).map_or(Vec::new(), |o| [o.tip, o.beam].into_iter().flatten().collect())
}
