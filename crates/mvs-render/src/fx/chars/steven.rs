//! Steven Universe's effects (the CharFx hooks and the classes' projectile rules):
//! the Watermelon minion's effects, the bubbles on fighters, his World actors'
//! emitters and the heal zone's decal, and the rules of StevenTetherStunProjectile_C and
//! Steven_BubbleCharge_C. The shield actor's mesh and HealGrenade_C's rotation are mvs-view's
//! (chars/steven.rs); StevenShieldProjectile_C's component is the shared path's.
//!
//! Comp_StevenShield:
//!   ShieldActivated @479: ShieldRemoved (RemoveVFX @1125), ActivateShield, SpawnVFX @507. SpawnVFX @366:
//!   SpawnEmitterAttached(PS_StevenPassive_Bubble, the active mesh, Root, (0, 0, 150), rot (0, 0, 0), scale
//!   GetActiveMeshVFXScale x GetVFXScale, KeepRelativeOffset, auto destroy), Team = SelectFloat(0.6, the owner's
//!   colour index, IsLocalOnline) @776. GetVFXScale: CurrentShieldCharges 0 / 1 / 2 / 3 -> 0 / 0.55 / 0.7 / 0.85.
//!   UpdateShieldChargesMulticast @1233: charges = n; while Shielding UpdateVFX @1270 (SetRelativeScale3D of the same
//!   product @278); with Increase, ShieldActivated again @1294 (the emitter spawned anew).
//!   ShieldRemoved @1111: RemoveVFX (K2_DestroyComponent: its particles go at once).
//!   So the bubble is drawn while `shield.shielding` holds, scaled by the charges each tick, and spawned anew when the
//!   charges rise while shielding.
//! Buff_StevenSharedShield, on the ally:
//!   ActivateShieldBuffMulticast @1620 -> @1331: charges > 0 -> the armor pack, SpawnVFX @1605. SpawnVFX @598: the same
//!   system on the owner's active mesh, Root, location (0, 0, the capsule's scaled half height / GetSizeScale), rot
//!   (0, 0, 0), scale GetActiveMeshVFXScale x GetVFXScale (the buff's CurrentShieldCharges), Team of the owner (the
//!   ally) @1091.
//!   UpdateShieldBuffMulticast @1812: charges = n, UpdateVFX @1839: SetWorldScale3D(GetVFXScale x the root capsule's
//!   CapsuleHalfHeight (unscaled) / 200) @400 (a world scale: no longer the mesh's).
//!   RemoveShieldsMulticast / ReceiveDestroyed @1652: RemoveVFX @1768 (every emitter of ShieldParticle destroyed).
//! Buff_StevenBubbleEnemy, on the bubbled fighter:
//!   ReceiveBeginPlay -> SpawnVFX @2784 -> @2853: PS_StevenDebuffBubble_Activate @3364 (BubbleActivate) and
//!   PS_StevenDebuff_BubbleRose @4191 (BubbleVisual), both on the active mesh's Root at (0, 0, the scaled half height /
//!   GetSizeScale), rot (0, 0, 0), scale GetActiveMeshVFXScale (x 1, and x BubbleVFXScaling (0.85) for the rose), no
//!   Team parameter.
//!   ReceiveTick @6526: BubbleVisual.K2_SetWorldRotation(rot (0, 90, 0)) each tick: the rose keeps a world yaw of 90.
//!   ReceiveEndPlay -> EndVFX @5371: PS_StevenDebuffBubble_Activate again @5936 (the same place and scale, kept by
//!   nothing: it runs out), BubbleActivate and BubbleVisual K2_DestroyComponent @6068 / @6141.
//! None of the three hands its emitters to Comp_FighterVisibilityManager.AddVFX: the pawn's hiding does not hide them.
//! GetActiveMeshVFXScale is 1 for every fighter of the port.
//! Deviation: a second ShieldActivated while shielding with no rise in the charges (a notify's begin with no end
//!   between) would spawn the bubble anew; the sim keeps no spawn count, so the running emitter goes on here.
//! Unverified: the shared copy's UpdateVFX is taken to have run once the buff's charges differ from those it spawned
//!   with (UpdateShieldBuffMulticast with the same count leaves the scale rule of the spawn here).
//! Deviation: ActivateShieldBuffMulticast on a live buff that already spawned (SpawnAndActivateAllyShield again on an
//!   entry of AllySharedShieldBuff, e.g. OnAllyRespawn) adds a second emitter in the build (ShieldParticle keeps
//!   both); one is drawn here. A buff that began with 0 charges spawns none and draws none (as the build).
//! Deviation: OnMeshChanged / OnActiveMeshChanged (a skin's mesh swap: the emitters spawned again on the new mesh)
//!   are not followed: the port swaps no active mesh in a match.
//!
//! Steven_ShieldActor_C / _Up_C: PS_Impact (PS_StevenShield_ImpactBurst, on the Capsule root at (0, 0, -20), rotation
//! yaw 30; the _Up child's (pitch 90, yaw 0) from its own template): the PSC's native bAutoActivate (its ctor
//! 142bddbb0) activates it at the actor's first spawn, before BeginPlay, whose SetFloatParameter(Team,
//! SelectFloat(0.6, the Instigator's colour value, IsLocalOnline)) @2286 comes before its first tick; the pool's reuse
//! replays it (InitCollisionsFromPool @71 SetVisibility, @105 Activate, from @4156). The sim takes the pool's release as
//! a destroy, so every shield seen is a spawn or a reuse: the burst plays once where the record is made, attached to
//! the actor (its scale with the mesh's, sizeScale), cut with the actor's end. ReceiveEndPlay @3445 (the _Up child does
//! not override it): PS_StevenShield_Outro at the actor's location with the SkeletalMesh's world rotation, scale 1,
//! Team = SelectFloat(0.6, the Instigator's colour index, IsLocalOnline) @3723: the sim posts no event for it, so it
//! plays where the actor leaves World.actors (any end; not at the match's clear).
//! Unverified: Comp_Spawner's spawn rotation is taken as the shield's own facing (mvs-view's note).
//!
//! HealOverTimeZoneBP_C: FogVisuals @3095: PS_Steven_HealAura attached to the Capsule at (0, 0, -120), Team =
//! float(the owner's GetColorIndex) @3389, from the first tick the actor is seen to its end. Not drawn: the outro's
//! FogParticle Alpha (Outro @2579, played from @5290 DeactivateFog, which nothing calls).
//! Its Decal: DecalSize (128, 200, 400), RelativeLocation Z -150 and RelativeRotation Pitch 90 under the Capsule root,
//! so it projects along the world Z through a box 256 cm deep whose centre is 150 under the zone; with the actor's yaw
//! (-90 facing right, +90 left; the zone takes the grenade's rotation through CompSpawnActor's transform @407) the
//! decal's local Y lies along the depth and its local Z along the play axis (fx/shared/firewall.rs has the same axes).
//! BeginPlay @6994-7238: a dynamic instance with Team = float(the owner's GetColorIndex), then Timeline_0 @7292 writes
//! its Dissolve (@7523), a parameter the material does not have: nothing to draw. Alpha stays the material's 1.
//! Unverified: the zone's rotation is its owner's facing at the first sight, as the shield's (the sim's zone keeps
//! none). Placed once: the zone does not move.

