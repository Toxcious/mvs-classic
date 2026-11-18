//! Garnet's effects: her pawn's singing VFX and hand glow, the
//! classes' projectile rules (the thrown gloves' activation burst, GarnetMarker2's star, beam and sparkles,
//! GarnetLightningBolt's second component), and once a presentation tick the World actors' VFX (GarnetElectricZone,
//! the shockwave hit boxes) and Buff_GarnetStopProjectile's VFX on the projectiles it holds.
//!
//! GarnetGauntletsProjectile_C / GarnetGauntletsProjectileAir_C (the Air class has no graph of its own: the parent's):
//! ParticleSystem (PS_Garnet_SPF_Trail at (-10, 0, 33) under StaticMesh (0, 0, -47), scale 2) is the shared path's
//! component with its Team (SetTeamColor @6611-6662). ActivateObject @3207-3433 (Activated, the sim's) ->
//! MulticastVelocity(ScaleUpVFX true) @4833: ParticleSystem's relative scale set to (1, 1, 1) (its own already: nothing
//! to draw) and PS_Garnet_GaunletActivate attached to StaticMesh (KeepRelativeOffset, scale 1: the StaticMesh's 2),
//! Team = SelectFloat(0.6, the colour index, IsLocalOnline) @5150-5201, seen as Projectile.activated turning true,
//! destroyed with the actor. DeathVFX PS_GarnetGauntletProjectileExplosionTemp: the sim's projectileDeath event. The
//! two glove meshes are mvs-view chars/garnet.rs's.
//!
//! GarnetMarker2_C (the class has no ParticleSystem component, so the shared path draws nothing): StarVFX
//! (PS_GarnetMarker, on the Capsule root): Team = SelectFloat(0.6, the Instigator's colour value, IsLocalOnline)
//! @2672-2723; hidden while the marker sits on an ally (AddMarkerUIState @117, shown again by MulticastDetach @8372:
//! GarnetMarker2.star_hidden). BeamParticleSystem (PS_Garnet_TetherBeam; AutoAttach SnapToTarget to the Capsule's
//! socket Gem, which a capsule lacks: at the actor): Color = MakeColor(1, 0, team, 0) @2832-2884, MakeColor(
//! SelectFloat(0, 1, Activated), 0, team, 0) at each activation toggle @7703-7857 (is_activated); ReceiveTick @3955 /
//! @4107 SetBeamSourcePoint(1 and 2, InstigatingGarnet's location) (SelectVector(ActivatedPosition, it, false): always
//! the Garnet's; beam_x / beam_y). The ally attach @3364-3687: PS_Garnet_Victory_Sparkles on the ally's root
//! (KeepRelativeOffset, scale 1, auto destroy), Team = SelectFloat(0.6, the ally's colour value, IsLocalOnline), seen
//! as attached_fighter becoming another fighter.
//! Deviation: a second attach of the same ally is not seen.
//!
//! GarnetLightningBolt_C: its ParticleSystem has no template; ParticleSystem2 (PS_Garnet_Projectile under StaticMesh)
//! is drawn as the class's second component (`more`), Team = SelectFloat(0.6, ...) @861-970 (the shared team).
//!
//! World actors:
//! - GarnetElectricZone_C: VFX (PS_Garnet_DropBubble, scale 2.2, on the Capsule root; the actor's rotation is zero:
//!   GarnetLightningBolt's CompSpawnActor @566 passes Conv_VectorToTransform), Team = SelectFloat(0.6, ...)
//!   @2767-2818, while the actor lives. Its charge shows nothing of its own (the graph writes no visual at FullPower
//!   @2916; the charge loop is a sound @2537). DeathVFX PS_GarnetUpSpecialFullChargedHit: the sim's.
//! - GarnetShockwaveHitBox_C (+ Left / Right): VFX (PS_Garnet_NAD_Shockwave, roll -90) under HitBox
//!   (bAbsoluteRotation, (pitch 0, yaw 90, roll 90)), Team = SelectFloat(0.6, the Owner's colour value,
//!   IsLocalOnline) @2163-2214, at the HitBox's place, which the timeline moves @1912 (the sim's hx / hy).
//! - Buff_GarnetStopProjectile on another kit's projectile (any class, so not a ProjRule): CreateVFX @3432-4067:
//!   SpawnEmitterAttached(PS_Garnet_StopProjectile, the stopped actor's root, no socket, rotation 0, scale 1,
//!   KeepRelativeOffset, auto destroy), Size = (FindScale + HurtboxAdditionalBuffer, 0, 0), Team = SelectFloat(0.6,
//!   the stopped actor's Instigator's colour value, IsLocalOnline); ReceiveEndPlay @2042 destroys it.
//!   Deviation: on projectiles only (an Item the buff lands on is not drawn here).
//!
//! Her pawn:
//! - SingingVFX: MulticastIsSigning(true) (@18948-19470) spawns PS_Garnet_SPN_Singing_Activate attached to her root
//!   (no socket, KeepRelativeOffset, scale 1), Team = SelectFloat(0.6, her colour value, IsLocalOnline); (false)
//!   destroys it. Seen as GarnetSingState.singing_vfx, hidden with her mesh.
//! - The hand glow: Buff_GarnetStackingSinging's stack update with NewStackCount >= ArmorBreakStack (3) @1300 spawns
//!   PS_SharpenBuff_Loop on the active mesh's L_Hand @1674 and R_Hand @2157 (location and rotation 0, scale
//!   GetActiveMeshVFXScale (1 in the port) x ArmorBreakVFXScale (0.75), KeepRelativeOffset, auto destroy; no Team
//!   parameter), added to Particles; an update below it runs RemoveVFX @2500 (all of them go).
//!   Deviation: each update at 3 or more adds a pair and keeps the old ones (Array_AddUnique, no RemoveVFX first):
//!   drawn here as one pair while the count stays at 3 or more.
//!
//! Deviation: a fighter is hidden here when it is dead or invisible; the smoke's AddInvisibility for the viewer
//! is not ported, as mvs-view's place_view.

