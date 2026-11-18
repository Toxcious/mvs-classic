//! Reindog's WreckingBall and BigBrother perks: Creature_WreckingBall_C
//! ([`WreckingBall`], a projectile's `ext`, on the tethered ally), its spawn and destroy (the tether's calls), and the
//! two perk buffs (markers the tether reads).
//!
//! `WreckingBall`'s overrides are called from the shared code: `hitX` / `hitY` ([`hit_xy`]), `step`
//! ([`step`]); its getters `isAttached` / `attachedTo` through `actors::world_actor` (`ally`).
//!
//! Read: Buff_Creature_BigBrother, Creature_WreckingBall, TetherProjectile (the pull's branches @1546-2511 and
//! @2512-4900), Comp_Spawner (DestroySpawnedActors @5301 -> @15). The values are the perk table's (the buffs' and the
//! ball's class defaults). Both perks are IsUnavailableOffline (not on the port's perk page); ported all the same.
//! Both are self-only: the tether reads the OWNER's buffs (CheckHasBuff on Fighter_1 = GetOwner, @1546 / @2512).
//!
//! Buff_Creature_WreckingBall: a BuffParentActor child with no graph of its own. Its effect is the tether's pull
//! (`tether`'s start_pull / pull_end), which reads it by CheckHasBuff.
//!
//! Buff_Creature_BigBrother (ReceiveBeginPlay @10): the owner's BPI_GetFighterCharacter == 6 (ReinDog) @165 ->
//! Comp_Pawn_AbilityManager.Abilities[0].Cooldown *= AbilityCooldownMultiplier (0.75) @281 / @368: the manager's row,
//! written in place, with no reset (the perk's buff is spawned again at each respawn on the same pawn, PerkManager's
//! ResetFighter -> Add: the row is multiplied again each time). Nothing reads that row after the abilities are made:
//! Comp_Pawn_AbilityManager.CreateAbilities copied each row into its AbilityCooldownContainer
//! (SetStructurePropertyByName Ability @8377, Array_Add AbilityContainers @8545), and the container's cooldown is its
//! own copy's (AbilityCooldownContainer DefaultAbilityCooldown @33 / @81). CreateAbilities runs from
//! PlayerDataReadyServer (PandaGameMode SpawnAndInitPlayer @2209 -> PlayerDataReadyMulticast -> @142469), ahead of the
//! Possess (@2794 / @4490) whose ReceivePossessed (@136259) starts the PerkManager (@136510) that spawns this buff. The
//! other readers of the array: CreateAbilityUI (the Icon alone) and Comp_State_Manager @4007 (a copy at its BeginPlay,
//! ahead of the possession). So the buff changes nothing a match shows: the tether's cooldown stays 24 s. Its other
//! half is the tether's own branch (the ally pulled to the plain location, then the OWNER stopped and launched).
// Unverified: the order rests on the multicast running at once on the standalone authority (PlayerDataReadyServer
// -> PlayerDataReadyMulticast), stock UE, not read in the exe; and in the Lab a second PlayerDataReadyServer
// (TrainGameMode SpawnPlayerPawn @124 / @164 / @247) appends a second set of containers that Array_Get(slot) never
// reaches. Either read otherwise would make the buff's write the cooldown (24 x 0.75 = 18, compounding by respawn).
//!
//! Creature_WreckingBall_C (the shield ball): a SpawnedActorParent child. Its graph: ReceiveBeginPlay -> the parent's,
//! then (not a dedicated server) Visual.SetMaterial(0, the instigator's GetColorIndex: 0 -> Mat_Shield_P2, 1 ->
//! Mat_Shield_P1) @173-314: the render's. The owner's Comp_Spawner spawns it at the pull's start (TetherProjectile
//! @2731: CompSpawnActor(Creature_WreckingBall_C, AttachToActor true, the ally, socket None, WorldTransforms false,
//! identity, speeds 0)): attached to the ally's root at its actor location (Comp_Spawner @10232-10515:
//! K2_AttachToActor(parent, None), then SetActorRelativeTransform(identity)), so it rides the ally. Its components:
//! Capsule (root, bAbsoluteScale, no profile), Hitbox (sphere 225, tag HitBox, under the Capsule), Visual (the
//! engine's Sphere x 4.5, under the Hitbox). The parent's hit flow deals HitBoxDamage (5, BKB 750, KBS 20,
//! DirectionalKnockback, HitEachActorOnlyOnce) to each enemy the sphere meets, from its spawn on (HitBoxOn? true) and
//! it is not spent by a hit (DestroyedOnHit false); no HurtBox, no MaxLifetime; DestroyOnExitedArena? true (the
//! parent's 0.1 s blast box check), Die With Instigator true (World's teardown). It ends by the pull's end
//! (DestroySpawnedActors, [`destroy_balls`]), by the blast box, or with its owner.
//!
//! Model. A projectile of the world's list (so it is in the owner's spawned actors as the build's Comp_Spawner list
//! has it) spawned by `comp_spawn_projectile` at the ally's place, with its movement off (attached) and its step
//! riding the ally's actor location, as the tether does once on its ally.
// Deviation: the ride is taken in the projectile pass (after the fighters' moves), the build's attach updates the
// child inside the parent's move; the hit tests come after either way.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, StepEnd, class_def, expiry};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::BuffClass;
use crate::fighter::{FighterId, actor_y, facing};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::{comp_spawn_projectile, destroy_projectile};

