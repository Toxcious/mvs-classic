//! Rick's effects (the CharFx hooks and the classes' projectile rules): the portals'
//! systems, the blaster's muzzle and its reflect, the fart bomb's cloud
//! (`fartBomb`), the polymorph grenade's aura and PS_NinjaBomb and the AirFlap's PS_Meeseeks_Flapping (`rickActors`).
//! Their meshes are mvs-view's (chars/rick.rs), whose placing the portals and the cloud read through `mvs_view::rick`.
//! The aimer's skillshot marker is the HUD's; the aimer's gun has no effect.
//! Unverified: SpawnEmitterAtLocation's scale is a uniform number here: VFXAttach's world X scale (its Y
//!   and Z are within 1e-4 of it).
//! Deviation: the Outro emitter is spawned at the first step the actor is dead or gone; a Lab reset ends
//!   the portals without it.

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::actors::{ActorRef, ProjectileId};
use mvs_sim::events::Event;
use mvs_sim::fighter::common::actor_def;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, rotation_rows};
use mvs_view::actors::actor_rot;
use mvs_view::rick::{PortalAt, PortalLayout, bomb_frame, portal_at, portal_layout, portal_wpo};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::Fx;
use crate::fx::attach::{att, warn_once, xyz};

/// What Rick's effects keep between steps, by the actors' sequence numbers.
#[derive(Debug, Default)]
pub(crate) struct Rick {
    portals: IndexMap<i32, PortalFx>,
    /// The blaster shots: the owner's id the component's Team was last given for, ReflectState.pause as last seen.
    shots: IndexMap<i32, (i32, u8)>,
    /// The fart clouds by the bomb's sequence number.
    clouds: IndexMap<i32, Vec<Cloud>>,
    grenades: IndexMap<i32, [Option<SystemId>; 2]>,
    flaps: IndexMap<i32, Option<SystemId>>,
}

fn me(c: &mut CharFx) -> &mut Rick {
    match c {
        CharFx::Rick(s) => s,
        _ => unreachable!("a Rick rule entered by another module"),
    }
}