use std::collections::{BTreeSet, HashMap};

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{ProjectileExt, attach_parent};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::garnet::sing::STACKING_SINGING;
use mvs_sim::chars::garnet::state::garnet_of;
use mvs_sim::chars::garnet::stop_projectile::STOP_PROJECTILE;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::elem::ElemExt;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::{actor_def, buff_def};
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{rot_rows, xyz};
use crate::fx::{Follow, Fx, under};

const SINGING: &str = "PS_Garnet_SPN_Singing_Activate";
const GLOW: &str = "PS_SharpenBuff_Loop";
const HANDS: [&str; 2] = ["L_Hand", "R_Hand"];
const ACTIVATE: &str = "PS_Garnet_GaunletActivate";
const SPARKLES: &str = "PS_Garnet_Victory_Sparkles";
/// ReceiveTick's SetBeamSourcePoint emitters.
const BEAM_EMITTERS: [i32; 2] = [1, 2];
const SHOCKWAVES: [&str; 3] =
    ["GarnetShockwaveHitBox_C", "GarnetShockwaveHitBoxLeft_C", "GarnetShockwaveHitBoxRight_C"];
const ELECTRIC_ZONE: &str = "GarnetElectricZone_C";
const STOP_VFX: &str = "PS_Garnet_StopProjectile";

/// What Garnet's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Garnet {
    /// Per Garnet (fighter id): the singing VFX (None: SpawnEmitterAttached made nothing), the hand glow.
    sing: HashMap<i32, Option<SystemId>>,
    glow: HashMap<i32, Vec<SystemId>>,
    /// Per Garnet: Buff_GarnetStackingSinging's ArmorBreakStack and ArmorBreakVFXScale (None: her data lacks them).
    glow_defs: HashMap<i32, Option<(f64, V3)>>,
    /// Per glove (actor sequence number): the activation burst, Activated seen.
    gloves: HashMap<i32, (Option<SystemId>, bool)>,
    markers: HashMap<i32, Marker>,
    /// Per World actor (its sequence number): the zone's or shockwave's VFX, its base scale and absolute rotation.
    actors: HashMap<i32, ActorVfx>,
    /// Per stopped projectile (its sequence number): Buff_GarnetStopProjectile's VFX.
    stops: HashMap<i32, Option<SystemId>>,
    warned: BTreeSet<String>,
}

#[derive(Debug)]
struct Marker {
    star: Option<SystemId>,
    beam: Option<SystemId>,
    sparkles: Option<SystemId>,
    /// The attached fighter seen (-1 none).
    ally: i32,
    activated: bool,
    team: f64,
}

#[derive(Debug)]
struct ActorVfx {
    h: Option<SystemId>,
    at: mvs_vfx::Attachment,
    /// The emitter's world scale on a root at scale 1.
    base: V3,
}

