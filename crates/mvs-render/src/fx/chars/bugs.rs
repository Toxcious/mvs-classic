//! Bugs Bunny's effects: his
//! rockets' particle components, the tunnel's hole and dirt, Buff_Charm's head emitter. Offsets are each class's
//! ubergraph's. His pawn's props and his classes' meshes are mvs-view's (chars/bugs.rs).
//!
//! The first draw of his systems under the loading screen (warmSlots) is the match's warm-up (crate::warm): every
//! system of his owner's emitters made before the first frame.

use std::collections::HashMap;

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{AttachParentOut, Projectile, attach_parent};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::bugs::charm::charmed;
use mvs_sim::chars::bugs::perks::SHOCKWAVE;
use mvs_sim::chars::bugs::rockets::{ROCKET_F, ROCKET_UP, at_smoke};
use mvs_sim::chars::bugs::tunnel::{TUNNEL, Tunnel};
use mvs_sim::events::Event;
use mvs_sim::fighter::common::{actor_def, latent_frames};
use mvs_sim::names::Name;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::{Attachment, SystemId};
use mvs_view::actors::actor_rot;

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::Fx;
use crate::fx::attach::{mul_row, rot_rows, xyz};

/// What Bugs Bunny's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Bugs {
    /// RocketUp's smoke by the rocket's sequence number (None: not drawn).
    smoke: HashMap<i32, Option<Smoke>>,
    /// The tunnels' holes by the actor's sequence number.
    holes: HashMap<i32, Hole>,
    /// The dirt bursts the tunnels' Delay has yet to draw.
    pending: Vec<Pending>,
    /// Buff_Charm's head emitter by the charmed fighter's id (None: not loaded).
    charm: HashMap<i32, Option<SystemId>>,
    warned: std::collections::BTreeSet<String>,
}

fn me(c: &mut CharFx) -> &mut Bugs {
    match c {
        CharFx::Bugs(s) => s,
        _ => unreachable!("a Bugs Bunny rule entered by another module"),
    }
}

impl Bugs {
    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Bugs: {ps} is not loaded");
        }
        false
    }

    fn clear(&mut self, fx: &mut Fx) {
        for h in self.holes.drain().filter_map(|(_, r)| r.h) {
            fx.player.kill(h);
        }
        for h in self.charm.drain().filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
        // (a RocketUp the reset leaves in the World draws its smoke again from its next step)
        for s in self.smoke.drain().filter_map(|(_, s)| s) {
            fx.player.kill(s.h);
        }
        self.pending.clear();
    }
}

