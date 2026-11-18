//! The Court's powerup chain and its three pickups (Map_M010_C PowerupSpawner,
//! PickupPowerup_Jump_C / _Power_C / _Speed_C, their buffs PickupJumpBuff_C / PickupPowerBuff_C / PickupSpeedBuff_C).
//! PowerupSpawner: Delay(ObjectSpawnDelay 24) @1440 (resume 1321 -> 554), then @554-1138: RandomIntegerInRange(0,
//! Array_LastIndex(PowerupList)) over [Speed, Power, Jump], the spawn at (0, 0, 900) (identity, AlwaysSpawn, no
//! owner), BPI_UpdateHitbox where the BPI_SpawnedActor cast holds, OnDestroyed bound to RespawnPowerup: Delay(30)
//! @1499 (resume 554) and the same spawn (no 24 s, no AllowHazards check). One Delay per node: a second destruction
//! while the wait runs adds no spawn.
//! A pickup (SpawnedActorParent child; its root a NoCollision Capsule, ProjectileMovement off by the parent's
//! EnableProjectileMovement false, MaxLifetime 0, no HitBox-tagged component: it neither moves nor hits nor ends on
//! its own) is held here, in the stage's state, not in the World's actors: a World actor kind has an owning fighter
//! whose team and data its passes read, and a stage-spawned actor has none (the same reason the level's placed
//! actors keep `StageState::actors`). Nothing of the World actors' passes applies to it (no hit, no hurt shape, no
//! solid, no mesh montage). Its BeginPlay: the parent's, then Delay(.5) @794 (resume @15): PowerupPickupCollision
//! (sphere 128 at relative Z 65, Pawn overlap, the other gameplay channels ignored) SetCollisionEnabled(QueryOnly),
//! the intro and type particles, sfx_map_M010_buff_spawn. Its begin overlap @486 (authority): OtherActor with a
//! Comp_State_Manager -> SpawnBuffActor(buff, OtherActor, OtherActor, 1, 10.0, BreakIfBlocked true, false) @657,
//! SpaceJamPickupVFXMulticast(OtherActor) @741 (pop text, the outro on the collector's root,
//! sfx_map_M010_buff_pickup at the pickup), K2_DestroyActor @764 (whatever SpawnBuffActor gave). No team, class or
//! collected lock.
//! Not ported (no effect the port can show): BPI_UpdateHitbox on it (SpawnedActorParent @4657: its HitBox, which no
//! component of the pickup carries), its passive Comp_Pawn_PassiveBuffManager entry (Buff_Elemental_Fireproof on
//! itself, StartingStacks 0), the parent's 0.1 s blast box check (DestroyOnExitedArena: (0, 0, 900) is always
//! inside).

use super::ids::{LEVEL_OWNER, POWERUP_EV};
use super::own_mut;
use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buff_class::BuffClass;
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::traces::{OBJ_PAWN, sphere_overlap_actors};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::stage_delay;

// The stage's numbers of its events (`StageEntry::timer` ev, owner LEVEL_OWNER) and Delay nodes (`StageTimer::key`:
// the Delay's bytecode offset; a pickup's own Delay keyed by the pickup's id, its own UUID per object).
const EV_SPAWN: i32 = POWERUP_EV; // PowerupSpawner's Delay(24) resumed (1321 -> 554)
const EV_RESPAWN: i32 = POWERUP_EV + 1; // RespawnPowerup's Delay(30) resumed (554)
const EV_ARM: i32 = POWERUP_EV + 2; // a pickup's BeginPlay Delay(.5) resumed (@15)
const KEY_SPAWN: i32 = 1440;
const KEY_RESPAWN: i32 = 1499;
const KEY_ARM: i32 = 1_000_000; // + the pickup's id
const SPAWN_DELAY: Real = 24.0; // Default__Map_M010_C.ObjectSpawnDelay
const RESPAWN_DELAY: Real = 30.0;
const ARM_DELAY: Real = 0.5;
// The spawn transform (0, 0, 900): sim x = -UE Y, y = UE Z. PowerupPickupCollision: SphereRadius 128 at relative Z
// 65 (the pickups' component templates; the actor's scale 1).
const SPAWN_X: Real = 0.0;
const SPAWN_Y: Real = 900.0;
const SENSOR_Z: Real = 65.0;
const SENSOR_R: Real = 128.0;
const BUFF_STACKS: i32 = 1;
const BUFF_DURATION: Real = 10.0;