impl Garnet {
    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Garnet: {ps} is not loaded");
        }
        false
    }

    fn warn(&mut self, s: String) {
        if self.warned.insert(s.clone()) {
            eprintln!("warning: Garnet: {s}");
        }
    }

    fn clear(&mut self, fx: &mut Fx) {
        let hs: Vec<SystemId> = self
            .sing
            .drain()
            .filter_map(|(_, h)| h)
            .chain(self.glow.drain().flat_map(|(_, v)| v))
            .chain(self.actors.drain().filter_map(|(_, a)| a.h))
            .chain(self.stops.drain().filter_map(|(_, h)| h))
            .chain(self.gloves.drain().filter_map(|(_, (h, _))| h))
            .chain(self.markers.drain().flat_map(|(_, m)| [m.star, m.beam, m.sparkles].into_iter().flatten()))
            .collect();
        for h in hs {
            fx.player.kill(h);
        }
    }

    /// garnetPawnFx's place (once a presentation tick).
    fn place_pawn(&mut self, fx: &mut Fx, w: &World) {
        let key = mvs_sim::chars::garnet::KEY;
        for f in w.all_ids() {
            if w.names.str(w.fighter(f).ch) != key {
                continue;
            }
            let fi = w.fighter(f);
            let id = fi.id;
            let dead = fi.life.dead > 0.0;
            let shown = !(fi.life.dead != 0.0 || fi.vis.invisible);
            let on = !dead && garnet_of(w, f).is_some_and(|g| g.sing.singing_vfx);
            if on && !self.sing.contains_key(&id) {
                let h = if self.has(fx, SINGING) {
                    let follow = Follow::Root { id, rel: IDENTITY, up: 0.0, scale: Some([1.0; 3]) };
                    let team = fx.team(id);
                    fx.spawn_follow(w, SINGING, follow, 1.0, &[("Team", team)])
                } else {
                    None
                };
                self.sing.insert(id, h);
            } else if !on && let Some(Some(h)) = self.sing.remove(&id) {
                fx.player.kill(h);
            }
            if let Some(Some(h)) = self.sing.get(&id) {
                fx.player.set_visible(*h, shown);
            }
            let def = self.glow_def(w, f);
            let lit = def.is_some_and(|(at, _)| !dead && sing_stacks(w, f) >= at);
            if lit && !self.glow.contains_key(&id) {
                let sc = def.map_or([1.0; 3], |d| d.1);
                let mut hs = Vec::new();
                if self.has(fx, GLOW) {
                    for s in HANDS {
                        let follow = fx.socket_follow(w, id, Some(s), [0.0; 3], &IDENTITY, sc);
                        hs.extend(fx.attach_to_mesh(w, id, GLOW, s, follow, &[]));
                    }
                }
                self.glow.insert(id, hs);
            } else if !lit && let Some(hs) = self.glow.remove(&id) {
                for h in hs {
                    fx.player.kill(h);
                }
            }
            for &h in self.glow.get(&id).into_iter().flatten() {
                fx.player.set_visible(h, shown);
            }
        }
    }

    /// Buff_GarnetStackingSinging's ArmorBreakStack and ArmorBreakVFXScale from her data; None (no glow, a warning)
    /// where the data lacks them.
    fn glow_def(&mut self, w: &World, f: FighterId) -> Option<(f64, V3)> {
        let id = w.fighter(f).id;
        if let Some(d) = self.glow_defs.get(&id) {
            return *d;
        }
        let d = w.char_data(f).fighter.buffs.as_ref().and_then(|m| m.get(STACKING_SINGING));
        let at = d.and_then(|d| d.get("ArmorBreakStack")).and_then(Value::as_f64);
        let k = d.and_then(|d| d.get("ArmorBreakVFXScale")).and_then(Value::as_obj);
        let g = |n: &str| k.and_then(|k| k.get(n)).and_then(Value::as_f64);
        let r = match (at, g("X"), g("Y"), g("Z")) {
            (Some(at), Some(x), Some(y), Some(z)) => Some((at, [x, y, z])),
            _ => {
                self.warn(format!(
                    "no ArmorBreakStack / ArmorBreakVFXScale of {STACKING_SINGING} in her data: the hand glow is not drawn"
                ));
                None
            }
        };
        self.glow_defs.insert(id, r);
        r
    }

    /// garnetActors' place: the zone's and the shockwaves' VFX, the stopped projectiles'.
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (aid, a) in w.actors.actors.iter() {
            if a.dead {
                continue;
            }
            let cls = w.names.str(a.cls);
            let zone = cls == ELECTRIC_ZONE;
            if !zone && !SHOCKWAVES.contains(&cls) {
                continue;
            }
            seen.push(a.seq);
            let xy = match &a.state {
                WorldActorState::ElectricZone(z) => (z.x, z.y),
                WorldActorState::ShockwaveHitBox(s) => (s.hx, s.hy),
                _ => continue,
            };
            let k = a.size_scale.unwrap_or(1.0);
            let v = match self.actors.remove(&a.seq) {
                Some(v) => v,
                None => self.actor_vfx(fx, w, ActorRef::Actor(aid), a.cls, a.owner_id, !zone),
            };
            let mut v = v;
            v.at.loc[1] = f64::from(-xy.0);
            v.at.loc[2] = f64::from(xy.1);
            // the root's scale (SetActorScale3D from Comp_SizeManager.SetSizeScale: the sim's size scale)
            v.at.scale = Some(std::array::from_fn(|i| v.base[i] * f64::from(k)));
            match v.h {
                Some(h) if fx.player.alive(h) => fx.player.attach(h, &v.at),
                Some(_) => v.h = None,
                None => {}
            }
            // (spawned at the origin: placed here before the player's first tick)
            self.actors.insert(a.seq, v);
        }
        // K2_DestroyActor (or the pool's release) takes the actor's components with it.
        let gone: Vec<i32> = self.actors.keys().copied().filter(|s| !seen.contains(s)).collect();
        for s in gone {
            if let Some(ActorVfx { h: Some(h), .. }) = self.actors.remove(&s) {
                fx.player.kill(h);
            }
        }
        self.place_stops(fx, w);
    }

    /// An emitter of a World actor's component VFX on its root, at its relative transform (under HitBox's absolute
    /// rotation for the shockwave), while the actor lives.
    fn actor_vfx(
        &mut self,
        fx: &mut Fx,
        w: &World,
        a: ActorRef,
        cls: mvs_sim::names::Name,
        owner_id: i32,
        absolute: bool,
    ) -> ActorVfx {
        let at = mvs_vfx::Attachment {
            loc: [0.0; 3],
            rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            scale: Some([1.0; 3]),
        };
        let mut o = ActorVfx { h: None, at, base: [1.0; 3] };
        let Some(d) = actor_def(w, a, cls) else { return o };
        let cs = &d.components;
        let Some(c) = cs.get("VFX").and_then(Value::as_obj) else { return o };
        let Some(ps) = c.get("Template").and_then(Value::as_str).filter(|s| !s.is_empty() && *s != "None") else {
            return o;
        };
        let ps = ps.to_owned();
        if !self.has(fx, &ps) {
            return o;
        }
        let par = attach_parent(cs, "VFX");
        // HitBox's bAbsoluteRotation: its world rotation is its relative one (the parent's own rotation is zero too)
        let par_rot: Rows = if absolute {
            let p = c.get("AttachParent").and_then(Value::as_str).and_then(|n| cs.get(n)).and_then(Value::as_obj);
            rot_rows(p.and_then(|p| p.get("RelativeRotation")))
        } else {
            std::array::from_fn(|i| par.rot[i]).map(f64::from)
        };
        let rot = compose_rows(&rot_rows(c.get("RelativeRotation")), &par_rot);
        let rs = xyz(c.get("RelativeScale3D"), 1.0);
        o.base = std::array::from_fn(|i| f64::from(par.scale[i]) * rs[i]);
        o.at.rot = std::array::from_fn(|r| std::array::from_fn(|c| rot[r * 3 + c]));
        o.at.scale = Some(o.base);
        let team = fx.team(owner_id);
        o.h = fx.player.spawn_attached(&ps, &o.at, 1.0, &[("Team", team)]);
        o
    }

    /// Buff_GarnetStopProjectile's VFX on the projectiles it holds.
    fn place_stops(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (pid, pr) in w.actors.projectiles.iter() {
            // (an actor with no owner has no view)
            if pr.dead || !mvs_sim::actors::projectile::is_owned_projectile(w, pid) {
                continue;
            }
            let Some(el) = pr.elem.as_ref() else { continue };
            // the last live one of the class; CreateVFX @1484 runs on the stop path @927 only (the sim writes
            // find_scale there): the reverse branch and the Heavy destroy draw nothing
            let Some(b) = el.buffs.iter().rev().find(|b| b.class == BuffClass::BuffGarnetStopProjectile) else {
                continue;
            };
            let ElemExt::GarnetStopProjectile(e) = b.ext else { continue };
            if !e.adjusted {
                continue;
            }
            seen.push(pr.seq);
            if self.stops.contains_key(&pr.seq) {
                continue;
            }
            let mut h = None;
            if self.has(fx, STOP_VFX) {
                let team = fx.team(pr.owner_id);
                let buffer = pr.owner.and_then(|o| buff_def(w, ActorRef::Fighter(o), w.lit(STOP_PROJECTILE)));
                let buffer = buffer.and_then(|d| d.get("HurtboxAdditionalBuffer")).and_then(Value::as_f64);
                let mut params = vec![("Team", team)];
                match buffer {
                    Some(b) => params.extend([("Size.X", f64::from(e.find_scale) + b), ("Size.Y", 0.0), ("Size.Z", 0.0)]),
                    None => self.warn(
                        "no HurtboxAdditionalBuffer of Buff_GarnetStopProjectile: its VFX Size is left at the system default"
                            .to_owned(),
                    ),
                }
                let f = Follow::Projectile { seq: pr.seq, rel: [0.0; 3], rel_rot: IDENTITY, scale: [1.0; 3] };
                h = fx.spawn_follow(w, STOP_VFX, f, 1.0, &params);
            }
            self.stops.insert(pr.seq, h);
        }
        let gone: Vec<i32> = self.stops.keys().copied().filter(|s| !seen.contains(s)).collect();
        for s in gone {
            if let Some(Some(h)) = self.stops.remove(&s) {
                fx.player.kill(h); // ReceiveEndPlay @2042
            }
        }
    }
}