impl CharFxHooks for Rick {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_actors(fx, w);
        self.place_portals(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

impl Rick {
    fn clear(&mut self, fx: &mut Fx) {
        for (_, r) in self.portals.drain(..) {
            r.end(fx, false);
        }
        for h in self.grenades.drain(..).flat_map(|(_, g)| g).chain(self.flaps.drain(..).map(|(_, h)| h)).flatten() {
            fx.player.kill(h);
        }
        for h in self.clouds.drain(..).flat_map(|(_, c)| c).filter_map(|c| c.h) {
            fx.player.kill(h);
        }
        self.shots.clear();
    }
}

/// out = at + rot . (s * rel).
fn under(at: V3, rot: &Rows, s: V3, rel: V3) -> V3 {
    let (a, b, c) = (s[0] * rel[0], s[1] * rel[1], s[2] * rel[2]);
    std::array::from_fn(|k| at[k] + a * rot[k] + b * rot[3 + k] + c * rot[6 + k])
}

fn team_of(fx: &Fx, w: &World, x: &WorldActor) -> f64 {
    fx.team(w.fighter(x.owner).id)
}

// ---- the portals -------------------------------------------------------------------------------------

/// AutoTeleportPortal_C, VisualOnlyPortal_C, ExitPortalParent_C (mvs-sim chars/rick/portal.rs). The mesh and its
/// notes: mvs-view chars/rick.rs. Here:
/// - SpawnVFX (the class's) SpawnEmitterAttached to VFXAttach (SnapToTarget, auto destroy) with Team @5517: its world
///   scale 1 times the later scale-down (the portal's k) and the root's scale.
/// - MulticastFighterTeleportedEffects @18018: SpawnEmitterAtLocation(Entering ? PS_C020_PortalTransitionIN : _OUT, the
///   actor's location, rotation 0, scale 1) @18163, no Team; FighterThroughPortal's float WPO on PortalVFX @16456.
/// - PortalOutro @3476: SpawnEmitterAttached(PS_C020_PortalPreOutro, VFXAttach, (-50, 0, 0), SnapToTargetIncludingScale)
///   with Team @4108; destroyed when a partner cancels the outro (OnPortalCountUpdated @16695).
/// - ReceiveEndPlay: PortalVFX @15379 and OutroPortalVFX @15455 destroyed, then @5866-5962
///   SpawnEmitterAtLocation(PS_C020_Portal00_Outro, VFXAttach's world location, rotation and scale) with Team @6228.
const PRE_OUTRO: &str = "PS_C020_PortalPreOutro";
const OUTRO: &str = "PS_C020_Portal00_Outro";
const TRANSITION_IN: &str = "PS_C020_PortalTransitionIN";
const TRANSITION_OUT: &str = "PS_C020_PortalTransitionOUT";
/// @3788.
const OUTRO_LOC: V3 = [-50.0, 0.0, 0.0];

#[derive(Debug)]
struct PortalFx {
    spawn_tick: f64,
    lay: PortalLayout,
    /// PortalVFX, OutroPortalVFX.
    vfx: Option<SystemId>,
    pre: Option<SystemId>,
    /// The outro_tick PreOutro was spawned for (-1 none), the through_tick last seen, the WPO last set.
    outro_seen: f64,
    through_seen: f64,
    wpo: Option<f64>,
    /// The latest tick's places and Team.
    at: PortalAt,
    team: f64,
}

fn portal_of(x: &WorldActor) -> Option<&mvs_sim::chars::rick::portal::PortalActor> {
    match &x.state {
        WorldActorState::PortalActor(p) => Some(p),
        _ => None,
    }
}

impl PortalFx {
    fn make(fx: &mut Fx, w: &World, a: ActorRef, x: &WorldActor) -> Option<PortalFx> {
        let p = portal_of(x)?;
        let d = actor_def(w, a, x.cls);
        let cls = w.names.str(x.cls);
        let Some(lay) = d.and_then(|d| portal_layout(&d.components)) else {
            warn_once(format!("Rick: {cls} has no PortalMesh / VFXAttach in its data"));
            return None;
        };
        let empty = Props::default();
        let cp = d.map_or(&empty, |d| &d.p);
        let at = portal_at(&lay, p, cp, x.size_scale.map(f64::from), w.flow.tick);
        let team = team_of(fx, w, x);
        let mut r = PortalFx {
            spawn_tick: f64::from(p.spawn_tick),
            lay,
            vfx: None,
            pre: None,
            outro_seen: -1.0,
            through_seen: -1.0,
            wpo: None,
            at,
            team,
        };
        match cp.get("SpawnVFX").and_then(Value::as_str) {
            Some(sys) => {
                r.vfx = fx.player.spawn_attached(sys, &r.vfx_at(x), 1.0, &[("Team", team)]);
                if r.vfx.is_none() {
                    warn_once(format!("Rick: {sys} is not loaded"));
                }
            }
            None => warn_once(format!("Rick: {cls} has no SpawnVFX")),
        }
        Some(r)
    }

    /// PortalVFX on VFXAttach (no relative location): at the mesh, k times the root's scale.
    fn vfx_at(&self, x: &WorldActor) -> mvs_vfx::Attachment {
        att(self.at.mesh_loc, &self.lay.mesh_rot, Some([self.at.k * f64::from(x.size_scale.unwrap_or(1.0)); 3]))
    }

    fn pre_at(&self) -> mvs_vfx::Attachment {
        let loc = under(self.at.mesh_loc, &self.lay.mesh_rot, self.at.attach_scale, OUTRO_LOC);
        att(loc, &self.lay.mesh_rot, Some(self.at.attach_scale))
    }

    fn tick(&mut self, fx: &mut Fx, w: &World, a: ActorRef, x: &WorldActor) {
        let Some(p) = portal_of(x) else { return };
        let empty = Props::default();
        let cp = actor_def(w, a, x.cls).map_or(&empty, |d| &d.p);
        self.at = portal_at(&self.lay, p, cp, x.size_scale.map(f64::from), w.flow.tick);
        self.team = team_of(fx, w, x);
        if let Some(h) = self.vfx {
            fx.player.attach(h, &self.vfx_at(x));
        }
        if let Some(h) = self.pre {
            fx.player.attach(h, &self.pre_at());
        }
        // MulticastFighterTeleportedEffects
        if p.through_tick >= 0.0 && f64::from(p.through_tick) != self.through_seen {
            self.through_seen = f64::from(p.through_tick);
            let sys = if p.through_in { TRANSITION_IN } else { TRANSITION_OUT };
            fx.player.spawn(sys, [0.0, f64::from(-p.x), f64::from(p.y)], [0.0; 3], [1.0; 3], &[]);
        }
        if let Some(v) = portal_wpo(p, w.flow.tick)
            && self.wpo != Some(v)
        {
            self.wpo = Some(v);
            if let Some(h) = self.vfx {
                fx.player.set_param(h, "WPO", v);
            }
        }
        // PortalOutro, and its cancel
        if p.outro_tick >= 0.0 && f64::from(p.outro_tick) != self.outro_seen {
            self.outro_seen = f64::from(p.outro_tick);
            if let Some(h) = self.pre.take() {
                fx.player.kill(h);
            }
            self.pre = fx.player.spawn_attached(PRE_OUTRO, &self.pre_at(), 1.0, &[("Team", self.team)]);
        } else if p.outro_tick < 0.0 && self.outro_seen >= 0.0 {
            self.outro_seen = -1.0;
            if let Some(h) = self.pre.take() {
                fx.player.kill(h);
            }
        }
    }

    /// ReceiveEndPlay: the two components destroyed, the Outro emitter at VFXAttach (`outro`: with it).
    fn end(self, fx: &mut Fx, outro: bool) {
        for h in [self.vfx, self.pre].into_iter().flatten() {
            fx.player.kill(h);
        }
        if outro {
            let s = [self.at.attach_scale[0]; 3];
            fx.player.spawn(OUTRO, self.at.mesh_loc, [0.0, -180.0, 0.0], s, &[("Team", self.team)]);
        }
    }
}

impl Rick {
    fn place_portals(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (id, x) in w.actors.actors.iter() {
            let Some(p) = portal_of(x) else { continue };
            if x.dead {
                continue;
            }
            let a = ActorRef::Actor(id);
            if let Some(r) = self.portals.get(&x.seq)
                && r.spawn_tick != f64::from(p.spawn_tick)
            {
                // (another portal under the same number after a rollback)
                let r = self.portals.shift_remove(&x.seq).expect("the portal just read");
                r.end(fx, false);
            }
            if !self.portals.contains_key(&x.seq) {
                let Some(r) = PortalFx::make(fx, w, a, x) else { continue };
                self.portals.insert(x.seq, r);
            }
            seen.push(x.seq);
            let r = self.portals.get_mut(&x.seq).expect("the portal just made");
            r.tick(fx, w, a, x);
        }
        let gone: Vec<i32> = self.portals.keys().filter(|k| !seen.contains(k)).copied().collect();
        for k in gone {
            let r = self.portals.shift_remove(&k).expect("a gone portal");
            r.end(fx, true);
        }
    }
}

// ---- the blaster shots -----------------------------------------------------------------

/// C020_BlasterProjectileWeak / _Charged: the ParticleSystem is the shared path's.
/// - SetTeamColor @440 -> DoOnce @132..157 -> @263 SpawnEmitterAttached(PS_Laser_Muzzle, the Instigator's Mesh, Root,
///   (0, 0, 300), rotation 0, scale 1, KeepRelativeOffset, auto destroy) with Team @381: once (made).
/// - MulticastReflect @1174 -> @40 ParticleSystem.Activate(true) @79 with the new owner's Team (SAP MulticastReflect's
///   SetTeamColor @791): the component made anew.
// Unverified: a reflect is seen as a new owner or as the reflect pause opening (ReflectState.pause leaving 0); a second
//   reflect by the same owner while the pause runs is not seen. Nothing of it shows (LOD0 off).
const BLASTERS: [&str; 2] = ["C020_BlasterProjectileWeak_C", "C020_BlasterProjectileCharged_C"];
const MUZZLE: &str = "PS_Laser_Muzzle";

fn shot_owner(w: &World, st: &ProjFxState) -> Option<(i32, u8)> {
    let pr = w.actors.projectiles.get(st.id)?;
    let o = pr.owner?;
    Some((w.fighter(o).id, pr.reflect.as_ref().map_or(0, |r| r.pause)))
}

fn blaster_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some((i, pause)) = shot_owner(w, st) else { return };
    let f = fx.socket_follow(w, i, Some("Root"), [0.0, 0.0, 300.0], &IDENTITY, [1.0; 3]);
    let team = fx.team(i);
    fx.attach_to_mesh(w, i, MUZZLE, "Root", f, &[("Team", team)]);
    me(c).shots.insert(st.seq, (i, pause));
}

fn blaster_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some((i, pause)) = shot_owner(w, st) else { return };
    let Some(o) = me(c).shots.get_mut(&st.seq) else { return };
    let reflected = i != o.0 || (pause != 0 && o.1 == 0);
    o.1 = pause;
    if !reflected {
        return;
    }
    o.0 = i;
    // @40 Activate(true) with the new owner's Team (SetTeamColor @791)
    fx.remake(w, st);
}

