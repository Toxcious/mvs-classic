//! Marvin the Martian's effects (the projectile rules and the
//! UFO's particle components). His meshes (the UFO, the cannon blast's rocket, the flag) are mvs-view
//! chars/marvin.rs.
//!
//! The World actors (CharFxHooks::actors, once a step; a view's systems made at the first step that sees its actor and
//! killed with it):
//! - C018RocketExplosion_C, C018PerkRocketExplosion_C: ExplosionVFX (PS_C018_MissileHit01, auto activated, at the
//!   Capsule), Team = SelectFloat(0.6, the instigator's colour value, IsLocalOnline) @369 (C018RocketExplosion's
//!   graph; the perk class has no graph of its own), its scale the root's (the sim's size_scale). Unverified: its
//!   rotation is the facing's yaw at its first sight (the spawn rotation not read).
//! - C018DisintegrationRay_C, _Down_C: VFX (PS_C018_NAD_Laser) under the Capsule, its world place set to (0, Y, Z) of
//!   its own (@1934) and both beam source points (1, 2) at the actor's (0, Y, Z) + VFXOffset (@2157-2383, every tick
//!   @2471-2713): the sim's start_x / start_y and end_x / end_y. Color = MakeColor(100, 1, Team) (@4148-4200: a colour
//!   parameter, each channel clamped to 1). Unverified: its rotation is the facing's yaw at its first sight.
//! - C018_Platform_C, C018_PerkPlatform_C: the particle components, Team = SelectFloat(0.6, the owner's
//!   colour value, IsLocalOnline) (@10089-10256): PS_C018_SpaceshipArrival (auto activated, Activate(true) again at
//!   BeginPlay @11151) at ShipMesh + (0, 0, -200); PS_C018_SpaceShipDeath (Activate(true) by DestroyVisualsMulticast
//!   @19657: the sim's destroy_at) at ShipMesh + (0, 0, 70); PS_C018_SpaceshipDepart (Activate(false) by
//!   ExpiredVisualsMulticast @20362: the sim's life_end_at) at ShipMesh + (0, 0, -200). They go with the actor.
//!   MulticastDamageEffects (@18829-19406): with the hit react montage, SpawnEmitterAttached(PS_HitImpact_TEMPLATE,
//!   Capsule, (0, 100, 250), keep relative offset, auto destroy): UE world (100, 0, 250) from the actor, Team; drawn
//!   at the start of a HitReactFront / Back instance. The beam (BeamParticleSystem, PS_C018_SpaceshipLaser) under
//!   BeamVFXParent, which the SCS attaches to ShipMesh's socket shipRocketMount: shown by the sim's beam_on (and the
//!   ship's visibility); every tick (@13107-13714) both its source points 1 and 2 = the component's location +
//!   Normal(AimTarget's location - it) x 10000. Color = MakeColor(1, 0, Team) (@10365-10417), a colour parameter.
//!   Deviation: the hit particle's rotation (an EX_RotationConst not read) is the actor's; the beam's
//!   rotation is the ship's (the socket's own turn not taken: a beam's points are world points).
//! - Buff_C018HomingVictim's mark on any fighter: ReceiveBeginPlay @5094 -> @963: on authority the owner a
//!   BP_FighterCharacter and the Instigator's Comp_Actor_Destructible valid (@1309: else no emitter: the sim's
//!   marker None), SpawnEmitterAttached(PS_C018_SPD_AimCursor, the owner's root component, no socket, location 0,
//!   rotation 0, scale 1, KeepRelativeOffset, auto destroy) @1670, Team = the victim's own @1865-1971. No graph ends
//!   it: the system's own end (about a second). Seen as the buff's presence edge at a step with a marker. Deviation:
//!   a buff that ends and is applied again between two steps draws no second mark.
//!
//! The classes' rules:
//! - C018CannonBlast_C, C018PerkCannonBlast_C: Trail, PS Rocket Head and PS Armor Break under the rocket mesh under
//!   Visuals, whose RelativeScale3D = VLerp(0.4, 1, RocketVisualGrowTimeline) (@2089-2152) grows them (`parent`). Team
//!   on Trail and PS Rocket Head (@1621 / @1679). Deviation: the default rule puts it on PS Armor Break too (a value its
//!   emitters may not read). Unverified: the timeline counts the projectile's age (its ticks).
//! - C018Flag_C, C018AirFlag_C: at BeginPlay's SetTeamColor (@6846-7440): SpawnEmitterAttached(PassiveVFX =
//!   PS_SPD_FlagStatic, PlayerOverlap, keep relative offset, scale 0.13, MakeRotator(0, 0, SelectFloat(90, -90, actor
//!   yaw < 0)): that is the yaw), Team; its Size = Scene's X scale every tick (@2382: the sim's scene_scale) and its
//!   world scale 0.13 x the Scene's x the Capsule's. MulticastActivate (@5390-5684): SpawnEmitterAttached(ActivateVFX =
//!   PS_C018_SPN_Shield, PlayerOverlap, the same yaw, scale 0.31, auto destroy), Team. Both go with the actor.
//! - C018BubblePop_C: its PS_C018_BubblePop component (PS_C018_NAN_Projectile, scale 1.5), Team @215.
//! - Reset: the actors' and the marks' systems killed, their views dropped.