/// CurrentStackAmount of her live Buff_GarnetStackingSinging (the last one of the class), 0 without it.
fn sing_stacks(w: &World, f: FighterId) -> f64 {
    let mut n = 0.0;
    for b in &w.fighter(f).buffs.buffs {
        if w.names.str(b.cls) == STACKING_SINGING && b.frames > 0.0 {
            n = b.stacks;
        }
    }
    f64::from(n)
}

impl CharFxHooks for Garnet {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_actors(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_pawn(fx, w);
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
    ("GarnetGauntletsProjectile_C", GLOVE),
    ("GarnetGauntletsProjectileAir_C", GLOVE),
    (
        "GarnetMarker2_C",
        ProjRule {
            made: Some(marker_made),
            tick: Some(marker_tick),
            end: Some(marker_end),
            systems: Some(marker_systems),
            ..ProjRule::NONE
        },
    ),
    ("GarnetLightningBolt_C", ProjRule { more: &["ParticleSystem2"], ..ProjRule::NONE }),
];

const GLOVE: ProjRule = ProjRule {
    made: Some(glove_made),
    tick: Some(glove_tick),
    end: Some(glove_end),
    systems: Some(glove_systems),
    ..ProjRule::NONE
};

fn me(c: &mut CharFx) -> &mut Garnet {
    match c {
        CharFx::Garnet(s) => s,
        _ => unreachable!("a Garnet rule entered by another module"),
    }
}

fn me_ref(c: &CharFx) -> &Garnet {
    match c {
        CharFx::Garnet(s) => s,
        _ => unreachable!("a Garnet rule entered by another module"),
    }
}

fn components<'a>(w: &'a World, st: &ProjFxState) -> Option<&'a Props> {
    let pr = w.actors.projectiles.get(st.id)?;
    actor_def(w, ActorRef::Projectile(st.id), pr.cls).map(|d| &d.components)
}