use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorRef, ProjectileId};
use mvs_sim::chars::steven::state::steven_of;
use mvs_sim::combat::BuffExt;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::{actor_def, buff_def};
use mvs_sim::fighter::{FighterId, cap_half, facing};
use mvs_sim::registry::props::num;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::{Attachment, SystemId};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, rot_rows, text, xyz};
use crate::fx::shared::minion::Minions;
use crate::fx::{FloorDecal, Follow, Fx, Scale};

/// The Watermelon's character (its data is Steven's) and its skin (PlayVFX's VFX_Skin key).
const MINION_KEY: &str = mvs_sim::chars::steven::KEY;
const WATERMELON_SKIN: &str = "WatermelonSteven";

const SHIELD_PS: &str = "PS_StevenPassive_Bubble";
const ACTIVATE_PS: &str = "PS_StevenDebuffBubble_Activate";
const ROSE_PS: &str = "PS_StevenDebuff_BubbleRose";
const SHARED: &str = "Buff_StevenSharedShield_C";
const BUBBLE: &str = "Buff_StevenBubbleEnemy_C";
const ROOT: &str = "Root";
/// SpawnVFX @366.
const SHIELD_Z: f64 = 150.0;

const SHIELD: &str = "Steven_ShieldActor_C";
const SHIELD_UP: &str = "Steven_ShieldActor_Up_C";
const ZONE: &str = "HealOverTimeZoneBP_C";
const IMPACT: &str = "PS_Impact";
/// FogVisuals @3095.
const AURA: &str = "PS_Steven_HealAura";
const AURA_Z: f64 = -120.0;
/// Steven_ShieldActor ReceiveEndPlay @3445.
const OUTRO: &str = "PS_StevenShield_Outro";
pub(crate) const HEAL_DECAL: &str = "StevenHealDecal_MAT";

/// StevenTetherStunProjectile_C's components and emitters.
const TIP: &str = "PS_TetherProjectile";
const BEAM: &str = "BeamParticleSystem";
const SOURCE: &str = "PS_StevenBeam_Point";
const GEM: &str = "Gem";
/// AttachBeam's SetBeamSourcePoint emitters.
const BEAM_EMITTERS: [i32; 2] = [1, 2];
const POINTS: &str = "PS_TetherPoints";
const ALLY_POINT: &str = "PS_StevenBeam_AllyPoint";
/// SpawnAttachPoint @234.
const POINTS_AT: V3 = [10.0, 0.0, 0.0];
const BURST: &str = "PS_StevenNANAIR_BubbleBurst";
const SPINE: &str = "fml_un_C_spine_c";
/// Steven_BubbleCharge's ParticleSystem's RelativeScale3D.
const BUBBLE_PS_SCALE: f64 = 0.1;

/// GetVFXScale of both shield classes.
fn charge_scale(n: f64) -> f64 {
    if n == 1.0 {
        0.55
    } else if n == 2.0 {
        0.7
    } else if n == 3.0 {
        0.85
    } else {
        0.0
    }
}

/// Steven's own bubble.
#[derive(Debug, Default)]
struct Own {
    h: Option<SystemId>,
    f: Option<Follow>,
    /// The charges of the last tick it was drawn at.
    charges: f64,
}

