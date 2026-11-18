//! Finn's down attack's shockwaves: FinnShockwaveActor ([`FinnShockwave`], a
//! DealerActor), AnimNotify_FinnDAttack_Hitframe.
//!
//! A SpawnedActorParent child spawned by Finn_D_Attack_Montage's Spawner (t 1.2503, Queued, no socket, not attached,
//! Translation (0, 200, 0): 200 ahead of Finn, 100 below his actor location; the yaw his facing's).
//! - ReceiveBeginPlay @1720: the parent's (the blast-box loop: DestroyOnExitedArena? true), then on authority
//!   NumberOfShocks from the Instigator's Comp_FinnMomentumCharge.MomentumAmount (@1760-2021): 3 from 0.95, 2 from 0.5,
//!   else 1 (another Instigator has no component: the read of None gives 0, one shock). Visuals(NumberOfShocks)
//!   @802 (render only); then SphereTraceSingle (radius 0.5, channel 3 ProjectileGroundTrace, every
//!   BP_FighterCharacter ignored) from the SceneRoot + (0, 0, 5) to SceneRoot - (0, 0, 400) @1015: a blocking hit's
//!   Location Z is starting_height (@1379), else the SceneRoot's Z (@1688).
//! - spawn_shockwave(damage, sphere, height, scale, particles): SphereTraceSingle (the same radius, channel and ignore
//!   list) at the sphere's world X / Y from height + 100 to height - 120 (@323); success = bBlockingHit AND NOT
//!   bInitialOverlap (@694). On success the sphere moves to the ImpactPoint (@890, no sweep), Spawn Shockwave
//!   Visuals(particles, ImpactPoint) (@1105) and shockwave_hit on every BP_FighterCharacter the sphere overlaps
//!   (GetOverlappingActors @1194).
//! - The chain (ubergraph): shock 0 (damage_shockwave0, Shockwave0, starting_height, scale (1, 1, 1)) @1406; on
//!   success Shockwave Audio (AK_Shockwave) @1501 and Delay(DelayBetweenShocksDuration 0.17) @1533 -> @15: with
//!   NumberOfShocks >= 2 shock 1 (Shockwave1, from Shockwave0's Z, scale (1, 1, 1)) @159, on success the audio and
//!   Delay(0.17) @286 -> @415: with NumberOfShocks >= 3 shock 2 (Shockwave2, from Shockwave1's Z, scale (2, 2, 2):
//!   the bytecode's constant @559) @559, on success the audio, the decal's material reset and
//!   DecalAlpha.PlayFromStart @769 (0.1 s, Alpha linear 1 -> 0), whose Finished @2053 goes to @345. Every miss and
//!   every stop of the chain by NumberOfShocks goes to @345: Delay(0.5) -> @400 K2_DestroyActor. AK_Shockwave is not
//!   set in the class data (None): Shockwave Audio posts nothing.
//! - shockwave_hit(damage, Object, scale): the Instigator's BPI_IsFacingLeft now (@79): facing_modifier 1 when facing
//!   left, else -1; v = (0, facing_modifier x 400, 900) x scale; the packet: Damage, BaseKnockBack = 0.7 x |v|,
//!   KnockBackScaling 16, KnockBackDirectionVector (v.Y, v.Z) with IgnoreFacing, HitReaction 4, HitPauseTime 0.15,
//!   HitStunMult 1, OnHitAudio sfx_share_class_tank_spike, OnHitEffectOverride PS_ShaggyRageImpact, ArmorPierce,
//!   AttackID 0, HitEachActorOnlyOnce false, AdditiveKBMode 0, KBI 1, no vacuum, PlayOnHitEffects and
//!   ApplyHitPauseToDamageDealer; pfgDoDamage(no hit box, Object, packet, no buffs) on its own handler @1678.
//!
//! The spheres (radius x scale): Shockwave0 32 x 3 = 96 at X 0, Shockwave1 32 x 6 = 192 at X 180, Shockwave2
//! 48 x 8 = 384 at X 360 (X along the actor's forward). GetOverlappingActors: the spheres are WorldStatic with a
//! Custom profile and no saved CollisionResponses, so they keep the SphereComponent archetype's OverlapAllDynamic
//! responses (UpdateResponseContainerFromArray 142c82ae0): Overlap to Pawn, Ignore to HitBox and HurtBox. A fighter's
//! root CapsuleComponent (Pawn, overlap events on, Block to WorldStatic) is the one component of it whose lesser
//! response is Overlap (UpdateOverlapsImpl 14297ec80), so the sphere is tested against the root capsule (the actor
//! location, the current half height and radius, upright on the play plane), never the hurt shapes; a capsule with
//! its collision off is not met.
//! shockwave_hit's pfgDoDamage has no team test in the graph: a team mate it meets gets the ally path.
// Deviation: the Delay chain's frames count from the spawn frame in the actor's step (the World's actor pass).
// The attack id is the spawn's (the packet's AttackID 0 is not read for the decay, as C017's).
//
// AnimNotify_FinnDAttack_Hitframe (a child of AnimNotify_Hitframe that overrides HitframeHitboxesOn): the parent's
// packet of the notify's values but VacuumInfo.VacuumSocketOffset = Lerp(VacuumInfo.VacuumSocketOffset,
// MaxVacuumOffset, FClamp(MomentumAmount, 0, 1)) per axis (@51-523; another owner's read of None gives 0), and
// VacuumKnockback false (@1625), DirectKnockbackTowardsSocket false (@1645), KnockbackTargetSocket None (@1665),
// KnockbackSocketOffset (0, 0) (@1705), AccountForVelocityWithVacuumKnockback false (@1996). The rest is the parent's.
// Its Received_NotifyBegin / NotifyEnd are the parent's (no override). The one placement: Finn_D_Attack_Montage
// (t 0.913, BranchingPoint; VacuumInfo offset (1000, 350), MaxVacuumOffset (1200, 350)).

