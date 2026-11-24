//! Harley Quinn's other actors: the gloves
//! (HarleyDownAirProjectile_C and HarleySideAirProjectile2_C, plain projectiles with their kind),
//! HarleyJackInTheBox_C ([`JackBox`], a world actor and a DealerActor: its detection, its explosion and its ally
//! launch) and the confetti buffs: Buff_ApplyConfetti, Buff_ConfettiCount ([`ConfettiCount`]) and
//! Buff_ConfettiExplosionCount ([`ConfettiExplosion`], whose buff actor deals the explosion's hits).
//!
//! The shared code reaches the jack-in-the-box's place ([`jack_place`]) and its dealer fields (`combat::hits` dealer:
//! [`dealer_parts`]), and the explosion's buff actor as a dealer (`BuffExt::dodge_mut` / `actor_owner`:
//! [`explosion_dodge_mut`], [`explosion_owner`]).
//!
//! HarleyDownAirProjectile (the glove of the air down attack; the side special's glove is a data-only child of it),
//! HarleyJackInTheBox, Buff_ApplyConfetti and Buff_ConfettiCount; the class defaults from the data. The bat bomb is in
//! bomb.rs. Not entered (named in no string of this folder): the smoke grenade child and the first version of the
//! side glove.
//!
//! HarleyDownAirProjectile and HarleySideAirProjectile2 are ParentProjectilePhysics children: the world's Projectile
//! is the parent (movement, MaxLifetime 0.275 s, the hitbox, clank, DestroyOnHitFighter). The child graph adds the
//! bounce handler and the end effects. ProjectileGravityScale: neither class, ParentProjectilePhysics nor
//! SpawnedActorParent authors it, so the component keeps UProjectileMovementComponent's constructor value, 1.0
//! (0x142983a00: +0x10c = 0x3f800000, between PreviousHitNormal +0x100 and Bounciness +0x114 = 0.6, Friction +0x118 =
//! 0.2, the stop threshold +0x11c = 5): the gloves fall with the world's gravity from the spawn on (Projectile takes
//! the constructor's value for a template that authors none). HarleyDownAirProjectile's Capsule template sets the
//! profile ProjectileNavPlatform over ParentProjectilePhysics' ProjectileNavNoPlatform (no Platform entry in its
//! responses: Block), so a pass-through platform blocks the gloves from every side; Projectile reads that from the
//! class data.
//!
//! HarleyJackInTheBox (down special "Jerk-in-the-Box": Special_JackInTheBox, Special_Air_JackInTheBox): a
//! SpawnedActorParent child placed at the R_Hand socket, 55 lower, where it stays (no movement component is enabled).
//! The ability trigger of the spawn (slot 0) is the world's (Fighter.actorSpawnAbility). ReceiveBeginPlay @3507: the
//! mesh's material and, for an instigator facing left, its Y scale negated @4123; a 0.25 s timer -> BalloonSpawn
//! @5784 (PS_HarleyBalloons on the mesh socket Box, sfx_cha_harley_jester_inflate); the ground probe @4169 (a line
//! from the HurtBox 70 down against WorldStatic and Platform: the idle animation JackInTheBox_Special_Idle on a hit
//! that is no moving platform, else _Air_Idle, and the HurtBox's Z extent 45 on a hit, 70 without); a 0.75 s timer ->
//! ActivateHitbox @5443: HurtBox query on, IsActive, HitBoxOn?, UpdateHitbox. The parent's MaxLifetime timer (14 s)
//! ends it without an explosion.
//! The hit shape is the detection shape (FighterDetectionHitbox, radius 125 at (0, 40.6) from the actor: the
//! component tagged HitBox): HandleFighterOverlap @2732 on its begin overlap. Another team's fighter: not
//! invulnerable -> nothing here, the parent's hit of the same overlap lands (HitBoxDamage, all zero) and
//! HitEnemyPlayer @9631 runs Explode; invulnerable -> the looping 0.1 s timer ScanForEnemyFighters @9326 (any overlap
//! keeps it; another team's fighter that is not invulnerable -> Explode; no overlap clears it). The same team,
//! Harley too, the first time @3177: OverlappedFighters, his JumpPressedDispatch bound to AfterJumpTriggered @3419,
//! the jump input label @3483. AllyEndOverlap @9797: out of the list, unbound, the label gone.
//! On an actor that is no fighter (a projectile's HurtBox, an item, a destructible actor: the types the template
//! overlaps, Pawn, DamageDestructible, ProjectileDestructible, ItemDestructible and HurtBox) the same begin overlap
//! runs the parent's pfgDoDamage (HitBoxDamage) alone, HandleFighterOverlap's cast failing: `hit_box_actor_hits`,
//! behind the fighters of the step.
//! AfterJumpTriggered @8827 (inside the ally's jump press, ahead of his jump logic): IsActive, DeactivateTrap, the
//! coop count, the "Jumps Reset!" text, pfgDoDamage(AllyHitBox) on him @9032 (no damage, BaseKnockBack 1000 straight
//! up), ResetJump(0) @9155, Explode.
//! Explode @1578 (a do-once): DeactivateTrap, HurtBox and the detection shapes off @1668-1744, every BaseFighter
//! overlapping the explosion shape (radius 275, object types Pawn, PhysicsBody, ProjectileDestructible, HurtBox) of
//! another team and not invulnerable gets pfgDoDamage(ExplosionHitboxDamage, ExplosionOnHitBuffs) @409;
//! ExplodeMulticast @5486 (the Explode animation, PS_HarleySPD_ConfettiMine at the actor + BoxVisualsOffset at scale
//! 0.7, sfx_cha_harley_jester_activate); Delay(1.0) @721 -> DestroySpawnedActor @15.
//! A packet with IgnoreFacing keeps its X as UE +Y whatever the facing (pfgDoDamage @11068, as `apply_projectile_hit`):
//! the explosion's (0.1, 1) leans to -x.
//! Unverified: the arming fires the begin overlap of the fighters already inside (the collision change's overlap
//! update); the two timers of BeginPlay and the lifetime count from the spawn frame's timer tick; the Delay(1.0)
//! counts from the tick after the explosion; a fighter is one overlap (the engine's are per shape pair);
//! ScanForEnemyFighters sees fighters only.
//! ReceiveAnyDamage @8011: Explode (`any_damage`; its one ported caller: C021_ForceField's Damage-1 hit). A fighter's
//! hit box over the HurtBox: the kind's `hurt_box`.
//! Not ported: the clank (BPI_CanProjectileClank = IsActive); the label and the pop text are the view's (labelFor,
//! resetFor); BPI_IncrementCoopActions.
//!
//! Buff_ApplyConfetti and Buff_ConfettiCount (the on-hit row of the bomb, the box and Harley's hit frames):
//! Buff_ApplyConfetti ReceiveBeginPlay @29: the instigator's CheckHasBuff(Buff_Harley_ConfettiExplosion) @110 picks
//! the perk's count class or Buff_ConfettiCount @413; the owner's SpawnBuffActor(that class, the owner, the
//! instigator, this actor's CurrentStackAmount, its BuffDuration) @432; K2_DestroyActor @10.
//! Buff_ConfettiCount (CanStack, PreventStacksWhenReapplied, MaxStacks 5, StackDecayBuffer 5, StackDecayRate 0.5: the
//! data, read by `spawn_buff`): BPI On Fighter Owner Knocked Back -> AddStack(1) @1497 (the knockback of the hit that
//! brings the buff precedes the row's spawn, so that hit is not counted). BPI On Max Stacks @2384:
//! SpawnBuffActor(Buff_Fire, the owner, the instigator, 6 stacks, duration 0) @2475, Confetti Visuals @1136
//! (PS_Explosion_Satchel attached to the owner's mesh at VFX_Root @1235, PS_HarleyDebuff_Confetti at the owner's
//! location at scale 2 with the Team parameter @1423, the UI state ConfettiIgnite: type 1, display 0, values -1,
//! active, PopText, AutoRemoveTime 1 @415-879), K2_DestroyActor @2573.
//! BPI After Status Update Multicast @1521: removed or stacks <= 0 -> RemoveStateForPlayer(UI_ID) @1866, else
//! AddStateForPlayer(UI_ID, type 1, display 0, the stacks of MaxStacks, not active, AutoRemoveTime -1) @1998-2325.
//! AddStack's own status call behind the hook (BuffParentActor @143) runs on the destroyed actor with the cap's count
//! and removed false: the count state EndPlay took away is added again, 5 of 5, and nothing removes it until the next
//! count buff on this fighter ends. `combat::buffs::add_stack` makes that call.
//! Deviation: PS_Explosion_Satchel is sent as an emitter at the owner's place (no attach to VFX_Root in the event).
//! The status's icon is the class's UI_Icon (@612, @2196): MI_State_Ignite here, MI_State_Confetti on the perk's
//! child.
//!
//! Buff_ConfettiExplosionCount (the Confetti Explosion perk's count): a child of Buff_ConfettiCount: its defaults (the
//! same duration, stacks, decay and reapply rule; UI_ID ConfettiExplosionCount, UI_Icon MI_State_Confetti,
//! ExplosionRadius 350, ExplosionHitBox) from the data; the knocked-back AddStack and the status are the parent's. Its
//! own BPI On Max Stacks @853 (no parent call: no Buff_Fire): IgnoreActors = the Instigator's
//! Comp_Actor_Destructible.GetAllies @924, plus the Instigator @1016; SphereOverlapActors(the owner's location,
//! ExplosionRadius, [Pawn], BP_FighterCharacter, IgnoreActors) @1178. The owner is not ignored: it is in its own
//! sphere. For each result @15-555: ExplosionHitBox.KnockBackDirectionVector = (result.Y - owner.Y, ExplosionRadius)
//! @291-337 (UE Y: the sim's -x, so the world direction is (vic.x - owner.x, 350): away from the owner, straight up
//! on the owner itself), then the buff's own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, result, ExplosionHitBox, null,
//! [], [], false, false) @432 (a DealerActor with buffActor, credited to the Instigator, as Buff_DelayKnockUp's).
//! After the loop ExplosionVisuals @768 (@1258: PS_SupermanSPU_Impact attached to the owner's Mesh at VFX_Root, scale
//! ExplosionRadius / 350, Team @1432-1742; PS_HarleyDebuff_Confetti at the owner's location, scale 2, Team
//! @1914-2179; the UI state ConfettiExplosion: type 1, display 0, values -1, timer -1, UI_Icon, active, "Exploded",
//! AutoRemoveTime 1 @2388-2856) and K2_DestroyActor @782.
//! Unverified: the build runs all of it inside the AddStack of the owner's KnockedBack broadcast (DamageAndLaunch
//! @10471, after the hit's HitReactionMulticast stored its KnockBackVelocity @9984), so the owner's launch is the
//! explosion's (its own AddHitStunFromKnockBack stores last). The port sends that broadcast ahead of the hit's launch
//! (receiveHit), where a nested hit's launch would be overwritten by the outer one: the hook marks the buff and the
//! explosion runs in the frame's timer pass (`BuffHooks::timer`), after the hit that reached the cap, or earlier at a
//! Buff_ApplyConfetti on the owner (below): a confetti hit's On Hit Buffs come after its KnockedBack (pfgDoDamage's
//! rows behind DamageAndLaunch @10471), and in the build SpawnBuffActor @432 then meets no count (the capped one was
//! destroyed @782) and starts a fresh one; the port runs the pending explosion and its destroy first, so the row's
//! spawn starts that fresh count too. A second knockback of the owner in between is a stack at the cap (nothing); in
//! the build the actor is gone by then.
//! Consequences of the deferral (deviation): (a) chained explosions: an explosion that caps another fighter's count
//! only marks it; when that fighter is earlier in the fighters' order (the timer pass's) or the cap comes from a hit
//! after the timer pass (an item's throw hits), it explodes a tick later, where the build nests it inside the first
//! one's loop. (b) The outer DamageAndLaunch's tail (KnockedBackIncludeSelf @10564,
//! HitDispatchMulticast(VSize(KnockbackLaunchVelocity)) @10701, @3589 on) reads the explosion's merged launch in the
//! build; the port runs that tail first, with the outer hit's own.
//! Simplification: no DODGED text for the explosion's handler (attackDodged on an invulnerable victim, no dodgeTick
//! host).
//! Deviation: PS_SupermanSPU_Impact is sent as an emitter at the owner's place (no attach to VFX_Root), as the
//! parent's.
//! Unverified: GetAllies taken as the Instigator's team mates in play (as Black Adam's pillar, bolt.rs).

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "NaN-keeping negated tests")]