use std::collections::HashSet;

use indexmap::IndexMap;
use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::actor_montage::mesh_instances;
use mvs_sim::actors::projectile::{AttachParentOut, Projectile, attach_parent};
use mvs_sim::actors::world_actor::actor_p;
use mvs_sim::actors::{ActorId, ActorRef, ProjectileExt, WorldActor, WorldActorState};
use mvs_sim::chars::marvin::ufo::Ufo;
use mvs_sim::combat::buffs::{BuffExt, buff, buff_of};
use mvs_sim::fighter::common::{actor_def, actor_def_owner};
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{Rows, V3, compose_rows, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::Fx;
use crate::fx::attach::att;

const DEG: f64 = std::f64::consts::PI / 180.0;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;

const UFOS: [&str; 2] = [mvs_sim::chars::marvin::ufo::PLATFORM, mvs_sim::chars::marvin::perks::PERK_PLATFORM];
const EXPLOSIONS: [&str; 2] =
    [mvs_sim::chars::marvin::ufo::ROCKET_EXPLOSION, mvs_sim::chars::marvin::perks::PERK_ROCKET_EXPLOSION];
const RAYS: [&str; 2] = [mvs_sim::chars::marvin::ray::RAY, mvs_sim::chars::marvin::ray::RAY_DOWN];

const ARRIVE: &str = "PS_C018_SpaceshipArrival";
const DEATH: &str = "PS_C018_SpaceShipDeath";
const DEPART: &str = "PS_C018_SpaceshipDepart";
const BEAM: &str = "BeamParticleSystem";
const HIT_IMPACT: &str = "PS_HitImpact_TEMPLATE";
/// (0, 100, 250) under the Capsule (yaw -90).
const HIT_OFF: V3 = [100.0, 0.0, 250.0];
/// @13295.
const BEAM_LENGTH: f64 = 10000.0;
const MARK: &str = "PS_C018_SPD_AimCursor";
/// The flag's systems' scales (@7058, @5592).
const PASSIVE_SCALE: f64 = 0.13;
const ACTIVATE_SCALE: f64 = 0.31;

/// A system and the place it rides (UE world: its Attachment, updated each step).
#[derive(Debug)]
struct Rider {
    h: SystemId,
    /// Under the ship at (0, 0, z) (Some), else at the actor + HIT_OFF.
    z: Option<f64>,
}

/// `UfoView`'s particle side.
#[derive(Debug)]
struct UfoFx {
    team: f64,
    riders: Vec<Rider>,
    beam: Option<SystemId>,
    death: Option<String>,
    depart: Option<String>,
    hit: bool,
    /// The platform's Set Ship Aim Direction latch (as the mesh view's).
    aimed: bool,
    destroyed_seen: bool,
    expired_seen: bool,
    /// The hit react instances drawn: their montage keys (name, instance number).
    hits: HashSet<(mvs_sim::names::Name, u32)>,
    /// The World tick of the last step seen (the hit's window: the instances started since).
    last_tick: Option<i32>,
}

#[derive(Debug)]
enum ActorFx {
    Ufo(Box<UfoFx>),
    /// The explosion's or the ray's system and its rows (the facing's yaw at the spawn, kept).
    One(Option<SystemId>, Rows),
}

/// The flag's systems (FlagFx).
#[derive(Debug, Default)]
struct FlagFx {
    passive: Option<SystemId>,
    activate: Option<SystemId>,
    shown: bool,
}

/// What Marvin the Martian's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Marvin {
    /// By World actor seq.
    actors: IndexMap<i32, ActorFx>,
    /// By fighter index: the mark's buff seen at the last step, its system.
    marks: Vec<(bool, Option<SystemId>)>,
    /// By flag projectile seq.
    flags: IndexMap<i32, FlagFx>,
    warned: HashSet<String>,
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (mvs_sim::chars::marvin::ufo::CANNON_BLAST, BLAST),
    (mvs_sim::chars::marvin::perks::PERK_CANNON_BLAST, BLAST),
    (mvs_sim::chars::marvin::flag::FLAG, FLAG_FX),
    (mvs_sim::chars::marvin::flag::AIR_FLAG, FLAG_FX),
    (mvs_sim::chars::marvin::bubble::BUBBLE_POP, ProjRule { more: &["PS_C018_BubblePop"], ..ProjRule::NONE }),
];