use super::state::finn_of;
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::{ExtraHurt, WorldSphere, shape_over_capsule};
use crate::combat::elem::charge_pct;
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::{DataEnum, DataVec};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, TraceHit, sphere_trace_single};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, facing, invulnerable, no_collision, no_hit};
use crate::math::{self};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{all_fighters, flag, name as data_name, name_of, num, num_in, obj, text};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::{spawn_place, spawn_spot};
use crate::world::spawning::{SpawnSpotA, SpawnSpotR, request_props};
use mvs_data::value::{Props, Value};

/// The shockwave's class.
pub const SHOCKWAVE: &str = "FinnShockwaveActor_C";

/// The spheres' component, damage member and scale (`SPHERES`).
const SPHERES: [(&str, &str, Real); 3] = [
    ("Shockwave0", "damage_shockwave0", 1.0),
    ("Shockwave1", "damage_shockwave1", 1.0),
    ("Shockwave2", "damage_shockwave2", 2.0),
];
/// The DecalAlpha timeline: length 0.1 s, Alpha linear 1 -> 0.
const DECAL_FADE: Real = 0.1;
const PS: [&str; 3] = ["ParticleSystem0", "ParticleSystem1", "ParticleSystem2"];

/// What the running Delay resumes into (`then`): the next shock (@15 / @415 with NumberOfShocks reaching it), @345's
/// Delay(0.5) (the chain stopped by NumberOfShocks), or @400 K2_DestroyActor.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum Then {
    Shock,
    Stop,
    Destroy,
}

/// One sphere (`spheres[n]`): its forward offset, radius (at the root's scale 1), its emitter and the emitter's scale.
#[derive(State, Debug, Default)]
pub struct Sphere {
    pub fwd: Real,
    pub r: Real,
    pub ps: Option<Name>,
    pub ps_scale: Real,
}

