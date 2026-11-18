//! LeBron James's effects (the CharFx hooks and the classes'
//! projectile rules; the pose is mvs-view chars/lebron.rs).
//!
//! What the classes have to draw (decoded, nothing else is in them):
//!   BasketBallProjectile_Shot / _Pass: the ParticleSystem component of ParentProjectilePhysics (PS_C016Basketball_Proj
//!     under StaticMesh): the shared projectile effect's. No rule here.
//!   C016_SassBubble: its ParticleSystem components have no Template and StaticMesh no mesh; what is seen is the
//!     montage's PlayVFX notifies and SetVisuals @1366-2333 (from ActivateObject @1239): SpeechBubble =
//!     SpawnEmitterAtLocation(VSizeXY(GrowthScale) < 10 ? PS_C016_Text02 : PS_C016_Text, the Instigator's location +
//!     (80, 0, 150), his rotation, scale (1, 1, 1) or (1.5, 1.25, 1.25), auto destroy), its Team = SelectFloat(0.6,
//!     the owner's colour index, IsLocalOnlinePlayer). ReceiveEndPlay @2484: SpeechBubble.K2_DestroyComponent. The sim
//!     raises the spawn as a projectileDeath event of the class (at the Instigator's place + 150 up): the rule below
//!     draws it with the graph's depth offset, rotation and scale, and ends it with the actor.
//!   C016ProjectileBarrier: the parent's view (fx/shared/barrier.rs), entered for the class here (BARRIERS).
//!   DashToBasketball: a Capsule alone; its DashToBallVisuals trail is below. C016_DunkShockwave: the shared
//!     ShaggyShockwaveActor drawing (fx/chars/shaggy/shockwave.rs), entered for the class here (SHOCKWAVES).
//!   Buff_C016_DistanceShot: ExplosionVisuals @1998: SpawnEmitterAtLocation(PS_C016_PERK_Explosion, the
//!     ball's location, rot 0, scale 1) @2068; its Team: the cast of the ball (the buff's owner) to BPI_BaseFighter
//!     fails @2157, so SelectFloat(0.6, ColorValue 0.0, IsLocalOnline false) @2436-2459 -> 0.0 for anyone's ball.
//!
//! Buff_C016_Ball_Conditional.PickUpVisuals @418-1088 (ReceiveBeginPlay @369): SpawnEmitterAtLocation(
//! PS_BasketballPickup, the owner's location, rotation 0, scale 1.6) with Team = SelectFloat(0.6, the colour value,
//! IsLocalOnline) @488-762; an owner that is a BP_Fighter_Character: SpawnEmitterAttached(PS_C016Basketball_Recall, its
//! Mesh, fml_un_C_rotator, location 0, rotation 0, scale 1, KeepRelativeOffset) with the same Team @919-1088. The sim
//! has no event for the buff's BeginPlay: it is seen here as the buff being on a fighter that had none the tick
//! before.
//! Deviation: a spawn that replaces a live buff (CanOverride: the passive row given again by ResetFighterLogic to a
//! pawn that holds the ball) runs PickUpVisuals in the build and is not seen here.
//! Unverified: the buff a pawn has at the match's first tick draws nothing here, the one of a new pawn of a Lab reset
//! does.
//!
//! Buff_BasketballPass / Buff_IgnitedBasketballPass (on the team mate who caught his ball): BuffApplied -> SpawnVFX
//! @2934, BuffRemoved -> RemoveVFX @2959. SpawnVFX: a locally controlled owner's SpawnPopupText "Pass Received" (the
//! HUD's: mvs-ui hud.rs pass_pops); Orbiting_PS_Actor (ParticleToOrbit = PS_C016Basketball_Buff, the Ignited child's
//! PS_C016Basketball_PERK_Buff) @366-623; then SpawnEmitterAtLocation(PS_C016Basketball_Recall, the buff actor's
//! location, rot 0, scale 1) @670 with Team = SelectFloat(0.6, GetColorIndex, IsLocalOnlinePlayer) @739-943. RemoveVFX:
//! the orbit actor and the kept recall go @48 / @264-361. Orbiting_PS_Actor: its root absolute-rotation, attached to the
//! owner's mesh at VFX_Root @284, SpawnEmitterAttached(ParticleToOrbit, the root, (ActiveMeshVFXScale.X x 130, 0, the
//! owner's scaled capsule half height / SizeScale.Z x 0.5), rot 0, ActiveMeshVFXScale) @479; its RotatingMovement turns
//! the root at 350 deg/s about the world Z. ActiveMeshVFXScale is taken as 1.
//! Deviation: Orbiting_PS_Actor's OnSizeChanged and OnActiveMeshChanged are not modelled: the scale stays the socket's
//! world scale, with its sign dropped.
//! Unverified: the turn is 350 deg per sim tick of 1/60 s.
//!
//! DashToBallVisuals @4557-5136: SpawnEmitterAttached(PS_C016_TrailDunk, the owner's Mesh, socket Root, location 0,
//! scale 1, SnapToTarget) @4652, Team @5043 = the owner's team value; DestroyVisuals @1331-1781 K2_DestroyComponent
//! (gone at once). Drawn while the sim actor's `trail` is on and it lives.