const BLAST: ProjRule =
    ProjRule { more: &["Trail", "PS Rocket Head", "PS Armor Break"], parent: Some(grown_parent), ..ProjRule::NONE };

const FLAG_FX: ProjRule = ProjRule {
    made: Some(flag_made),
    tick: Some(flag_tick),
    end: Some(flag_end),
    systems: Some(flag_systems),
    ..ProjRule::NONE
};

fn me(c: &mut CharFx) -> &mut Marvin {
    match c {
        CharFx::Marvin(s) => s,
        _ => unreachable!("a Marvin rule entered by another module"),
    }
}

fn me_ref(c: &CharFx) -> &Marvin {
    match c {
        CharFx::Marvin(s) => s,
        _ => unreachable!("a Marvin rule entered by another module"),
    }
}

/// The particle components' attach parent (the rocket mesh under Visuals) with Visuals' scale of the tick.
fn grown_parent(_w: &World, pr: &Projectile, cs: &Props, name: &str) -> AttachParentOut {
    let mut p = attach_parent(cs, name);
    let g = mvs_view::marvin_growth(pr.age);
    p.loc = p.loc.map(|v| mvs_sim::math::narrow(f64::from(v) * g));
    p.scale = p.scale.map(|v| mvs_sim::math::narrow(f64::from(v) * g));
    p
}