/// The shared copy on an ally.
#[derive(Debug, Default)]
struct Shared {
    /// The buff the emitter belongs to (its serial).
    b: Option<u32>,
    h: Option<SystemId>,
    f: Option<Follow>,
    /// The charges it spawned with.
    spawned: f64,
    /// UpdateVFX has run (the Unverified note at the top).
    updated: bool,
    charges: f64,
}

/// The bubble on an enemy.
#[derive(Debug, Default)]
struct Bubble {
    b: Option<u32>,
    rose: Option<SystemId>,
    rose_f: Option<Follow>,
    activate: Option<SystemId>,
    /// The relative Z at the spawn.
    z: f64,
}

/// One fighter's bubbles (own, shared and bubbles entries).
#[derive(Debug, Default)]
struct Bubbles {
    own: Own,
    shared: Shared,
    bubble: Bubble,
}

/// A World actor's record: the shield's impact burst and the outro's place, or the zone's aura and
/// decal.
#[derive(Debug)]
enum ActorView {
    Shield {
        impact: Option<SystemId>,
        /// PS_Impact's relative location and its rows (the template's under the actor's yaw).
        rel: V3,
        yaw: Rows,
        rot: Rows,
        /// The actor's place of the latest tick.
        loc: V3,
        /// The mesh's world rotator (degrees: its relative one under the actor's yaw), for the outro.
        outro: V3,
        owner: i32,
    },
    Zone {
        h: Option<SystemId>,
        decal: Option<FloorDecal>,
    },
}

/// A tether's record.
#[derive(Debug)]
struct Tether {
    id: ProjectileId,
    tip: Option<SystemId>,
    beam: Option<SystemId>,
    source: Option<SystemId>,
    /// PS_TetherPoints (SpawnAttachPoint).
    points: Option<SystemId>,
    /// ConnectedVFX (AttachFXMulticast).
    ally_point: Option<SystemId>,
    /// EndCollision (EndVFX @2095).
    end_burst: Option<SystemId>,
    pinned: bool,
    attached: bool,
}

/// What Steven Universe's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Steven {
    melons: Minions,
    /// A Steven is in the match (stevenFx: no hook draws otherwise); None until the first look.
    mine: Option<bool>,
    /// Per fighter id.
    bubbles: HashMap<i32, Bubbles>,
    /// The end's PS_StevenDebuffBubble_Activate, kept for the match's end alone.
    bursts: Vec<SystemId>,
    /// Per World actor (its sequence number), in spawn order.
    actors: IndexMap<i32, Option<ActorView>>,
    /// Per tether (its sequence number).
    tethers: HashMap<i32, Tether>,
    warned: BTreeSet<String>,
}

