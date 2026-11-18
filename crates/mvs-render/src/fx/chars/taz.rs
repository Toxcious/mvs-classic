//! Taz's effects (the CharFx hooks): PS_Heal on a healed fighter, the dogpile
//! cloud and the TornadoReflect arrows. The EatProjectiles perk's anvil draws no
//! emitter (its ParticleSystem is not drawn here either); its mesh is the view's.

use std::f64::consts::FRAC_PI_2;

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::taz::dogpile::world_scale;
use mvs_sim::chars::taz::perks_reflect::{ALLY_BUFF, TORNADO_BUFF};
use mvs_sim::events::{Event, TazHealKind};
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::{FighterId, facing};
use mvs_sim::name;
use mvs_sim::names::Name;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, compose_rows, rotation_rows};

use super::{CharFxHooks, ProjRule};
use crate::fx::attach::{att, rot_rows, warn_once};
use crate::fx::{Follow, Fx, Scale};

/// What Taz's effects keep between steps: the dogpile clouds by the actors' sequence numbers, the arrows by fighter.
#[derive(Debug, Default)]
pub(crate) struct Taz {
    clouds: IndexMap<i32, Cloud>,
    arrows: Vec<Option<Arrows>>,
}

const PS: &str = "PS_Heal";
const LICK_SCALE: f64 = 1.25;

