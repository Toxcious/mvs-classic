//! Finn's effects. His trails are AnimNotify_PlayVFX of his
//! montages (the generic notify VFX). Emote_Finn's 2D flipbooks are the emote VFX's. PS_Finn_FeathersBye's
//! ParticleModuleAccelerationDragScaleOverLife_1 is bEnabled false in the build.
//!
//! - The coin (FinnTreasure_C): PS_Finn_CoinSparkle and PS_Finn_CoinSparkles as the class's `more` components;
//!   PS_Finn_CoinSparkle is shown at the start and at PauseFade and set to Blink > 0.5 in DeathBlinkWarning's update
//!   @3818-3852. Its Team is the shared path's (the owner's).
//! - The gem (FinnDashThrownTarget_C): PS_Finn_CoinGlow00 on the Capsule with Team = SelectFloat(0.6, the colour
//!   index, IsLocalOnline) @4384-4435, hidden by the apple @3631 (the weak throw).
//! - Finn_HighFive_Shockwave_C: PS_Finn_SPN_HighFive (yaw 90 on the Capsule); FinnGemBoostBlast_C: ParticleSystem
//!   PS_Cleanse (scale 0.65 on the Capsule). Each with the owner's Team, killed with the actor. Unverified: neither
//!   class's graph sets a Team on its component that was read; the shared projectile path's rule taken.
//!   Not drawn, as the build: the aimer's ArrowComponent (an editor arrow; Unverified: UArrowComponent's
//!   bHiddenInGame default taken), the shockwave's Sphere (bHiddenInGame).
//! - The side special's dash trail: TeleportVisuals @10011-10477: SpawnEmitterAttached(PS_Finn_DashTrail, the
//!   instigator's Mesh, no socket, location 0, MakeRotator(0, the pitch, 0), scale 1, KeepRelativeOffset, auto
//!   destroy), Team = SelectFloat(0.6, the colour index, IsLocalOnline); once at each new World tick of the sim's
//!   `trail_at`.
//! - Reset: the Lab's reset clears the actors' systems and takes the trail's tick again.
//!
//! - The shockwave's decal (FinnShockwaveActor_C): the Decal component (ShaggyGroundDecal_MAT,
//!   RelativeRotation (90, 26.565, -63.435) on the Capsule root, the default DecalSize (128, 256, 256) as the box's
//!   half extents). Visuals(NumberOfShocks) at BeginPlay (@802 -> @2235-3774) sets its relative scale (0.1,
//!   [0, 0.4, 0.8, 1.4][n], 0.7) and location ([0, 8, 120, 267][n], 30, 0) (the sim's `shocks`). The actor's rotation
//!   is the spawn's yaw (the sim's `facing`): ShaggyShockwaveActor's placement (local X the world +Z). Team:
//!   SelectFloat(0.6, the owner's colour value, IsLocalOnline). Alpha: the DecalAlpha timeline (the sim's
//!   `decal_alpha`). Placed at the step that first sees the actor (the root's scale of then), drawn each frame until
//!   the actor is gone.
//!
//! The BMO is the view's (mvs-view chars/finn.rs). Not ported here: the shop and the aimer's and gem's widgets (the
//! HUD lane); see the track's rows. PS_Finn_CoinGlow00's shown state
//! reads the item's fade hide and the weak throw only (the draw's `shown` is not read here: Unverified).

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::actors::projectile::attach_parent;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::finn::state::finn_of;
use mvs_sim::fighter::common::actor_def;
use mvs_vfx::SystemId;
use mvs_vfx::math::{Rows, V3, compose_rows, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, num, rot_rows, text, xyz};
use crate::fx::{FloorDecal, Fx};

const DEG: f64 = std::f64::consts::PI / 180.0;
const TREASURE: &str = "FinnTreasure_C";
const HIGH_FIVE_SHOCKWAVE: &str = "Finn_HighFive_Shockwave_C";
const GEM_BOOST_BLAST: &str = "FinnGemBoostBlast_C";
/// The warning key of a shockwave class without its Decal.
const SHOCKWAVE_DECAL: &str = "FinnShockwaveActor_C Decal";
const SPARKLE: &str = "PS_Finn_CoinGlow00";
const DASH_TRAIL: &str = "PS_Finn_DashTrail";
pub(crate) const DECAL_MAT: &str = "ShaggyGroundDecal_MAT";
/// Visuals(n): the Decal's relative X and its relative scale's Y.
const SHOCK_X: [f64; 4] = [0.0, 8.0, 120.0, 267.0];
const SHOCK_SY: [f64; 4] = [0.0, 0.4, 0.8, 1.4];

