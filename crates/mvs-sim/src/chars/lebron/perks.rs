//! LeBron's signature perks: Buff_C016_DistanceShot, Buff_C016_KeepPossession (actor buffs
//! a ball gets through GiveBuffToSpawnedActors) and the marker Buff_C016_HotHands. Sources: Buff_C016_DistanceShot,
//! Buff_C016_KeepPossession (offsets are their ubergraphs'); HotHands is read by BasketBallProjectile and
//! DashToBasketball (no graph of its own). Class defaults from the lebron perk part.
//!
//! DistanceShot and KeepPossession have GiveBuffToSpawnedActors: the PerkManager's buff on him gives its class to
//! every actor he spawns (SpawnedActorParent GrantBuffsFromInstigator @819), whose Instigator is he (SpawnBuffActor
//! @2217 resolves a non-pawn Instigator to its owner pawn: the actor's owner here). Each one's BeginPlay casts its
//! owner to BasketBallProjectile_C and binds nothing on any other actor (his bubble, barrier, chalk) or on himself
//! (the PerkManager's buff on the pawn): the actor-side entries below act on a ball alone.

use crate::actors::ActorRef;
use crate::combat::BuffClass;
use crate::combat::HitPacket;
use crate::combat::defense::add_temp_health_pack;
use crate::combat::elem::elem_buff;
use crate::combat::hits::apply_dealer_hit;
use crate::combat::packet::DataVec;
use crate::combat::perks::PerkDealer;
use crate::events::Event;
use crate::fighter::common::{actor_is_a, buff_def, lookup_owner};
use crate::fighter::control::{dealer_dodge_over, dealer_dodged};
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_PAWN, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, no_hit};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::{is_match_fighter, match_fighters};
use mvs_data::value::{Props, Value};

const BALL_CLASS: Name = name!("BasketBallProjectile_C");
const EXPLOSION_VFX: Name = name!("PS_C016_PERK_Explosion");
const DISTANCE_SHOT: Name = name!("Buff_C016_DistanceShot_C");

/// cast<BasketBallProjectile_C>(GetOwner()) (DistanceShot @1194, KeepPossession @64): the class or a child of it (the
/// Shot and the Pass). A class whose chain the data does not give is taken as not one.
fn is_ball(w: &World, a: ActorRef) -> bool {
    let Some(cls) = crate::actors::world_actor::actor_cls(w, a) else { return false };
    actor_is_a(w, ActorRef::Fighter(lookup_owner(w, a)), cls, BALL_CLASS) == Some(true)
}