use indexmap::IndexSet;
use mvs_data::value::Props;

use crate::Real;
use crate::State;
use crate::actors::projectile::set_hitbox;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, buff_of, end_buff, spawn_buff, stack_buff};
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{
    HitShape, actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, hit_box_actor_hits, projectile_blocks,
};
use crate::combat::packet::{DataVec, OnHitBuffRow};
use crate::combat::phases::{give_vbuf, no_hit, victim_pose};
use crate::combat::spawned_hits::hit_box_types;
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, UiStateOp, UiStateParams};
use crate::fighter::common::{actor_data, actor_def, latent_frames, warn_once};
use crate::fighter::control::{DodgeState, attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::moves::reset_jump_counts;
use crate::fighter::traces::solids::QUERY_WORLDSTATIC;
use crate::fighter::traces::{OBJ_PAWN, TraceBody, body_info, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, bind_jump, invulnerable, unbind_jump};
use crate::math::to_int32;
use crate::math::ue::{SMALL_NUMBER, deg_atan2, normal, safe_normal};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::pawns::{is_match_fighter, match_fighters};
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::{request_props, spawn_overlap};

use crate::combat::hit::knockback::timer_ticks;

const JACK: &str = "HarleyJackInTheBox_C";
const COUNT: &str = "Buff_ConfettiCount_C";
const EXPLOSION: &str = "Buff_ConfettiExplosionCount_C";

// ---- what Harley Quinn's actor files share

/// `sound`: a one-shot at a place, for the fighter of id `fighter`.
pub(super) fn sound(w: &mut World, fighter: i32, ev: Name, x: Real, y: Real) {
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// `emitter`: a particle system spawned at a place (SpawnEmitterAtLocation); `ox` is the depth offset (UE X), `rot`
/// the rotator (pitch, yaw, roll) in degrees.
#[allow(clippy::too_many_arguments, reason = "the helper's parameters")]
pub(super) fn emitter(
    w: &mut World,
    cls: Name,
    fighter: i32,
    vfx: Name,
    x: Real,
    y: Real,
    scale: Real,
    ox: Real,
    rot: [Real; 3],
) {
    w.events.push(Event::ProjectileDeath { cls, fighter, vfx: Some(vfx), x, y, ox, rot, scale, tint: None, to: None });
}

/// `touches`: a sphere of an actor against a fighter in the play plane: his hurt shapes at the current pose and, for a
/// shape that overlaps the Pawn channel too (capsule), his root capsule. False for an opponent the port's own Dodgeball
/// rule spares what `by` deals.
// Deviation: the build's shapes are a sphere and, for an actor with Supports2DHitboxes, a capsule along the depth
// axis; a hurt shape off the play plane and the capsule's exact section are taken as the circle test.
pub(crate) fn touches(w: &mut World, by: FighterId, o: FighterId, x: Real, y: Real, r: Real, capsule: bool) -> bool {
    if w.fighter(o).life.dead > 0.0 || crate::fighter::spared_by(w, by, o) {
        return false;
    }
    if (w.fighter(o).body.x - x).abs() > r + 600.0 || (actor_y(w, o) - y).abs() > r + 600.0 {
        return false;
    }
    if capsule && !crate::world::capsules_in_sphere(w, &[o], x, y, r).is_empty() {
        return true;
    }
    if no_hit(w, o) {
        return false;
    }
    let Some(pose) = victim_pose(w, o) else { return false };
    let s = WorldSphere { x, y, z: Some(0.0), r, ..WorldSphere::default() };
    let met = hurt_hit(w, o, std::slice::from_ref(&s), &pose).is_some();
    give_vbuf(w, pose);
    met
}

// ---- HarleyDownAirProjectile and HarleySideAirProjectile2

/// ProjectileBounce @223 (bound to ProjectileMovement.OnProjectileBounce in ReceiveBeginPlay @1680):
/// sfx_cha_harley_gloveshot_bounce at the actor @253 and PS_HarleyNADAIR_Ricochet at scale 1.5 @779, its roll
/// (MakeRotator(DegAtan2, 0, 0) @732: the first pin is Roll) DegAtan2(R.Y, R.Z) @686 of R = GetReflectionVector
/// @594 of Normal(GetVelocity(), 0.0001) @520-552 about the ImpactNormal. GetVelocity is the root's ComponentVelocity,
/// which the movement writes at the end of its tick: inside the move it is the velocity before this tick's step, the
/// way the glove came in. A glove falling straight onto a floor gives R straight up, roll 0. Then on the authority
/// @80: HitBoxOn? = false and UpdateHitbox @91-102,
/// HitBoxOn? = true @10, HitBoxDamage.VelocityInsteadOfFacing and IgnoreFacing = true @21-57 and UpdateHitbox: the
/// hit set is cleared, and from then on the hit's direction is the velocity's (`projectile_hit_dir`).
/// The handler runs inside the move, for every impact (`ProjectileHooks::bounce_impact`): the actor is at the hit. The
/// off ends every overlap of the hit box and the on (ActivateHitbox's SetCollisionEnabled) begins again what it
/// overlaps at the impact place, at once (`spawn_overlap`): a fighter there is hit (DestroyOnHitFighter ends the
/// glove there), another projectile's HurtBox or an item begun again.
/// Not ported: an item's own list of the boxes over it (Item.overlappedBy) keeps the glove through the off.
// Deviation: the velocity the glove meets the surface with (this tick's gravity step in it) stands in for the
// ComponentVelocity of the tick before; the two differ by one frame's gravity.
fn glove_bounce(w: &mut World, id: ProjectileId, nx: Real, ny: Real, ivx: Real, ivy: Real) -> bool {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    let (cls, fighter, x, y) = (p.cls, p.owner_id, p.x, p.y);
    sound(w, fighter, name!("sfx_cha_harley_gloveshot_bounce"), x, y);
    // In UE axes (Y = -x, Z = y). GetReflectionVector: D - 2 (D . N) N with N the surface normal's safe normal.
    let d = normal([0.0, -ivx, ivy], 0.0001);
    let n = safe_normal([0.0, -nx, ny], SMALL_NUMBER);
    let dn = d[1] * n[1] + d[2] * n[2];
    let roll = deg_atan2(d[1] - 2.0 * dn * n[1], d[2] - 2.0 * dn * n[2]);
    emitter(w, cls, fighter, name!("PS_HarleyNADAIR_Ricochet"), x, y, 1.5, 0.0, [0.0, 0.0, roll]);
    let p = w.actors.projectiles.get_mut(id).expect("a live projectile");
    set_hitbox(p, false);
    p.overlapping.clear();
    set_hitbox(p, true);
    p.hit.velocity_instead_of_facing = Some(true);
    p.hit.ignore_facing = Some(true);
    spawn_overlap(w, id);
    false
}

/// ReceiveEndPlay @1813 (OnReleasedToPool @1837 is the pooled form of the same end) -> @1711 PS_Harley_ToyPunch_Hit at
/// the actor, scale 1, with the Team parameter, and @1313 sfx_cha_harley_gloveshot_destroy.
fn glove_end(w: &mut World, id: ProjectileId) {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    let (cls, fighter, x, y) = (p.cls, p.owner_id, p.x, p.y);
    emitter(w, cls, fighter, name!("PS_Harley_ToyPunch_Hit"), x, y, 1.0, 0.0, [0.0; 3]);
    sound(w, fighter, name!("sfx_cha_harley_gloveshot_destroy"), x, y);
}

fn glove_kind() -> ProjectileHooks {
    ProjectileHooks { bounce_impact: Some(glove_bounce), end: Some(glove_end), ..ProjectileHooks::default() }
}

// ---- HarleyJackInTheBox

/// `JackBox` (HarleyJackInTheBox_C), a `DealerActor`. The kind's values and the actor's fields besides (`lifetimePause`,
/// `elem`, `charge`, `dodge`, `sizeScale`, the id, `dead`) are the [`WorldActor`]'s.
#[derive(State, Debug)]
pub struct JackBox {
    /// HitBoxDamage: the contact packet.
    pub hit: HitPacket,
    /// The instigator's AttackID of the spawn.
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    /// The instigator's facing at the spawn (the mesh's Y scale).
    pub facing: Real,
    /// The ground probe's result.
    pub grounded: bool,
    /// Ticks lived, the spawn frame's included.
    pub age: i32,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// BalloonSpawn ran.
    pub balloons: bool,
    /// IsActive.
    pub active: bool,
    /// Explode's do-once.
    pub exploded: bool,
    pub explode_age: i32,
    /// The age EnemyCheckTimer fires at next.
    pub scan_at: Real,
    /// The ids of the fighters overlapping the detection shape.
    pub inside: Vec<i32>,
    /// OverlappedFighters.
    pub allies: Vec<FighterId>,
    pub hit_set: IndexSet<i32>,
    /// The actors that are no fighter the detection shape overlaps (`hit_box_actor_hits`).
    pub over: IndexSet<ActorRef>,
    /// Their HitEachActorOnlyOnce track.
    pub once: IndexSet<ActorRef>,
    /// The fighter id of the last AfterJumpTriggered ("Jumps Reset!"), at `reset_age`.
    pub reset_for: i32,
    pub reset_age: i32,
    pub ended: bool,
    /// The detection and explosion shapes' centre above the actor, unscaled.
    pub detect_y: Real,
    pub detect_r: Real,
    pub blast_r: Real,
    /// UpdateMaxLifetimeWithAuthority(0.0) was called (the Batman tutorial's targets): no MaxLifetime end. Not in the
    /// record: the trace's record has no field for it.
    /// Unverified: a MaxLifetime of 0 is taken as no lifetime timer (SpawnedActorParent's update is not read).
    pub endless: bool,
}

impl Default for JackBox {
    /// The field initial values.
    fn default() -> Self {
        JackBox {
            hit: HitPacket::default(),
            attack_id: 0,
            x: 0.0,
            y: 0.0,
            facing: 0.0,
            grounded: false,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            balloons: false,
            active: false,
            exploded: false,
            explode_age: 0,
            scan_at: Real::INFINITY,
            inside: Vec::new(),
            allies: Vec::new(),
            hit_set: IndexSet::new(),
            over: IndexSet::new(),
            once: IndexSet::new(),
            reset_for: -1,
            reset_age: -1,
            ended: false,
            detect_y: 0.0,
            detect_r: 0.0,
            blast_r: 0.0,
            endless: false,
        }
    }
}

fn jack_of(w: &World, id: ActorId) -> Option<&JackBox> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JackBox(j) => Some(j),
        _ => None,
    }
}