impl CharFxHooks for Steven {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        if self.mine(w) {
            self.place_actors(fx, w);
        }
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        if !self.mine(w) {
            return;
        }
        self.melons.place(fx, w, mvs_sim::chars::steven::KEY, MINION_KEY, WATERMELON_SKIN);
        self.place_bubbles(fx, w);
    }

    fn frame(&mut self, fx: &mut Fx, _w: &World, _alpha: f64) {
        for v in self.actors.values() {
            if let Some(ActorView::Zone { decal: Some(d), .. }) = v {
                fx.decals.push(d.clone());
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.melons.reset(fx);
        self.clear_actors(fx);
        self.clear_bubbles(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.melons.end(fx);
        self.clear_actors(fx);
        self.clear_bubbles(fx);
    }

    fn minion_event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        self.melons.event(fx, w, e, MINION_KEY, WATERMELON_SKIN)
    }
}

impl Steven {
    fn mine(&mut self, w: &World) -> bool {
        // (a mode's pawns come after the start: the answer latches once a Steven is in the match)
        if self.mine.is_none()
            && w.pawns
                .as_deref()
                .unwrap_or(&w.players)
                .iter()
                .any(|&f| w.names.str(w.fighter(f).ch) == mvs_sim::chars::steven::KEY)
        {
            self.mine = Some(true);
        }
        self.mine == Some(true)
    }

    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Steven: {ps} is not loaded");
        }
        false
    }

    // ---- the bubbles on fighters

    /// Each fighter's own bubble, shared copy and enemy bubble.
    fn place_bubbles(&mut self, fx: &mut Fx, w: &World) {
        for &f in w.pawns.as_deref().unwrap_or(&w.players).iter().filter(|&&f| !w.fighter(f).life.is_minion) {
            let id = w.fighter(f).id;
            let mut e = self.bubbles.remove(&id).unwrap_or_default();
            self.place_own(fx, w, f, id, &mut e.own);
            self.place_shared(fx, w, f, id, &mut e.shared);
            self.place_bubble(fx, w, f, id, &mut e.bubble);
            self.bubbles.insert(id, e);
        }
        // a pawn that left the world ends its bubbles
        while let Some(id) = self.bubbles.keys().find(|&&id| w.by_id(id).is_none()).copied() {
            if let Some(e) = self.bubbles.remove(&id) {
                for h in [e.own.h, e.shared.h, e.bubble.activate, e.bubble.rose].into_iter().flatten() {
                    fx.player.kill(h);
                }
            }
        }
    }

    fn place_own(&mut self, fx: &mut Fx, w: &World, f: FighterId, id: i32, o: &mut Own) {
        let s = steven_of(w, f).map(|s| &s.shield);
        match s {
            Some(s) if s.shielding && charge_scale(f64::from(s.charges)) > 0.0 => {
                // the charges rose while shielding: ShieldActivated again @1294 (a new emitter)
                if s.charges > mvs_sim::math::narrow(o.charges)
                    && let Some(h) = o.h.take()
                {
                    fx.player.kill(h);
                }
                o.charges = f64::from(s.charges);
                if o.h.is_none() && self.has(fx, SHIELD_PS) {
                    // SpawnVFX @366 on Steven: the scale read each tick (UpdateVFX's SetRelativeScale3D)
                    let fl = fx.socket_follow(w, id, Some(ROOT), [0.0, 0.0, SHIELD_Z], &IDENTITY, [1.0; 3]);
                    let team = fx.team(id);
                    o.h = spawn_on_mesh(fx, w, id, SHIELD_PS, &fl, &[("Team", team)], |a| own_scale(a, o.charges));
                    o.f = Some(fl);
                }
            }
            _ => {
                if let Some(h) = o.h.take() {
                    fx.player.kill(h);
                }
            }
        }
        if let (Some(h), Some(fl)) = (o.h, &o.f) {
            let k = o.charges;
            move_to(fx, w, h, fl, |a| own_scale(a, k));
        }
    }

    fn place_shared(&mut self, fx: &mut Fx, w: &World, f: FighterId, id: i32, e: &mut Shared) {
        let sb = live(w, f, SHARED);
        let n = sb.map_or(0.0, |b| match &b.ext {
            BuffExt::StevenSharedShield(s) => s.charges,
            _ => 0.0,
        });
        let serial = sb.map(|b| b.serial);
        if serial != e.b {
            if let Some(h) = e.h.take() {
                fx.player.kill(h);
            }
            e.b = serial;
            e.updated = false;
            e.spawned = f64::from(n);
            e.charges = f64::from(n);
            if serial.is_some() && n > 0.0 && self.has(fx, SHIELD_PS) {
                // SpawnVFX @598 on the ally
                let fm = w.fighter(f);
                let z = cap_half(w, f) / fm.body.size_scale;
                let fl = fx.socket_follow(w, id, Some(ROOT), [0.0, 0.0, f64::from(z)], &IDENTITY, [1.0; 3]);
                let team = fx.team(id);
                let r = shared_rule(e, f64::from(fm.capsule.cap_half_u));
                e.h = spawn_on_mesh(fx, w, id, SHIELD_PS, &fl, &[("Team", team)], |a| shared_scale(a, r));
                e.f = Some(fl);
            }
        } else if serial.is_some() {
            if f64::from(n) != e.spawned {
                e.updated = true;
            }
            e.charges = f64::from(n);
        }
        if let (Some(h), Some(fl)) = (e.h, &e.f) {
            let r = shared_rule(e, f64::from(w.fighter(f).capsule.cap_half_u));
            move_to(fx, w, h, fl, |a| shared_scale(a, r));
        }
    }

    fn place_bubble(&mut self, fx: &mut Fx, w: &World, f: FighterId, id: i32, u: &mut Bubble) {
        let bb = live(w, f, BUBBLE).map(|b| b.serial);
        if bb != u.b {
            if u.b.is_some() {
                self.end_bubble(fx, w, id, u, true);
            }
            if bb.is_some() {
                u.b = bb;
                self.begin_bubble(fx, w, f, id, u);
            }
        }
        if let (Some(h), Some(fl)) = (u.rose, &u.rose_f) {
            move_to(fx, w, h, fl, rose_place);
        }
    }

    /// The bubble's place: Root, (0, 0, the scaled half height / GetSizeScale), rot 0.
    fn activate(&mut self, fx: &mut Fx, w: &World, id: i32, z: f64) -> Option<SystemId> {
        if !self.has(fx, ACTIVATE_PS) {
            return None;
        }
        let fl = fx.socket_follow(w, id, Some(ROOT), [0.0, 0.0, z], &IDENTITY, [1.0; 3]);
        fx.attach_to_mesh(w, id, ACTIVATE_PS, ROOT, fl, &[])
    }

    fn begin_bubble(&mut self, fx: &mut Fx, w: &World, f: FighterId, id: i32, u: &mut Bubble) {
        u.z = f64::from(cap_half(w, f) / w.fighter(f).body.size_scale);
        u.activate = self.activate(fx, w, id, u.z);
        if !self.has(fx, ROSE_PS) {
            return;
        }
        let cls = w.names.get(BUBBLE);
        let k = cls
            .and_then(|c| buff_def(w, ActorRef::Fighter(f), c))
            .and_then(|d| d.get("BubbleVFXScaling"))
            .and_then(Value::as_obj)
            .and_then(|o| num(o, "X"))
            .unwrap_or(0.85);
        let fl = fx.socket_follow(w, id, Some(ROOT), [0.0, 0.0, u.z], &IDENTITY, [f64::from(k); 3]);
        u.rose = spawn_on_mesh(fx, w, id, ROSE_PS, &fl, &[], rose_place);
        u.rose_f = Some(fl);
    }

    /// EndVFX @5371.
    fn end_bubble(&mut self, fx: &mut Fx, w: &World, id: i32, u: &mut Bubble, burst: bool) {
        if burst && let Some(h) = self.activate(fx, w, id, u.z) {
            self.bursts.retain(|h| fx.player.alive(*h));
            self.bursts.push(h);
        }
        for h in [u.activate.take(), u.rose.take()].into_iter().flatten() {
            fx.player.kill(h);
        }
        u.rose_f = None;
        u.b = None;
    }

    fn clear_bubbles(&mut self, fx: &mut Fx) {
        for e in self.bubbles.values_mut() {
            let hs = [e.own.h.take(), e.shared.h.take(), e.bubble.activate.take(), e.bubble.rose.take()];
            for h in hs.into_iter().flatten() {
                fx.player.kill(h);
            }
            e.shared.b = None;
            e.bubble.b = None;
            e.bubble.rose_f = None;
        }
        for h in self.bursts.drain(..) {
            fx.player.kill(h);
        }
    }

    // ---- the World actors

    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (aid, a) in w.actors.actors.iter() {
            if a.dead {
                continue;
            }
            let cls = w.names.str(a.cls);
            if cls != SHIELD && cls != SHIELD_UP && cls != ZONE {
                continue;
            }
            seen.push(a.seq);
            if !self.actors.contains_key(&a.seq) {
                let v = self.make_actor(fx, w, ActorRef::Actor(aid));
                self.actors.insert(a.seq, v);
            }
            let k = a.size_scale.unwrap_or(1.0);
            match (self.actors.get_mut(&a.seq), &a.state) {
                (
                    Some(Some(ActorView::Shield { impact, rel, yaw, rot, loc, .. })),
                    WorldActorState::StevenShield(s),
                ) => {
                    loc[1] = f64::from(-s.x);
                    loc[2] = f64::from(s.y);
                    if let Some(h) = *impact {
                        let d = mul_rows(*rel, yaw);
                        let at = att([0, 1, 2].map(|i| loc[i] + f64::from(k) * d[i]), rot, Some([f64::from(k); 3]));
                        fx.player.attach(h, &at);
                    }
                }
                (Some(Some(ActorView::Zone { h: Some(h), .. })), WorldActorState::HealZone(z)) => {
                    // the root's scale over the aura's place and scale
                    fx.player.attach(
                        *h,
                        &att(
                            [0.0, f64::from(-z.x), f64::from(z.y) + AURA_Z * f64::from(k)],
                            &IDENTITY,
                            Some([f64::from(k); 3]),
                        ),
                    );
                }
                _ => {}
            }
        }
        // K2_DestroyActor takes the actor's components with it.
        let gone: Vec<i32> = self.actors.keys().copied().filter(|s| !seen.contains(s)).collect();
        for s in gone {
            let v = self.actors.shift_remove(&s).flatten();
            if let Some(ActorView::Shield { loc, outro, owner, .. }) = &v {
                let team = fx.team(*owner);
                fx.player.spawn(OUTRO, *loc, *outro, [1.0; 3], &[("Team", team)]);
            }
            drop_view(fx, v);
        }
    }

    fn make_actor(&mut self, fx: &mut Fx, w: &World, r: ActorRef) -> Option<ActorView> {
        let ActorRef::Actor(aid) = r else { return None };
        let a = w.actors.actors.get(aid)?;
        let cs = actor_def(w, r, a.cls).map(|d| &d.components);
        match &a.state {
            WorldActorState::StevenShield(s) => {
                let h = std::f64::consts::FRAC_PI_2;
                let yaw = rotation_rows(0.0, if s.facing < 0.0 { h } else { -h }, 0.0);
                let mesh = cs.and_then(|c| c.get("SkeletalMesh")).and_then(Value::as_obj);
                let rr = mesh.and_then(|m| m.get("RelativeRotation")).and_then(Value::as_obj);
                let g = |k: &str| rr.and_then(|o| num(o, k)).unwrap_or(0.0);
                // a pure yaw parent: the world rotator is the relative one with the actor's yaw added
                let outro = [g("Pitch"), g("Yaw") + if s.facing < 0.0 { 90.0 } else { -90.0 }, g("Roll")];
                let c = cs.and_then(|c| c.get(IMPACT)).and_then(Value::as_obj);
                let rel = xyz(c.and_then(|c| c.get("RelativeLocation")), 0.0);
                let rot = compose_rows(&rot_rows(c.and_then(|c| c.get("RelativeRotation"))), &yaw);
                let loc = [0.0, -s.x, s.y];
                let mut impact = None;
                if let Some(ps) = c.and_then(|c| text(c, "Template"))
                    && self.has(fx, ps)
                {
                    let k = a.size_scale.unwrap_or(1.0);
                    let d = mul_rows(rel, &yaw);
                    let at =
                        att([0, 1, 2].map(|i| f64::from(loc[i]) + f64::from(k) * d[i]), &rot, Some([f64::from(k); 3]));
                    let team = fx.team(a.owner_id);
                    impact = fx.player.spawn_attached(ps, &at, 1.0, &[("Team", team)]);
                }
                Some(ActorView::Shield {
                    impact,
                    rel,
                    yaw,
                    rot,
                    loc: loc.map(f64::from),
                    outro: outro.map(f64::from),
                    owner: a.owner_id,
                })
            }
            WorldActorState::HealZone(z) => {
                let team = f64::from(fx.color_index(a.owner_id));
                let g = a.size_scale.unwrap_or(1.0);
                let h = if self.has(fx, AURA) {
                    let at = att(
                        [0.0, f64::from(-z.x), f64::from(z.y) + AURA_Z * f64::from(g)],
                        &IDENTITY,
                        Some([f64::from(g); 3]),
                    );
                    fx.player.spawn_attached(AURA, &at, 1.0, &[("Team", team)])
                } else {
                    None
                };
                let decal = cs.and_then(|c| c.get("Decal")).and_then(Value::as_obj).map(|dc| {
                    let f = if facing(w, a.owner) < 0.0 { -1.0 } else { 1.0 };
                    zone_decal(dc, HEAL_DECAL, a.seq, f, f64::from(g), (f64::from(z.x), f64::from(z.y)), team)
                });
                Some(ActorView::Zone { h, decal })
            }
            _ => None,
        }
    }

    fn clear_actors(&mut self, fx: &mut Fx) {
        for (_, v) in self.actors.drain(..) {
            drop_view(fx, v);
        }
    }
}

