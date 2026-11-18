//! The signature perk of Batman Perk_Batman_HeavyGrapple: its buff Buff_Batman_HeavyGrapple_C and the hook
//! GrappleHookProjectile_Perk_C; the conditions BatmanHasHeavyGrapple_C / BatmanNoHeavyGrapple_C
//! (and mod.rs's conditions).
//!
//! Buff_Batman_HeavyGrapple_C: a BuffParentActor child with no function and no default of its own: a marker. What
//! it does is read by the conditions: with it the ConditionalSpawner of Batman_Special_F_Montage /
//! Batman_Special_Air_F_Montage spawns GrappleHookProjectile_Perk_C in place of GrappleHookProjectileV2_C, and
//! Batman_Special_F_Charge_Montage's HC1 is turned off. Entered empty, so the PerkManager's spawn of it is a known
//! class.
//!
//! GrappleHookProjectile_Perk_C (its ubergraph's offsets), a GrappleHookProjectileV2 child: the kit's hook kind
//! (`grapple`) with its own class data (ReelTravelVelocity 7500, ChargeAnimationDuration 0.35, ExplosionSize 450,
//! ExplosionHitbox) and these graphs:
//! - ReceiveBeginPlay @1015: the parent's, then with a valid Comp_Actor_MoveCommandManager on GetOwner() its
//!   MoveCommandCompleted bound to AfterGrappleTravel (@1155 bind, @1229 AddMulticastDelegate). Nothing unbinds it:
//!   every CompleteMoveCommand of the owner while the hook lives runs AfterGrappleTravel, the found and the not found
//!   alike.
//! - AfterGrappleTravel @1271 -> @458: authority; ExplosionHitbox.AttackID = the Instigator's PlayerState
//!   BPI_GetPlayerAttackCount @609-655 (0 when the cast fails, @987); SphereOverlapActors(self, (0, owner Y, owner Z),
//!   ExplosionSize, [Pawn 2, HurtBox 18], BP_FighterCharacter, no ignored actor) @884; each result in order @15-410:
//!   its own Comp_Pawn_HitBoxHandler.pfgDoDamage(null HitBox, it, ExplosionHitbox, no OnHitBuffs, no elementals) @79;
//!   then PrecisionGrappleVFX @415.
//! - PrecisionGrappleVFX @1276: SpawnEmitterAtLocation(PS_ShaggyClap, the owner's location, rot(0, 0, 0),
//!   ExplosionSize x 0.0057 on each axis) @1503 (a projectileDeath event).
//!
//! pfgDoDamage (Comp_Pawn_HitBoxHandler) on each fighter of the sphere, the graph having no filter of its own:
//! - a dead one pops @281; HitBox null: the packet is the call's; AttackID: the packet's when it is not 0
//!   @5331-5581 (the attack count written above), else the actor's BPI_GetAttackID;
//! - HitEachActorOnlyOnce with AttackTag None @2883-2985: one explosion hit per fighter and hook (the hook's own
//!   HitBox hits carry the tag Hit: a separate record, the projectile's hit set);
//! - the projectile pops (projectile_blocks, @4300-4598), then an invulnerable fighter takes the dodge path @4608 ->
//!   @7381 (CanBeHitBy, the hook's BPI_CanMarkFighter), with nothing tracked; else tracked @5290 and, for a team mate
//!   (the owner among them), NotifyAllyOfHit (ally_hit), for an enemy the hit and the actor's HitEnemyPlayer signals.
//!
//! Deviation: the hit's AttackID is the packet's in the build; the port's hit path reads the dealer's (the
//! projectile's attack id), so it is set to the packet's for the call and put back.
//! Unverified: the sphere's order is the gather's (sphere_overlap_actors: fighters in sim order).
//! Unverified: the knockback's side is the hook's yaw (FacingFlipOnSpawn: the facing at the spawn), as every packet a
//! projectile's handler deals.