impl CharFxHooks for Bugs {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.tunnel_actors(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_charm(fx, w);
        self.place_pending(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (ROCKET_F, ProjRule { more: &["ParticleSystem1"], parent: Some(squash_parent), ..ProjRule::NONE }),
    (
        ROCKET_UP,
        ProjRule { more: &["ParticleSystem1"], tick: Some(smoke_tick), end: Some(smoke_end), ..ProjRule::NONE },
    ),
    // A World actor: entered for own_emitter only (MulticastDustVFX, the event of mvs-sim chars/bugs/tunnel.rs).
    (TUNNEL, ProjRule { own_emitter: Some(tunnel_dirt), ..ProjRule::NONE }),
    // Entered for own_emitter only: its BeginPlay burst's Team (mvs-sim chars/bugs/perks.rs shockwave_begin).
    (SHOCKWAVE, ProjRule { own_emitter: Some(shockwave_burst), ..ProjRule::NONE }),
];

/// TunnelShockwave_C ReceiveBeginPlay (off a dedicated server): SpawnEmitterAtLocation(PS_SupermanSPU_Impact, the
/// actor's location, rot 0, scale 1, auto destroy) @198, then Team = SelectFloat(0.6, Conv_IntToFloat(the
/// Instigator's Comp_Pawn_SkinManager GetColorIndex), IsLocalOnlinePlayer) @448-590. The Instigator is the
/// traveller, the event's `fighter`.
fn shockwave_burst(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx: Some(v), x, y, .. } = *e else { return false };
    if fighter < 0 {
        return false;
    }
    let team = if fx.team(fighter) == 0.6 { 0.6 } else { f64::from(fx.color_index(fighter)) };
    fx.player.spawn(w.names.str(v), [0.0, f64::from(-x), f64::from(y)], [0.0; 3], [1.0; 3], &[("Team", team)]);
    true
}

// ---- the rockets ----
// The shared path draws the class's ParticleSystem (PS_HeavyProjectile_ArmorBreak, Team from ParentProjectilePhysics
// SetTeamColor @1873); these rules add the rest.
// - The flame: ParticleSystem1 (PS_BugsBunnyRocket) under StaticMesh on both, at (0, -350, 0) (Up: -349.69), yaw 90,
//   scale 1, bStartWithTickEnabled false (drawn while the actor lives, as the other actors' particle components with
//   bStartWithTickEnabled false). StaticMesh.SetVisibility(true, false) @125 / @130 does not propagate: its children
//   are not hidden with it before AT_ON, so the flame is drawn from the spawn. Nothing writes its Team (SetTeamColor
//   writes ParticleSystem's alone; the system has no Team parameter, so the shared path's Team changes nothing).
// - RocketF's RocketSquash (@554, the timeline of mvs-view chars/bugs.rs's rocket mesh) sets StaticMesh's relative
//   scale: both particle components are its children and follow it (the parent rule).
// - RocketUp's smoke: Delay(7.0) @1764 -> @1819 SpawnEmitterAttached(PS_BugsBunnyRocket_DarkSmoke, ParticleSystem1,
//   no socket, location 0, rotation 0, scale 1, SnapToTarget, auto destroy): at ParticleSystem1's origin, turned with
//   it, at world scale 1 (SnapToTarget's relative scale is 1 over the parent's world scale); no Team parameter. Not
//   stored and never deactivated: it dies with the rocket (its attach parent's owner). Drawn from the first step whose
//   rocket age is at or past AT_SMOKE (a rollback that takes the age back below it leaves it in place).

const SMOKE: &str = "PS_BugsBunnyRocket_DarkSmoke";
const DEG: f64 = std::f64::consts::PI / 180.0;

/// `squashParent`: RocketF's StaticMesh as the attach parent of its particle components: the construction script's
/// transform with the squash timeline's scale from AT_ON (StaticMesh hangs from the root Capsule directly, so
/// attach_parent is its own transform).
fn squash_parent(_w: &World, pr: &Projectile, cs: &Props, name: &str) -> AttachParentOut {
    let mut p = attach_parent(cs, name);
    if pr.age >= mvs_view::rocket_at_on() {
        p.scale = mvs_view::rocket_squash_scale(pr.age, p.scale.map(f64::from)).map(mvs_sim::math::narrow);
    }
    p
}

/// RocketUp's smoke: its handle and ParticleSystem1's place under the actor's root (`smokeFollow`'s rel / relRot).
#[derive(Debug)]
struct Smoke {
    h: SystemId,
    rel: V3,
    rel_rot: Rows,
}

/// ParticleSystem1's place under the actor's root (its attach parent's transform and its own).
fn smoke_rel(cs: &Props) -> (V3, Rows) {
    let c = cs.get("ParticleSystem1").and_then(Value::as_obj);
    let par = attach_parent(cs, "ParticleSystem1");
    let pr: Rows = std::array::from_fn(|k| par.rot[k]).map(f64::from);
    let l = xyz(c.and_then(|c| c.get("RelativeLocation")), 0.0);
    let own = [f64::from(par.scale[0]) * l[0], f64::from(par.scale[1]) * l[1], f64::from(par.scale[2]) * l[2]];
    let d = mul_row(own.map(f64::from), &pr);
    let rel = std::array::from_fn(|k| f64::from(par.loc[k]) + d[k]);
    (rel.map(f64::from), compose_rows(&rot_rows(c.and_then(|c| c.get("RelativeRotation"))), &pr))
}

/// The smoke's world transform at world scale 1 (the SnapToTarget attachment), from the actor's place of the step.
fn smoke_at(w: &World, pr: &Projectile, st: &ProjFxState, s: (&V3, &Rows)) -> Attachment {
    let rot = actor_rot(w, pr);
    let d = mul_row(*s.0, &rot);
    let loc = [f64::from(pr.scale) * d[0], -st.x + f64::from(pr.scale) * d[1], st.y + f64::from(pr.scale) * d[2]];
    let r = compose_rows(s.1, &rot);
    Attachment { loc, rot: std::array::from_fn(|i| std::array::from_fn(|j| r[i * 3 + j])), scale: Some([1.0; 3]) }
}

fn smoke_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let b = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    match b.smoke.get(&st.seq) {
        Some(Some(s)) => {
            let at = smoke_at(w, pr, st, (&s.rel, &s.rel_rot));
            if fx.player.alive(s.h) {
                fx.player.attach(s.h, &at);
            }
            return;
        }
        Some(None) => return,
        None => {}
    }
    if f64::from(pr.age) < f64::from(at_smoke()) {
        return;
    }
    let Some(cs) = actor_def(w, ActorRef::Projectile(st.id), pr.cls).map(|d| &d.components) else {
        b.smoke.insert(st.seq, None);
        return;
    };
    let (rel, rel_rot) = smoke_rel(cs);
    let made = if b.has(fx, SMOKE) {
        let at = smoke_at(w, pr, st, (&rel, &rel_rot));
        fx.player.spawn_attached(SMOKE, &at, 1.0, &[]).map(|h| Smoke { h, rel, rel_rot }) // @1819
    } else {
        None
    };
    b.smoke.insert(st.seq, made);
}