fn jack_mut(w: &mut World, id: ActorId) -> Option<&mut JackBox> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JackBox(j) => Some(j),
        _ => None,
    }
}

/// The root's scale (`JackBox.k`).
pub(super) fn scale_of(w: &World, id: ActorId) -> Real {
    w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0)
}

/// The jack-in-the-box's place (its `x`, `y`).
pub fn jack_place(w: &World, a: &WorldActor, j: &JackBox) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((j.x, j.y))
}

/// The jack-in-the-box as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, j: &JackBox) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (j.x, j.y, j.facing, j.attack_id)
}

pub(super) fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live actor").owner
}

pub(super) fn dead_of(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

/// `a.p.X ?? {}` of a class packet: the packet of the class data.
fn packet(w: &World, id: ActorId, key: &str) -> HitPacket {
    HitPacket::from_props(&w.names, obj(actor_p(w, ActorRef::Actor(id)), key).unwrap_or(&Props::new()), None)
}

/// The KnockBackDirectionVector of a packet of the class data, as an IgnoreFacing hit takes it: (-X, Y) (UE +Y is the
/// sim's -x).
fn leaning(w: &World, id: ActorId, key: &str) -> V2 {
    let k = obj(actor_p(w, ActorRef::Actor(id)), key).and_then(|h| obj(h, "KnockBackDirectionVector"));
    V2 { x: -k.and_then(|k| num(k, "X")).unwrap_or(0.0), y: k.and_then(|k| num(k, "Y")).unwrap_or(0.0) }
}

pub(super) fn rows_of(w: &World, id: ActorId, key: &str) -> Vec<OnHitBuffRow> {
    OnHitBuffRow::rows(&w.names, actor_p(w, ActorRef::Actor(id)).get(key))
}

pub(super) fn mid_frame_hit(w: &mut World, o: FighterId) {
    if w.overlap_mid_frame {
        w.fighter_mut(o).frame.hit_this_frame = true;
    }
}

pub(super) fn at_of(w: &World, o: FighterId) -> V2 {
    V2 { x: w.fighter(o).body.x, y: actor_y(w, o) }
}

fn jack_deactivate(j: &mut JackBox) {
    j.active = false;
    j.scan_at = Real::INFINITY;
}

/// Explode @1578 (a do-once).
fn jack_explode(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    if dead_of(w, id) {
        return;
    }
    let (f, k) = (owner_of(w, id), scale_of(w, id));
    let Some(j) = jack_mut(w, id) else { return };
    if j.exploded {
        return;
    }
    j.exploded = true;
    j.explode_age = j.age;
    jack_deactivate(j);
    // The detection shapes' collision off: the end overlaps (AllyEndOverlap).
    let allies = std::mem::take(&mut j.allies);
    j.inside.clear();
    j.over.clear();
    let (jx, cy, blast_r) = (j.x, j.y + j.detect_y * k, j.blast_r);
    let jy = j.y;
    for o in allies {
        unbind_jump(w, o, me);
    }
    let rows = rows_of(w, id, "ExplosionOnHitBuffs");
    let pkt = packet(w, id, "ExplosionHitboxDamage");
    let dir = leaning(w, id, "ExplosionHitboxDamage");
    let team = w.fighter(f).team.team;
    for o in match_fighters(w) {
        if w.fighter(o).team.team == team || !touches(w, f, o, jx, cy, blast_r * k, true) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), o) || invulnerable(w, o) {
            continue;
        }
        mid_frame_hit(w, o);
        let d = dealer(w, me);
        let at = at_of(w, o);
        if apply_projectile_hit(w, &d, o, &pkt, &rows, at, Some(dir), &[], charge_pct(w, me)) {
            let d = dealer(w, me);
            actor_hit_enemy(w, &d, o);
        }
    }
    let off = obj(actor_p(w, me), "BoxVisualsOffset");
    let (oz, ox) = (off.and_then(|o| num(o, "Z")).unwrap_or(0.0), off.and_then(|o| num(o, "X")).unwrap_or(0.0));
    let (fid, cls) = (w.fighter(f).id, w.actors.actors.get(id).expect("a live actor").cls);
    emitter(w, cls, fid, name!("PS_HarleySPD_ConfettiMine"), jx, jy + oz, 0.7, ox, [0.0; 3]);
    sound(w, fid, name!("sfx_cha_harley_jester_activate"), jx, jy);
}