use super::grapple::{self, Grapple, g_mut, owner, pr_mut, pr_ref, valid_target};
use crate::State;
use crate::actors::projectile::ProjectileExt;
use crate::actors::{ActorClass, ProjectileClass};
use crate::actors::{ActorRef, ProjectileId};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::buff_of;
use crate::combat::{HitPacket, elem, hits, phases};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_HURT_BOX, OBJ_PAWN, sphere_overlap_actors};
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, Condition, ProjectileHooks};
use crate::registry::props::{flag, num, num_or, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::pawns::is_match_fighter;
use crate::{fighter, name};
use mvs_data::value::Props;

/// Buff_Batman_HeavyGrapple_C.
pub const HEAVY_GRAPPLE: &str = "Buff_Batman_HeavyGrapple_C";
/// GrappleHookProjectile_Perk_C.
pub const PERK_GRAPPLE: &str = "GrappleHookProjectile_Perk_C";

/// `PerkGrapple`: the kit's hook and what the child adds.
#[derive(State, Debug, Default)]
pub struct PerkGrapple {
    pub g: Grapple,
    /// The fighter whose MoveCommandCompleted BeginPlay bound (GetOwner()).
    pub bound: Option<FighterId>,
    /// The handler's ActorsHitByAttacks[None]: the fighters its explosion took (ids).
    pub boom_hit: Vec<i32>,
    /// Broadcasts of this frame held to the hook's tick (see `move_command_completed`).
    pub boom_due: i32,
}

fn pg(w: &World, pr: ProjectileId) -> &PerkGrapple {
    match &pr_ref(w, pr).ext {
        ProjectileExt::PerkGrapple(g) => g,
        _ => panic!("not a GrappleHookProjectile_Perk_C"),
    }
}

fn pg_mut(w: &mut World, pr: ProjectileId) -> &mut PerkGrapple {
    match &mut pr_mut(w, pr).ext {
        ProjectileExt::PerkGrapple(g) => g,
        _ => panic!("not a GrappleHookProjectile_Perk_C"),
    }
}

static NO_PROPS: std::sync::LazyLock<Props> = std::sync::LazyLock::new(Props::new);

/// AfterGrappleTravel @458.
fn after_grapple_travel(w: &mut World, pr: ProjectileId) {
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    let f = owner(w, pr);
    let cp = grapple::class_p(w, pr);
    let p: Props = obj(cp, "ExplosionHitbox").unwrap_or(&NO_PROPS).clone();
    let r = num(cp, "ExplosionSize").unwrap_or(450.0);
    let count = w.fighter(f).decay.attack_count;
    let id = if count != 0 { count } else { pr_ref(w, pr).attack_id }; // @655, then pfgDoDamage @5331-5581
    let types = [crate::real(OBJ_PAWN), crate::real(OBJ_HURT_BOX)];
    let (fx, fy) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    let found = sphere_overlap_actors(w, fx, fy, r, &types, &[], -1.0);
    // the class filter
    let list: Vec<FighterId> = found
        .into_iter()
        .filter_map(|a| match a {
            ActorRef::Fighter(o) if is_match_fighter(w, o) => Some(o),
            _ => None,
        })
        .collect();
    let was = pr_ref(w, pr).attack_id; // see the module's Deviation
    pr_mut(w, pr).attack_id = id;
    let once = flag(&p, "HitEachActorOnlyOnce");
    let hp = HitPacket::from_props(&w.names, &p, None);
    let team = w.fighter(f).team.team;
    for vic in list {
        // (noHit: damage turned off or no collision, the gates every port caller of pfgDoDamage takes first)
        let vid = w.fighter(vic).id;
        if w.fighter(vic).life.dead > 0.0 || phases::no_hit(w, vic) || (once && pg(w, pr).boom_hit.contains(&vid)) {
            continue;
        }
        let d = hits::dealer(w, ActorRef::Projectile(pr));
        if hits::projectile_blocks(w, &d, vic) {
            continue;
        }
        if fighter::invulnerable(w, vic) {
            // @7452 CanBeHitBy pops a team mate (the owner too) with nothing tracked.
            if w.fighter(vic).team.team != team {
                let stuck = pg(w, pr).g.stuck;
                let tick = w.flow.tick;
                let bkb = num_or(&p, "BaseKnockBack", 0.0);
                fighter::control::attack_dodged(w, ActorRef::Projectile(pr), vic, id, bkb, tick, None, !stuck);
            }
            continue;
        }
        if once {
            pg_mut(w, pr).boom_hit.push(vid); // TrackActorsHitByTag @5290 (NotifyAllyOfHit's @392 for a team mate)
        }
        if w.fighter(vic).team.team == team {
            hits::ally_hit(w, &d, vic, &hp);
            // NotifyAllyOfHit -> the hook's own HitAlly @4186: IsValidGrappleActor -> QueuedAttachFighter = it,
            // QueuedAttachAlly = true, for ReceiveTick @755 (the kind's step) to take. Not the Instigator. An
            // attached hook's DoOnce is closed: nothing follows. Deviation: an explosion run from the kind's step is
            // behind that tick's ReceiveTick: the queue is taken a frame later.
            if vic != f && valid_target(w, pr, vic) {
                let g = g_mut(w, pr);
                g.queued = Some(vic);
                g.queued_ally = true;
            }
            continue;
        }
        // HitLocation: the victim's capsule (a null HitBox).
        let at = V2 { x: w.fighter(vic).body.x, y: fighter::actor_y(w, vic) };
        let pct = elem::charge_pct(w, ActorRef::Projectile(pr));
        if hits::apply_projectile_hit(w, &d, vic, &hp, &[], at, None, &[], pct) {
            hits::actor_hit_enemy(w, &d, vic);
        }
    }
    pr_mut(w, pr).attack_id = was;
    // PrecisionGrappleVFX @1276.
    let s = r * 0.0057;
    let (cls, fid) = (pr_ref(w, pr).cls, w.fighter(f).id);
    let (fx, fy) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_ShaggyClap")),
        x: fx,
        y: fy,
        ox: 0.0,
        rot: [0.0; 3],
        scale: s,
        tint: None,
        to: None,
    });
}