// ---- Buff_C016_DistanceShot
// ReceiveBeginPlay @1134 (authority): the owner a BasketBallProjectile_C @1194, else nothing; then OnHitEnemy bound to
// the ball's Comp_Pawn_HitBoxHandler HitEnemyPlayer @1269 / @1292. The ball's own HitEnemyPlayer (@5362) is bound
// later (SpawnedActorParent HitBoxesOn @8152), so this one runs first: the ActorBuffSignal 'HitEnemyPlayerFirst'.
// OnHitEnemy(HitPlayer) @1356:
//   Vector_Distance(HitPlayer's location, the Instigator's location) >= MinimumDistance (1500) @1522, else nothing;
//   IgnoreActors = the Instigator's Comp_Actor_Destructible.GetAllies @1641-1686, plus the Instigator @1733;
//   SphereOverlapActors(the ball's location, ExplosionRadius 350, [Pawn], BP_FighterCharacter, IgnoreActors) @1895;
//   per result @15-699: ExplosionHitbox.KnockBackDirectionVector = (Y, Z) of the ball's GetVelocity() for HitPlayer,
//     else (Y, Z) of (the result's location - the ball's location), not normalised @560; then the buff's own
//     Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the result, ExplosionHitbox, null, [], [], false, false) @699 (a
//     DealerActor with buffActor, credited to the Instigator, as Harley's Buff_ConfettiExplosionCount);
//   after the loop: ExplosionVisuals @1035 (@1998: SpawnEmitterAtLocation(PS_C016_PERK_Explosion, the ball's location,
//     rot(0, 0, 0), scale 1) @2068, Team: the ball is no BPI_BaseFighter @2157, so ColorValue 0.0 and IsLocalOnline
//     false @2436-2459: Team 0.0 @2333), then the ball's K2_DestroyActor @1069, whose EndPlay destroys this buff (a
//     child of the ball).
// ExplosionHitbox (class data): Damage 6, BaseKnockBack 1100, KnockBackScaling 15, HitPauseTime 0.1, IgnoreFacing,
// HitEachActorOnlyOnce (the buff's own handler: one explosion per buff, the ball dies with it), PlayOnHitEffects,
// ApplyHitPauseToDamageDealer; HitReaction 0, AttackDecayType 0 as the data states them.
// The ball's own HitEnemyPlayer (@5362, ball.rs) is bound after this one and is skipped once this destroyed the ball
// (ProcessMulticastDelegate skips a PendingKill entry at its turn: native): no HitEnemy, no VO ability_pass_hit, no
// hit response. Below MinimumDistance this returns at once and the ball's handler runs.
// Unverified: SphereOverlapActors' order is the fighters' order, as the port's other sphere overlaps.
// Unverified: GetAllies taken as the Instigator's team mates in play (as Harley's explosion).
// Unverified: the ball's velocity is the one the port holds at the broadcast: for a hit raised inside the move, the
// velocity at the move's end (the build's handler reads it inside the sweep).
// Simplification: no DODGED text for an explosion over an invulnerable fighter (a handler no dodge tick visits), as
// Harley's and Wonder Woman's buff-actor hits.
// Deviation: the buff actor is in no list, so what the build hands it to as an actor (the victim's taken conditions,
// KnockedBack, HitByEnemy, EvadedAttack) gets none (combat/hits.rs `apply_dealer_hit`).
fn distance_shot(w: &mut World, a: ActorRef, hit_player: FighterId) {
    let ActorRef::Projectile(ball) = a else { return };
    let Some(inst) = crate::actors::world_actor::actor_owner(w, a) else { return }; // the buff's Instigator
    let d: Props = buff_def(w, ActorRef::Fighter(inst), DISTANCE_SHOT).cloned().unwrap_or_default();
    let num = |k: &str| d.get(k).and_then(Value::as_f64);
    let (dx, dy) =
        ((w.fighter(hit_player).body.x - w.fighter(inst).body.x), (actor_y(w, hit_player) - actor_y(w, inst)));
    let dist = ((dx * dx) + (dy * dy)).sqrt();
    // (a negated test: a NaN distance returns)
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a >= b)` keeps the NaN result")]
    if !(dist >= crate::math::narrow(num("MinimumDistance").unwrap_or(0.0))) {
        return;
    }
    let team = w.fighter(inst).team.team;
    let mut ignore: Vec<Option<ActorRef>> = match_fighters(w)
        .into_iter()
        .filter(|&o| o != inst && w.fighter(o).team.team == team && w.fighter(o).life.dead <= 0.0)
        .map(|o| Some(ActorRef::Fighter(o)))
        .collect();
    ignore.push(Some(ActorRef::Fighter(inst)));
    let Some(p) = w.actors.projectiles.get(ball) else { return };
    let (bx, by, vx, vy) = (p.x, p.y, p.vx, p.vy);
    let radius = num("ExplosionRadius").unwrap_or(0.0);
    let base = d
        .get("ExplosionHitbox")
        .and_then(Value::as_obj)
        .map_or_else(HitPacket::default, |h| HitPacket::from_props(&w.names, h, None));
    // The buff actor's own handler: a dealer no list holds (this buff is on the ball, not on a fighter's list),
    // credited to the Instigator; its dodge record is shared by the loop.
    let dealer = PerkDealer {
        cls: DISTANCE_SHOT,
        owner: inst,
        p: d.clone(),
        attack_id: base.attack_id.unwrap_or(0),
        hit: base.clone(),
        x: bx,
        y: by,
        facing: 1.0,
        charge: None,
        buff_actor: true,
        dodge: std::cell::RefCell::new(None),
    };
    for r in sphere_overlap_actors(w, bx, by, crate::math::narrow(radius), &[crate::real(OBJ_PAWN)], &ignore, -1.0) {
        let ActorRef::Fighter(vic) = r else { continue };
        if !is_match_fighter(w, vic) {
            continue; // ActorClassFilter BP_FighterCharacter
        }
        // The packet's (X, Y) is UE (Y, Z): the sim's x is UE -Y. (wx, wy): the same in the sim's axes.
        let (wx, wy) = if vic == hit_player { (vx, vy) } else { (w.fighter(vic).body.x - bx, actor_y(w, vic) - by) };
        let mut hit = base.clone();
        hit.knock_back_direction_vector = Some(DataVec { x: Some(-wx), y: Some(wy), z: None });
        if no_hit(w, vic) {
            continue;
        }
        if crate::combat::hits::invulnerable(w.fighter(vic)) {
            // AttackingActor: the buff actor
            let tick = w.flow.tick;
            dealer_dodged(w, &dealer, vic, hit.attack_id.unwrap_or(0), hit.base_knock_back.unwrap_or(0.0), tick);
            continue;
        }
        dealer_dodge_over(w, &dealer, vic);
        // HitLocation of a null HitCollision: the victim's capsule.
        let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
        apply_dealer_hit(w, &dealer, vic, &hit, at, V2 { x: wx, y: wy });
    }
    let id = w.fighter(inst).id;
    w.events.push(Event::ProjectileDeath {
        cls: DISTANCE_SHOT,
        fighter: id,
        vfx: Some(EXPLOSION_VFX),
        x: bx,
        y: by,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if w.actors.projectiles.get(ball).is_some_and(|p| !p.dead) {
        crate::world::spawning::destroy_projectile(w, ball, false); // K2_DestroyActor @1069
    }
    // The buff ends with the ball (endActorChildren in the destroy).
}

/// DistanceShot's actor-side `on`: the 'HitEnemyPlayerFirst' broadcast of a ball's handler, `arg` the victim.
fn distance_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, a_arg: Option<&ActorBuffArg>) -> bool {
    let arg = a_arg.map(|x| ActorRef::Fighter(x.by));
    debug_assert!(elem_buff(w, a, i).is_some_and(|b| b.class == BuffClass::BuffC016DistanceShot));
    let Some(ActorRef::Fighter(by)) = arg else { return false };
    if s == ActorBuffSignal::HitEnemyPlayerFirst && is_ball(w, a) {
        distance_shot(w, a, by);
    }
    false
}

// ---- Buff_C016_KeepPossession
// ReceiveBeginPlay @212 -> @10 (authority): the owner a BasketBallProjectile_C @64, else nothing; then OnPickedUp bound
// to its BasketBallPickedUp @143 / @166 (ball.rs sends it at the catch @6929: the ActorBuffSignal 'BasketBallPickedUp',
// arg.by the catcher).
// OnPickedUp(PickedUpBy) @227: cast to BaseFighter @227 (a fighter always is), then its Comp_Armor.AddTempHealthPack(
// {TempHealthType BasketBallTempHealth, CanOverride true, HealthAmount 3.0, Time 3.0}) @357-481. No team test.
// A team mate's pickup @8856 sends it too (ball.rs, arg.by the team mate); the send @650 (an ally inside the ball at
// its BeginPlay, CheckAllyInCollisionAtStart) is never reached in the port (ball.rs begin).
fn keep_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, a_arg: Option<&ActorBuffArg>) -> bool {
    let arg = a_arg.map(|x| ActorRef::Fighter(x.by));
    debug_assert!(elem_buff(w, a, i).is_some_and(|b| b.class == BuffClass::BuffC016KeepPossession));
    let Some(ActorRef::Fighter(by)) = arg else { return false };
    if s == ActorBuffSignal::BasketBallPickedUp && is_ball(w, a) {
        add_temp_health_pack(w, by, name!("BasketBallTempHealth"), 3.0, true, 3.0);
    }
    false
}

/// This file's registry entries. Buff_C016_HotHands is a marker (no graph of its own). Its readers are all on team
/// paths: BasketBallProjectile's ally pickup @7215 (CheckHasBuff, with IsANoLookPass @7423: Buff_IgnitedBasketballPass
/// in place of Buff_BasketballPass @7531; ball.rs) and DashToBasketball @1048 (an ignited team ball:
/// Buff_c016_IgnitedDunk, whose hits give Buff_Fire x4 and whose Dunk1 spawns the FireWall; later.rs). In a 1v1 the
/// perk does nothing, as in the build.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let on = |on| ActorBuffHooks { on: Some(on), ..ActorBuffHooks::default() };
    let list = [
        BuffEntry {
            actor: Some(on(distance_on)),
            ..BuffEntry::new(BuffClass::BuffC016DistanceShot, BuffHooks::default())
        },
        BuffEntry {
            actor: Some(on(keep_on)),
            ..BuffEntry::new(BuffClass::BuffC016KeepPossession, BuffHooks::default())
        },
        BuffEntry::new(BuffClass::BuffC016HotHands, BuffHooks::default()),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