fn drop_view(fx: &mut Fx, v: Option<ActorView>) {
    match v {
        Some(ActorView::Shield { impact: Some(h), .. }) | Some(ActorView::Zone { h: Some(h), .. }) => fx.player.kill(h),
        _ => {}
    }
}

/// The zone decal (and Velma's grenade decal; `default` the material where the component names none): UE world
/// (X depth, Y = -sim x, Z = sim y): the root, plus the relative location through the actor's rotation; DecalSize x
/// RelativeScale3D x the root scale as the half extents; Team the owner's colour index.
pub(super) fn zone_decal(
    dc: &Props,
    default: &str,
    seq: i32,
    f: f64,
    g: f64,
    (x, y): (f64, f64),
    team: f64,
) -> FloorDecal {
    let rl = xyz(dc.get("RelativeLocation"), 0.0);
    let sz = dc.get("DecalSize").and_then(Value::as_obj);
    let sz = [("X", 128.0), ("Y", 256.0), ("Z", 256.0)].map(|(k, d)| sz.and_then(|o| num(o, k)).unwrap_or(d));
    let sc = xyz(dc.get("RelativeScale3D"), 1.0);
    FloorDecal {
        actor: seq,
        material: text(dc, "DecalMaterial").unwrap_or(default).to_string(),
        loc: [rl[1] * f * g, -(x + rl[0] * f * g), y + rl[2] * g],
        rows: [0.0, 0.0, 1.0, f, 0.0, 0.0, 0.0, f, 0.0],
        extent: [0, 1, 2].map(|k| f64::from(sz[k]) * sc[k] * g),
        team: Some(team),
        alpha: None,
        dissolve: None,
    }
}