/// HandleFighterOverlap for a begin overlap of the detection shape, with the parent's hit of the same overlap.
fn jack_overlap(w: &mut World, id: ActorId, o: FighterId) {
    let me = ActorRef::Actor(id);
    let Some(j) = jack_of(w, id) else { return };
    if !j.active {
        return;
    }
    let (f, attack_id, hit) = (owner_of(w, id), j.attack_id, j.hit.clone());
    if w.fighter(o).team.team != w.fighter(f).team.team {
        // The parent's pfgDoDamage pops ahead of its dodge for a fighter the smoke's projectile block covers
        // (`projectile_blocks`); HandleFighterOverlap's own tests go on.
        // Unverified: the explosion for such an enemy is taken from HandleFighterOverlap's non-invulnerable branch, not
        // the parent's hit (the order of the two calls on one overlap as the hit's above).
        let blocked = projectile_blocks(w, &dealer(w, me), o);
        if invulnerable(w, o) {
            // The parent's pfgDoDamage on the same overlap takes the dodge branch: HitBoxDamage's BaseKnockBack is 0 and
            // the class is no BPI_Marker, so @7858 pops (no dodge, no burst, no text).
            if !blocked {
                let tick = w.flow.tick;
                attack_dodged(w, me, o, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            }
            // K2_SetTimerDelegate(ScanForEnemyFighters, 0.1, looping) @3096
            if let Some(j) = jack_mut(w, id) {
                j.scan_at = crate::real(j.age) + timer_ticks(Some(0.1));
            }
            return;
        }
        let oid = w.fighter(o).id;
        if jack_of(w, id).is_some_and(|j| j.hit_set.contains(&oid)) {
            return;
        }
        if blocked {
            jack_explode(w, id);
            return;
        }
        dodge_over(w, me, o);
        if let Some(j) = jack_mut(w, id) {
            j.hit_set.insert(oid);
        }
        mid_frame_hit(w, o);
        let rows = rows_of(w, id, "On Hit Buffs");
        let d = dealer(w, me);
        let at = at_of(w, o);
        if apply_projectile_hit(w, &d, o, &hit, &rows, at, None, &[], charge_pct(w, me)) {
            let d = dealer(w, me);
            actor_hit_enemy(w, &d, o);
        }
        jack_explode(w, id);
        return;
    }
    // The parent's hit of the same begin overlap (HitBoxDamage, no ForceKnockback) on a team mate: CanBeHitBy refuses,
    // then NotifyAllyOfHit (@21344), none for the Instigator nor an invulnerable mate (@4608 -> @7452 pops).
    if o != f && !invulnerable(w, o) && !projectile_blocks(w, &dealer(w, me), o) {
        let d = dealer(w, me);
        ally_hit(w, &d, o, &hit);
    }
    let Some(j) = jack_mut(w, id) else { return };
    if j.allies.contains(&o) {
        return;
    }
    j.allies.push(o);
    bind_jump(w, o, me);
}

/// HurtBox as a body of the traces (`WorldActorHooks::bodies`): a BoxComponent (profile ProjectileDestructible,
/// QueryAndPhysics in the template) under the Capsule root at (30.45, 0, 60.6), extent (65, 200, 70), its Z extent set
/// to 45 by BeginPlay's ground probe on a hit, 70 without (@4925-5095: SetBoxExtent), place and extent at the root's
/// scale; ActivateHitbox's SetCollisionEnabled(QueryOnly) @5443 keeps the query on, Explode's (NoCollision) @1668 ends
/// it. The actor stands at the spawn yaw +-90 along the instigator's facing (Comp_Spawner; the mesh's Y scale is
/// negated for a left facing to face the camera): local X is the play plane's x times the facing, local Y the depth.
/// What meets it: the object queries for ProjectileDestructible (PolymorphGrenade's blast, the Earth Smash pillar's
/// box, C021_ForceField's overlap); the other bodies of the class are NoCollision or HitBox, which no query gathers.
fn jack_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let (Some(a), Some(j)) = (w.actors.actors.get(id), jack_of(w, id)) else { return };
    if j.exploded || a.dead {
        return;
    }
    let hb = actor_def(w, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, "HurtBox"));
    let Some(hb) = hb else { return };
    let Some(info) = body_info(Some(hb), None) else { return };
    if !info.on {
        return;
    }
    let l = obj(hb, "RelativeLocation");
    let e = obj(hb, "BoxExtent");
    let k = scale_of(w, id);
    let lx = l.and_then(|l| num(l, "X")).unwrap_or(0.0);
    let lz = l.and_then(|l| num(l, "Z")).unwrap_or(0.0);
    let ex = e.and_then(|e| num(e, "X")).unwrap_or(32.0);
    let px = j.x + lx * j.facing * k;
    let py = j.y + lz * k;
    out.push(TraceBody {
        ax: px,
        ay: py,
        bx: px,
        by: py,
        hx: ex * k,
        hy: (if j.grounded { 45.0 } else { 70.0 }) * k,
        r: 0.0,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("HurtBox"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// The same HurtBox for a fighter's hit box (profile ProjectileDestructible: the HitBox channel overlaps it), the box in
/// the play plane's axes: local X along x, local Z up, local Y the depth.
fn jack_hurt(w: &World, id: ActorId) -> Option<WorldSphere> {
    let (a, j) = (w.actors.actors.get(id)?, jack_of(w, id)?);
    if j.exploded || a.dead {
        return None;
    }
    let hb = actor_def(w, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, "HurtBox"))?;
    if !body_info(Some(hb), None)?.on {
        return None;
    }
    let l = obj(hb, "RelativeLocation");
    let e = obj(hb, "BoxExtent");
    let k = scale_of(w, id);
    let lx = l.and_then(|l| num(l, "X")).unwrap_or(0.0);
    let lz = l.and_then(|l| num(l, "Z")).unwrap_or(0.0);
    let mut b = [0.0; 9];
    b[0] = e.and_then(|e| num(e, "X")).unwrap_or(32.0) * k;
    b[5] = e.and_then(|e| num(e, "Y")).unwrap_or(32.0) * k;
    b[7] = (if j.grounded { 45.0 } else { 70.0 }) * k;
    Some(WorldSphere {
        x: j.x + lx * j.facing * k,
        y: j.y + lz * k,
        z: Some(0.0),
        r: 0.0,
        bx: Some(b),
        ..WorldSphere::default()
    })
}

/// UpdateMaxLifetimeWithAuthority(0.0) on a jack-in-the-box (`JackBox::endless`).
pub fn set_endless(w: &mut World, id: ActorId) {
    if let Some(j) = jack_mut(w, id) {
        j.endless = true;
    }
}

/// Whether the sphere `s` meets the jack-in-the-box's HurtBox (`jack_hurt`): none once it has exploded.
pub fn hurt_box_meets(w: &World, id: ActorId, s: &WorldSphere) -> bool {
    jack_hurt(w, id).is_some_and(|hb| crate::combat::boxes::shape_over_box(s, &hb))
}

/// ReceiveAnyDamage @8011: Explode.
pub fn receive_any_damage(w: &mut World, id: ActorId) {
    jack_explode(w, id);
}

/// FighterDetectionHitbox (object type HitBox; the component tagged HitBox) as the circle the step tests, from
/// ActivateHitbox to Explode's collision off.
fn jack_detect(w: &World, id: ActorId) -> Option<WorldSphere> {
    let (a, j) = (w.actors.actors.get(id)?, jack_of(w, id)?);
    if !j.active || j.exploded || a.dead {
        return None;
    }
    let k = scale_of(w, id);
    Some(WorldSphere { x: j.x, y: j.y + j.detect_y * k, r: j.detect_r * k, ..WorldSphere::default() })
}

fn jack_hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    match jack_detect(w, id) {
        Some(s) => {
            out.push(s);
            true
        }
        None => false,
    }
}

/// The kind's `spawn`: the state around the class data, the ground probe, then the parent's BeginPlay blast-box loop.
fn jack_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    // The probe: from the HurtBox's centre 70 down, against the stage's floor and its platforms.
    let hy = at.y
        + obj(&a.components, "HurtBox")
            .and_then(|c| obj(c, "RelativeLocation"))
            .and_then(|l| num(l, "Z"))
            .unwrap_or(0.0);
    let on = |left: Real, right: Real, top: Real| at.x >= left && at.x <= right && top <= hy && top >= hy - 70.0;
    // (the stage's WorldStatic solids with the block: `static_shapes`; a polygon solid by its upper outline at x)
    let grounds =
        crate::world::stage_query::static_shapes(w, QUERY_WORLDSTATIC, name!("HarleyJackInTheBox ground probe"));
    let grounded = grounds
        .boxes
        .iter()
        .any(|g| on(crate::math::narrow(g.left), crate::math::narrow(g.right), crate::math::narrow(g.top)))
        || grounds.polys.iter().any(|q| {
            let g = q.bounds();
            on(crate::math::narrow(g.left), crate::math::narrow(g.right), q.top_at(at.x))
        })
        || w.stage.stage_platforms.iter().any(|p| on(p.left, p.right, p.top));
    let det = obj(&a.components, "FighterDetectionHitbox");
    let mut j = JackBox {
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&Props::new()), None),
        attack_id: r.attack_id,
        x: at.x,
        y: at.y,
        facing: crate::fighter::facing(w, f),
        grounded,
        detect_y: det.and_then(|d| obj(d, "RelativeLocation")).and_then(|l| num(l, "Z")).unwrap_or(0.0),
        detect_r: det.and_then(|d| num(d, "SphereRadius")).unwrap_or(0.0),
        blast_r: obj(&a.components, "ExplosionHitbox").and_then(|d| num(d, "SphereRadius")).unwrap_or(0.0),
        ..JackBox::default()
    };
    let sb = spawn_box(w);
    // the parent's BeginPlay loop
    let dead = blast_box_loop(
        &sb,
        &mut j.x,
        &mut j.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut j.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    (WorldActorState::JackBox(j), dead)
}