fn smoke_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(Some(s)) = me(c).smoke.remove(&st.seq) {
        fx.player.kill(s.h);
    }
}

// ---- the tunnel ----
// BugsBunnyTunnelParent_C. Its pick-up widget is the HUD's.
// - The hole: Comp_Pawn_Tunnel calls Init Facing right after FinishSpawningActor (@2355) -> @9405 -> @4043:
//   not on a dedicated server and PortalVFX not valid -> PortalVFX = SpawnEmitterAttached(PS_BugsBunnyTunnel, Scene,
//   no socket, (0, 0, -20), PortalVFXRotation, PortalVFXScale, SnapToTarget, auto destroy) @4123, SetRelativeScale3D(
//   PortalVFXScale) @4211 (the relative scale is PortalVFXScale whatever SnapToTarget made of it), SetTemplate (the
//   same system) @4271, Team = SelectFloat(0.6, the Instigator's colour index, IsLocalOnlinePlayer) @4474-4525.
//   Scene is under the root Capsule with no relative transform (the class data); the actor: the sim's x / y, yaw
//   -90 facing left / +90 right, scale 1.25 (Comp_Pawn_Tunnel @1794; the yaws of @1713 / @1744). PortalVFXRotation
//   (0, 0, 0), PortalVFXScale (0.75, 0.75, 1) (the class defaults): world location the actor + (0, 0, -25), world
//   scale (0.9375, 0.9375, 1.25). ReceiveEndPlay destroys it @9184-9227 (K2_DestroyComponent: gone at once);
//   TransitionOut (@4584) destroys the actor at once, so the hole lives as long as the tunnel. PortalOutro (@4610,
//   OutroPortalVFX and the second hole of @365) is called by C020's PortalParent alone: nothing for Bugs.
// - The dirt: MulticastDustVFX @14688: SpawnEmitterAtLocation(PS_BugsBunnyDirtBurst, the actor's location,
//   rotation 0 (the bytecode's EX_RotationConst), scale 1, auto destroy) @14716, Team = SelectFloat(0.6, the
//   Instigator's BPI_GetColorValue, IsLocalOnline) @14939-14990. Called in ProjectileOverlap's begin overlap @12025
//   (the entered tunnel, when a projectile goes through) and, after Delay(ProjectieThroughTunnelTime) @13443, on the
//   closest tunnel (the cast of @11840) @586. The sim raises one projectileDeath event of the class at the overlap
//   (to: the closest's place); the rule below draws the first and counts the Delay for the second.
//   Deviation: the Delay is counted here, not in the sim: due at the event's tick + latentFrames(time) - 1 (the
//   Delay counted in its call frame: the tunnel ticks after the projectiles' moves, as its Stop Delay; drawn at the
//   first placed step at or past it), dropped when the entered tunnel is gone by then (its latent action goes with
//   it) or the closest is (the cast's object is no longer valid). A second overlap of the same tunnel while it runs
//   does not start another (the same latent UUID) but moves its target to the new closest (the cast variable is
//   written again). A rollback that takes the event back leaves a scheduled one in place.
//   Buff_TunnelTeleport's PS_BugsBunnyDirtBurst (@5177) is not drawn: Comp_Pawn_Tunnel's UseTeleportBuff is false.