/// v times the rows.
fn mul_rows(v: V3, m: &Rows) -> V3 {
    crate::fx::attach::mul_row(v, m)
}

/// The fighter's first live buff of class `cls`.
fn live<'a>(w: &'a World, f: FighterId, cls: &str) -> Option<&'a mvs_sim::combat::FighterBuff> {
    w.fighter(f).buffs.buffs.iter().find(|b| b.frames > 0.0 && w.names.str(b.cls) == cls)
}

/// attachToMesh with a place the module sets each tick (a scale the shared followers do not know): spawned at the
/// follow's place with `adjust` applied, in the mesh's AttachChildren.
fn spawn_on_mesh(
    fx: &mut Fx,
    w: &World,
    id: i32,
    ps: &str,
    fl: &Follow,
    params: &[(&str, f64)],
    adjust: impl Fn(&mut Attachment),
) -> Option<SystemId> {
    let mut a = fx.place(w, fl)?;
    adjust(&mut a);
    let h = fx.player.spawn_attached(ps, &a, 1.0, params)?;
    if let Some(p) = fx.pawns.get_mut(&id) {
        p.mesh_psc.push((ps.to_string(), ROOT.to_string(), h));
    }
    Some(h)
}

/// The system to its follow's place now, with `adjust` applied (before the player's tick).
fn move_to(fx: &mut Fx, w: &World, h: SystemId, fl: &Follow, adjust: impl Fn(&mut Attachment)) {
    if !fx.player.alive(h) {
        return;
    }
    if let Some(mut a) = fx.place(w, fl) {
        adjust(&mut a);
        fx.player.attach(h, &a);
    }
}

/// SpawnVFX @366's scale: the socket's times GetVFXScale.
fn own_scale(a: &mut Attachment, charges: f64) {
    let k = charge_scale(charges);
    if let Some(s) = &mut a.scale {
        for v in s.iter_mut() {
            *v *= k;
        }
    }
}

/// The shared copy's scale rule: Some(world scale) once UpdateVFX has run, else the factor over the socket's.
#[derive(Clone, Copy)]
enum SharedRule {
    World(f64),
    Factor(f64),
}

fn shared_rule(e: &Shared, cap_half_u: f64) -> SharedRule {
    if e.updated {
        // SetWorldScale3D(GetVFXScale x CapsuleHalfHeight / 200) @400
        SharedRule::World(charge_scale(e.charges) * cap_half_u / 200.0)
    } else {
        SharedRule::Factor(charge_scale(e.spawned))
    }
}

/// (the view's mirror sign kept for the world scale)
fn shared_scale(a: &mut Attachment, r: SharedRule) {
    if let Some(s) = &mut a.scale {
        for v in s.iter_mut() {
            *v = match r {
                SharedRule::World(x) if *v < 0.0 => -x,
                SharedRule::World(x) => x,
                SharedRule::Factor(k) => *v * k,
            };
        }
    }
}