/// The jack-in-the-box's tick.
fn jack_step(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (f, k) = (owner_of(w, id), scale_of(w, id));
    let p = actor_p(w, me);
    let doe = flag(p, "DestroyOnExitedArena?");
    let balloons = timer_ticks(num(p, "DelayUntilBalloons"));
    let active_at = timer_ticks(num(p, "DelayUntilHitboxActive"));
    let life = timer_ticks(num(p, "MaxLifetime"));
    let Some(j) = jack_mut(w, id) else { return };
    j.age += 1;
    let n = crate::real(j.age - 1); // the timer ticks counted since the spawn frame's
    let die = |w: &mut World| {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
    };
    if blast_timer_exit(doe, &mut j.blast_at, &j.blast_bounds, n, j.x, j.y) {
        die(w);
        return;
    }
    if j.exploded {
        if crate::real(j.age) >= crate::real(j.explode_age) + latent_frames(1.0) {
            die(w);
        }
        return;
    }
    #[allow(clippy::float_cmp, reason = "an exact tick match")]
    if n == balloons {
        j.balloons = true;
        let (x, y, fid) = (j.x, j.y, w.fighter(f).id);
        sound(w, fid, name!("sfx_cha_harley_jester_inflate"), x, y);
    }
    let Some(j) = jack_mut(w, id) else { return };
    #[allow(clippy::float_cmp, reason = "an exact tick match")]
    if n == active_at {
        j.active = true;
    }
    if n >= life && !j.endless {
        die(w);
        return;
    }
    if !j.active {
        return;
    }
    let (jx, cy, dr) = (j.x, j.y + j.detect_y * k, j.detect_r * k);
    for o in match_fighters(w) {
        let oid = w.fighter(o).id;
        let Some(j) = jack_mut(w, id) else { return };
        let at = j.inside.iter().position(|&i| i == oid);
        if !touches(w, f, o, jx, cy, dr, true) {
            let Some(at) = at else { continue };
            let Some(j) = jack_mut(w, id) else { return };
            j.inside.remove(at);
            if let Some(a) = j.allies.iter().position(|&x| x == o) {
                j.allies.remove(a);
                unbind_jump(w, o, me);
            }
            continue;
        }
        if at.is_some() {
            continue;
        }
        let Some(j) = jack_mut(w, id) else { return };
        j.inside.push(oid);
        jack_overlap(w, id, o);
        if jack_of(w, id).is_none_or(|j| j.exploded) {
            return;
        }
    }
    // (the gathered object types FighterDetectionHitbox overlaps)
    let comp = actor_def(w, ActorRef::Fighter(f), w.actors.actors.get(id).expect("a live actor").cls)
        .and_then(|d| obj(&d.components, "FighterDetectionHitbox"));
    let types = hit_box_types(comp);
    let shape = jack_detect(w, id).map(|s| HitShape::Sphere { x: s.x, y: s.y, r: s.r });
    let Some(j) = jack_mut(w, id) else { return };
    let (mut over, mut once, hit) = (std::mem::take(&mut j.over), std::mem::take(&mut j.once), j.hit.clone());
    hit_box_actor_hits(w, me, &hit, &types, shape, &mut over, &mut once);
    if let Some(j) = jack_mut(w, id) {
        j.over = over;
        j.once = once;
    }
    let Some(j) = jack_of(w, id) else { return };
    if j.exploded || dead_of(w, id) || crate::real(j.age) < j.scan_at {
        return;
    }
    // ScanForEnemyFighters @9326.
    let Some(j) = jack_mut(w, id) else { return };
    if j.inside.is_empty() {
        j.scan_at = Real::INFINITY; // K2_ClearAndInvalidateTimerHandle @9573
        return;
    }
    j.scan_at += timer_ticks(Some(0.1));
    let inside = j.inside.clone();
    let team = w.fighter(f).team.team;
    for o in match_fighters(w) {
        if inside.contains(&w.fighter(o).id) && w.fighter(o).team.team != team && !invulnerable(w, o) {
            jack_explode(w, id);
        }
    }
}