const JUMP: Name = name!("PickupPowerup_Jump_C");
const POWER: Name = name!("PickupPowerup_Power_C");
const SPEED: Name = name!("PickupPowerup_Speed_C");
const JUMP_BUFF: Name = name!("PickupJumpBuff_C");
const POWER_BUFF: Name = name!("PickupPowerBuff_C");
const SPEED_BUFF: Name = name!("PickupSpeedBuff_C");
/// Default__Map_M010_C.PowerupList, in order.
const POWERUP_LIST: [Name; 3] = [SPEED, POWER, JUMP];

/// The buff a pickup class's overlap grants (`PICKUP_KINDS`'s `buff`; the rest of its entry, the intro's colour, the
/// pop text and the outro, is the drawing's).
fn buff_of(cls: Name) -> Option<Name> {
    if cls == JUMP {
        Some(JUMP_BUFF)
    } else if cls == POWER {
        Some(POWER_BUFF)
    } else if cls == SPEED {
        Some(SPEED_BUFF)
    } else {
        None
    }
}

/// `CourtPickup`: a live pickup. The drawing reads: cls, x / y (its root), armed (the tick its sensor came on: the
/// intro and type particles start there; nothing of it is visible before, its mesh and type component are hidden in
/// game).
#[derive(State, Debug, Default)]
pub struct CourtPickup {
    pub cls: Name,
    /// The chain's count of spawned pickups (`CourtPowerup::seq` at its spawn).
    pub id: i32,
    pub x: Real,
    pub y: Real,
    /// The world tick of its spawn.
    pub born: i32,
    /// The world tick its sensor came on, -1 before.
    pub armed: i32,
    pub dead: bool,
}

/// `CourtTaken`: SpaceJamPickupVFXMulticast(Collector) of the last collection: the pop text (UI_PopTextParams:
/// lifetime 1.1, scale 1, attached to the collector), the outro on the collector's root, the pickup sound at (x, y).
/// The drawing starts them at `tick`.
#[derive(State, Debug, Default)]
pub struct CourtTaken {
    pub cls: Name,
    pub id: i32,
    pub fighter: i32,
    pub tick: i32,
    pub x: Real,
    pub y: Real,
}

/// `CourtPowerup`: the chain's state. `script::create` makes `actor` -1; `begin` fills the rest (a world with
/// AllowHazards false has none of it and never spawns: `begun` false, the record's keys then absent).
#[derive(State, Debug, Default)]
pub struct CourtPowerup {
    /// The live pickup's id (`CourtPickup::id`), -1 while there is none.
    pub actor: i32,
    pub begun: bool,
    /// PowerupPickup: the live one.
    pub pickup: Option<CourtPickup>,
    /// Pickups spawned so far.
    pub seq: i32,
    /// The last collection, for the drawing.
    pub taken: Option<CourtTaken>,
}

fn pickup_ak_key(id: i32) -> SoundKey {
    SoundKey { tag: name!("m010Pickup"), n: id }
}

fn powerup(w: &mut World) -> &mut CourtPowerup {
    &mut own_mut(w).script.powerup
}

/// `powerupBegin`: PowerupSpawner from BeginPlay (`script::create`, AllowHazards true): the 24 s Delay.
pub fn begin(w: &mut World) {
    let p = powerup(w);
    p.begun = true;
    p.pickup = None;
    p.seq = 0;
    p.taken = None;
    stage_delay(w, LEVEL_OWNER, KEY_SPAWN, SPAWN_DELAY, EV_SPAWN);
}

/// `powerupTimer`: a due Delay of the powerup chain (ev `POWERUP_EV` and up, owner `LEVEL_OWNER`).
pub fn timer(w: &mut World, ev: i32) {
    if ev == EV_SPAWN || ev == EV_RESPAWN {
        spawn_pickup(w);
    } else if ev == EV_ARM {
        // a Delay of a destroyed pickup is gone with it (latent actions are the object's): only the live one arms
        if powerup(w).pickup.as_ref().is_some_and(|k| !k.dead && k.armed < 0) {
            arm(w);
        }
    }
}

