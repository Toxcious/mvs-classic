//! Black Adam's effects: the ElectricChain's beam and hit burst, the
//! Bolt's thunder beam, C021_ForceField's shield, the EarthSmash victim's decals and the SlowReflect buff's attract
//! on projectiles and items. What the shared paths draw (the classes' components, the projectileDeath one-shots) is
//! not here; the decode notes of each class are in the module.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt, class_p, max_lifetime_at};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::consts::DELTA_SECONDS;
use mvs_sim::events::Event;
use mvs_sim::registry::props::num;
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::att;
use crate::fx::{Follow, Fx};

const CHAIN: &str = "C021_ElectricChain_C";
const STARTER: &str = "C021_ElectricChain_Starter_C";
const FIELD: &str = "C021_ForceField_C";
const PERK_FIELD: &str = "C021_ForceField_Perk_C";
const BOLT: &str = "C021_Bolt_C";
const EARTH_SMASH: &str = "Buff_C021_EarthSmashVictim_C";
const CHAIN_BEAM: &str = "PS_C021_DischargeBeam";
const CHAIN_HIT: &str = "PS_C021_DischargeLink_EndPoint_onHit";
const SHIELD: &str = "PS_C021_SPD_Shield";
const THUNDER: &str = "PS_C021_ThunderSky";
const DECAL: &str = "PS_C021_Impact_Decal";
const WARN_DECAL: &str = "PS_C021_Impact_Warning_Decal";
const QUAKE: &str = "PS_C021_GroundLightnings";
const ATTRACT: &str = "PS_C021_SPN_Attract_onProjectiles";
/// Spawn Emitter Team Color's Offset (300, 0, 0) @145 / @1127 (UE X).
const DECAL_OFFSET: f64 = 300.0;

/// A sim place in UE axes.
fn ue(x: f64, y: f64) -> [f64; 3] {
    [0.0, -x, y]
}

/// Comp_Pawn_SkinManager.GetColorIndex's pair as SelectFloat(0.6, the index, IsLocalOnlinePlayer).
fn team_index(fx: &Fx, id: i32) -> f64 {
    if fx.team(id) == 0.6 { 0.6 } else { f64::from(fx.color_index(id)) }
}

/// One chain's effects.
#[derive(Debug)]
struct ChainFx {
    team: f64,
    beam: Option<SystemId>,
    hit: Option<SystemId>,
    /// The still DoOnce closed.
    still: bool,
    arrived: bool,
}

/// One EarthSmash victim buff's decals (QuakeFx).
#[derive(Debug, Default)]
struct QuakeFx {
    warn: Option<SystemId>,
    decal: Option<SystemId>,
    quake: Option<SystemId>,
}

/// What Black Adam's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct BlackAdam {
    /// Per chain projectile (sequence number).
    chains: IndexMap<i32, ChainFx>,
    /// Per force field (the World actor's sequence number): its shield.
    fields: IndexMap<i32, Option<SystemId>>,
    /// Per EarthSmash victim buff (the fighter's id, the entry's serial).
    quakes: IndexMap<(i32, u32), QuakeFx>,
    /// Per SlowReflect buff (an item's: true; the actor's sequence number; the entry's serial).
    attracts: IndexMap<(bool, i32, u32), Option<SystemId>>,
}