/// `FinnShockwave` (FinnShockwaveActor_C).
#[derive(State, Debug)]
pub struct FinnShockwave {
    /// The last shock's packet.
    pub hit: HitPacket,
    pub facing: Real,
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    /// The DecalAlpha timeline's output (0 .. 1): 1 until the third shock plays it.
    pub decal_alpha: Real,
    /// The timeline's position, -1 before PlayFromStart.
    pub decal_t: Real,
    /// NumberOfShocks (1 .. 3).
    pub shocks: i32,
    /// The next shock.
    pub stage: i32,
    /// The running Delay's frames left.
    pub wait: Real,
    pub then: Option<Then>,
    /// The next shock's starting height (the last sphere's Z).
    pub height: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
    /// The actor's rotation (UE Pitch, Yaw, Roll), for its emitters' events.
    pub rot: [Real; 3],
    pub spheres: Vec<Sphere>,
}

impl Default for FinnShockwave {
    /// The field initial values (`hit = {}`: a sparse packet, nothing set).
    fn default() -> Self {
        FinnShockwave {
            hit: HitPacket { sparse: true, ..HitPacket::default() },
            facing: 0.0,
            attack_id: 0,
            x: 0.0,
            y: 0.0,
            decal_alpha: 1.0,
            decal_t: -1.0,
            shocks: 1,
            stage: 0,
            wait: 0.0,
            then: None,
            height: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
            rot: [0.0; 3],
            spheres: Vec::new(),
        }
    }
}

/// The FinnShockwaveActor_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &FinnShockwave) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The FinnShockwaveActor_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &FinnShockwave) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The FinnShockwaveActor_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &FinnShockwave) -> &HitPacket {
    &s.hit
}

fn sw_of(w: &World, id: ActorId) -> Option<&FinnShockwave> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::FinnShockwave(s) => Some(s),
        _ => None,
    }
}

fn sw_mut(w: &mut World, id: ActorId) -> Option<&mut FinnShockwave> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::FinnShockwave(s) => Some(s),
        _ => None,
    }
}

/// SphereTraceSingle(from, to, 0.5, ProjectileGroundTrace, every fighter ignored).
fn trace(w: &mut World, id: ActorId, x: Real, y0: Real, y1: Real) -> Option<TraceHit> {
    let ignore: Vec<Option<ActorRef>> = match_fighters(w).into_iter().map(|f| Some(ActorRef::Fighter(f))).collect();
    sphere_trace_single(w, x, y0, x, y1, 0.5, TRACE_PROJECTILE_GROUND, &ignore, Some(ActorRef::Actor(id)))
}