/// `powerupTick`: the pickup's begin overlaps, once a tick in the leftover pass (`StageEntry::tick`), after the
/// fighters' movement.
/// Deviation: the build fires a begin overlap inside the mover's move (its tick group); the port tests in the
/// leftover pass of the same frame, so the buff is spawned later in the frame than the build's.
pub fn tick(w: &mut World) {
    if powerup(w).pickup.as_ref().is_some_and(|k| !k.dead && k.armed >= 0) {
        overlaps(w);
    }
}

/// `powerupReset`: World.reset (the Lab's TrainGameMode Destroy All Players destroys every SpawnedActorParent_C):
/// the pickup goes, with no buff and no multicast, and its OnDestroyed starts the 30 s respawn. At the World's first
/// reset there is none.
pub fn reset(w: &mut World) {
    if powerup(w).pickup.as_ref().is_some_and(|k| !k.dead) {
        destroy_pickup(w);
    }
}

/// @554-1138: the class, the spawn, the binding.
fn spawn_pickup(w: &mut World) {
    // RandomIntegerInRange(0, 2): one draw of the world's stream (`World::random_int`). Deviation: the build's
    // FMath::Rand stream and the order of its draws across the frame are not modelled.
    let n = crate::real(i32::try_from(POWERUP_LIST.len()).expect("a short list"));
    let at = usize::try_from(to_int32(w.random_int(n))).expect("an index of the list");
    let cls = POWERUP_LIST[at];
    let born = w.flow.tick;
    let p = powerup(w);
    let id = p.seq;
    p.seq = id + 1;
    p.pickup = Some(CourtPickup { cls, id, x: SPAWN_X, y: SPAWN_Y, born, armed: -1, dead: false });
    p.actor = id;
    // its BeginPlay (FinishSpawningActor): the parent's, then Delay(.5) @794. Deviation: the Delay runs as one of the
    // level script's (owner LEVEL_OWNER, its own key), in the leftover pass, not in the pickup's own tick
    // (SpawnedActorParent ticks: its latent actions run behind its ReceiveTick); the frame it fires in is the same
    // here (created in the leftover pass, counted from the next frame), its place within the frame is not. The
    // overlap generation begins after BeginPlay (EF "Overlaps around BeginPlay"), but its sensor is NoCollision
    // until the Delay ends: no overlap here.
    stage_delay(w, LEVEL_OWNER, KEY_ARM + id, ARM_DELAY, EV_ARM);
}