impl Marvin {
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Marvin: {ps} is not loaded");
        }
        false
    }

    fn warn(&mut self, s: String) {
        if self.warned.insert(s.clone()) {
            eprintln!("warning: Marvin: {s}");
        }
    }

    /// A particle component's template of the actor's class (None: none, warned).
    fn template(&mut self, fx: &mut Fx, w: &World, x: &WorldActor, comp: &str) -> Option<String> {
        let cls = w.names.str(x.cls);
        let t = actor_def(w, ActorRef::Fighter(x.owner), x.cls)
            .and_then(|d| d.components.get(comp))
            .and_then(Value::as_obj)
            .and_then(|c| c.get("Template"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && *s != "None")
            .map(str::to_owned);
        match t {
            None => {
                self.warn(format!("no template of {cls} {comp}"));
                None
            }
            Some(t) => self.has(fx, &t).then_some(t),
        }
    }

    fn make(&mut self, fx: &mut Fx, w: &World, id: ActorId, x: &WorldActor) -> ActorFx {
        let cls = w.names.str(x.cls);
        let team = fx.team(x.owner_id);
        let k = x.size_scale.unwrap_or(1.0);
        match &x.state {
            WorldActorState::Ufo(u) => {
                let mut v = UfoFx {
                    team,
                    riders: Vec::new(),
                    beam: None,
                    death: None,
                    depart: None,
                    hit: self.has(fx, HIT_IMPACT),
                    aimed: false,
                    destroyed_seen: false,
                    expired_seen: false,
                    hits: HashSet::new(),
                    last_tick: None,
                };
                let (ship_loc, ship_rot) = ufo_ship(&mut v, u, f64::from(k));
                // the arrival at BeginPlay
                if let Some(t) = self.template(fx, w, x, ARRIVE) {
                    v.ship_fx(fx, &t, -200.0, ship_loc, &ship_rot, f64::from(k));
                }
                if let Some(t) = self.template(fx, w, x, BEAM) {
                    let c = color_param([1.0, 0.0, team]);
                    let at = att(ship_loc, &ship_rot, Some([f64::from(k); 3]));
                    let params = [("Team", team), ("Color.X", c[0]), ("Color.Y", c[1]), ("Color.Z", c[2])];
                    v.beam = fx.player.spawn_attached(&t, &at, 1.0, &params);
                    if let Some(h) = v.beam {
                        fx.player.set_visible(h, false);
                    }
                }
                v.death = self.template(fx, w, x, DEATH);
                v.depart = self.template(fx, w, x, DEPART);
                let _ = id;
                ActorFx::Ufo(Box::new(v))
            }
            WorldActorState::RocketExplosion(e) => {
                let rot = facing_rows(f64::from(e.facing));
                let Some(t) = self.template(fx, w, x, "ExplosionVFX") else { return ActorFx::One(None, rot) };
                let at = att([0.0, f64::from(-e.x), f64::from(e.y)], &rot, Some([f64::from(k); 3]));
                ActorFx::One(fx.player.spawn_attached(&t, &at, 1.0, &[("Team", team)]), rot)
            }
            WorldActorState::Ray(r) => {
                let rot = facing_rows(f64::from(r.facing));
                let Some(t) = self.template(fx, w, x, "VFX") else { return ActorFx::One(None, rot) };
                let c = color_param([100.0, 1.0, team]);
                let at = att([0.0, f64::from(-r.start_x), f64::from(r.start_y)], &rot, Some([f64::from(k); 3]));
                let params = [("Team", team), ("Color.X", c[0]), ("Color.Y", c[1]), ("Color.Z", c[2])];
                let h = fx.player.spawn_attached(&t, &at, 1.0, &params);
                if let Some(h) = h {
                    ray_points(fx, h, f64::from(r.end_x), f64::from(r.end_y));
                }
                ActorFx::One(h, rot)
            }
            _ => {
                self.warn(format!("{cls}: not a Marvin actor kind"));
                ActorFx::One(None, facing_rows(1.0))
            }
        }
    }

    /// `marvinActors.place`: each live actor's view made at its first sight and placed; the gone ones' killed.
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = HashSet::new();
        for (id, x) in w.actors.actors.iter() {
            let cls = w.names.str(x.cls);
            if x.dead || !(UFOS.contains(&cls) || EXPLOSIONS.contains(&cls) || RAYS.contains(&cls)) {
                continue;
            }
            seen.insert(x.seq);
            if !self.actors.contains_key(&x.seq) {
                let v = self.make(fx, w, id, x);
                self.actors.insert(x.seq, v);
            }
            let k = x.size_scale.unwrap_or(1.0);
            match (&x.state, self.actors.get_mut(&x.seq)) {
                (WorldActorState::Ufo(u), Some(ActorFx::Ufo(v))) => v.place(fx, w, id, x, u, f64::from(k)),
                (WorldActorState::RocketExplosion(e), Some(ActorFx::One(Some(h), rot))) => {
                    fx.player.attach(*h, &att([0.0, f64::from(-e.x), f64::from(e.y)], rot, Some([f64::from(k); 3])));
                }
                (WorldActorState::Ray(r), Some(ActorFx::One(Some(h), rot))) => {
                    fx.player.attach(
                        *h,
                        &att([0.0, f64::from(-r.start_x), f64::from(r.start_y)], rot, Some([f64::from(k); 3])),
                    );
                    ray_points(fx, *h, f64::from(r.end_x), f64::from(r.end_y));
                }
                _ => {}
            }
        }
        let gone: Vec<i32> = self.actors.keys().copied().filter(|s| !seen.contains(s)).collect();
        for s in gone {
            if let Some(v) = self.actors.shift_remove(&s) {
                v.kill(fx);
            }
        }
    }

    /// `markFx.place`.
    fn place_marks(&mut self, fx: &mut Fx, w: &World) {
        let victim = w.names.get(mvs_sim::chars::marvin::homing::HOMING_VICTIM);
        let ids = w.all_ids();
        if self.marks.len() < ids.len() {
            self.marks.resize(ids.len(), (false, None));
        }
        for (i, &f) in ids.iter().enumerate() {
            let e = victim.and_then(|c| buff_of(w, f, c)).and_then(|b| buff(w, b)).and_then(|b| match &b.ext {
                BuffExt::C018HomingVictim(e) => Some(e.marker.is_some()),
                _ => None,
            });
            let now = e.is_some();
            if now && !self.marks[i].0 && e == Some(true) {
                if let Some(h) = self.marks[i].1.take() {
                    fx.player.kill(h);
                }
                if self.has(fx, MARK) {
                    let id = w.fighter(f).id;
                    let h = fx.player.spawn_attached(MARK, &root_att(w, f), 1.0, &[("Team", fx.team(id))]);
                    self.marks[i].1 = h;
                }
            }
            self.marks[i].0 = now;
            match self.marks[i].1 {
                Some(h) if !fx.player.alive(h) => self.marks[i].1 = None,
                Some(h) => fx.player.attach(h, &root_att(w, f)),
                None => {}
            }
        }
    }

    fn clear(&mut self, fx: &mut Fx, kill: bool) {
        for (_, v) in self.actors.drain(..) {
            if kill {
                v.kill(fx);
            }
        }
        for m in &mut self.marks {
            if let Some(h) = m.1.take()
                && kill
            {
                fx.player.kill(h);
            }
            m.0 = false;
        }
    }
}

