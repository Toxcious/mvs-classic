//! ProjectileBarrier (sim actors/barrier.rs): a shared class's effects, made for
//! every match after the characters'.
//!
//! What the class has to draw: no mesh and no particle component (Capsule, Comp_Actor_Destructible and the two
//! collision spheres are all it has). Its graph makes one emitter:
//!   SetTeamColor @1635 (the parent's BeginPlay sequence calls it; nothing on a dedicated server @1650): BarrierVFX =
//!     SpawnEmitterAttached(PS_C016Defense, the root component, no socket, location 0, rotation 0, scale 1,
//!     KeepRelativeOffset, auto destroy) @112, SetWorldScale3D(1, 1, 1) on it @208, and with the owner's
//!     Comp_Pawn_SkinManager its Team = SelectFloat(0.6, the colour index, IsLocalOnlinePlayer) @453-595. The
//!     system's name is a literal of the parent's graph (the asset is LeBron's).
//!   ReceiveEndPlay @2312: BarrierVFX.K2_DestroyComponent (its particles go at once).
//! The effects of a block (BlockedVFX: the sound and PS_HitImpact_Directional) are the sim's events.
//!
//! A class is drawn when it is in [`VIEWS`] or a character's list in [`ENTERED`]: a child class is entered for
//! itself, the parent's entry does not cover
//! it (an actor of a class nobody entered warns once and shows nothing).
//! Unverified: the emitter turns with the actor (attached to its owner: the owner's facing).

use std::collections::{BTreeSet, HashMap};

use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_vfx::SystemId;

use crate::fx::Fx;
use crate::fx::attach::{att, mesh_rows};
use crate::fx::chars::{CharFxHooks, ProjRule};

/// The parent barrier class, by the system its SetTeamColor spawns (`barrierViews`).
const VIEWS: &[(&str, &str)] = &[("ProjectileBarrier_C", "PS_C016Defense")];
/// The child classes the characters enter (`barrierViews` calls of their modules): LeBron's C016ProjectileBarrier
/// (the parent's graph and components throughout, the child overrides ShouldActorBeBlockedAndDestroyed alone).
const ENTERED: &[&[(&str, &str)]] = &[crate::fx::chars::lebron::BARRIERS, crate::fx::chars::gizmo::BARRIERS];

/// One record per live barrier, by the actor's sequence number.
#[derive(Debug)]
struct View {
    h: Option<SystemId>,
    seen: u64,
    /// The emitter's Team: the owner's, set again when it changes.
    team: f64,
}

/// What its effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Barrier {
    views: HashMap<i32, View>,
    tick: u64,
    warned: BTreeSet<String>,
}

impl Barrier {
    fn clear(&mut self, fx: &mut Fx) {
        for v in self.views.drain().map(|(_, v)| v) {
            if let Some(h) = v.h {
                fx.player.kill(h);
            }
        }
    }
}

impl CharFxHooks for Barrier {
    /// Once per presentation tick: an emitter per barrier that is there, at the actor's place under its owner's
    /// facing, gone with its actor.
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.tick += 1;
        for (_, a) in w.actors.actors.iter() {
            let WorldActorState::ProjectileBarrier(b) = &a.state else { continue };
            if a.dead {
                continue;
            }
            let cls = w.names.str(a.cls);
            let Some(&(_, system)) = VIEWS.iter().chain(ENTERED.iter().copied().flatten()).find(|(c, _)| *c == cls)
            else {
                if self.warned.insert(cls.to_owned()) {
                    eprintln!("warning: {cls}: a barrier class without a view (fx/shared/barrier.rs VIEWS)");
                }
                continue;
            };
            let left = w.by_id(a.owner_id).is_some_and(|f| w.fighter(f).body.facing_dir <= 0.0);
            let at = att([0.0, f64::from(-b.x), f64::from(b.y)], &mesh_rows(left), Some([1.0; 3]));
            let team = fx.team(a.owner_id);
            let v = match self.views.get_mut(&a.seq) {
                Some(v) => v,
                None => {
                    let h = fx.player.spawn_attached(system, &at, 1.0, &[("Team", team)]);
                    self.views.entry(a.seq).or_insert(View { h, seen: 0, team })
                }
            };
            if let Some(h) = v.h {
                fx.player.attach(h, &at);
                if team != v.team {
                    v.team = team;
                    fx.player.set_param(h, "Team", team);
                }
            }
            v.seen = self.tick;
        }
        let tick = self.tick;
        let player = &mut fx.player;
        self.views.retain(|_, v| {
            let keep = v.seen == tick;
            if !keep && let Some(h) = v.h {
                player.kill(h);
            }
            keep
        });
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