/// What Finn's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Finn {
    /// The actors' components by actor seq: its system and its scale at a root scale of 1.
    actors: IndexMap<i32, (Option<SystemId>, V3)>,
    /// The gems' sparkles by item seq.
    sparkles: IndexMap<i32, Option<SystemId>>,
    /// By fighter id: the `trail_at` last taken (a new one spawns the trail).
    trails: IndexMap<i32, i32>,
    /// The shockwaves' decals by actor seq, as placed at their first sight (their alpha the sim's of each step).
    shocks: IndexMap<i32, FloorDecal>,
    warned: std::collections::HashSet<String>,
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[(
    TREASURE,
    ProjRule { more: &["PS_Finn_CoinSparkle", "PS_Finn_CoinSparkles"], tick: Some(coin_tick), ..ProjRule::NONE },
)];

/// DeathBlinkWarning's update: PS_Finn_CoinSparkle shown while the blink is above 0.5 (or no fade runs).
fn coin_tick(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let mvs_sim::actors::ProjectileExt::FinnTreasure(coin) = &pr.ext else { return };
    let f = coin.fade;
    if let Some(Some(h)) = st.more.first() {
        fx.player.set_visible(*h, f < 0.0 || blink_at(f64::from(f)) > 0.5);
    }
}

/// `blinkAt` (mvs-view chars/finn.rs: change both).
fn blink_at(t: f64) -> f64 {
    const BLINK: [(f64, f64); 14] = [
        (0.0, 1.0),
        (0.07063842, 0.009337544),
        (0.24687612, 1.0),
        (0.65320194, 0.0),
        (0.8392309, 1.0),
        (1.2406613, 0.0),
        (1.475645, 1.0),
        (1.8477024, 0.0),
        (2.0092535, 1.0),
        (2.2882967, 0.0),
        (2.4988027, 1.0),
        (2.6848316, 0.0),
        (2.81701, 1.0),
        (2.9736657, 0.0015562773),
    ];
    let mut v = BLINK[0].1;
    for (k, b) in BLINK {
        if t < k {
            break;
        }
        v = b;
    }
    v
}

impl Finn {
    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Finn: {ps} is not loaded");
        }
        false
    }

    fn kill_all(&mut self, fx: &mut Fx) {
        let hs: Vec<SystemId> = self
            .actors
            .drain(..)
            .filter_map(|(_, (h, _))| h)
            .chain(self.sparkles.drain(..).filter_map(|(_, h)| h))
            .collect();
        for h in hs {
            fx.player.kill(h);
        }
    }
}