/// The yaw rows of a facing (right: yaw -90, left: 90).
fn facing_rows(facing: f64) -> Rows {
    rotation_rows(0.0, if facing > 0.0 { -HALF_PI } else { HALF_PI }, 0.0)
}

/// The ray's beam source points 1 and 2 at its end.
fn ray_points(fx: &mut Fx, h: SystemId, ex: f64, ey: f64) {
    let end = [0.0, -ex, ey];
    fx.player.set_beam_source_point(h, 1, end, 0);
    fx.player.set_beam_source_point(h, 2, end, 0);
}

/// A fighter's root component (the mark's parent): its actor place, yaw by its facing.
fn root_att(w: &World, f: mvs_sim::fighter::FighterId) -> mvs_vfx::Attachment {
    let fi = w.fighter(f);
    let rot = rotation_rows(0.0, if fi.body.facing_dir > 0.0 { -HALF_PI } else { HALF_PI }, 0.0);
    att([0.0, f64::from(-fi.body.x), f64::from(mvs_sim::fighter::actor_y(w, f))], &rot, None)
}

/// The ship component's place and rows (the aim latch updated).
fn ufo_ship(v: &mut UfoFx, u: &Ufo, k: f64) -> (V3, Rows) {
    if u.aim_on {
        v.aimed = true;
    }
    let yaw = if v.aimed { mvs_view::marvin_aim_yaw(u) } else { 0.0 };
    mvs_view::marvin_ship_place([0.0, f64::from(-u.x), f64::from(u.y)], yaw, k)
}

impl UfoFx {
    /// A particle component under ShipMesh at (0, 0, z) (the ship's yaw leaves Z alone), Team.
    fn ship_fx(&mut self, fx: &mut Fx, t: &str, z: f64, ship_loc: V3, ship_rot: &Rows, k: f64) {
        let at = att([ship_loc[0], ship_loc[1], ship_loc[2] + z * k], ship_rot, Some([k; 3]));
        if let Some(h) = fx.player.spawn_attached(t, &at, 1.0, &[("Team", self.team)]) {
            self.riders.push(Rider { h, z: Some(z) });
        }
    }