fn make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::PerkGrapple(PerkGrapple::default())
}

/// ReceiveBeginPlay @1015: the parent's, then the binding @1155-1229.
fn begin(w: &mut World, pr: ProjectileId) {
    grapple::begin(w, pr);
    let f = owner(w, pr);
    pg_mut(w, pr).bound = Some(f);
}

/// AfterGrappleTravel's binding. In the build the hook dies inside the owner's own broadcasts (OnJump @6081 on
/// OnIsJumping, OnEvade @6045 on EvadeTriggered, AfterOwnerKnockedBack @4170 on KnockedBack), and
/// AnimNotify_GrappleMoveCommand is a Queued state: a jump or dodge branch that stops the charge montage in the
/// controller's input has its NotifyEnd (BPI_EndGrappleMoveCommand -> CompleteMoveCommand) at the frame's
/// TriggerAnimNotifies, in the mesh tick, the hook already gone: no explosion. The port ends a queued state at a stop
/// made ahead of the frame's anim update at the stop itself, before the branch's jump or evade is written, and the
/// hook reads those at its own tick.
/// So: a broadcast that comes ahead of the owner's anim update of the frame (the controller's input) is held to the
/// hook's tick of that frame, which runs the kit's ReceiveTick tests first (the jump, evade and knockback of the frame
/// destroy it there); one from the anim update or later (the notify's end at the dispatch, the deferred ends, the
/// GrappleMoveTimer in the timer pass) runs at once unless the owner has jumped, dodged or been knocked back since
/// the hook's BeginPlay.
/// Deviation: the held explosion runs in the hook's tick, after every fighter's mesh tick of the frame, not in the
/// owner's TriggerAnimNotifies: the same frame and the owner's place after his move, later than the other fighters'
/// anim updates and overlaps of that frame.
fn move_command_completed(w: &mut World, pr: ProjectileId, f: FighterId, _found: bool) {
    if pg(w, pr).bound != Some(f) {
        return;
    }
    if !w.fighter(f).root.anim_updated {
        pg_mut(w, pr).boom_due += 1;
        return;
    }
    if grapple::owner_left(w, pr) {
        return;
    }
    after_grapple_travel(w, pr);
}

/// The held explosions where the owner's dispatch had them, ahead of the kit's ReceiveTick part; none when the owner
/// jumped, dodged or was knocked back (the kit's tests then destroy the hook).
fn step(w: &mut World, pr: ProjectileId) -> bool {
    if grapple::owner_left(w, pr) {
        pg_mut(w, pr).boom_due = 0;
    }
    while pg(w, pr).boom_due > 0 {
        after_grapple_travel(w, pr);
        pg_mut(w, pr).boom_due -= 1;
    }
    grapple::step(w, pr)
}

/// BatmanHasHeavyGrapple / BatmanNoHeavyGrapple Check: the owner's BPI_CheckIfHasBuff(Buff_Batman_HeavyGrapple_C)
/// .hasBuff @79-257, and its negation.
fn has_heavy_grapple(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_Batman_HeavyGrapple_C")).is_some()
}

fn no_heavy_grapple(w: &mut World, f: FighterId) -> bool {
    !has_heavy_grapple(w, f)
}

/// This file's registry entries.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        make: Some(make),
        begin: Some(begin),
        move_command_completed: Some(move_command_completed),
        step: Some(step),
        ..grapple::kind()
    };
    r.actors.enter(
        PERK_GRAPPLE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::GrappleHookProjectilePerk),
            ActorHooks::Projectile(kind),
        ),
    )?;
    // The marker (see the module's header): no graph.
    r.buffs.enter(HEAVY_GRAPPLE, BuffEntry::new(BuffClass::BuffBatmanHeavyGrapple, BuffHooks::default()))?;
    r.conditions.enter("BatmanHasHeavyGrapple_C", Condition::Fn(has_heavy_grapple))?;
    r.conditions.enter("BatmanNoHeavyGrapple_C", Condition::Fn(no_heavy_grapple))?;
    Ok(())
}