impl CharFxHooks for Taz {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.clouds(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_arrows(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear_clouds(fx);
        self.clear_arrows(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear_clouds(fx);
        self.clear_arrows(fx);
    }

    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        match e {
            Event::TazHealFx { fighter, kind } => {
                heal_fx(fx, w, *fighter, *kind);
                true
            }
            _ => false,
        }
    }
}

/// `tazHealFx` (the sim's tazHealFx events: healAllies, C015_HealPickup), a single
/// shot each:
/// - lick: Pawn_C015 MulticastHealingVFX(ally) @1231 -> @16526 SpawnEmitterAttached(PS_Heal, the ally's Mesh, Root,
///   (0, 0, 0), rotation 0, (1.25, 1.25, 1.25), SnapToTarget, bAutoDestroy): SnapToTarget sets the relative scale to
///   Scale / the socket's world scale, so the world scale is 1.25. No Team is set.
/// - pickup: C015_HealPickup HealVisualMulticast(the picker) @694 -> @149 SpawnEmitterAttached(PS_Heal, the picker's
///   CapsuleComponent, socket None, (0, 0, 0), rotation 0, (1, 1, 1), KeepRelativeOffset, bAutoDestroy): the actor's
///   place and rotation (yaw -90 facing right, +90 left), relative scale 1 under the actor scale. No Team is set.
// Unverified: neither is AddVFX'ed (no visibility tracking in either graph): the emitter stays drawn while its
// fighter is hidden; on the root (pickup) it does not follow the mesh's hide either.
// Unverified: the pickup's emitter takes the actor scale of the spawn tick and does not read it again each step.
fn heal_fx(fx: &mut Fx, w: &World, id: i32, kind: TazHealKind) {
    let Some(fid) = w.by_id(id) else { return };
    match kind {
        TazHealKind::Lick => {
            let mut f = fx.socket_follow(w, id, Some("Root"), [0.0; 3], &IDENTITY, [1.0; 3]);
            if let Follow::Socket { scale, .. } = &mut f {
                *scale = Scale::Fixed([LICK_SCALE; 3]);
            }
            fx.attach_to_mesh(w, id, PS, "Root", f, &[]);
        }
        TazHealKind::Pickup => {
            let b = &w.fighter(fid).body;
            let s = b.actor_scale.map_or([b.size_scale; 3], |[xy, z]| [xy, xy, z]);
            fx.spawn_follow(
                w,
                PS,
                Follow::Root { id, rel: IDENTITY, up: 0.0, scale: Some(s.map(f64::from)) },
                1.0,
                &[],
            );
        }
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];

// ---- the dogpile cloud ----------------------------------------------------------------------------

/// C015Dogpile_C on the render side (offsets of its ubergraph; the sim: mvs_sim::chars::taz::dogpile, a World.actors
/// kind, so the shared projectile path does not see it).
///
/// Its components (the class's component data): Capsule (root, bAbsoluteScale: the notify's Scale3D (1, 1, 1) is its
/// world scale) > ScalingComponent (bHiddenInGame, which hides it alone) > ParticleSystem (PS_C015_FightCloud, yaw 90);
/// and ParticleSystem2 (PS_C015_FightCloud_Arms, yaw 90) on the Capsule. Both auto-activate with the actor.
/// - ScalingComponent.SetRelativeScale3D(VLerp(HitBoxMinScale, HitBoxMaxScale, Alpha)) @705, each tick of
///   HitBoxTimeline: the sim's world_scale, on the cloud alone (the arms are not under it). The timeline's Stop at
///   the first hit (@1127) holds it.
/// - SetTeamColor @3208: Team = SelectFloat(0.5, the Instigator's colour value, IsLocalOnline) on both components
///   (@3372, @3423, @3481): 0.5 for the local player here, not the 0.6 of the other graphs. Taken at the spawn.
/// - The hit's PS_C015_FightCloud_Hit (@2840, Team by SelectFloat(0.6) @3059) is the sim's projectileDeath event of
///   the class, drawn by the shared spawn.
/// - The actor's end (DestroyAllActorsOnNotifyEnd, the arena exit): K2_DestroyActor takes its components with it, the
///   particles gone at once (kill).
///
/// The actor's rotation: the Instigator's facing yaw (-90 facing right, 90 left, as the attached projectiles'
/// actorRot), the components' yaw 90 on top.
// Deviation: the Root socket's own rotation is not applied.
// Deviation: drawn at the sim's place of the latest tick (no in-between frame), as Bugs' tunnel.
#[derive(Debug)]
struct Cloud {
    /// The actor's age when last seen (a rollback that makes another under the same id is younger).
    age: i32,
    cloud: Option<SystemId>,
    arms: Option<SystemId>,
    /// ParticleSystem's and ParticleSystem2's relative rotations.
    cloud_rot: Rows,
    arms_rot: Rows,
}

const DOGPILE: Name = name!("C015Dogpile_C");

impl Taz {
    fn clouds(&mut self, fx: &mut Fx, w: &World) {
        if self.clouds.is_empty() && w.actors.actors.iter().next().is_none() {
            return;
        }
        let mut seen = Vec::new();
        for (aid, a) in w.actors.actors.iter() {
            let WorldActorState::Dogpile(d) = &a.state else { continue };
            if a.dead {
                continue;
            }
            seen.push(a.seq);
            if self.clouds.get(&a.seq).is_some_and(|v| d.age < v.age) {
                // (another cloud under the same id after a rollback)
                let v = self.clouds.shift_remove(&a.seq).expect("seen above");
                drop_cloud(fx, &v);
            }
            let left = facing(w, a.owner) < 0.0;
            let actor_rot = rotation_rows(0.0, if left { FRAC_PI_2 } else { -FRAC_PI_2 }, 0.0);
            let loc = [0.0, -d.x, d.y];
            let cs = world_scale(w, aid);
            // (ParticleSystem2 on the Capsule: the root's scale, the sim's size scale)
            let arms_s = a.size_scale.unwrap_or(1.0);
            if !self.clouds.contains_key(&a.seq) {
                let comps = actor_def(w, ActorRef::Fighter(a.owner), DOGPILE).map(|d| &d.components);
                let comp = |n: &str| comps.and_then(|c| c.get(n)).and_then(Value::as_obj);
                let id = w.fighter(a.owner).id;
                // @3372
                let team = if fx.team(id) == 0.6 { 0.5 } else { f64::from(fx.color_index(id)) };
                let rel = |n: &str| rot_rows(comp(n).and_then(|c| c.get("RelativeRotation")));
                let (cloud_rot, arms_rot) = (rel("ParticleSystem"), rel("ParticleSystem2"));
                let mut sys = |n: &str, rot: &Rows, s: f64| {
                    let t = comp(n).and_then(|c| c.get("Template")).and_then(Value::as_str);
                    let h = t.and_then(|t| {
                        let a = att(loc.map(f64::from), &compose_rows(rot, &actor_rot), Some([s; 3]));
                        fx.player.spawn_attached(t, &a, 1.0, &[("Team", team)])
                    });
                    if h.is_none() {
                        warn_once(format!("Taz: the dogpile's {n} is not drawn (its system is not loaded)"));
                    }
                    h
                };
                let cloud = sys("ParticleSystem", &cloud_rot, f64::from(cs));
                let arms = sys("ParticleSystem2", &arms_rot, f64::from(arms_s));
                self.clouds.insert(a.seq, Cloud { age: d.age, cloud, arms, cloud_rot, arms_rot });
            }
            let v = self.clouds.get_mut(&a.seq).expect("made above");
            v.age = d.age;
            if let Some(h) = v.cloud {
                fx.player.attach(
                    h,
                    &att(loc.map(f64::from), &compose_rows(&v.cloud_rot, &actor_rot), Some([f64::from(cs); 3])),
                );
            }
            if let Some(h) = v.arms {
                fx.player.attach(
                    h,
                    &att(loc.map(f64::from), &compose_rows(&v.arms_rot, &actor_rot), Some([f64::from(arms_s); 3])),
                );
            }
        }
        self.clouds.retain(|seq, v| {
            let keep = seen.contains(seq);
            if !keep {
                drop_cloud(fx, v);
            }
            keep
        });
    }

    fn clear_clouds(&mut self, fx: &mut Fx) {
        for v in self.clouds.values() {
            drop_cloud(fx, v);
        }
        self.clouds.clear();
    }
}

fn drop_cloud(fx: &mut Fx, v: &Cloud) {
    for h in [v.cloud, v.arms].into_iter().flatten() {
        fx.player.kill(h);
    }
}

// ---- the TornadoReflect arrows --------------------------------------------------------------------

/// The TornadoReflect perk's arrows (mvs_sim::chars::taz::perks_reflect): PS_C015_Combo2_TornadoTrail_Arrows on a
/// reflecting fighter, seen here as the sim's state (no event): Taz while his Buff_C015Tornado has Reflecting, a team
/// mate while his Buff_AllyTornadoed has it.
/// - Buff_C015Tornado ReflectVisuals (@1869 -> @1485 -> the DoOnce @621, not on a dedicated server, a
///   BP_FighterCharacter owner @727-792): SpawnEmitterAttached(the system, the owner's Mesh, Root, (0, 0, 0), rot
///   (0, 0, 0), scale (1, FacingLeft ? -1 : 1, 1), KeepRelativeOffset, bAutoDestroy) @1021 as TurnVFX,
///   SetFloatParameter(Team, SelectFloat(0.6, ColorValue, IsLocalOnline)) @1253-1359; its FacingChanged @446-579 sets
///   the relative scale again from the new facing; EndPlay destroys it @412.
/// - Buff_AllyTornadoed MulticastActivateReflect @648-1422: the same on the mate's Mesh @1026 and Team @1364;
///   FacingChanged @395-528; EndPlay @570.
///
/// Not added to the visibility tracking (neither graph calls AddVFX): drawn while its fighter is hidden too.
// Unverified: the DoOnce @621 of Buff_C015Tornado (one spawn per buff actor) and the ally's Reflecting gate are the
// sim's Reflecting: a new buff (the next tornado) spawns it again, as a new actor's DoOnce would.
const ARROWS: &str = "PS_C015_Combo2_TornadoTrail_Arrows";

/// A reflecting fighter's arrows (None: its system is not loaded) and the facing its follower was made for.
#[derive(Debug)]
struct Arrows {
    h: Option<SystemId>,
    left: bool,
}

fn reflecting(w: &World, f: FighterId) -> bool {
    // (the instance's own Reflecting, written by its graph: set_buff_values; the class default is false)
    w.fighter(f).buffs.buffs.iter().any(|b| {
        b.frames > 0.0
            && (b.cls == TORNADO_BUFF || b.cls == ALLY_BUFF)
            && b.own.as_ref().and_then(|o| o.values.get("Reflecting")).and_then(Value::as_bool) == Some(true)
    })
}

/// The arrows' follower: Root at relative scale (1, -1, 1) facing left.
fn arrows_follow(fx: &Fx, w: &World, id: i32, left: bool) -> Follow {
    let s = if left { [1.0, -1.0, 1.0] } else { [1.0; 3] };
    fx.socket_follow(w, id, Some("Root"), [0.0; 3], &IDENTITY, s)
}

impl Taz {
    fn place_arrows(&mut self, fx: &mut Fx, w: &World) {
        let ids = w.all_ids();
        if self.arrows.len() < ids.len() {
            self.arrows.resize_with(ids.len(), || None);
        }
        for (i, &f) in ids.iter().enumerate() {
            let on = reflecting(w, f);
            let id = w.fighter(f).id;
            let left = facing(w, f) < 0.0;
            match (&mut self.arrows[i], on) {
                (Some(a), true) => {
                    // The relative scale follows the facing (SetRelativeScale3D on FacingChanged).
                    if let Some(h) = a.h
                        && a.left != left
                    {
                        let nf = arrows_follow(fx, w, id, left);
                        if let Some(e) = fx.followers.iter_mut().find(|(x, _)| *x == h) {
                            e.1 = nf;
                        }
                    }
                    a.left = left;
                }
                (Some(a), false) => {
                    if let Some(h) = a.h {
                        fx.player.kill(h);
                    }
                    self.arrows[i] = None;
                }
                (None, true) => {
                    let nf = arrows_follow(fx, w, id, left);
                    let team = fx.team(id);
                    let h = fx.attach_to_mesh(w, id, ARROWS, "Root", nf, &[("Team", team)]);
                    if h.is_none() {
                        warn_once(format!("Taz: {ARROWS} is not drawn (its system is not loaded)"));
                    }
                    self.arrows[i] = Some(Arrows { h, left });
                }
                (None, false) => {}
            }
        }
    }

    fn clear_arrows(&mut self, fx: &mut Fx) {
        for a in self.arrows.drain(..).flatten() {
            if let Some(h) = a.h {
                fx.player.kill(h);
            }
        }
    }
}