impl CharFxHooks for Finn {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        // the shockwave's and the blast's particle systems
        let mut seen = Vec::new();
        for (id, a) in w.actors.actors.iter() {
            let cls = w.names.str(a.cls);
            let name = match (cls, &a.state) {
                (HIGH_FIVE_SHOCKWAVE, WorldActorState::HighFiveShockwave(_)) => "PS_Finn_SPN_HighFive",
                (GEM_BOOST_BLAST, WorldActorState::GemBoostBlast(_)) => "ParticleSystem",
                _ => continue,
            };
            if a.dead {
                continue;
            }
            seen.push(a.seq);
            let Some((x, y, _)) = mvs_sim::actors::world_actor::actor_place(w, ActorRef::Actor(id)) else { continue };
            if !self.actors.contains_key(&a.seq) {
                let made = (|| {
                    let cs = &actor_def(w, ActorRef::Actor(id), a.cls)?.components;
                    let c = cs.get(name)?.as_obj()?;
                    let template = text(c, "Template")?;
                    if !self.has(fx, template) {
                        return None;
                    }
                    let par = attach_parent(cs, name);
                    let rot = actor_rot(w, a.cls, id, name);
                    let rs = xyz(c.get("RelativeScale3D"), 1.0);
                    let base: V3 = std::array::from_fn(|k| f64::from(par.scale[k]) * rs[k]);
                    let team = fx.team(a.owner_id);
                    let at = att([0.0, f64::from(-x), f64::from(y)], &rot, Some(base));
                    Some((fx.player.spawn_attached(template, &at, 1.0, &[("Team", team)]), base))
                })();
                let (h, base) = made.unwrap_or((None, [1.0; 3]));
                self.actors.insert(a.seq, (h, base));
            }
            let Some(&(Some(h), base)) = self.actors.get(&a.seq) else { continue };
            let rot = actor_rot(w, a.cls, id, name);
            let k = a.size_scale.unwrap_or(1.0);
            let scale: V3 = std::array::from_fn(|i| base[i] * f64::from(k));
            fx.player.attach(h, &att([0.0, f64::from(-x), f64::from(y)], &rot, Some(scale)));
        }
        let gone: Vec<i32> = self.actors.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some((Some(h), _)) = self.actors.shift_remove(&k) {
                fx.player.kill(h);
            }
        }
        self.place_gems(fx, w);
        self.place_shocks(fx, w);
    }

    /// The shockwaves' decals (placed again each frame: the frame's decals are cleared after it).
    fn frame(&mut self, fx: &mut Fx, _w: &World, _alpha: f64) {
        fx.decals.extend(self.shocks.values().cloned());
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        // TeleportVisuals: PS_Finn_DashTrail on the instigator's mesh at each new trail tick
        for f in w.all_ids() {
            let Some(fs) = finn_of(w, f) else { continue };
            let id = w.fighter(f).id;
            let t = fs.dash.trail_at;
            let Some(&seen) = self.trails.get(&id) else {
                self.trails.insert(id, t);
                continue;
            };
            if t == seen {
                continue;
            }
            self.trails.insert(id, t);
            if t < 0 || !self.has(fx, DASH_TRAIL) {
                continue;
            }
            let rot = rotation_rows(f64::from(fs.dash.trail_pitch) * DEG, 0.0, 0.0);
            let follow = fx.socket_follow(w, id, None, [0.0; 3], &rot, [1.0; 3]);
            let team = fx.team(id);
            fx.attach_to_mesh(w, id, DASH_TRAIL, "None", follow, &[("Team", team)]);
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        // the reset: the actors' views cleared and the trail ticks taken again on the next step
        self.kill_all(fx);
        self.trails.clear();
        self.shocks.clear();
    }

    fn end(&mut self, fx: &mut Fx) {
        self.kill_all(fx);
        self.trails.clear();
        self.shocks.clear();
    }
}

/// The root's rotation of the shockwave and the blast: the component's own relative rotation over its parents (the
/// attach), no turn of the actor's.
fn actor_rot(w: &World, cls: mvs_sim::names::Name, id: mvs_sim::actors::ActorId, name: &str) -> Rows {
    let Some(cs) = actor_def(w, ActorRef::Actor(id), cls).map(|d| &d.components) else {
        return rotation_rows(0.0, 0.0, 0.0);
    };
    let par = attach_parent(cs, name);
    let own = cs
        .get(name)
        .and_then(mvs_data::value::Value::as_obj)
        .map_or(rotation_rows(0.0, 0.0, 0.0), |c| rot_rows(c.get("RelativeRotation")));
    compose_rows(&own, &std::array::from_fn(|k| f64::from(par.rot[k])))
}