/// An emitter at component `name`'s place under the actor's root (its attach parents and its own relative transform;
/// its relative scale `scale` over the parents').
fn on_component(
    fx: &mut Fx,
    w: &World,
    st: &ProjFxState,
    ps: &str,
    name: &str,
    params: &[(&str, f64)],
) -> Option<SystemId> {
    let cs = components(w, st)?;
    let c = cs.get(name).and_then(Value::as_obj)?;
    let par = attach_parent(cs, name);
    let own_rot = rot_rows(c.get("RelativeRotation"));
    let crs = xyz(c.get("RelativeScale3D"), 1.0);
    let (rel, rs, _, rel_rot) =
        under(&par, xyz(c.get("RelativeLocation"), 0.0), &own_rot, crs, None, par.scale.map(f64::from));
    fx.spawn_follow(w, ps, Follow::Projectile { seq: st.seq, rel, rel_rot, scale: rs }, 1.0, params)
}

// ---- the gloves

fn glove_made(c: &mut CharFx, _fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    me(c).gloves.insert(st.seq, (None, false));
}

fn glove_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if s.gloves.get(&st.seq).is_none_or(|g| g.1) || !pr.activated {
        return;
    }
    // attached to StaticMesh (its place and scale under the root), rotation 0 relative to it
    let has_mesh = components(w, st).is_some_and(|cs| cs.get("StaticMesh").is_some());
    let h = if has_mesh && s.has(fx, ACTIVATE) {
        let team = fx.team(pr.owner_id);
        on_component(fx, w, st, ACTIVATE, "StaticMesh", &[("Team", team)])
    } else {
        None
    };
    me(c).gloves.insert(st.seq, (h, true));
}