fn blaster_end(c: &mut CharFx, _fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    me(c).shots.shift_remove(&st.seq);
}

const BLASTER_RULE: ProjRule =
    ProjRule { made: Some(blaster_made), tick: Some(blaster_tick), end: Some(blaster_end), ..ProjRule::NONE };

// ---- the fart bomb's cloud ------------------------------------------------------------

/// FartBomb_SpawnedActor_C: EffectMulticast @3465 SpawnEmitterAttached(PS_C020_FartCloud, Mesh, no socket, (0, 0, 100),
/// rotation 0, scale 1, KeepRelativeOffset, auto destroy) with Team @3810; under the mirrored actor its world scale is
/// (1, -1, 1) facing left. The sim raises it as a projectileDeath event at a fixed place; the rule takes it and attaches
/// it to the bomb's Mesh.
// Unverified: after Mesh.K2_DestroyComponent the cloud is taken as detached and left running where it was (it follows
//   the mesh until then); the actor's K2_DestroyActor 0.6 s later is taken to destroy it.
const MISSILE: &str = "c020_WeaponModeMissile_C";
const FARTBOMB: &str = "FartBomb_SpawnedActor_C";
const CLOUD: &str = "PS_C020_FartCloud";
/// @3465's location under Mesh.
const CLOUD_LOC: V3 = [0.0, 0.0, 100.0];