/// `WRECKING_BALL_BUFF`.
pub const WRECKING_BALL_BUFF: Name = name!("Buff_Creature_WreckingBall_C");
/// `BIG_BROTHER_BUFF`.
pub const BIG_BROTHER_BUFF: Name = name!("Buff_Creature_BigBrother_C");
/// `BALL`.
pub const BALL: Name = name!("Creature_WreckingBall_C");

/// `WreckingBall extends Projectile`: `ally`, the attach parent (the getters read it).
#[derive(State, Debug, Default)]
pub struct WreckingBall {
    pub ally: Option<FighterId>,
}

fn ball(p: &Projectile) -> &WreckingBall {
    match &p.ext {
        ProjectileExt::WreckingBall(b) => b,
        e => panic!("Creature_WreckingBall_C without its state: {e:?}"),
    }
}

/// `WreckingBall.hitX` / `hitY`: the attach keeps it at the ally's actor location whenever the ally moves (the pull's
/// move comes after the projectile pass): the hit sphere is read there; its own place with no ally.
pub fn hit_xy(w: &World, p: &Projectile) -> (Real, Real) {
    match ball(p).ally {
        Some(a) => (w.fighter(a).body.x, actor_y(w, a)),
        None => (p.x, p.y),
    }
}

/// `WreckingBall.step` (no parent call): rides the ally's actor location, then the parent's timers (`expiry`: the
/// blast box check). Called by the shared step past its dead test.
pub fn step(w: &mut World, id: ProjectileId) -> Option<StepEnd> {
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    if p.dead {
        return None;
    }
    let at = ball(p).ally.map(|a| (w.fighter(a).body.x, actor_y(w, a), facing(w, a)));
    let p = w.actors.projectiles.get_mut(id).expect("a live projectile");
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.age += 1;
    if let Some((x, y, face)) = at {
        p.x = x;
        p.y = y;
        p.facing = face;
    }
    expiry(w, id)
}

/// The constructor's tail (`new WreckingBall`): AttachToActor true, so the movement is never active and the spawn is
/// not pushed out; the Hitbox sphere (no HitBox2D): SphereRadius x its world scale (the Capsule's: bAbsoluteScale,
/// 1); no HurtBox.
fn construct(w: &World, p: &mut Projectile) {
    p.attached = true;
    let hb = class_def(w, p).and_then(|d| obj(&d.components, "Hitbox"));
    let r = hb.and_then(|h| num(h, "SphereRadius")).unwrap_or(0.0);
    let s = hb.and_then(|h| obj(h, "RelativeScale3D")).and_then(|s| num(s, "X")).unwrap_or(1.0);
    p.hit_radius = r * s * p.scale;
    p.hurt_radius = 0.0;
    p.ext = ProjectileExt::WreckingBall(WreckingBall::default());
}

/// `spawnBall` (TetherProjectile @2731): the owner's CompSpawnActor of the ball on the ally.
pub fn spawn_ball(w: &mut World, owner: FighterId, ally: FighterId) {
    let (x, y) = (w.fighter(ally).body.x, actor_y(w, ally));
    let Some(pr) = comp_spawn_projectile(w, owner, BALL, x, y, [Some(0.0); 3], 0.0, 0.0, None) else { return };
    let face = facing(w, ally);
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return };
    if let ProjectileExt::WreckingBall(b) = &mut p.ext {
        b.ally = Some(ally);
        p.facing = face;
    }
}

/// `destroyBalls`: Comp_Spawner.DestroySpawnedActors(Creature_WreckingBall_C, DestroyAllActors true, false, false)
/// @4742 -> @15: every actor of that class on the owner's SpawnedActors list (FilterArray by class), destroyed from
/// the last; none found: three Delay(0.033) retries @1885-1998 that find none either (no ball is spawned in between),
/// so nothing.
pub fn destroy_balls(w: &mut World, owner: FighterId) {
    let mut i = w.fighter(owner).items.spawned_actors.len();
    while i > 0 {
        i -= 1;
        let Some(&ActorRef::Projectile(pr)) = w.fighter(owner).items.spawned_actors.get(i) else { continue };
        if w.actors.projectiles.get(pr).is_some_and(|p| p.cls == BALL && !p.dead) {
            destroy_projectile(w, pr, false);
        }
    }
}

/// This file's registry entries: the two perk buffs (the BigBrother write to the ability manager's row is read by
/// nothing: see the header) and the ball's kind.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        "Buff_Creature_WreckingBall_C",
        BuffEntry::new(BuffClass::BuffCreatureWreckingBall, BuffHooks::default()),
    )?;
    r.buffs
        .enter("Buff_Creature_BigBrother_C", BuffEntry::new(BuffClass::BuffCreatureBigBrother, BuffHooks::default()))?;
    r.actors.enter(
        "Creature_WreckingBall_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::CreatureWreckingBall),
            ActorHooks::Projectile(ProjectileHooks { construct: Some(construct), ..ProjectileHooks::default() }),
        ),
    )
}