    fn place(&mut self, fx: &mut Fx, w: &World, id: ActorId, x: &WorldActor, u: &Ufo, k: f64) {
        let a = ActorRef::Actor(id);
        let at = [0.0, -u.x, u.y];
        let (ship_loc, ship_rot) = ufo_ship(self, u, k);
        // the graph's edges: the destroy and the expiry
        if u.destroy_at != 0.0 && !self.destroyed_seen {
            self.destroyed_seen = true;
            if let Some(t) = self.death.clone() {
                self.ship_fx(fx, &t, 70.0, ship_loc, &ship_rot, k);
            }
        }
        if u.life_end_at != 0.0 && !self.expired_seen {
            self.expired_seen = true;
            if let Some(t) = self.depart.clone() {
                self.ship_fx(fx, &t, -200.0, ship_loc, &ship_rot, k);
            }
        }
        // a hit react instance started in the ticks since the last step (beside the playing one: the sim's
        // montage_play_side): the particle (a new instance aged at most those ticks)
        let tick = w.flow.tick;
        let n = match self.last_tick {
            Some(t) if tick >= t => (tick - t).min(30),
            _ => 1,
        };
        self.last_tick = Some(tick);
        let p = actor_p(w, a);
        let name = |key: &str| p.get(key).and_then(Value::as_str).and_then(|s| w.names.get(s));
        let (front, back) = (name("HitReactFrontMontage"), name("HitReactBackMontage"));
        for st in mesh_instances(w, a) {
            let m = st.montage.name;
            if Some(m) != front && Some(m) != back {
                continue;
            }
            let key = (m, st.montage.uid);
            if self.hits.insert(key) && st.age <= f64::from(n) && self.hit {
                let loc = std::array::from_fn(|i| f64::from(at[i]) + HIT_OFF[i] * k);
                let rot = rotation_rows(0.0, -HALF_PI, 0.0);
                if let Some(h) = fx.player.spawn_attached(
                    HIT_IMPACT,
                    &att(loc.map(f64::from), &rot, Some([k; 3])),
                    1.0,
                    &[("Team", self.team)],
                ) {
                    self.riders.push(Rider { h, z: None });
                }
            }
        }
        // the riders
        self.riders.retain(|r| fx.player.alive(r.h));
        for r in &self.riders {
            let a = match r.z {
                Some(z) => att([ship_loc[0], ship_loc[1], ship_loc[2] + z * k], &ship_rot, Some([k; 3])),
                None => att(
                    std::array::from_fn(|i| f64::from(at[i]) + HIT_OFF[i] * k),
                    &rotation_rows(0.0, -HALF_PI, 0.0),
                    Some([k; 3]),
                ),
            };
            fx.player.attach(r.h, &a);
        }
        // the beam: at the mount (the mesh view's bone), its points the mount + Normal(AimTarget's location - it) x
        // 10000 (a None target: the origin)
        let Some(beam) = self.beam else { return };
        fx.player.set_visible(beam, !u.ship_hidden && u.beam_on);
        let mount = fx
            .actor_bones
            .get(&a)
            .and_then(|b| b.iter().find(|(n, _)| n.eq_ignore_ascii_case(mvs_view::MARVIN_UFO_MOUNT)))
            .map(|(_, x)| x.loc);
        let Some(m) = mount else { return };
        fx.player.attach(beam, &att(m, &ship_rot, Some([k; 3])));
        let t = u.aim_target.map_or([0.0; 3], |t| [0.0, -w.fighter(t).body.x, mvs_sim::fighter::actor_y(w, t)]);
        let mut d = [f64::from(t[0]) - m[0], f64::from(t[1]) - m[1], f64::from(t[2]) - m[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        d = if l > 1e-4 { d.map(|c| c / l) } else { [0.0; 3] };
        let p = std::array::from_fn(|i| m[i] + d[i] * BEAM_LENGTH);
        fx.player.set_beam_source_point(beam, 1, p, 0);
        fx.player.set_beam_source_point(beam, 2, p, 0);
        let _ = x;
    }
}

impl ActorFx {
    fn kill(self, fx: &mut Fx) {
        match self {
            ActorFx::Ufo(v) => {
                for r in v.riders {
                    fx.player.kill(r.h);
                }
                if let Some(h) = v.beam {
                    fx.player.kill(h);
                }
            }
            ActorFx::One(h, _) => {
                if let Some(h) = h {
                    fx.player.kill(h);
                }
            }
        }
    }
}

// ---- the flag ----

/// The components' place on PlayerOverlap: yaw SelectFloat(90, -90, the actor's yaw < 0) over the actor's rotation,
/// scale k x the Scene's x the Capsule's.
fn flag_att(w: &World, pr: &Projectile, st: &ProjFxState, k: f64) -> mvs_vfx::Attachment {
    let scene = match &pr.ext {
        ProjectileExt::C018Flag(f) => f.scene_scale,
        _ => 1.0,
    };
    let rot = mvs_view::actors::actor_rot(w, pr);
    let yaw = rot[1].atan2(rot[0]);
    let turn = rotation_rows(0.0, if yaw < 0.0 { 90.0 * DEG } else { -90.0 * DEG }, 0.0);
    let s = k * f64::from(scene) * f64::from(pr.scale);
    att([0.0, -st.x, st.y], &compose_rows(&turn, &rot), Some([s; 3]))
}

/// The flag's PassiveVFX / ActivateVFX template of the class's data (None: none, warned).
fn flag_system(m: &mut Marvin, fx: &mut Fx, w: &World, pr: &Projectile, prop: &str) -> Option<String> {
    let owner = pr.owner?;
    let from = actor_def_owner(w, owner, pr.cls);
    let t = actor_def(w, ActorRef::Fighter(from), pr.cls)
        .and_then(|d| d.p.get(prop))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && *s != "None")
        .map(str::to_owned);
    match t {
        None => {
            m.warn(format!("no {prop} of {}", w.names.str(pr.cls)));
            None
        }
        Some(t) => m.has(fx, &t).then_some(t),
    }
}

fn flag_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let m = me(c);
    let mut o = FlagFx::default();
    if let Some(t) = flag_system(m, fx, w, pr, "PassiveVFX") {
        let scene = match &pr.ext {
            ProjectileExt::C018Flag(f) => f.scene_scale,
            _ => 1.0,
        };
        let at = flag_att(w, pr, st, PASSIVE_SCALE);
        o.passive =
            fx.player.spawn_attached(&t, &at, 1.0, &[("Team", fx.team(pr.owner_id)), ("Size", f64::from(scene))]);
    }
    m.flags.insert(st.seq, o);
}