impl CharFxHooks for BlackAdam {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_fields(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_quakes(fx, w);
        self.place_attracts(fx, w);
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
    (CHAIN, ProjRule { made: Some(chain_made), tick: Some(chain_tick), end: Some(chain_end), ..ProjRule::NONE }),
    (
        STARTER,
        ProjRule {
            shown: Some(launched),
            made: Some(chain_made),
            tick: Some(chain_tick),
            end: Some(chain_end),
            ..ProjRule::NONE
        },
    ),
    // C021_Bolt_C is a World actor: entered for own_emitter only (Bolt Visuals @9414, the event of bolt.rs).
    (BOLT, ProjRule { own_emitter: Some(bolt_emitter), ..ProjRule::NONE }),
];

fn me(c: &mut CharFx) -> &mut BlackAdam {
    match c {
        CharFx::BlackAdam(s) => s,
        _ => unreachable!("a Black Adam rule entered by another module"),
    }
}

fn chain_of(pr: &Projectile) -> Option<&mvs_sim::chars::black_adam::chain::Chain> {
    match &pr.ext {
        ProjectileExt::Chain(c) => Some(c),
        _ => None,
    }
}

/// The Starter's ParticleSystem: shown from the parent's ThrowLightning @774.
fn launched(_w: &World, pr: &Projectile) -> bool {
    chain_of(pr).is_some_and(|c| c.launched)
}

fn chain_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let team = team_index(fx, pr.owner_id);
    me(c).chains.insert(st.seq, ChainFx { team, beam: None, hit: None, still: false, arrived: false });
}

/// ChainView.hitActor: IsValid(HitActor) now (a caught projectile that died or is gone is not).
fn hit_actor(w: &World, a: Option<ActorRef>) -> bool {
    match a {
        None => false,
        Some(ActorRef::Projectile(id)) => w.actors.projectiles.get(id).is_some_and(|p| !p.dead),
        Some(_) => true,
    }
}

/// ChainView.tailLeft: the seconds left of TailArrivalTimer while it runs, else of MaxLifetimeTimer; None when
/// neither runs.
fn tail_left(pr: &Projectile, tail_at: f64) -> Option<f64> {
    let age = f64::from(pr.age);
    if tail_at >= 0.0 && age < tail_at {
        return Some((tail_at - age) * f64::from(DELTA_SECONDS));
    }
    let l = max_lifetime_at(pr);
    (l > 0 && pr.age < l).then(|| f64::from(l - pr.age) * f64::from(DELTA_SECONDS))
}

/// The chain's tick: ThrowLightning's beam @124, ReceiveTick's target and tail @820-1898, AfterTailArrival
/// @8071, OnHitVFXMulticast @8664.
// Deviation: ThrowLightning's ParticleSystem.Activate(true) @737 does not reset a running component.
fn chain_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(ch) = chain_of(pr) else { return };
    let Some(o) = me(c).chains.get_mut(&st.seq) else { return };
    let at = ue(st.x, st.y);
    let src = ue(f64::from(ch.src_x), f64::from(ch.src_y));
    // (the store is asked first: a system the store lacks is not asked for again each tick)
    if ch.launched && o.beam.is_none() && fx.player.emitter_count(CHAIN_BEAM).is_some() {
        let t = color_param([1.0, 1.0, o.team]);
        o.beam = fx.player.spawn(
            CHAIN_BEAM,
            src,
            [0.0; 3],
            [1.0; 3],
            &[("Team.X", t[0]), ("Team.Y", t[1]), ("Team.Z", t[2])],
        );
        if let Some(b) = o.beam {
            fx.player.set_beam_source_point(b, 0, src, 0);
            fx.player.set_beam_target_point(b, 0, src, 0);
        }
    }
    if pr.vx != 0.0 || pr.vy != 0.0 {
        if let Some(b) = o.beam {
            fx.player.set_beam_target_point(b, 0, at, 0);
        }
    } else if !o.still {
        o.still = true;
        if hit_actor(w, ch.caught) && o.hit.is_none() {
            o.hit = fx.player.spawn(CHAIN_HIT, at, [0.0; 3], [1.0; 3], &[("Team", o.team)]);
        }
        if let Some(b) = o.beam {
            fx.player.set_beam_target_point(b, 0, at, 0);
        }
    }
    let Some(b) = o.beam else { return };
    if ch.tail_at >= 0.0 && f64::from(pr.age) >= f64::from(ch.tail_at) && !o.arrived {
        o.arrived = true;
        fx.player.set_beam_source_point(b, 0, at, 0);
    }
    if let Some(left) = tail_left(pr, f64::from(ch.tail_at)) {
        let d = (src[1] - at[1]).hypot(src[2] - at[2]);
        let speed = num(class_p(w, pr), "TailSpeed").unwrap_or(0.0);
        let t = if d > 0.0 { (left / (d / f64::from(speed))).clamp(0.0, 1.0) } else { 0.0 };
        fx.player.set_beam_source_point(b, 0, [0.0, at[1] + (src[1] - at[1]) * t, at[2] + (src[2] - at[2]) * t], 0);
    }
}