/// spawn_shockwave for the shock `stage`, then the Delay the chain starts behind it.
fn shock(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls, k) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    let Some(s) = sw_of(w, id) else { return };
    let n = usize::try_from(s.stage).unwrap_or(0);
    let Some(sp) = s.spheres.get(n) else { return };
    let (fwd, r, ps, ps_scale) = (sp.fwd, sp.r, sp.ps, sp.ps_scale);
    let (sx, facing, height, shocks, rot) = (s.x, s.facing, s.height, s.shocks, s.rot);
    let p = actor_p(w, ActorRef::Actor(id));
    let delay = num(p, "DelayBetweenShocksDuration").unwrap_or(0.0);
    let audio = if flag(p, "AK_Shockwave") { name_of(w, p, "AK_Shockwave") } else { None };
    // The spheres and their emitters are the root's children: their relative X, radius and scale at the root's scale
    // (SetSizeScale comes after BeginPlay, so shock 0 is at 1).
    let x = sx + fwd * facing * k;
    let t = trace(w, id, x, height + 100.0, height - 120.0);
    if let Some(s) = sw_mut(w, id) {
        s.then = Some(Then::Destroy);
        s.wait = latent_frames(0.5);
    }
    let Some(t) = t.filter(|t| t.blocking && !t.initial) else { return }; // @345
    if let Some(s) = sw_mut(w, id) {
        s.height = t.iy; // the sphere at the ImpactPoint
    }
    let fighter = w.fighter(f).id;
    if let Some(ps) = ps {
        w.events.push(Event::ProjectileDeath {
            cls,
            fighter,
            vfx: Some(ps),
            x: t.ix,
            y: t.iy,
            ox: 0.0,
            rot,
            scale: ps_scale * k,
            tint: None,
            to: None,
        });
    }
    hit_overlaps(w, id, n, t.ix, t.iy, r * k);
    if let Some(ev) = audio {
        w.events.push(Event::Sound { fighter, ev, x: Some(t.ix), y: Some(t.iy), hit: None, pooled: false, key: None });
    }
    let Some(s) = sw_mut(w, id) else { return };
    if n == 2 {
        // DecalAlpha.PlayFromStart: its update at 0 now, Finished at the length -> @345 Delay(0.5).
        s.decal_t = 0.0;
        s.decal_alpha = 1.0;
        s.wait = latent_frames(DECAL_FADE) + latent_frames(0.5);
        return;
    }
    // Delay(DelayBetweenShocksDuration), then the next shock when NumberOfShocks reaches it, else @345.
    s.wait = latent_frames(delay);
    let next = i32::try_from(n).unwrap_or(0) + 2;
    s.then = Some(if shocks >= next { Then::Shock } else { Then::Stop });
    s.stage += 1;
}