impl Finn {
    /// The shockwave actors: each live shockwave's decal made at its first sight, its alpha the sim's; the gone ones'
    /// dropped.
    fn place_shocks(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (id, a) in w.actors.actors.iter() {
            let WorldActorState::FinnShockwave(sw) = &a.state else { continue };
            if a.dead {
                continue;
            }
            seen.push(a.seq);
            if !self.shocks.contains_key(&a.seq) {
                let dc = actor_def(w, ActorRef::Actor(id), a.cls).and_then(|d| d.components.get("Decal")?.as_obj());
                let Some(dc) = dc else {
                    if self.warned.insert(SHOCKWAVE_DECAL.to_owned()) {
                        eprintln!("warning: Finn: no Decal component of FinnShockwaveActor_C in the data");
                    }
                    continue;
                };
                let n = sw.shocks.clamp(0, 3) as usize;
                let sc = dc.get("RelativeScale3D").and_then(|v| v.as_obj());
                let sz = dc.get("DecalSize").and_then(|v| v.as_obj());
                let rl_in = dc.get("RelativeLocation").and_then(|v| v.as_obj());
                let g =
                    |o: Option<&mvs_data::value::Props>, key: &str, d: f64| o.and_then(|o| num(o, key)).unwrap_or(d);
                let k = a.size_scale.unwrap_or(1.0);
                let rl =
                    [SHOCK_X[n] * f64::from(k), g(rl_in, "Y", 30.0) * f64::from(k), g(rl_in, "Z", 0.0) * f64::from(k)];
                let f = if sw.facing < 0.0 { -1.0 } else { 1.0 };
                self.shocks.insert(
                    a.seq,
                    FloorDecal {
                        actor: a.seq,
                        material: DECAL_MAT.to_owned(),
                        loc: [rl[1] * f, -(f64::from(sw.x) + rl[0] * f), f64::from(sw.y) + rl[2]],
                        rows: [0.0, 0.0, 1.0, 0.0, f, 0.0, -f, 0.0, 0.0],
                        extent: [
                            g(sz, "X", 128.0) * g(sc, "X", 0.1) * f64::from(k),
                            g(sz, "Y", 256.0) * SHOCK_SY[n] * f64::from(k),
                            g(sz, "Z", 256.0) * g(sc, "Z", 0.7) * f64::from(k),
                        ],
                        team: Some(fx.team(a.owner_id)),
                        alpha: None,
                        dissolve: None,
                    },
                );
            }
            if let Some(d) = self.shocks.get_mut(&a.seq) {
                d.alpha = Some(f64::from(sw.decal_alpha));
            }
        }
        self.shocks.retain(|k, _| seen.contains(k));
    }

    /// The gems' PS_Finn_CoinGlow00 (spawned with the item, hidden by the apple and the fade).
    fn place_gems(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (_, it) in w.actors.items.iter() {
            let ItemExt::Gem(g) = &it.ext else { continue };
            if it.dead {
                continue;
            }
            seen.push(it.seq);
            let h = h_of(self, fx, w, it);
            let Some(h) = h else { continue };
            let yaw = if it.facing > 0.0 { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 };
            let at = att(
                [0.0, f64::from(-it.x), f64::from(it.y)],
                &rotation_rows(f64::from(it.pitch).to_radians(), yaw, 0.0),
                Some([f64::from(it.scale); 3]),
            );
            fx.player.attach(h, &at);
            fx.player.set_visible(h, !g.weak_throw && !g.fade_hidden);
        }
        let gone: Vec<i32> = self.sparkles.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(Some(h)) = self.sparkles.shift_remove(&k) {
                fx.player.kill(h);
            }
        }
    }
}

fn h_of(s: &mut Finn, fx: &mut Fx, w: &World, it: &mvs_sim::actors::item::Item) -> Option<SystemId> {
    if let Some(h) = s.sparkles.get(&it.seq) {
        return *h;
    }
    let owner = it.owner.or(it.spawner)?;
    // (only with the class's PS_Finn_CoinSparkle component, itemDefOf: the spawner's class data)
    let def_of = it.spawner.or(it.owner)?;
    let sc = actor_def(w, ActorRef::Fighter(def_of), it.cls)
        .is_some_and(|d| d.components.contains_key("PS_Finn_CoinSparkle"));
    let h = if sc && s.has(fx, SPARKLE) {
        let team = fx.team(w.fighter(owner).id);
        let at = att(
            [0.0, f64::from(-it.x), f64::from(it.y)],
            &rotation_rows(0.0, 0.0, 0.0),
            Some([f64::from(it.scale); 3]),
        );
        fx.player.spawn_attached(SPARKLE, &at, 1.0, &[("Team", team)])
    } else {
        None
    };
    s.sparkles.insert(it.seq, h);
    h
}
