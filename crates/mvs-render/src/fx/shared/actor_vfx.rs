//! AnimNotify_PlayVFX on a spawned actor's mesh (the sim's actorVfx event):
//! SpawnVFX for an owner that is no fighter and no preview actor. A shared class's
//! effects, made for every match after the characters'.
//!
//! - SocketName not None: SpawnEmitterAttached(ParticleTeam0, MeshComp, SocketName, Location, Rotation, VFX Scale,
//!   keep relative offset): no team select and no Mirror() (the fighter branch's); the socket is a bone of the actor's
//!   view (a class's view enters its skinned mesh's bones: mvs_view::actors::ActorMesh::bones; the views of the tick's
//!   spawned actors are made ahead of its events, the poser's place).
//! - SocketName None: SpawnEmitterAtLocation(ParticleTeam0, Location, Rotation, VFX Scale): the notify's values as
//!   world values.
//! - UseTeamColorParam?: Team = float(the owner's owner's BPI_GetColorValue), the fighter's ColorIndex.
//! - NotifyEnd with KillOnNotifyEnd: MeshComp's particle children whose template is ParticleTeam0 or ParticleTeam1
//!   are deactivated (they complete by themselves). A destroyed actor takes its attached systems with it.
//!
//! CustomTimeDilation: SpawnedActorParent's reflect pause sets every particle component of the actor to 0.5 @2277 and
//! back to 1 @969: the systems held here tick at 0.5 while the actor's ReflectState.pause is 2.

use std::collections::HashSet;

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::events::{Event, NotifyRef};
use mvs_vfx::SystemId;

use crate::fx::attach::{flag, rot_rows, text, xyz};
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{Follow, Fx};

/// What its effects keep between steps: the systems each actor's mesh holds (template, handle) by the actor's
/// sequence number, and the actors whose systems were last set to 0.5.
#[derive(Debug, Default)]
pub(crate) struct ActorVfx {
    held: IndexMap<i32, Vec<(String, SystemId)>>,
    slowed: HashSet<i32>,
    /// The (system, bone) pairs warned of.
    warned: HashSet<(String, String)>,
}

/// The spawned actor of sequence number `seq` (a projectile or a World actor: the ones whose montages play notifies).
fn actor_of(w: &World, seq: i32) -> Option<ActorRef> {
    if let Some((id, _)) = w.actors.projectiles.iter().find(|(_, p)| p.seq == seq) {
        return Some(ActorRef::Projectile(id));
    }
    w.actors.actors.iter().find(|(_, a)| a.seq == seq).map(|(id, _)| ActorRef::Actor(id))
}

impl ActorVfx {
    fn clear(&mut self, fx: &mut Fx) {
        for (_, l) in self.held.drain(..) {
            for (_, h) in l {
                fx.player.kill(h);
            }
        }
        self.slowed.clear();
    }

    #[allow(clippy::too_many_arguments)]
    fn on(&mut self, fx: &mut Fx, w: &World, fighter: i32, seq: i32, n: NotifyRef, x: f64, y: f64) {
        let Some(a) = actor_of(w, seq) else { return };
        let p = &mvs_sim::actors::actor_montage::actor_notify_data(w, &w.data, a, n).p;
        let Some(ps) = text(p, "ParticleTeam0").filter(|s| !s.is_empty() && *s != "None").map(str::to_owned) else {
            return;
        };
        let team = [("Team", f64::from(fx.color_index(fighter)))];
        let params: &[(&str, f64)] = if flag(p, "UseTeamColorParam?") { &team } else { &[] };
        let loc = xyz(p.get("Location"), 0.0);
        let scale = xyz(p.get("VFX Scale"), 1.0);
        let r = p.get("Rotation").and_then(|v| v.as_obj());
        let g = |k: &str| r.and_then(|r| r.get(k)).and_then(|v| v.as_f64()).unwrap_or(0.0);
        // Deviation: the notify's Rotation goes in as the build's FRotator (degrees), not rows
        // where the spawn takes a rotator in radians, which would pitch the system by 1 rad.
        let rotator = [g("Pitch"), g("Yaw"), g("Roll")];
        let socket = text(p, "SocketName").filter(|s| !s.is_empty() && *s != "None");
        // Deviation: a spawn at a place is at scale 1, where SpawnEmitterAtLocation takes VFX Scale.
        let Some(socket) = socket else {
            fx.player.spawn(&ps, loc, rotator, [1.0; 3], params);
            return;
        };
        let rot = rot_rows(p.get("Rotation"));
        let has_bone =
            fx.actor_bones.get(&a).is_some_and(|b| b.iter().any(|(name, _)| name.eq_ignore_ascii_case(socket)));
        let h = if has_bone {
            let f = Follow::Bone { a, bone: socket.to_owned(), loc, rel: rot, scale };
            fx.spawn_follow(w, &ps, f, 1.0, params)
        } else {
            // (no view of the actor, or no such bone in it: at the actor's place at the notify)
            if self.warned.insert((ps.clone(), socket.to_owned())) {
                eprintln!("actor VFX {ps}: no bone {socket} on actor {seq}'s view");
            }
            fx.player.spawn(&ps, [loc[0], loc[1] - x, loc[2] + y], rotator, [1.0; 3], params)
        };
        if let Some(h) = h {
            self.held.entry(seq).or_default().push((ps, h));
        }
    }