fn chain_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(o) = me(c).chains.shift_remove(&st.seq) {
        for h in [o.beam, o.hit].into_iter().flatten() {
            fx.player.kill(h);
        }
    }
}

/// Bolt Visuals @9414: PS_C021_ThunderSky at (0, 0, 0), source the actor @9515, target Hit_Location @9570, Team
/// (1, 1, the colour index) @9885 (a plain vector: no colour conversion).
fn bolt_emitter(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, fighter, x, y, to, .. } = *e else { return false };
    let Some(to) = to else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == THUNDER) {
        return false;
    }
    let team = team_index(fx, fighter);
    let params = [("Team.X", 1.0), ("Team.Y", 1.0), ("Team.Z", team)];
    if let Some(b) = fx.player.spawn(THUNDER, [0.0; 3], [0.0; 3], [1.0; 3], &params) {
        fx.player.set_beam_source_point(b, 0, ue(f64::from(x), f64::from(y)), 0);
        fx.player.set_beam_target_point(b, 0, ue(f64::from(to[0]), f64::from(to[1])), 0);
    }
    true
}

impl BlackAdam {
    /// actors(): C021_ForceField_C's and C021_ForceField_Perk_C's PS_Shield at the field, yaw -90 facing right and
    /// +90 facing left, scale the Sphere's, Team @4896-5109.
    // Deviation: before Timeline_0's first update the build's shield would be scale 1; here it is the sim's sphere
    // scale.
    fn place_fields(&mut self, fx: &mut Fx, w: &World) {
        let h = std::f64::consts::FRAC_PI_2;
        let mut seen = Vec::new();
        for (_, a) in w.actors.actors.iter() {
            if a.dead {
                continue;
            }
            let f = match &a.state {
                WorldActorState::ForceField(f) => f,
                WorldActorState::PerkForceField(p) => &p.base,
                _ => continue,
            };
            let cls = w.names.str(a.cls);
            if cls != FIELD && cls != PERK_FIELD {
                continue;
            }
            seen.push(a.seq);
            let rows = rotation_rows(0.0, if f.facing > 0.0 { -h } else { h }, 0.0);
            let at = att(ue(f64::from(f.x), f64::from(f.y)), &rows, Some([f64::from(f.sphere_scale); 3]));
            match self.fields.get(&a.seq) {
                Some(Some(s)) => fx.player.attach(*s, &at),
                Some(None) => {}
                None => {
                    let team = team_index(fx, a.owner_id);
                    let s = fx.player.spawn_attached(SHIELD, &at, 1.0, &[("Team", team)]);
                    self.fields.insert(a.seq, s);
                }
            }
        }
        let gone: Vec<i32> = self.fields.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(Some(s)) = self.fields.shift_remove(&k) {
                fx.player.kill(s);
            }
        }
    }

    /// Buff_C021_EarthSmashVictim_C: Spawn Emitter Team Color @8321-8905 (the decals at the buff actor + (300, 0, 0),
    /// pitch the surface angle, yaw 90), Spawn Earthquake Visual Multicast @6346; every one destroyed at the buff's
    /// end @457-516. The Instigator is the buff's source fighter.
    fn place_quakes(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for f in w.all_ids() {
            let fi = w.fighter(f);
            for b in &fi.buffs.buffs {
                if w.names.str(b.cls) != EARTH_SMASH {
                    continue;
                }
                let BuffExt::EarthSmashVictim(e) = &b.ext else { continue };
                let key = (fi.id, b.serial);
                seen.push(key);
                let by = b.src.map_or(fi.id, |s| w.fighter(s).id);
                let team = team_index(fx, by);
                let at = [DECAL_OFFSET, f64::from(-e.x), f64::from(e.y)];
                let rot = [e.angle, 90.0, 0.0];
                let r = self.quakes.entry(key).or_default();
                if e.warned && r.warn.is_none() {
                    r.warn = fx.player.spawn(WARN_DECAL, at, rot.map(f64::from), [1.0; 3], &[("Team", team)]);
                }
                if e.burst && r.decal.is_none() {
                    let s = 0.6 + 0.4 * e.pct;
                    r.decal = fx.player.spawn(DECAL, at, rot.map(f64::from), [f64::from(s); 3], &[("Team", team)]);
                }
                let q = att(ue(f64::from(e.x), f64::from(e.y)), &IDENTITY, Some([1.0; 3]));
                match r.quake {
                    Some(h) => fx.player.attach(h, &q),
                    None if e.quake => r.quake = fx.player.spawn_attached(QUAKE, &q, 1.0, &[("Team", team)]),
                    None => {}
                }
            }
        }
        let gone: Vec<(i32, u32)> = self.quakes.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(r) = self.quakes.shift_remove(&k) {
                for h in [r.warn, r.decal, r.quake].into_iter().flatten() {
                    fx.player.kill(h);
                }
            }
        }
    }

    /// Buff_C021_SlowReflectProjectile_C: CreateVFX @2678, PS_C021_SPN_Attract_onProjectiles on the owner's root,
    /// Team of the owner's Instigator @3313; destroyed at the buff's end @839.
    fn place_attracts(&mut self, fx: &mut Fx, w: &World) {
        let reflect = |e: Option<&mvs_sim::combat::elem::ElemState>| {
            e.and_then(|e| e.buffs.iter().find(|b| b.class == BuffClass::BuffC021SlowReflectProjectile))
                .map(|b| b.serial)
        };
        let mut on: Vec<((bool, i32, u32), ActorRef, i32)> = Vec::new();
        for (id, pr) in w.actors.projectiles.iter() {
            if let (Some(s), false) = (reflect(pr.elem.as_ref()), pr.dead) {
                on.push(((false, pr.seq, s), ActorRef::Projectile(id), pr.owner_id));
            }
        }
        for (id, it) in w.actors.items.iter() {
            if let (Some(s), false) = (reflect(it.elem.as_ref()), it.dead) {
                // (an attracted item is a thrown one: owned)
                let owner = it.owner.map_or(-1, |f| w.fighter(f).id);
                on.push(((true, it.seq, s), ActorRef::Item(id), owner));
            }
        }
        for &(key, a, owner) in &on {
            if owner < 0 || self.attracts.contains_key(&key) {
                continue;
            }
            let team = fx.team(owner);
            let f = Follow::Actor { turned: true, a, rel: [0.0; 3], rel_rot: IDENTITY, scale: [1.0; 3] };
            let h = fx.spawn_follow(w, ATTRACT, f, 1.0, &[("Team", team)]);
            self.attracts.insert(key, h);
        }
        let gone: Vec<(bool, i32, u32)> =
            self.attracts.keys().copied().filter(|k| !on.iter().any(|(o, _, _)| o == k)).collect();
        for k in gone {
            if let Some(Some(h)) = self.attracts.shift_remove(&k) {
                fx.player.kill(h);
            }
        }
    }

    fn clear(&mut self, fx: &mut Fx) {
        for (_, o) in self.chains.drain(..) {
            for h in [o.beam, o.hit].into_iter().flatten() {
                fx.player.kill(h);
            }
        }
        for (_, s) in self.fields.drain(..) {
            if let Some(s) = s {
                fx.player.kill(s);
            }
        }
        for (_, r) in self.quakes.drain(..) {
            for h in [r.warn, r.decal, r.quake].into_iter().flatten() {
                fx.player.kill(h);
            }
        }
        for (_, h) in self.attracts.drain(..) {
            if let Some(h) = h {
                fx.player.kill(h);
            }
        }
    }
}