#[derive(Debug)]
struct Cloud {
    h: Option<SystemId>,
}

/// The cloud's place on bomb `id` (None after the Mesh's DestroyComponent: left where it was).
fn cloud_at(w: &World, id: ProjectileId) -> Option<mvs_vfx::Attachment> {
    let pr = w.actors.projectiles.get(id)?;
    let ProjectileExt::FartBomb(b) = &pr.ext else { return None };
    if b.mesh_gone {
        return None;
    }
    let rel = actor_def(w, ActorRef::Projectile(id), pr.cls)
        .and_then(|d| d.components.get("Mesh")?.as_obj()?.get("RelativeLocation"))
        .map_or([0.0; 3], |v| xyz(Some(v), 0.0));
    let m = bomb_frame(
        &actor_rot(w, pr),
        f64::from(pr.scale),
        f64::from(pr.facing),
        pr.size_scale.map(f64::from),
        f64::from(pr.x),
        f64::from(pr.y),
        rel,
    );
    Some(att(under(m.loc, &m.rot, m.scale, CLOUD_LOC), &m.rot, Some(m.scale)))
}

fn cloud_emitter(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, x, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == CLOUD) {
        return false;
    }
    // (the bomb that detonated: the owner's live one nearest the event's x, the actor's at the Detonate; a
    // ReceiveAnyDamage ahead of the bomb's move of the tick raises it before the move)
    let mut best: Option<(ProjectileId, f64)> = None;
    for (id, pr) in w.actors.projectiles.iter() {
        if pr.dead || w.names.str(pr.cls) != FARTBOMB || pr.owner.map(|o| w.fighter(o).id) != Some(fighter) {
            continue;
        }
        let d = (pr.x - x).abs();
        if best.is_none_or(|b| d < mvs_sim::math::narrow(b.1)) {
            best = Some((id, f64::from(d)));
        }
    }
    let Some((id, _)) = best else {
        warn_once("Rick: the fart cloud found no bomb: drawn at the event".to_owned());
        return false;
    };
    let seq = w.actors.projectiles.get(id).map_or(-1, |p| p.seq);
    let team = fx.team(fighter);
    let h = cloud_at(w, id).and_then(|at| fx.player.spawn_attached(CLOUD, &at, 1.0, &[("Team", team)]));
    me(c).clouds.entry(seq).or_default().push(Cloud { h });
    true
}

