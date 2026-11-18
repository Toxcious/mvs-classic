//! MeeSeeks_AirFlap_C ([`AirFlap`], a DealerActor): the Meeseeks of the air
//! special, a SpawnedActorParent child.
//!
//! Spawned by the Spawner notify of C020_Meeseeks_Air_Montage (t .176 to 1.406: AttachToActor, Socket Root, translation
//! 0, DestroyAllActorsOnNotifyEnd: the world ends it at the notify's end).
//! - SpawnedActorParent's BeginPlay first (@41): HitBoxOn? true in the class data, so its UpdateHitbox is the handler's
//!   HitBoxesOn(HitBoxTags [HitBox], HitBoxDamage, HitBoxScale (1, 1, 1)) (SpawnedActorParent @15092 -> @8531).
//! - @51-259: SkeletalMesh.SetRelativeScale3D(the Instigator's BPI_IsFacingLeft ? (1, -1, 1) : (1, 1, 1)) (a visual
//!   mirror of the attached mesh), @300-550 PS_MeeseeksSpawn attached to SkeletalMesh at fml_un_C_spine_a with the owner's
//!   Team colour, @608 Delay(0.3) -> @15 HitBoxOn? = false, UpdateHitbox (HitBoxesOff).
//! - SetTeamColor @1181 -> @718: the mesh's team material (the view's).
//!
//! Its hit boxes: the components tagged with HitBoxTags ([HitBox]: eight capsules / spheres on fml_un_C_spine_c,
//! fml_un_L_loarm, L_Hand, fml_un_L_uparm and the right ones, fml_un_C_head; mesh scale 1, no mesh offset), moved with
//! their bones by C020_Meeseeks_FlapAttack, the single-node clip (Meeseeks_SkelMesh, AnimationSingleNode, AnimToPlay of
//! the class's SkeletalMesh component, played once from the spawn at rate 1; its AnimClass Meeseeks_AnimBP_C does not
//! run): the time (age - 1) / 60 s (as the drawing has it), held at its end (`combat::boxes::actor_boxes`).
//!
//! For the drawing (the view): (`x`, `y`) the actor (the owner's Root socket), `facing` the mesh's mirror (the
//! Instigator's facing at BeginPlay), `hit_on` the boxes; PS_MeeseeksSpawn (attached at fml_un_C_spine_a) and
//! PS_Meeseeks_Flapping (a component 375 above the mesh) are its effects.

use indexmap::IndexSet;

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::{BoxSampler, WorldSphere, actor_boxes, push_actor_shapes};
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::spawned_hits::class_rows;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_class_def, actor_data, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::{max, to_int32};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawning::request_props;
use mvs_data::value::Value;

const CLS: &str = "MeeSeeks_AirFlap_C";

/// `AirFlap` (MeeSeeks_AirFlap_C).
#[derive(State, Debug)]
pub struct AirFlap {
    /// HitBoxDamage.
    pub hit: HitPacket,
    /// The mesh's mirror: the Instigator's facing at BeginPlay.
    pub facing: Real,
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// The age the Delay(0.3) fires at.
    pub off_at: i32,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
}

impl Default for AirFlap {
    /// The field initial values.
    fn default() -> Self {
        AirFlap {
            hit: HitPacket::default(),
            facing: 0.0,
            attack_id: 0,
            x: 0.0,
            y: 0.0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            hit_on: false,
            off_at: 0,
            hit_set: IndexSet::new(),
        }
    }
}

/// The MeeSeeks_AirFlap_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &AirFlap) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The MeeSeeks_AirFlap_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &AirFlap) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The MeeSeeks_AirFlap_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &AirFlap) -> &HitPacket {
    &s.hit
}

fn flap_of(w: &World, id: ActorId) -> Option<&AirFlap> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::AirFlap(s) => Some(s),
        _ => None,
    }
}

fn flap_mut(w: &mut World, id: ActorId) -> Option<&mut AirFlap> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::AirFlap(s) => Some(s),
        _ => None,
    }
}

/// Attached at the owner's Root socket (Mesh.GetSocketLocation in the owner's pose; his actor location when the pose or
/// the socket is not sampled): the actor's place.
fn follow(w: &World, f: FighterId) -> (Real, Real) {
    let bs: &BoxSampler = &w.fixed.samplers[&w.fighter(f).ch];
    let si = w.names.get("socket:Root").and_then(|n| bs.by_name.get(&n).copied());
    let pose = si.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
    let (Some(si), Some(pose)) = (si, pose) else {
        warn_text(w, &format!("{CLS}: the Root socket not sampled (the owner location taken)"));
        return (w.fighter(f).body.x, actor_y(w, f));
    };
    let o = usize::try_from(bs.offsets[usize::try_from(si).unwrap_or(0)]).unwrap_or(0);
    let facing = crate::fighter::facing(w, f);
    let at = (
        crate::fighter::mesh_wx(w, f) + crate::real(pose[o]) * facing,
        crate::fighter::mesh_wy(w, f) + crate::real(pose[o + 1]),
    );
    crate::fighter::anim::put_pose(w, pose);
    at
}