fn flag_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let ProjectileExt::C018Flag(f) = &pr.ext else { return };
    let m = me(c);
    let Some(mut o) = m.flags.shift_remove(&st.seq) else { return };
    if let Some(h) = o.passive {
        fx.player.set_param(h, "Size", f64::from(f.scene_scale));
        fx.player.attach(h, &flag_att(w, pr, st, PASSIVE_SCALE));
    }
    if f.flag_visible && !o.shown {
        o.shown = true;
        if let Some(t) = flag_system(m, fx, w, pr, "ActivateVFX") {
            let at = flag_att(w, pr, st, ACTIVATE_SCALE);
            o.activate = fx.player.spawn_attached(&t, &at, 1.0, &[("Team", fx.team(pr.owner_id))]);
        }
    }
    if let Some(h) = o.activate {
        fx.player.attach(h, &flag_att(w, pr, st, ACTIVATE_SCALE));
    }
    m.flags.insert(st.seq, o);
}

fn flag_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).flags.shift_remove(&st.seq) else { return };
    for h in [o.passive, o.activate].into_iter().flatten() {
        fx.player.kill(h);
    }
}

fn flag_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).flags.get(&st.seq).map_or_else(Vec::new, |o| [o.passive, o.activate].into_iter().flatten().collect())
}

impl CharFxHooks for Marvin {
    /// marvinFx's actors (once a step).
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_actors(fx, w);
    }

    /// marvinFx's place: the marks.
    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_marks(fx, w);
    }

    /// The Lab's reset: the actors' and the marks' systems killed, their views dropped. (The poses'
    /// reset is the view's: PoseHook::reset.)
    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx, true);
        self.flags.clear();
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx, true);
        for (_, o) in self.flags.drain(..) {
            for h in [o.passive, o.activate].into_iter().flatten() {
                fx.player.kill(h);
            }
        }
    }
}