/// AfterJumpTriggered @8827 (inside the ally's jump press, ahead of his jump logic).
fn jack_jump_pressed(w: &mut World, id: ActorId, o: FighterId) {
    let me = ActorRef::Actor(id);
    if dead_of(w, id) {
        return;
    }
    let oid = w.fighter(o).id;
    let Some(j) = jack_mut(w, id) else { return };
    if !j.active {
        return;
    }
    jack_deactivate(j);
    j.reset_for = oid;
    j.reset_age = j.age;
    let f = owner_of(w, id);
    if !projectile_blocks(w, &dealer(w, me), o) && !invulnerable(w, o) {
        mid_frame_hit(w, o);
        let pkt = packet(w, id, "AllyHitBox");
        let dir = leaning(w, id, "AllyHitBox");
        let d = dealer(w, me);
        let at = at_of(w, o);
        apply_projectile_hit(w, &d, o, &pkt, &[], at, Some(dir), &[], charge_pct(w, me));
        // NotifyAllyOfHit at the call's end (@21344): a team mate, not Harley herself.
        if o != f {
            let d = dealer(w, me);
            ally_hit(w, &d, o, &pkt);
        }
    }
    reset_jump_counts(w, o);
    jack_explode(w, id);
}

/// ReceiveEndPlay @8157 (the labels) and the parent's (DestroySpawnedActor @15507 / EndPlay @17018): DeathVFX
/// PS_HarleySPD_ConfettiMine at DeathVFXScale and the 'Ak Death Location' sound, after an explosion too.
fn jack_end(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(j) = jack_mut(w, id) else { return };
    if j.ended {
        return;
    }
    j.ended = true;
    let allies = std::mem::take(&mut j.allies);
    let (x, y) = (j.x, j.y);
    // (not in the Lab reset's pass: an old pawn there)
    if !reset_pass(w) {
        for o in allies {
            unbind_jump(w, o, me);
        }
    }
    let f = owner_of(w, id);
    let (fid, cls) = (w.fighter(f).id, w.actors.actors.get(id).expect("a live actor").cls);
    let p = actor_p(w, me);
    let off = obj(p, "Death VFX Offset");
    let (oz, ox) = (off.and_then(|o| num(o, "Z")).unwrap_or(0.0), off.and_then(|o| num(o, "X")).unwrap_or(0.0));
    let vfx = text(p, "DeathVFX").filter(|s| !s.is_empty()).map(|s| s.to_owned());
    let scale = obj(p, "DeathVFXScale").and_then(|s| num(s, "X")).unwrap_or(1.0);
    let ak = text(p, "Ak Death Location").filter(|s| !s.is_empty()).map(|s| s.to_owned());
    if let Some(v) = vfx {
        let v = w.lit(&v);
        emitter(w, cls, fid, v, x, y + oz, scale, ox, [0.0; 3]);
    }
    if let Some(a) = ak {
        let a = w.lit(&a);
        sound(w, fid, a, x, y);
    }
}

// ---- the confetti buffs

/// Buff_ConfettiCount_C's extension (`{ from }`).
#[derive(State, Debug, Default)]
pub struct ConfettiCount {
    /// The extension is made (the buff's begin ran).
    pub begun: bool,
    /// GetInstigator.
    pub from: Option<FighterId>,
}