/// The Delay's resume @15: SetCollisionEnabled(QueryOnly), whose overlap update runs inside the call (EF
/// "SetCollisionEnabled updates overlaps at once"): a fighter already inside collects it there, ahead of the intro
/// particles and the spawn sound @53-299, which the build then makes on the destroyed actor all the same.
fn arm(w: &mut World) {
    let tick = w.flow.tick;
    let id = {
        let k = powerup(w).pickup.as_mut().expect("the live pickup");
        k.armed = tick;
        k.id
    };
    overlaps(w);
    // PF_Post_Event(sfx_map_M010_buff_spawn, self) @299: Lib_Audio's IsValid test (@85-181) fails on an actor the
    // overlap above destroyed (K2_DestroyActor @764 ran first): no spawn sound then.
    if let Some(k) = powerup(w).pickup.as_ref().filter(|k| !k.dead && k.id == id) {
        let (x, y) = (k.x, k.y);
        w.events.push(Event::Sound {
            fighter: -1,
            ev: name!("sfx_map_M010_buff_spawn"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: Some(pickup_ak_key(id)),
        });
    }
}

/// The sensor's overlaps: the first fighter whose root capsule (object type Pawn) is inside collects it. Every
/// fighter has a Comp_State_Manager, and a collection destroys the pickup, so no fighter is ever inside a live armed
/// sensor across two tests: each overlap found is a begin.
/// Deviation: only the fighters are tested; a Pawn-type body of another actor (ItemHelper_BananaGuard's
/// CollisionCylinder) with a Comp_State_Manager would collect it in the build. Unverified: two fighters entering in
/// one frame: the first of `sphere_overlap_actors`' order (fighter order; the build's overlap order is not read).
fn overlaps(w: &mut World) {
    let (kx, ky) = {
        let k = powerup(w).pickup.as_ref().expect("the live pickup");
        (k.x, k.y)
    };
    let cy = ky + SENSOR_Z;
    // the cheap bound first (no query in a frame where no fighter is near)
    let all = crate::registry::props::all_fighters(w);
    let mut near = false;
    for &f in &all {
        let (fx, fy) = (w.fighter(f).body.x, crate::fighter::actor_y(w, f));
        if (fx - kx).abs() <= SENSOR_R + crate::fighter::cap_radius(w, f) + 1.0
            && (fy - cy).abs() <= SENSOR_R + crate::fighter::capsule_half(w, f) + 1.0
        {
            near = true;
        }
    }
    if !near {
        return;
    }
    for a in sphere_overlap_actors(w, kx, cy, SENSOR_R, &[crate::real(OBJ_PAWN)], &[], -1.0) {
        let ActorRef::Fighter(f) = a else { continue };
        if !all.contains(&f) {
            continue;
        }
        collect(w, f);
        return;
    }
}

/// @516-764: the buff (stack 1, 10 s, the collector its own instigator), the multicast, K2_DestroyActor.
fn collect(w: &mut World, f: FighterId) {
    let tick = w.flow.tick;
    let (cls, id, x, y) = {
        let k = powerup(w).pickup.as_ref().expect("the live pickup");
        (k.cls, k.id, k.x, k.y)
    };
    let buff = buff_of(cls).expect("a pickup class of PowerupList");
    crate::combat::buffs::spawn_buff(w, f, buff, BUFF_DURATION, Some(BUFF_STACKS), Some(f), true, None);
    let fighter = w.fighter(f).id;
    powerup(w).taken = Some(CourtTaken { cls, id, fighter, tick, x, y });
    w.events.push(Event::Sound {
        fighter: -1,
        ev: name!("sfx_map_M010_buff_pickup"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: true,
        key: None,
    });
    destroy_pickup(w);
}

/// K2_DestroyActor: the pickup's own AkComponent stops what it plays (PF_Post_Event's StopWhenAttachedToDestroyed
/// true, EF 1071), then OnDestroyed -> RespawnPowerup -> Delay(30) @1499 (one per node).
/// Deviation: a collection in the port's leftover pass makes the Delay due 1801 ticks later; the build collects in
/// the fighters' tick group (`delay_at` GROUPS: counted this frame), which gives 1800. A Lab reset's destruction
/// (IDLE) counts from the next step in both.
fn destroy_pickup(w: &mut World) {
    let id = powerup(w).pickup.as_ref().expect("the live pickup").id;
    w.events.push(Event::SoundStop {
        fighter: -1,
        key: pickup_ak_key(id),
        fade: None,
        ev: None,
        curve: None,
        cut: false,
    });
    let p = powerup(w);
    p.pickup = None;
    p.actor = -1;
    stage_delay(w, LEVEL_OWNER, KEY_RESPAWN, RESPAWN_DELAY, EV_RESPAWN);
}

/// `buffs({ [JUMP_BUFF]: {}, [POWER_BUFF]: {}, [SPEED_BUFF]: {} })`: the three buffs (BuffParentActor children; their
/// modifiers are the parent's folds: PickupJumpBuff JumpMods JumpCountMod +1, MultJumpHeightMod 1.5, MultJumpSpeedMod
/// 1.2; PickupPowerBuff DealtDamageMods DamageMod 1.5 and KnockBackMod 1.1, both Multiply; PickupSpeedBuff SpeedMod
/// 1.3, Multiply; StageData.buffs). Their own graphs are the owner-attached particles alone: BeginPlay
/// SpawnEmitterAttached(PS_C016_PowerUp_<type>_Buff, the owner's BPI_GetMainMesh, socket Root, scale 1) @165,
/// ReceiveDestroyed K2_DestroyComponent on it @301. The drawing shows it while the buff lives (`Fighter.buffs`, by
/// class).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for (n, c) in [
        ("PickupJumpBuff_C", BuffClass::PickupJumpBuff),
        ("PickupPowerBuff_C", BuffClass::PickupPowerBuff),
        ("PickupSpeedBuff_C", BuffClass::PickupSpeedBuff),
    ] {
        r.buffs.enter(n, BuffEntry::new(c, BuffHooks::default()))?;
    }
    Ok(())
}