const HOLE: &str = "PS_BugsBunnyTunnel";
const DIRT: &str = "PS_BugsBunnyDirtBurst";
/// Comp_Pawn_Tunnel @1794.
const ACTOR_SCALE: f64 = 1.25;
/// @4123.
const HOLE_Z: f64 = -20.0;

/// A dirt burst the Delay has yet to draw.
#[derive(Debug)]
struct Pending {
    /// The world tick the Delay ends at.
    due: i32,
    owner: i32,
    /// The entered tunnel (the latent action's).
    x: f64,
    y: f64,
    /// The closest (the cast variable).
    tx: f64,
    ty: f64,
}

/// A tunnel's hole: its place (a rollback's other tunnel under the same sequence number is another) and its handle.
#[derive(Debug)]
struct Hole {
    x: f64,
    y: f64,
    h: Option<SystemId>,
}

/// A live tunnel of `owner` (a pawn's id) at (x, y), with its actor's class data's ProjectieThroughTunnelTime.
fn live_tunnel(w: &World, owner: i32, x: f64, y: f64) -> Option<f64> {
    w.actors.actors.iter().find_map(|(aid, a)| {
        let WorldActorState::Tunnel(t) = &a.state else { return None };
        if a.dead || a.owner_id != owner || f64::from(t.x) != x || f64::from(t.y) != y {
            return None;
        }
        let d = actor_def(w, ActorRef::Actor(aid), a.cls);
        Some(d.and_then(|d| d.p.get("ProjectieThroughTunnelTime")).and_then(Value::as_f64).unwrap_or(0.0))
    })
}

impl Bugs {
    fn dirt(&mut self, fx: &mut Fx, owner: i32, x: f64, y: f64) {
        if self.has(fx, DIRT) {
            let team = fx.team(owner);
            fx.player.spawn(DIRT, [0.0, -x, y], [0.0; 3], [1.0; 3], &[("Team", team)]);
        }
    }

    /// `tunnelViews().actors`: a hole per live tunnel.
    fn tunnel_actors(&mut self, fx: &mut Fx, w: &World) {
        let live: Vec<(i32, i32, &Tunnel, (ActorRef, Name))> = w
            .actors
            .actors
            .iter()
            .filter_map(|(aid, a)| match &a.state {
                WorldActorState::Tunnel(t) if !a.dead => Some((a.seq, a.owner_id, t, (ActorRef::Actor(aid), a.cls))),
                _ => None,
            })
            .collect();
        // (the gone ones first: a third tunnel's spawn destroys the oldest in the same step)
        let gone: Vec<i32> = self.holes.keys().copied().filter(|s| !live.iter().any(|l| l.0 == *s)).collect();
        for s in gone {
            if let Some(Hole { h: Some(h), .. }) = self.holes.remove(&s) {
                fx.player.kill(h);
            }
        }
        for (seq, owner, t, a) in live {
            // (another tunnel under the same sequence number after a rollback)
            if let Some(r) = self.holes.get(&seq)
                && (r.x != f64::from(t.x) || r.y != f64::from(t.y))
            {
                if let Some(h) = r.h {
                    fx.player.kill(h);
                }
                self.holes.remove(&seq);
            }
            if self.holes.contains_key(&seq) {
                continue;
            }
            let h = self.hole_of(fx, w, a, owner, t);
            self.holes.insert(seq, Hole { x: f64::from(t.x), y: f64::from(t.y), h });
        }
    }