use std::collections::HashMap;

use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::events::Event;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, V3, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::Fx;
use crate::fx::attach::att;

/// His barrier class on the shared barrier view, with the system its SetTeamColor spawns.
pub(crate) const BARRIERS: &[(&str, &str)] = &[("C016ProjectileBarrier_C", "PS_C016Defense")];
/// His shockwave class on the shared shockwave view.
pub(crate) const SHOCKWAVES: &[&str] = &["C016_DunkShockwave_C"];

const BUBBLE: &str = "C016_SassBubble_C";
/// SetVisuals' two systems with their scales (Temp_struct_Variable @1503, Temp_struct_Variable_1 @1534).
const TEXTS: [(&str, V3); 2] = [("PS_C016_Text02", [1.0, 1.0, 1.0]), ("PS_C016_Text", [1.5, 1.25, 1.25])];
/// The X of (80, 0, 150) @1665; the event's y has the 150.
const TEXT_DEPTH: f64 = 80.0;

const PERK_EXPLOSION: &str = "PS_C016_PERK_Explosion";

const PICKUP: &str = "PS_BasketballPickup";
const RECALL: &str = "PS_C016Basketball_Recall";
const RECALL_SOCKET: &str = "fml_un_C_rotator";

/// The pass buffs' orbiting systems, by buff class.
const PASS_PS: [(&str, &str); 2] = [
    ("Buff_BasketballPass_C", "PS_C016Basketball_Buff"),
    ("Buff_IgnitedBasketballPass_C", "PS_C016Basketball_PERK_Buff"),
];
const ORBIT_SOCKET: &str = "VFX_Root";
/// @180
const ORBIT_X: f64 = 130.0;
/// RotatingMovement RotationRate yaw, rad/s.
const ORBIT_RATE: f64 = 350.0 * std::f64::consts::PI / 180.0;
const DT: f64 = 1.0 / 60.0;

const TRAIL: &str = "PS_C016_TrailDunk";
const TRAIL_SOCKET: &str = "Root";

/// A fighter's pass buff visuals.
#[derive(Debug, Default)]
struct PassView {
    cls: Option<&'static str>,
    orbit: Option<SystemId>,
    recall: Option<SystemId>,
    yaw: f64,
    /// The orbit's height over the socket, unscaled: half the owner's capsule at SpawnVFX (the emitter's relative
    /// location, fixed at the spawn).
    z: f64,
}

/// What LeBron James's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Lebron {
    /// Per fighter id whether it held the ball the tick before (absent: not looked at yet).
    had: HashMap<i32, bool>,
    /// The first place of the match is behind us.
    started: bool,
    passes: HashMap<i32, PassView>,
    /// The dash actors' trails, by the actor's sequence number, and the tick they were seen at.
    trails: HashMap<i32, (Option<SystemId>, u64)>,
    tick: u64,
    /// The sass bubbles' SpeechBubble systems, by the bubble actor's sequence number.
    speech: HashMap<i32, SystemId>,
}

fn me(c: &mut CharFx) -> &mut Lebron {
    match c {
        CharFx::Lebron(s) => s,
        _ => unreachable!("a LeBron rule entered by another module"),
    }
}

fn kill(fx: &mut Fx, h: Option<SystemId>) {
    if let Some(h) = h {
        fx.player.kill(h);
    }
}

impl Lebron {
    fn remove_pass(fx: &mut Fx, p: &mut PassView) {
        kill(fx, p.orbit.take());
        kill(fx, p.recall.take());
        p.cls = None;
    }