/// K2_SetWorldRotation(0, 90, 0) @6526: the world scale stays, without the view's mirror.
fn rose_place(a: &mut Attachment) {
    let r = rotation_rows(0.0, std::f64::consts::FRAC_PI_2, 0.0);
    a.rot = [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]];
    if let Some(s) = &mut a.scale {
        for v in s.iter_mut() {
            *v = v.abs();
        }
    }
}

// ---- the classes' rules

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (
        "StevenTetherStunProjectile_C",
        ProjRule {
            made: Some(tether_made),
            tick: Some(tether_tick),
            end: Some(tether_end),
            systems: Some(tether_systems),
            own_emitter: Some(tether_burst),
            ..ProjRule::NONE
        },
    ),
    (
        "Steven_BubbleCharge_C",
        ProjRule { scale: Some(bubble_scale), own_emitter: Some(bubble_burst), ..ProjRule::NONE },
    ),
];

fn me(c: &mut CharFx) -> &mut Steven {
    match c {
        CharFx::Steven(s) => s,
        _ => unreachable!("a Steven rule entered by another module"),
    }
}

fn me_ref(c: &CharFx) -> &Steven {
    match c {
        CharFx::Steven(s) => s,
        _ => unreachable!("a Steven rule entered by another module"),
    }
}

/// The rule `component` passes: the Team is its caller's.
static NO_TEAM: ProjRule = ProjRule { no_team: true, ..ProjRule::NONE };

/// A particle component of the class's template under its attach parents, under the actor's rotation and root scale
/// (as the shared path's own component), with Team.
fn component(s: &mut Steven, fx: &mut Fx, w: &World, st: &ProjFxState, name: &str, team: f64) -> Option<SystemId> {
    let pr = w.actors.projectiles.get(st.id)?;
    let cs = &actor_def(w, ActorRef::Projectile(st.id), pr.cls)?.components;
    let ps = cs.get(name).and_then(Value::as_obj).and_then(|c| text(c, "Template"))?;
    if !s.has(fx, ps) {
        return None;
    }
    let h = fx.projectile_component(w, pr, cs, name, Some(&NO_TEAM))?;
    fx.player.set_param(h, "Team", team);
    Some(h)
}

/// SpawnEmitterAttached on the actor's root (KeepRelativeOffset, rotation 0, scale 1): at the relative location through
/// the actor's rotation and root scale; `snap`: SnapToTarget, world scale 1.
#[allow(clippy::too_many_arguments)]
fn root_attach(
    s: &mut Steven,
    fx: &mut Fx,
    w: &World,
    st: &ProjFxState,
    ps: &str,
    rel: V3,
    team: f64,
    snap: bool,
) -> Option<SystemId> {
    if !s.has(fx, ps) {
        return None;
    }
    let pr = w.actors.projectiles.get(st.id)?;
    let scale = if snap && pr.scale != 0.0 { [1.0 / pr.scale; 3] } else { [1.0; 3] };
    let f = Follow::Projectile { seq: st.seq, rel, rel_rot: IDENTITY, scale: scale.map(f64::from) };
    fx.spawn_follow(w, ps, f, 1.0, &[("Team", team)])
}

/// The owner's Gem socket's follow.
fn gem(fx: &Fx, w: &World, id: i32) -> Follow {
    fx.socket_follow(w, id, Some(GEM), [0.0; 3], &IDENTITY, [1.0; 3])
}

fn tether_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let i = pr.owner_id;
    // Team Value: SelectFloat(0.6, the colour value, IsLocalOnline)
    let team = fx.team(i);
    let tip = component(s, fx, w, st, TIP, team);
    let beam = component(s, fx, w, st, BEAM, team);
    if let Some(h) = beam {
        let c = color_param([0.0, 1.0, team]);
        fx.player.set_param(h, "Color.X", c[0]);
        fx.player.set_param(h, "Color.Y", c[1]);
        fx.player.set_param(h, "Color.Z", c[2]);
    }
    // TetherSourceVFX @11844 (SnapToTarget: world scale 1)
    let mut source = None;
    if s.has(fx, SOURCE) {
        let mut f = gem(fx, w, i);
        if let Follow::Socket { scale, .. } = &mut f {
            *scale = Scale::Fixed([1.0; 3]);
        }
        source = fx.attach_to_mesh(w, i, SOURCE, GEM, f, &[("Team", team)]);
    }
    let o = Tether {
        id: st.id,
        tip,
        beam,
        source,
        points: None,
        ally_point: None,
        end_burst: None,
        pinned: false,
        attached: false,
    };
    s.tethers.insert(st.seq, o);
}

fn tether_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(mut o) = s.tethers.remove(&st.seq) else { return };
    tether_step(s, &mut o, fx, w, st);
    s.tethers.insert(st.seq, o);
}