    /// PlayVFX Received_NotifyEnd @523-1162: the first of MeshComp's children (attach order) that is a particle system
    /// of either template is deactivated, and the walk ends there (@1151 -> @1414). A deactivated one still finishing
    /// is still a child, so it is found again.
    // Deviation: the actor's other particle children (none drawn here) are not in the walk.
    fn off(&mut self, fx: &mut Fx, w: &World, seq: i32, n: NotifyRef) {
        let Some(l) = self.held.get(&seq) else { return };
        let Some(a) = actor_of(w, seq) else { return };
        let p = &mvs_sim::actors::actor_montage::actor_notify_data(w, &w.data, a, n).p;
        let (t0, t1) = (text(p, "ParticleTeam0"), text(p, "ParticleTeam1"));
        let x = l.iter().find(|(ps, h)| fx.player.alive(*h) && (Some(ps.as_str()) == t0 || Some(ps.as_str()) == t1));
        if let Some(&(_, h)) = x {
            fx.player.deactivate(h);
        }
    }
}

impl CharFxHooks for ActorVfx {
    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        match *e {
            Event::ActorVfx { fighter, actor, n, on, x, y } => {
                if on {
                    self.on(fx, w, fighter, actor, n, f64::from(x), f64::from(y));
                } else {
                    self.off(fx, w, actor, n);
                }
                true
            }
            _ => false,
        }
    }

    /// An actor gone from the world takes its attached systems (DestroyComponent with the actor); the reflect pause.
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        if self.held.is_empty() {
            return;
        }
        let mut live = HashSet::new();
        live.extend(w.actors.projectiles.iter().filter(|(_, p)| !p.dead).map(|(_, p)| p.seq));
        live.extend(w.actors.actors.iter().filter(|(_, a)| !a.dead).map(|(_, a)| a.seq));
        // (the reflect pause: only projectiles are reflected among the actors that hold systems here)
        for (_, pr) in w.actors.projectiles.iter() {
            let Some(rs) = &pr.reflect else { continue };
            let Some(l) = self.held.get(&pr.seq) else { continue };
            let slow = rs.pause == 2;
            if !slow && !self.slowed.remove(&pr.seq) {
                continue;
            }
            if slow {
                self.slowed.insert(pr.seq);
            }
            for (_, h) in l {
                fx.player.set_time_dilation(*h, if slow { 0.5 } else { 1.0 });
            }
        }
        let ids: Vec<i32> = self.held.keys().copied().collect();
        for id in ids {
            let l = self.held.get_mut(&id).expect("a held list");
            l.retain(|(_, h)| fx.player.alive(*h));
            let gone = !live.contains(&id);
            if gone || l.is_empty() {
                for (_, h) in self.held.shift_remove(&id).unwrap_or_default() {
                    if gone {
                        fx.player.kill(h);
                    }
                }
                self.slowed.remove(&id);
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