/// shockwave_hit on every fighter the sphere overlaps (GetOverlappingActors(BP_FighterCharacter) @1194: no other
/// actor), of either team. The Instigator himself: DamageAndLaunch's CanBeHitBy refuses and NotifyAllyOfHit skips him,
/// so nothing. A team mate: the same refusal, then NotifyAllyOfHit (@21344; none while invulnerable: @4608 -> @7452).
fn hit_overlaps(w: &mut World, id: ActorId, n: usize, x: Real, y: Real, r: Real) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let p = actor_p(w, me);
    let damage = num(p, SPHERES[n].1).unwrap_or(0.0);
    let attack_id = sw_of(w, id).map_or(0, |s| s.attack_id);
    for vic in all_fighters(w) {
        // a minion too (the BP_FighterCharacter filter @1194)
        if vic == f || w.fighter(vic).life.dead != 0.0 || no_hit(w, vic) || no_collision(w, vic) {
            continue;
        }
        let cr = cap_radius(w, vic);
        let ch = math::max(capsule_half(w, vic) - cr, 0.0);
        let cy = actor_y(w, vic);
        let body = ExtraHurt { x: w.fighter(vic).body.x, y0: cy - ch, y1: cy + ch, r: cr };
        if !shape_over_capsule(&WorldSphere { x, y, r, ..WorldSphere::default() }, &body) {
            continue;
        }
        let scale = SPHERES[n].2;
        let modifier = if facing(w, f) < 0.0 { 1.0 } else { -1.0 }; // facing_modifier
        // (0, mod x 400, 900) x scale: UE Y, Z
        let vy = (modifier * 400.0) * scale;
        let vz = 900.0 * scale;
        let hit = HitPacket {
            sparse: true,
            damage: Some(damage),
            base_knock_back: Some(crate::math::ue::size2d([vy, vz]) * 0.7),
            knock_back_scaling: Some(16.0),
            knock_back_direction_vector: Some(DataVec { x: Some(vy), y: Some(vz), z: None }),
            ignore_facing: Some(true),
            hit_reaction: Some(DataEnum::Value(4)),
            hit_pause_time: Some(0.15),
            hit_stun_mult: Some(1.0),
            on_hit_audio: Some(name!("sfx_share_class_tank_spike")),
            on_hit_effect_override: Some(name!("PS_ShaggyRageImpact")),
            armor_pierce: Some(true),
            attack_id: Some(0),
            play_on_hit_effects: Some(true),
            apply_hit_pause_to_damage_dealer: Some(true),
            attack_decay_type: Some(DataEnum::Value(0)),
            attack_decay_name: Some(name!("None")),
            additive_kb_mode: Some(DataEnum::Value(0)),
            kb_scaling_starting_dmg: Some(0.0),
            hit_each_actor_only_once: Some(false),
            account_for_velocity_with_vacuum_knockback: Some(true),
            ignore_kbi: Some(false),
            kbi_multiplier: Some(1.0),
            ..HitPacket::default()
        };
        if let Some(s) = sw_mut(w, id) {
            s.hit = hit.clone();
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if w.fighter(vic).team.team == w.fighter(f).team.team {
            if !invulnerable(w, vic) {
                ally_hit(w, &dealer(w, me), vic, &hit);
            }
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        // IgnoreFacing: the vector is a world one (UE Y, Z): sim x = -Y.
        let at = V2 { x: w.fighter(vic).body.x, y: w.fighter(vic).body.y };
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &[], at, Some(V2 { x: -vy, y: vz }), &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

fn shockwave_step(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(s) = sw_mut(w, id) else { return };
    s.age += 1;
    if s.decal_t >= 0.0 && s.decal_t < DECAL_FADE {
        s.decal_t = math::min(DECAL_FADE, s.decal_t + DELTA_SECONDS);
        s.decal_alpha = math::max(0.0, 1.0 - s.decal_t / DECAL_FADE);
    }
    let mut run = None;
    if let Some(then) = s.then {
        s.wait -= 1.0;
        if s.wait <= 0.0 {
            s.then = None;
            run = Some(then);
        }
    }
    match run {
        Some(Then::Destroy) => {
            if let Some(a) = w.actors.actors.get_mut(id) {
                a.dead = true;
            }
            return;
        }
        Some(Then::Shock) => shock(w, id),
        Some(Then::Stop) => {
            // @345: Delay(0.5) -> @400
            if let Some(s) = sw_mut(w, id) {
                s.then = Some(Then::Destroy);
                s.wait = latent_frames(0.5);
            }
        }
        None => {}
    }
    // CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), as Garnet's shockwave.
    let dead = w.actors.actors.get(id).is_none_or(|a| a.dead);
    let Some(s) = sw_mut(w, id) else { return };
    let left =
        !dead && blast_timer_exit_rounded(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y);
    if left && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// ReceiveBeginPlay's own part (@1720 on), behind the parent's charge copy and GrantBuffsFromInstigator: the starting
/// height and the first shock.
fn begin_play(w: &mut World, id: ActorId) {
    let Some((x, y)) = sw_of(w, id).map(|s| (s.x, s.y)) else { return };
    let t = trace(w, id, x, y + 5.0, y - 400.0);
    // starting_height: the hit's Location Z, else the SceneRoot's
    let height = t.filter(|t| t.blocking).map_or(y, |t| t.y);
    if let Some(s) = sw_mut(w, id) {
        s.height = height;
    }
    shock(w, id);
}

/// The kind's `spawn`: the place, PostActorConstruction's FindTeleportSpot (ahead of BeginPlay), the parent's
/// BeginPlay blast-box loop.
fn shockwave_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let fc = facing(w, f);
    let mut spheres = Vec::new();
    for (n, (comp, _, _)) in SPHERES.iter().enumerate() {
        let c = obj(&a.components, comp);
        let pc = obj(&a.components, PS[n]);
        let k = c.and_then(|c| num_in(c, "RelativeScale3D", "X")).unwrap_or(1.0);
        spheres.push(Sphere {
            fwd: c.and_then(|c| num_in(c, "RelativeLocation", "X")).unwrap_or(0.0),
            r: c.and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0) * k,
            ps: pc.and_then(|c| text(c, "Template")).map(|t| data_name(w, t)),
            ps_scale: pc.and_then(|c| num_in(c, "RelativeScale3D", "X")).unwrap_or(1.0),
        });
    }
    let amount = finn_of(w, f).map_or(0.0, |s| s.momentum.amount);
    let sp = spawn_place(w, f, &np);
    let at = spawn_spot(
        w,
        &SpawnSpotR { cls: r.cls, p: np.clone() },
        &SpawnSpotA { components: a.components.clone() },
        sp.x,
        sp.y,
    );
    let mut s = FinnShockwave {
        facing: fc,
        attack_id: r.attack_id,
        rot: [0.0, if fc < 0.0 { 90.0 } else { -90.0 }, 0.0],
        spheres,
        shocks: if amount >= 0.95 {
            3
        } else if amount >= 0.5 {
            2
        } else {
            1
        },
        x: at.x,
        y: at.y,
        ..FinnShockwave::default()
    };
    let sb = spawn_box(w);
    // the parent's BeginPlay loop
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    (WorldActorState::FinnShockwave(s), dead)
}

// ---------------------------------------------------------------- AnimNotify_FinnDAttack_Hitframe

/// HitframeHitboxesOn's packet (the notify's values with the vacuum offset lerped by the momentum).
fn d_attack_packet(w: &World, f: FighterId, p: &Props) -> Props {
    let a = math::min(1.0, math::max(0.0, finn_of(w, f).map_or(0.0, |s| s.momentum.amount)));
    let v = obj(p, "VacuumInfo").cloned().unwrap_or_default();
    let axis = |o: Option<&Props>, k: &str| o.and_then(|o| num(o, k)).unwrap_or(0.0);
    let (from, to) = (obj(&v, "VacuumSocketOffset"), obj(p, "MaxVacuumOffset"));
    let lerp = |k: &str| {
        let (x, y) = (axis(from, k), axis(to, k));
        x + (a * (y - x))
    };
    let mut offset = Props::new();
    offset.insert("X".into(), Value::Num(f64::from(lerp("X"))));
    offset.insert("Y".into(), Value::Num(f64::from(lerp("Y"))));
    let mut v = v;
    v.insert("VacuumSocketOffset".into(), Value::Obj(offset));
    let mut out = p.clone();
    out.insert("VacuumInfo".into(), Value::Obj(v));
    out.insert("VacuumKnockback".into(), Value::Bool(false));
    out.insert("Direct Knockback Toward Socket".into(), Value::Bool(false));
    out.insert("KnockbackTargetSocket".into(), Value::Str("None".into()));
    let mut zero = Props::new();
    zero.insert("X".into(), Value::Num(0.0));
    zero.insert("Y".into(), Value::Num(0.0));
    out.insert("KnockbackSocketOffset".into(), Value::Obj(zero));
    out.insert("Account for Velocity with Vacuum Knockback".into(), Value::Bool(false));
    out
}

fn d_attack_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let p = d_attack_packet(w, f, &crate::fighter::anim::montages::notify_data(w, &data, f, n).p);
    crate::registry::notifies::hitframe_begin_with(w, f, n, idx, Some(p));
}

fn d_attack_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    crate::registry::notifies::hitframe_end(w, f, n, idx);
}

/// This file's registry entries (FinnShockwaveActor_C, AnimNotify_FinnDAttack_Hitframe_C). `foreign_owner`: the reads
/// of the owner are its Comp_FinnMomentumCharge (none on another fighter, so one shock), its facing, team and the
/// spawn place. `size`: `shock` reads the scale, the Decal's view too.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(shockwave_spawn),
        begin_play: Some(begin_play),
        step: Some(shockwave_step),
        size: Some(SizeHooks::default()),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        SHOCKWAVE,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(ActorClass::World(WorldActorClass::FinnShockwaveActor), ActorHooks::World(hooks))
        },
    )?;
    r.notifies.enter(
        "AnimNotify_FinnDAttack_Hitframe_C",
        NotifyHandler { begin: Some(d_attack_begin), end: Some(d_attack_end), ..NotifyHandler::default() },
    )
}