/// Buff_ConfettiExplosionCount_C's extension (`ExplosionState`: `{ from, boom }`).
#[derive(State, Debug, Default)]
pub struct ConfettiExplosion {
    /// The extension is made (the buff's begin ran).
    pub begun: bool,
    /// GetInstigator.
    pub from: Option<FighterId>,
    /// The cap was reached: the explosion runs in the frame's timer pass.
    pub boom: bool,
    /// The buff actor's Comp_Pawn_HitBoxHandler dodge variables (a `host` made new for each explosion: not
    /// recorded).
    pub dodge: Option<DodgeState>,
}

/// The dodge variables of the explosion's buff actor handler (a `host` made new for each explosion).
pub fn explosion_dodge_mut(e: &mut ConfettiExplosion) -> Option<&mut Option<DodgeState>> {
    Some(&mut e.dodge)
}

/// The fighter the explosion's buff actor credits its hits to (`ExplosionState.from`, the Instigator).
pub fn explosion_owner(e: &ConfettiExplosion) -> Option<FighterId> {
    e.from
}

fn explosion_of(w: &World, b: BuffRef) -> Option<&ConfettiExplosion> {
    match &buff(w, b)?.ext {
        BuffExt::ConfettiExplosion(e) => Some(e),
        _ => None,
    }
}

fn explosion_mut(w: &mut World, b: BuffRef) -> Option<&mut ConfettiExplosion> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::ConfettiExplosion(e) => Some(e),
        _ => None,
    }
}

/// A UI state of the confetti buffs: type 1, display 0, timer -1, no subicon.
#[allow(clippy::too_many_arguments, reason = "the UI state's fields")]
fn ui_add(id: Name, icon: Name, current: Real, max: Real, active: bool, pop: Name, auto_remove: Real) -> UiStateOp {
    UiStateOp::Add(UiStateParams {
        id,
        state_type: 1,
        display_type: 0,
        current_value: current,
        max_value: max,
        timer: -1.0,
        icon_material: icon,
        active,
        pop_text: pop,
        auto_remove_time: auto_remove,
        subicon: None,
    })
}

/// `d.UI_Icon ?? or` of a buff class's data.
fn icon_of(w: &World, f: FighterId, cls: Name, or: Name) -> Name {
    let d = crate::fighter::common::buff_def(w, ActorRef::Fighter(f), cls);
    d.and_then(|d| text(d, "UI_Icon")).map_or(or, |s| w.lit(s))
}

/// `countStatus` (BPI After Status Update Multicast @1521): removed or stacks <= 0 -> RemoveStateForPlayer(UI_ID)
/// @1866, else AddStateForPlayer(UI_ID, type 1, display 0, the stacks of MaxStacks, not active, AutoRemoveTime -1)
/// @1998-2325.
fn count_status(w: &mut World, f: FighterId, class: BuffClass, stacks: Real, _applied: bool, removed: bool) {
    let cls = w.lit(class.registered_name().expect("a registered class"));
    let d = crate::fighter::common::buff_def(w, ActorRef::Fighter(f), cls);
    let id = d.and_then(|d| text(d, "UI_ID")).map_or(name!("ConfettiIgniteCount"), |s| w.lit(s));
    let op = if removed || stacks <= 0.0 {
        UiStateOp::Remove(id)
    } else {
        let max = d.and_then(|d| num(d, "MaxStacks")).unwrap_or(1.0);
        ui_add(id, icon_of(w, f, cls, name!("MI_State_Ignite")), stacks, max, false, Name::NONE, -1.0)
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

/// `projectileDeath` of the fighter's own events at the owner's place.
fn fighter_emitter(w: &mut World, f: FighterId, cls: Name, vfx: Name, scale: Real) {
    let (fid, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    let e = Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale,
        tint: None,
        to: None,
    };
    w.fighter_mut(f).events.push(e);
}

/// `countStack`: BPI On Fighter Owner Knocked Back -> AddStack(1) @1497.
fn count_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s == BuffSignal::KnockedBack {
        stack_buff(w, f, b, 1.0);
    }
    false
}

fn count_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(e) = buff_mut(w, b) {
        e.ext = BuffExt::ConfettiCount(ConfettiCount { begun: true, from });
    }
}

/// BPI On Max Stacks @2384. True: K2_DestroyActor (EndPlay's status, removed, then AddStack's own behind the hook).
fn count_max_stacks(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let from = match buff(w, b).map(|e| &e.ext) {
        Some(BuffExt::ConfettiCount(e)) => e.from,
        _ => None,
    };
    spawn_buff(w, f, name!("Buff_Fire_C"), 0.0, Some(6), from, true, None);
    let cls = name!("Buff_ConfettiCount_C");
    fighter_emitter(w, f, cls, name!("PS_Explosion_Satchel"), 1.0);
    fighter_emitter(w, f, cls, name!("PS_HarleyDebuff_Confetti"), 2.0);
    let icon = icon_of(w, f, cls, name!("MI_State_Ignite"));
    w.fighter_mut(f).controls.ui_state_ops.push(ui_add(
        name!("ConfettiIgnite"),
        icon,
        -1.0,
        -1.0,
        true,
        name!("Ignited"),
        1.0,
    ));
    true
}

fn explosion_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(e) = buff_mut(w, b) {
        e.ext = BuffExt::ConfettiExplosion(ConfettiExplosion { begun: true, from, boom: false, dodge: None });
    }
}

/// BPI On Max Stacks @853: the explosion and the destroy run in the timer pass (the deviation above).
fn explosion_max_stacks(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    if let Some(e) = explosion_mut(w, b) {
        e.boom = true;
    }
    false
}

fn explosion_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if explosion_of(w, b).is_some_and(|e| e.boom) {
        boom(w, f, b);
    }
    false
}

/// The pending explosion of a capped count (`explosion_max_stacks` marked it): the hits, the visuals, K2_DestroyActor
/// (EndPlay's status, removed). The rest of the AddStack that reached the cap runs on the pending-kill actor, whose
/// Owner DestroyActor nulled (`combat::buffs::add_stack`): its status finds no fighter owner and adds nothing.
fn boom(w: &mut World, f: FighterId, b: BuffRef) {
    explode(w, f, b);
    end_buff(w, f, b);
}