fn bomb_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(list) = me(c).clouds.get(&st.seq) else { return };
    let Some(at) = cloud_at(w, st.id) else { return };
    for h in list.iter().filter_map(|c| c.h) {
        fx.player.attach(h, &at);
    }
}

fn bomb_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    for h in me(c).clouds.shift_remove(&st.seq).unwrap_or_default().into_iter().filter_map(|c| c.h) {
        fx.player.kill(h);
    }
}

const BOMB_RULE: ProjRule =
    ProjRule { own_emitter: Some(cloud_emitter), tick: Some(bomb_tick), end: Some(bomb_end), ..ProjRule::NONE };

/// The classes' own effects, by class name. The missile's trail is the shared path's
/// ParticleSystem with Team.
pub(crate) const RULES: &[(&str, ProjRule)] =
    &[(BLASTERS[0], BLASTER_RULE), (BLASTERS[1], BLASTER_RULE), (MISSILE, ProjRule::NONE), (FARTBOMB, BOMB_RULE)];

// ---- the grenade and the AirFlap ----------------------------------------------------

/// PolymorphGrenade_C (World.actors): ParticleSystem PS_C020PolymorphSatchelAura under the Capsule, RelativeScale3D
/// 0.82, Team @3878. BeginPlay @3936: SpawnEmitterAttached(PS_NinjaBomb, Box, ...) with Team @4196: Box under MineMesh
/// (scale 3) at (0, 0, 2.5), RelativeScale3D (0.75, 0.75, 1): the actor + R(body) . (0, 0, 7.5), world scale
/// (2.25, 2.25, 3) (its LOD levels all disabled: nothing shows). The actor's rotation: the spawn's yaw (the thrower's
/// facing) under the welded body's turn in the play plane (`angle`, about UE X). K2_DestroyActor takes both with it.
/// MeeSeeks_AirFlap_C (World.actors): PS_Meeseeks_Flapping under the SkeletalMesh at (0, 0, 375), no Team, under the
/// mesh's mirror (SetRelativeScale3D (1, -1, 1) facing left @259).
const GRENADE: &str = "PolymorphGrenade_C";
const AIRFLAP: &str = "MeeSeeks_AirFlap_C";
const NINJA: &str = "PS_NinjaBomb";
const FLAPPING: &str = "PS_Meeseeks_Flapping";
/// Box's Z under MineMesh (scale 3), its world scale.
const BOX_UP: f64 = 3.0 * 2.5;
const BOX_SCALE: V3 = [3.0 * 0.75, 3.0 * 0.75, 3.0];

fn right() -> Rows {
    rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0)
}

fn left() -> Rows {
    rotation_rows(0.0, std::f64::consts::FRAC_PI_2, 0.0)
}

/// Rows `rot` turned by `angle` about UE X (the sim plane's counter-clockwise turn: Y' = c Y + s Z, Z' = -s Y + c Z).
fn turn_x(rot: &Rows, angle: f64) -> Rows {
    let (s, c) = angle.sin_cos();
    let mut out = *rot;
    for r in 0..3 {
        let (y, z) = (rot[3 * r + 1], rot[3 * r + 2]);
        out[3 * r + 1] = c * y + s * z;
        out[3 * r + 2] = -s * y + c * z;
    }
    out
}

fn comp<'w>(w: &'w World, a: ActorRef, x: &WorldActor, n: &str) -> Option<&'w Props> {
    actor_def(w, a, x.cls)?.components.get(n)?.as_obj()
}

/// The grenade's aura and PS_NinjaBomb places.
fn grenade_at(
    w: &World,
    a: ActorRef,
    x: &WorldActor,
    g: &mvs_sim::chars::rick::grenade::PolymorphGrenade,
) -> [mvs_vfx::Attachment; 2] {
    let body = turn_x(&if g.facing < 0.0 { left() } else { right() }, f64::from(g.angle));
    let k = x.size_scale.unwrap_or(1.0);
    let s = xyz(comp(w, a, x, "ParticleSystem").and_then(|c| c.get("RelativeScale3D")), 1.0);
    let aura = att([0.0, f64::from(-g.x), f64::from(g.y)], &body, Some(s.map(|v| v * f64::from(k))));
    let ninja = att(
        under([0.0, f64::from(-g.x), f64::from(g.y)], &body, [f64::from(k); 3], [0.0, 0.0, BOX_UP]),
        &body,
        Some(BOX_SCALE.map(|v| v * f64::from(k))),
    );
    [aura, ninja]
}