    /// PickUpVisuals.
    fn pick_up_visuals(fx: &mut Fx, w: &World, id: i32, x: f64, y: f64) {
        let team = fx.team(id);
        fx.player.spawn(PICKUP, [0.0, -x, y], [0.0; 3], [1.6; 3], &[("Team", team)]);
        // (a view without the bone: the component itself, as an attach to a socket that is not there)
        let socket = if fx.rig(id).is_some_and(|(_, r)| r.has_bone(RECALL_SOCKET)) { RECALL_SOCKET } else { "None" };
        fx.attach_socket(w, id, RECALL, socket, [0.0; 3], &IDENTITY, [1.0; 3], &[("Team", team)]);
    }

    /// The pass buff's SpawnVFX.
    /// `pass`: the buff class and its orbiting system; `(x, y)`: the owner's place.
    fn spawn_pass(fx: &mut Fx, w: &World, id: i32, (x, y): (f64, f64), p: &mut PassView, pass: (&'static str, &str)) {
        let (cls, ps) = pass;
        p.cls = Some(cls);
        p.yaw = 0.0;
        p.z = w.by_id(id).map_or(0.0, |fid| {
            f64::from(mvs_sim::fighter::capsule_half(w, fid) / w.fighter(fid).body.size_scale * 0.5)
        });
        let team = if fx.team(id) == 0.6 { 0.6 } else { f64::from(fx.color_index(id)) };
        let at = Self::orbit_place(fx, w, id, 0.0, p.z);
        p.orbit = at.and_then(|at| fx.player.spawn_attached(ps, &at, 1.0, &[("Team", team)]));
        // (attachToMesh: in the mesh's AttachChildren; its place is set each tick by place())
        if let (Some(h), Some(pw)) = (p.orbit, fx.pawns.get_mut(&id)) {
            pw.mesh_psc.push((ps.to_string(), ORBIT_SOCKET.to_string(), h));
        }
        p.recall = fx.player.spawn(RECALL, [0.0, -x, y], [0.0; 3], [1.0; 3], &[("Team", team)]);
    }

    /// The orbit's place: the socket's location, turned by `yaw` about the world Z at ORBIT_X out and half the
    /// capsule at the spawn (`z`) up, at the socket's world scale (its sign dropped).
    fn orbit_place(fx: &Fx, w: &World, id: i32, yaw: f64, z: f64) -> Option<mvs_vfx::Attachment> {
        let f = fx.socket_follow(w, id, Some(ORBIT_SOCKET), [0.0; 3], &IDENTITY, [1.0; 3]);
        let s = fx.place(w, &f)?;
        let sc = s.scale.unwrap_or([1.0; 3]).map(f64::abs);
        let r = rotation_rows(0.0, yaw, 0.0);
        let (ox, oz) = (ORBIT_X * sc[0], z * sc[2]);
        let loc = [0, 1, 2].map(|k| s.loc[k] + ox * r[k] + oz * r[6 + k]);
        Some(att(loc, &r, Some(sc)))
    }

    /// DashToBasketball's trails: one per live actor with its trail on, gone with it.
    fn place_trails(&mut self, fx: &mut Fx, w: &World) {
        self.tick += 1;
        for (_, a) in w.actors.actors.iter() {
            let WorldActorState::DashToBasketball(d) = &a.state else { continue };
            if a.dead || !d.trail {
                continue;
            }
            let tick = self.tick;
            if let Some(t) = self.trails.get_mut(&a.seq) {
                t.1 = tick;
                continue;
            }
            let id = a.owner_id;
            let team = fx.team(id);
            let h = fx.attach_socket(w, id, TRAIL, TRAIL_SOCKET, [0.0; 3], &IDENTITY, [1.0; 3], &[("Team", team)]);
            self.trails.insert(a.seq, (h, tick));
        }
        let tick = self.tick;
        let gone: Vec<i32> = self.trails.iter().filter(|(_, t)| t.1 != tick).map(|(k, _)| *k).collect();
        for k in gone {
            if let Some((h, _)) = self.trails.remove(&k) {
                kill(fx, h);
            }
        }
    }

    fn clear(&mut self, fx: &mut Fx) {
        for (_, (h, _)) in self.trails.drain() {
            kill(fx, h);
        }
        for p in self.passes.values_mut() {
            Self::remove_pass(fx, p);
        }
        for (_, h) in self.speech.drain() {
            fx.player.kill(h);
        }
    }
}

impl CharFxHooks for Lebron {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_trails(fx, w);
        let ball = mvs_sim::chars::lebron::state::BALL;
        for fid in w.all_ids() {
            let f = w.fighter(fid);
            let id = f.id;
            let dead = f.life.dead != 0.0;
            let has = !dead && f.buffs.buffs.iter().any(|b| b.cls == ball && b.frames > 0.0);
            let had = self.had.get(&id).copied();
            if has && had != Some(true) && !(!self.started && had.is_none()) {
                Self::pick_up_visuals(fx, w, id, f64::from(f.body.x), f64::from(mvs_sim::fighter::actor_y(w, fid)));
            }
            self.had.insert(id, has);
            let pass = if dead {
                None
            } else {
                f.buffs.buffs.iter().filter(|b| b.frames > 0.0).find_map(|b| {
                    let c = w.names.str(b.cls);
                    PASS_PS.iter().find(|(k, _)| *k == c).copied()
                })
            };
            let p = self.passes.entry(id).or_default();
            if pass.map(|(c, _)| c) != p.cls {
                if p.cls.is_some() {
                    Self::remove_pass(fx, p);
                }
                if let Some(pass) = pass {
                    let at = (f64::from(f.body.x), f64::from(mvs_sim::fighter::actor_y(w, fid)));
                    Self::spawn_pass(fx, w, id, at, p, pass);
                }
            } else if p.cls.is_some() {
                p.yaw += ORBIT_RATE * DT;
            }
            // (the orbit follows its socket and turns: placed here, before the player's tick)
            if let Some(h) = p.orbit
                && let Some(at) = Self::orbit_place(fx, w, id, p.yaw, p.z)
            {
                fx.player.attach(h, &at);
            }
        }
        self.started = true;
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
        for v in self.had.values_mut() {
            *v = false;
        }
        self.started = true;
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (BUBBLE, ProjRule { own_emitter: Some(bubble_emitter), end: Some(bubble_end), ..ProjRule::NONE }),
    ("Buff_C016_DistanceShot_C", ProjRule { own_emitter: Some(distance_shot_emitter), ..ProjRule::NONE }),
];

/// ExplosionVisuals' PS_C016_PERK_Explosion at Team 0 (the shared spawn would take the owner's).
fn distance_shot_emitter(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, x, y, ox, scale, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == PERK_EXPLOSION) {
        return false;
    }
    fx.player.spawn(
        PERK_EXPLOSION,
        [f64::from(ox), f64::from(-x), f64::from(y)],
        [0.0; 3],
        [f64::from(scale); 3],
        &[("Team", 0.0)],
    );
    true
}