fn tether_step(s: &mut Steven, o: &mut Tether, fx: &mut Fx, w: &World, st: &ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let ProjectileExt::StevenTether(t) = &pr.ext else { return };
    let i = pr.owner_id;
    // SpawnAttachPoint @234 (the 0.3 s link, or the ally's overlap when not yet pinned)
    if t.pinned && !o.pinned {
        o.pinned = true;
        let team = f64::from(fx.color_index(i));
        o.points = root_attach(s, fx, w, st, POINTS, POINTS_AT, team, false);
    }
    // AttachFXMulticast @12785: ConnectedVFX, the tip destroyed @13095
    if t.ally.is_some() && !o.attached {
        o.attached = true;
        let team = fx.team(i);
        o.ally_point = root_attach(s, fx, w, st, ALLY_POINT, [0.0; 3], team, false);
        if let Some(h) = o.tip.take() {
            fx.player.kill(h);
        }
    }
    // EndVFX @13327: the tip (@13370) and TetherSourceVFX (@13446) go at the burst, 0.5 s before the actor
    if t.end_vfx {
        for h in [o.tip.take(), o.source.take()].into_iter().flatten() {
            fx.player.kill(h);
        }
    }
    let Some(h) = o.beam else { return };
    // Unverified: the beam's source is set every tick; the class's TickInterval is unread.
    if let Some(a) = fx.place(w, &gem(fx, w, i)) {
        for e in BEAM_EMITTERS {
            fx.player.set_beam_source_point(h, e, a.loc, 0);
        }
    }
}

/// The actor's end: its own components with it, TetherSourceVFX destroyed (@4826).
fn tether_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).tethers.remove(&st.seq) else { return };
    for h in [o.tip, o.beam, o.source, o.points, o.ally_point, o.end_burst].into_iter().flatten() {
        fx.player.kill(h);
    }
}

/// The actor's own systems (hitpause dilation): its components and what it attached to its root.
fn tether_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c)
        .tethers
        .get(&st.seq)
        .map_or(Vec::new(), |o| [o.tip, o.beam, o.points, o.ally_point, o.end_burst].into_iter().flatten().collect())
}

/// EndVFX @2095: EndCollision = SpawnEmitterAttached(PS_StevenNANAIR_BubbleBurst, Capsule, None, 0, rot 0, scale 1,
/// SnapToTarget, auto destroy), Team = Team Value @2743: on the actor's root at world scale 1, going with the actor
/// where it has not run out. The sim posts it as the class's emitter event at the actor's place (tether.rs EndVFX),
/// beside the Gem copy @2394 (SpawnEmitterAtLocation at the Instigator's Gem: the shared spawn of the event). The
/// event of the Capsule is told from the Gem's by its place: the tether's own x / y.
fn tether_burst(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx: Some(v), x, y, .. } = *e else { return false };
    if w.names.str(v) != BURST {
        return false;
    }
    let s = me(c);
    let mut seqs: Vec<i32> = s.tethers.keys().copied().collect();
    seqs.sort_unstable();
    for seq in seqs {
        let Some(id) = s.tethers.get(&seq).map(|o| o.id) else { continue };
        let Some(pr) = w.actors.projectiles.get(id) else { continue };
        let ProjectileExt::StevenTether(t) = &pr.ext else { continue };
        if pr.owner_id != fighter || !t.end_vfx || pr.x != x || pr.y != y {
            continue;
        }
        if let Some(h) = s.tethers.get_mut(&seq).and_then(|o| o.end_burst.take()) {
            fx.player.kill(h);
        }
        let st = ProjFxState::new(seq, id, None, "", f64::from(pr.x), f64::from(pr.y), None);
        let team = fx.team(fighter);
        let h = root_attach(s, fx, w, &st, BURST, [0.0; 3], team, true);
        if let Some(o) = s.tethers.get_mut(&seq) {
            o.end_burst = h;
        }
        return true;
    }
    false
}

/// Steven_BubbleCharge (neutral air's bubble): ReceiveTick, while not Activated, sets StaticMesh's world scale to
/// GrowthScale (ubergraph @1660-1756, SetWorldScale3D @2008); its ParticleSystem (PS_StevenNANAIR_Bubble_Intro,
/// RelativeScale3D 0.1 under StaticMesh) follows, so its world scale is GrowthScale x 0.1 (the sim's
/// BubbleCharge.growth). UpdateVisuals' SetWorldScale3D @3933-4057 is called by nothing. The shared component
/// multiplies the rule's scale by the root's (pr.scale), so the rule gives growth x 0.1 over it.
fn bubble_scale(_w: &World, pr: &Projectile, ms: V3) -> Option<V3> {
    let ProjectileExt::BubbleCharge(b) = &pr.ext else { return None };
    let k = if pr.scale != 0.0 { BUBBLE_PS_SCALE * f64::from(b.growth) / f64::from(pr.scale) } else { 0.0 };
    Some(ms.map(|v| v * k))
}

/// The pop: SetVisuals(true) @2764-2814 SpawnEmitterAttached(PS_StevenNANAIR_BubbleBurst, the Instigator's Mesh,
/// fml_un_C_spine_c, 0, rot 0, GrowthScale x 0.1, KeepRelativeOffset, auto destroy): the relative scale on the socket,
/// so the mesh's scale and the bone's ride on it and it follows Steven; Team @3168 SelectFloat(0.6, the colour value,
/// IsLocalOnline). The sim's event (bubble.rs activate) carries the scale GrowthScale x 0.1.
fn bubble_burst(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx: Some(v), scale, .. } = *e else { return false };
    if w.names.str(v) != BURST {
        return false;
    }
    let team = fx.team(fighter);
    fx.attach_socket(w, fighter, BURST, SPINE, [0.0; 3], &IDENTITY, [f64::from(scale); 3], &[("Team", team)]);
    true
}