fn explode(w: &mut World, f: FighterId, b: BuffRef) {
    let cls = name!("Buff_ConfettiExplosionCount_C");
    let inst = explosion_of(w, b).and_then(|e| e.from);
    let d = crate::combat::buffs::inst_view(w, b);
    let radius = d.num("ExplosionRadius").unwrap_or(350.0);
    let base = HitPacket::from_props(&w.names, d.obj("ExplosionHitBox").unwrap_or(&Props::new()), None);
    let icon = text_of_icon(w, b);
    match inst {
        None => warn_once(w, name!("Buff_ConfettiExplosionCount_C without an Instigator: no hit")),
        Some(inst) => {
            let team = w.fighter(inst).team.team;
            // the dodge entries of the buff actor's handler (`attack_dodged`): it hits once
            if let Some(e) = explosion_mut(w, b) {
                e.dodge = None;
            }
            let mut ignore: Vec<Option<ActorRef>> = match_fighters(w)
                .into_iter()
                .filter(|&o| o != inst && w.fighter(o).team.team == team && w.fighter(o).life.dead <= 0.0)
                .map(|o| Some(ActorRef::Fighter(o)))
                .collect();
            ignore.push(Some(ActorRef::Fighter(inst)));
            let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
            let list = sphere_overlap_actors(w, fx, fy, radius, &[crate::real(OBJ_PAWN)], &ignore, -1.0);
            for a in list {
                let ActorRef::Fighter(vic) = a else { continue };
                if !is_match_fighter(w, vic) {
                    continue; // ActorClassFilter BP_FighterCharacter
                }
                let ux = fx - w.fighter(vic).body.x; // result.Y - owner.Y in UE (the sim's x is UE -Y)
                let mut hit = base.clone();
                hit.knock_back_direction_vector = Some(DataVec { x: Some(ux), y: Some(radius), z: None });
                if crate::combat::phases::no_hit_by(w, inst, vic) {
                    continue;
                }
                let me = ActorRef::Buff(b);
                let attack_id = hit.attack_id.unwrap_or(0);
                if invulnerable(w, vic) {
                    // AttackingActor: the buff actor
                    let tick = w.flow.tick;
                    attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
                } else {
                    dodge_over(w, me, vic);
                    let mut dl = dealer(w, me);
                    dl.attack_id = attack_id;
                    dl.x = fx;
                    dl.y = fy;
                    // HitLocation of a null HitCollision: the victim's capsule. The packet's IgnoreFacing X is UE +Y:
                    // world -x.
                    let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                    apply_projectile_hit(w, &dl, vic, &hit, &[], at, Some(V2 { x: -ux, y: radius }), &[], 0.0);
                }
            }
        }
    }
    fighter_emitter(w, f, cls, name!("PS_SupermanSPU_Impact"), radius / 350.0);
    fighter_emitter(w, f, cls, name!("PS_HarleyDebuff_Confetti"), 2.0);
    w.fighter_mut(f).controls.ui_state_ops.push(ui_add(
        name!("ConfettiExplosion"),
        icon,
        -1.0,
        -1.0,
        true,
        name!("Exploded"),
        1.0,
    ));
}

/// `d.UI_Icon ?? 'MI_State_Confetti'` of the live buff's values.
fn text_of_icon(w: &World, b: BuffRef) -> Name {
    let d = crate::combat::buffs::inst_view(w, b);
    d.get("UI_Icon").and_then(mvs_data::value::Value::as_str).map_or(name!("MI_State_Confetti"), |s| w.lit(s))
}

/// Buff_ApplyConfetti ReceiveBeginPlay @29.
fn apply_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    let Some(x) = buff(w, b) else { return };
    // BuffDuration as SpawnBuffActor left it: 0 (no life timer) on every row of the data.
    let seconds = if x.frames == Real::INFINITY { 0.0 } else { (x.frames - 1.0) * DELTA_SECONDS };
    let stacks = x.stacks;
    // A count capped earlier in this tick is gone in the build (deviation above): its pending explosion runs now.
    let capped = w
        .fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|x| {
            x.cls == name!("Buff_ConfettiExplosionCount_C")
                && x.frames > 0.0
                && matches!(&x.ext, BuffExt::ConfettiExplosion(e) if e.boom)
        })
        .map(|x| BuffRef { fighter: f, serial: x.serial });
    if let Some(c) = capped {
        boom(w, f, c);
    }
    // @110-432: the Instigator's CheckHasBuff(Buff_Harley_ConfettiExplosion) picks the class (no Instigator: false).
    let perk = from.is_some_and(|g| buff_of(w, g, name!("Buff_Harley_ConfettiExplosion_C")).is_some());
    let cls = if perk { name!("Buff_ConfettiExplosionCount_C") } else { name!("Buff_ConfettiCount_C") };
    spawn_buff(w, f, cls, seconds, Some(to_int32(stacks)), from, true, None);
    end_buff(w, f, b);
}

/// This file's registry entries (the gloves' kinds, the jack-in-the-box's, the three confetti buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let glove = ActorEntry::new(
        ActorClass::Projectile(ProjectileClass::HarleyDownAirProjectile),
        ActorHooks::Projectile(glove_kind()),
    );
    r.actors.enter("HarleyDownAirProjectile_C", glove)?;
    let side = ActorEntry {
        parents: &["HarleyDownAirProjectile_C"],
        ..ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::HarleySideAirProjectile2),
            ActorHooks::Projectile(glove_kind()),
        )
    };
    r.actors.enter("HarleySideAirProjectile2_C", side)?;
    let jack = WorldActorHooks {
        spawn: Some(jack_spawn),
        // its shapes read the actor's scale (the ground probe of BeginPlay is ahead of SetSizeScale: at 1); the view too
        size: Some(SizeHooks::default()),
        bodies: Some(jack_bodies),
        hurt_box: Some(jack_hurt),
        hit_shapes: Some(jack_hit_shapes),
        any_damage: Some(|w, id, _dmg, _causer| jack_explode(w, id)), // ReceiveAnyDamage @8011
        step: Some(jack_step),
        jump_pressed: Some(jack_jump_pressed),
        end: Some(jack_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        JACK,
        ActorEntry::new(ActorClass::World(WorldActorClass::HarleyJackInTheBox), ActorHooks::World(jack)),
    )?;
    r.buffs.enter(
        "Buff_ApplyConfetti_C",
        BuffEntry::new(BuffClass::BuffApplyConfetti, BuffHooks { begin: Some(apply_begin), ..BuffHooks::default() }),
    )?;
    r.buffs.enter(
        EXPLOSION,
        BuffEntry::new(
            BuffClass::BuffConfettiExplosionCount,
            BuffHooks {
                begin: Some(explosion_begin),
                on: Some(count_on),
                max_stacks: Some(explosion_max_stacks),
                timer: Some(explosion_timer),
                status: Some(count_status),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        COUNT,
        BuffEntry::new(
            BuffClass::BuffConfettiCount,
            BuffHooks {
                begin: Some(count_begin),
                on: Some(count_on),
                max_stacks: Some(count_max_stacks),
                status: Some(count_status),
                ..BuffHooks::default()
            },
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_jack_in_the_box_starts_where_the_ts_class_does() {
        let j = JackBox::default();
        assert_eq!(j.blast_at, f64::from(0.1_f32));
        assert_eq!(j.scan_at, Real::INFINITY);
        assert_eq!((j.reset_for, j.reset_age), (-1, -1));
        assert!(!j.active && !j.exploded && !j.ended);
    }

    #[test]
    fn deactivating_stops_the_enemy_scan() {
        let mut j = JackBox { active: true, scan_at: 3.0, ..JackBox::default() };
        jack_deactivate(&mut j);
        assert!(!j.active);
        assert_eq!(j.scan_at, Real::INFINITY);
    }

    #[test]
    fn the_explosion_buff_actor_credits_its_instigator() {
        let mut e = ConfettiExplosion::default();
        assert_eq!(explosion_owner(&e), None);
        assert!(explosion_dodge_mut(&mut e).is_some_and(|d| d.is_none()));
        let mut c = ConfettiCount::default();
        assert!(!c.begun);
        c.begun = true;
        assert_eq!(c.from, None);
    }

    #[test]
    fn a_confetti_status_is_a_type_one_state() {
        let op = ui_add(name!("a"), name!("b"), 2.0, 5.0, false, name!("c"), -1.0);
        let UiStateOp::Add(p) = op else { panic!("an add op") };
        assert_eq!((p.state_type, p.display_type), (1, 0));
        assert_eq!((p.current_value, p.max_value, p.timer), (2.0, 5.0, -1.0));
        assert!(p.subicon.is_none());
    }
}