/// SetVisuals' SpeechBubble: on the owner's activated bubble (the newest, were there two); one gone within the tick
/// has destroyed the component with itself.
fn bubble_emitter(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, fighter, x, y, .. } = *e else { return false };
    let Some(sys) = vfx.map(|v| w.names.str(v)) else { return false };
    let Some(&(sys, scale)) = TEXTS.iter().find(|(s, _)| *s == sys) else { return false };
    let seq = w
        .actors
        .projectiles
        .iter()
        .filter(|(_, pr)| {
            w.names.str(pr.cls) == BUBBLE && pr.owner_id == fighter && pr.owner.is_some() && pr.activated && !pr.dead
        })
        .map(|(_, pr)| pr.seq)
        .last();
    let Some(seq) = seq else { return true };
    let s = me(c);
    if let Some(h) = s.speech.remove(&seq) {
        fx.player.kill(h);
    }
    let right = w.by_id(fighter).is_some_and(|f| w.fighter(f).body.facing_dir > 0.0);
    let h = std::f64::consts::FRAC_PI_2;
    let rot = rotation_rows(0.0, if right { -h } else { h }, 0.0);
    let at = att([TEXT_DEPTH, f64::from(-x), f64::from(y)], &rot, Some(scale));
    let team = fx.team(fighter);
    if let Some(h) = fx.player.spawn_attached(sys, &at, 1.0, &[("Team", team)]) {
        s.speech.insert(seq, h);
    }
    true
}

/// ReceiveEndPlay: SpeechBubble.K2_DestroyComponent.
fn bubble_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(h) = me(c).speech.remove(&st.seq) {
        fx.player.kill(h);
    }
}