fn glove_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some((Some(h), _)) = me(c).gloves.remove(&st.seq) {
        fx.player.kill(h);
    }
}

fn glove_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).gloves.get(&st.seq).and_then(|g| g.0).into_iter().collect()
}

// ---- GarnetMarker2

fn marker_color(activated: bool, team: f64) -> [f64; 3] {
    color_param([if activated { 0.0 } else { 1.0 }, 0.0, team])
}

fn component_of(
    s: &mut Garnet,
    fx: &mut Fx,
    w: &World,
    st: &ProjFxState,
    name: &str,
    params: &[(&str, f64)],
) -> Option<SystemId> {
    let cs = components(w, st)?;
    let ps = cs.get(name).and_then(Value::as_obj)?.get("Template").and_then(Value::as_str)?.to_owned();
    if !s.has(fx, &ps) {
        return None;
    }
    on_component(fx, w, st, &ps, name, params)
}

fn marker_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let team = fx.team(pr.owner_id);
    let star = component_of(s, fx, w, st, "StarVFX", &[("Team", team)]);
    let k = marker_color(false, team);
    let beam =
        component_of(s, fx, w, st, "BeamParticleSystem", &[("Color.X", k[0]), ("Color.Y", k[1]), ("Color.Z", k[2])]);
    s.markers.insert(st.seq, Marker { star, beam, sparkles: None, ally: -1, activated: false, team });
}

fn marker_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(mut o) = s.markers.remove(&st.seq) else { return };
    marker_update(s, &mut o, fx, w, st);
    s.markers.insert(st.seq, o);
}

fn marker_update(s: &mut Garnet, o: &mut Marker, fx: &mut Fx, w: &World, st: &ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let ProjectileExt::GarnetMarker2(m) = &pr.ext else { return };
    if let Some(h) = o.star {
        fx.player.set_visible(h, !m.star_hidden);
    }
    // the ally attach @3364: the sparkles on its root
    let ai = m.attached_fighter.filter(|&a| Some(a) != pr.owner).map_or(-1, |a| w.fighter(a).id);
    if ai != o.ally {
        o.ally = ai;
        if ai >= 0 && s.has(fx, SPARKLES) {
            if let Some(h) = o.sparkles.take() {
                fx.player.kill(h);
            }
            let follow = Follow::Root { id: ai, rel: IDENTITY, up: 0.0, scale: Some([1.0; 3]) };
            let team = fx.team(ai);
            o.sparkles = fx.spawn_follow(w, SPARKLES, follow, 1.0, &[("Team", team)]);
        }
    }
    let Some(beam) = o.beam else { return };
    if m.is_activated != o.activated {
        o.activated = m.is_activated;
        let k = marker_color(o.activated, o.team);
        fx.player.set_param(beam, "Color.X", k[0]);
        fx.player.set_param(beam, "Color.Y", k[1]);
        fx.player.set_param(beam, "Color.Z", k[2]);
    }
    for e in BEAM_EMITTERS {
        fx.player.set_beam_source_point(beam, e, [0.0, f64::from(-m.beam_x), f64::from(m.beam_y)], 0);
    }
}

fn marker_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).markers.remove(&st.seq) else { return };
    // (the sparkles are on the ally's root, auto destroyed: they play out)
    for h in [o.star, o.beam].into_iter().flatten() {
        fx.player.kill(h);
    }
}

fn marker_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).markers.get(&st.seq).map_or(Vec::new(), |o| [o.star, o.beam].into_iter().flatten().collect())
}