    fn hole_of(&mut self, fx: &mut Fx, w: &World, a: (ActorRef, Name), owner: i32, t: &Tunnel) -> Option<SystemId> {
        if !self.has(fx, HOLE) {
            return None;
        }
        let (a, cls) = a;
        let ps = xyz(actor_def(w, a, cls).and_then(|d| d.p.get("PortalVFXScale")), 1.0);
        let yaw = if t.facing_left { -90.0 } else { 90.0 };
        let r = rotation_rows(0.0, yaw * DEG, 0.0);
        let at = Attachment {
            loc: [0.0, f64::from(-t.x), f64::from(t.y) + ACTOR_SCALE * HOLE_Z],
            rot: std::array::from_fn(|i| std::array::from_fn(|j| r[i * 3 + j])),
            scale: Some(ps.map(|k| ACTOR_SCALE * k)),
        };
        let team = fx.team(owner);
        fx.player.spawn_attached(HOLE, &at, 1.0, &[("Team", team)])
    }

    /// `tunnelViews().place`: the Delay's dirt bursts that are due.
    fn place_pending(&mut self, fx: &mut Fx, w: &World) {
        if self.pending.is_empty() {
            return;
        }
        let now = w.flow.tick;
        let due: Vec<Pending> = {
            let (due, keep) = std::mem::take(&mut self.pending).into_iter().partition(|p| now >= p.due);
            self.pending = keep;
            due
        };
        for p in due {
            if live_tunnel(w, p.owner, p.x, p.y).is_some() && live_tunnel(w, p.owner, p.tx, p.ty).is_some() {
                self.dirt(fx, p.owner, p.tx, p.ty); // @586
            }
        }
    }
}

/// The tunnel's ownEmitter: the dirt of the overlap @12025 and the Delay's second.
fn tunnel_dirt(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, x, y, to, .. } = *e else { return false };
    let (Some(v), Some(to)) = (vfx, to) else { return false };
    if w.names.str(v) != DIRT {
        return false;
    }
    let b = me(c);
    b.dirt(fx, fighter, f64::from(x), f64::from(y)); // @12025
    let Some(time) = live_tunnel(w, fighter, f64::from(x), f64::from(y)) else { return true };
    match b.pending.iter_mut().find(|q| q.owner == fighter && q.x == f64::from(x) && q.y == f64::from(y)) {
        Some(p) => {
            p.tx = f64::from(to[0]);
            p.ty = f64::from(to[1]);
        }
        None => {
            let due = w.flow.tick + latent_frames(mvs_sim::math::narrow(time)) as i32 - 1;
            b.pending.push(Pending {
                due,
                owner: fighter,
                x: f64::from(x),
                y: f64::from(y),
                tx: f64::from(to[0]),
                ty: f64::from(to[1]),
            });
        }
    }
    true
}

// ---- Buff_Charm's look on the charmed fighter ----
// ReceiveBeginPlay, the owner a BP_Fighter_Character (@1685): SpawnEmitterAttached(PS_BugsBunny_CharmedBuff, the
// owner's Mesh, fml_un_C_head, location (0, 0, 0), rotation (0, 0, 0), scale (1, 1, 1), KeepRelativeOffset, auto
// destroy, auto activate) @1709, kept as CharmBuffVFX @1827; SetFloatParameter(Team, SelectFloat(0.6, the owner's
// BPI_GetColorValue, IsLocalOnline)) @1945-2051: the charmed fighter's own colour.
// ReceiveDestroyed: CharmBuffVFX.K2_DestroyComponent @5286 (IsValid @5243): the component goes at once with the buff.
// The component is not given to Comp_FighterVisibilityManager.AddVFX (no call in the class), so the pawn's hiding
// does not hide it. Drawn while charmed() holds and the fighter is alive (the buffs end with the defeat).

const CHARM: &str = "PS_BugsBunny_CharmedBuff";
const CHARM_SOCKET: &str = "fml_un_C_head";

impl Bugs {
    fn place_charm(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            let fi = w.fighter(f);
            let id = fi.id;
            if fi.life.dead > 0.0 || !charmed(w, f) {
                if let Some(Some(h)) = self.charm.remove(&id) {
                    fx.player.kill(h);
                }
                continue;
            }
            if self.charm.contains_key(&id) {
                continue;
            }
            let h = if self.has(fx, CHARM) {
                let follow = fx.socket_follow(w, id, Some(CHARM_SOCKET), [0.0; 3], &IDENTITY, [1.0; 3]);
                let team = fx.team(id);
                fx.attach_to_mesh(w, id, CHARM, CHARM_SOCKET, follow, &[("Team", team)])
            } else {
                None
            };
            self.charm.insert(id, h);
        }
    }
}