/// The HitBox components where they are now, for the hit tests and the drawing: the HitBoxTags' components at
/// HitBoxScale (the handler's HitBoxesOn), the mesh mirrored to face left (@51-259). `None` without the clip's tracks.
fn flap_shapes(w: &World, id: ActorId) -> Option<Vec<WorldSphere>> {
    let (a, s) = (w.actors.actors.get(id)?, flap_of(w, id)?);
    let (bs, t) = actor_boxes(w, a.owner, a.cls)?;
    let clip = actor_class_def(w, a.owner, a.cls)
        .and_then(|d| d.components.get("SkeletalMesh"))
        .and_then(Value::as_obj)
        .and_then(|m| m.get("AnimationData"))
        .and_then(Value::as_obj)
        .and_then(|d| d.get("AnimToPlay"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut pose = vec![0.0_f32; usize::try_from(t.stride).unwrap_or(0)];
    let time = crate::real(max(crate::real(s.age - 1), 0.0)) * DELTA_SECONDS;
    let sampled = clip.as_deref().is_some_and(|c| BoxSampler::sample(&t, c, time, &mut pose));
    if !sampled {
        warn_text(w, &format!("{CLS}: no track of the clip {}", clip.as_deref().unwrap_or("undefined")));
        return Some(Vec::new());
    }
    let p = actor_p(w, ActorRef::Actor(id));
    let scale = obj(p, "HitBoxScale").and_then(|v| num(v, "X")).unwrap_or(1.0);
    let mut out = Vec::new();
    if let Some(Value::Arr(tags)) = p.get("HitBoxTags") {
        for tag in tags.iter().filter_map(Value::as_str) {
            push_actor_shapes(
                &mut out,
                &t,
                bs,
                &pose,
                bs.tagged(tag),
                s.x,
                s.y,
                s.facing,
                true,
                scale,
                a.size_scale.unwrap_or(1.0),
            );
        }
    }
    Some(out)
}

fn flap_hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    match flap_shapes(w, id) {
        Some(v) => {
            out.extend(v);
            true
        }
        None => false,
    }
}

fn flap_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let at = follow(w, f);
    let Some(s) = flap_mut(w, id) else { return };
    s.age += 1;
    (s.x, s.y) = at;
    // @15: HitBoxOn? = false, UpdateHitbox (the handler's HitBoxesOff, which forgets the actors hit).
    if s.age == s.off_at {
        s.hit_on = false;
        s.hit_set.clear();
    }
    // CheckBlastBoxExit on its 0.1 s timer: the spawn frame is not counted.
    let gone = blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y);
    if gone && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The HitBox components against the enemies' hurt shapes: the handler's pfgDoDamage gates in the projectile test's order
/// (as Garnet's ShockwaveHitBox).
// Deviation: its boxes against the other spawned actors (a projectile's HurtBox: no DestroyProjectiles in its
// HitBoxDamage, so only HitByEnemy would be broadcast there) are not tested.
fn flap_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(s) = flap_of(w, id) else { return };
    if !s.hit_on {
        return;
    }
    let (attack_id, hit, x, y) = (s.attack_id, s.hit.clone(), s.x, s.y);
    let team = w.fighter(f).team.team;
    let mut shapes: Option<Vec<WorldSphere>> = None;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = flap_of(w, id) else { return };
        if w.fighter(vic).team.team == team || !alive(w, vic) || no_hit(w, vic) || s.hit_set.contains(&vid) {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        if shapes.is_none() {
            shapes = flap_shapes(w, id);
        }
        let met = shapes.as_ref().is_some_and(|sh| hurt_hit(w, vic, sh, &vp).is_some());
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hit.do_los_check.unwrap_or(false) && los_blocked(w, x, y, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = flap_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x, y }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// The kind's `spawn`: the constructor (the Root socket's place, the Instigator's facing), the parent's BeginPlay
/// blast-box loop, PS_MeeseeksSpawn (@454, on the mesh's fml_un_C_spine_a socket: taken at the actor, the drawing attaches
/// it).
fn flap_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let at_root = flag(&np, "AttachToActor")
        && np.get("Socket").and_then(Value::as_str) == Some("Root")
        && !flag(&np, "WorldTransforms")
        && !t.is_some_and(|t| flag(t, "X") || flag(t, "Y") || flag(t, "Z"));
    if !at_root {
        warn_text(
            w,
            &format!("{CLS}: a spawn other than attached at the Root socket with no translation is not ported"),
        );
    }
    let (x, y) = follow(w, f);
    let mut s = AirFlap {
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&mvs_data::value::Props::new()), None),
        // @51-259: BPI_IsFacingLeft of the Instigator (false when the cast fails).
        facing: crate::fighter::facing(w, f),
        attack_id: r.attack_id,
        x,
        y,
        hit_on: flag(&a.p, "HitBoxOn?"),
        // BeginPlay's Delay(0.3) inside the spawn (the owner's notify): the actor's tick of the spawn frame counts it.
        off_at: to_int32(latent_frames(0.3)),
        ..AirFlap::default()
    };
    let sb = spawn_box(w);
    let (mut bx, mut by) = (s.x, s.y);
    let dead = blast_box_loop(
        &sb,
        &mut bx,
        &mut by,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    (s.x, s.y) = (bx, by);
    let ev = Event::ProjectileDeath {
        cls: r.cls,
        fighter: w.fighter(f).id,
        vfx: Some(name!("PS_MeeseeksSpawn")),
        x: s.x,
        y: s.y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    };
    w.events.push(ev);
    (WorldActorState::AirFlap(s), dead)
}

/// This file's registry entry (MeeSeeks_AirFlap_C), entered by [`super::meeseeks::register`] in registration order.
/// `foreign_owner`: a Meeseeks spawned off another fighter's notify reads the owner as the Instigator.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ActorHooks::World(WorldActorHooks {
        spawn: Some(flap_spawn),
        // flap_shapes reads the scale, the view too
        size: Some(SizeHooks::default()),
        hit_shapes: Some(flap_hit_shapes),
        step: Some(flap_step),
        hits: Some(flap_hits),
        ..WorldActorHooks::default()
    });
    r.actors.enter(CLS, ActorEntry::new(ActorClass::World(WorldActorClass::MeeSeeksAirFlap), hooks))
}