/// The AirFlap's PS_Meeseeks_Flapping place and template.
fn flap_at(
    w: &World,
    a: ActorRef,
    x: &WorldActor,
    f: &mvs_sim::chars::rick::air_flap::AirFlap,
) -> Option<mvs_vfx::Attachment> {
    let p = comp(w, a, x, FLAPPING)?;
    let ms = xyz(comp(w, a, x, "SkeletalMesh").and_then(|c| c.get("RelativeScale3D")), 1.0);
    let rel = xyz(p.get("RelativeLocation"), 0.0);
    let is_left = f.facing < 0.0;
    let k = x.size_scale.unwrap_or(1.0);
    let s = [ms[0] * f64::from(k), if is_left { -ms[1] } else { ms[1] } * f64::from(k), ms[2] * f64::from(k)];
    let rot = if is_left { left() } else { right() };
    Some(att(under([0.0, f64::from(-f.x), f64::from(f.y)], &rot, s, rel), &rot, Some(s)))
}

impl Rick {
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (id, x) in w.actors.actors.iter() {
            if x.dead {
                continue;
            }
            let a = ActorRef::Actor(id);
            match &x.state {
                WorldActorState::PolymorphGrenade(g) if w.names.str(x.cls) == GRENADE => {
                    seen.push(x.seq);
                    let at = grenade_at(w, a, x, g);
                    let hs = match self.grenades.get(&x.seq) {
                        Some(hs) => *hs,
                        None => {
                            let team = team_of(fx, w, x);
                            let aura = comp(w, a, x, "ParticleSystem")
                                .and_then(|c| c.get("Template"))
                                .and_then(Value::as_str)
                                .map(str::to_owned);
                            let ha = aura
                                .as_deref()
                                .and_then(|t| fx.player.spawn_attached(t, &at[0], 1.0, &[("Team", team)]));
                            if ha.is_none() {
                                warn_once(format!("Rick: the grenade aura is not loaded: {aura:?}"));
                            }
                            let hn = fx.player.spawn_attached(NINJA, &at[1], 1.0, &[("Team", team)]);
                            if hn.is_none() {
                                warn_once(format!("Rick: {NINJA} is not loaded"));
                            }
                            self.grenades.insert(x.seq, [ha, hn]);
                            continue;
                        }
                    };
                    for (h, at) in hs.into_iter().zip(&at) {
                        if let Some(h) = h {
                            fx.player.attach(h, at);
                        }
                    }
                }
                WorldActorState::AirFlap(f) if w.names.str(x.cls) == AIRFLAP => {
                    seen.push(x.seq);
                    let at = flap_at(w, a, x, f);
                    match self.flaps.get(&x.seq) {
                        Some(h) => {
                            if let (Some(h), Some(at)) = (h, &at) {
                                fx.player.attach(*h, at);
                            }
                        }
                        // (flapRec: no system for an AirFlap whose mesh is not drawn, its mesh or materials not
                        // loaded; it is not tried again)
                        None if !fx.actor_drawn(a) => {
                            self.flaps.insert(x.seq, None);
                        }
                        None => {
                            let t = comp(w, a, x, FLAPPING).and_then(|c| c.get("Template")).and_then(Value::as_str);
                            let h = t.zip(at.as_ref()).and_then(|(t, at)| fx.player.spawn_attached(t, at, 1.0, &[]));
                            if h.is_none() {
                                warn_once(format!("Rick: the AirFlap's {FLAPPING} is not loaded"));
                            }
                            self.flaps.insert(x.seq, h);
                        }
                    }
                }
                _ => {}
            }
        }
        // K2_DestroyActor takes the actor's components with it.
        self.grenades.retain(|k, hs| {
            let keep = seen.contains(k);
            if !keep {
                for h in hs.iter().flatten() {
                    fx.player.kill(*h);
                }
            }
            keep
        });
        self.flaps.retain(|k, h| {
            let keep = seen.contains(k);
            if !keep && let Some(h) = h {
                fx.player.kill(*h);
            }
            keep
        });
    }
}
