//! The Court's ball chain (Map_M010_C SpawnBall, Item_Basketball_C). SpawnBall:
//! Delay(ObjectSpawnDelay 24) @1326 (resume @15), then BeginDeferredActorSpawnFromClass(Item_Basketball_C, (0, 0,
//! 1500), identity, AlwaysSpawn, no owner) @82, FinishSpawningActor @191, BPI_UpdateHitbox where the
//! BPI_SpawnedActor cast holds @379, OnDestroyed bound to the respawn @416-439, PF_Post_Event(
//! sfx_map_M010_ball_release, the ball) @480. The respawn: Delay(5) @1385 (resume @15) and the same spawn (no 24 s,
//! no AllowHazards check). One Delay per node: a second destruction while the wait runs adds no spawn. The ball is
//! an Item of World.items with no instigator (`spawn_stage_item`; its class defaults are StageData.actors); the
//! hoops score it (`hoop`).
//! BPI_UpdateHitbox (SpawnedActorParent @4657 UpdateHitbox: the handler's HitBoxDamage from the class's own) has
//! nothing to do in the port: an item's hit reads its class's HitBoxDamage (`Item::hit`).

use super::ids::{BALL_EV, LEVEL_OWNER};
use super::own_mut;
use crate::Real;
use crate::State;
use crate::actors::item::{ItemDef, ItemExt, num};
use crate::actors::{ItemClass, ItemId};
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::stage::stage_delay;

const EV_SPAWN: i32 = BALL_EV; // SpawnBall's Delay(24) resumed (1326 -> 15)
const EV_RESPAWN: i32 = BALL_EV + 1; // the OnDestroyed Delay(5) resumed (1385 -> 15)
const KEY_SPAWN: i32 = 1326;
const KEY_RESPAWN: i32 = 1385;
const SPAWN_DELAY: Real = 24.0; // Default__Map_M010_C.ObjectSpawnDelay
const RESPAWN_DELAY: Real = 5.0;
// (0, 0, 1500): sim x = -UE Y, y = UE Z
const SPAWN_X: Real = 0.0;
const SPAWN_Y: Real = 1500.0;

/// `CourtBall`: the chain's state: the ball's item id now (`Item::seq`), -1 while there is none.
#[derive(State, Debug, Default)]
pub struct CourtBall {
    pub item: i32,
}

/// `Basketball`: Item_Basketball_C's own variables. `kb`: the DamageManagers whose
/// KnockedBack has DropItem bound (PickUp @2352-2426, AddMulticastDelegate: once per object), as [fighter id, its
/// Fighter.knockedBackCount at the bind or the last call] pairs, in bind order.
#[derive(State, Debug)]
pub struct Basketball {
    /// ThrownFromLocation's Y (Thrown @1191: the actor location at the throw), as a sim x (-Y); 0 for the class
    /// default (0, 0, 0) and after ResetBall's zeroing (@2052-2218: a hit normal with |Y| > .9, resetBall).
    pub thrown_x: Real,
    /// CanBasketballBounce (default true; Thrown @1152 sets it, a slow capsule hit @931 clears it).
    pub can_bounce: bool,
    /// SpawnedActorParentVelocity as the hit reads it: the velocity at the start of the ball's step this frame
    /// (SpawnedActorParent ReceiveTick @8771: GetVelocity, written after the previous move).
    /// Unverified: taken as the velocity before this frame's move for the flight's hit and the body's contacts alike.
    pub pvx: Real,
    pub pvy: Real,
    pub kb: Vec<i32>,
    /// OnDestroyed bound to the level script's respawn (a ball SpawnBall made).
    pub chained: bool,
    /// LastHolder (SpawnedActorParent, BPI_SetLastHolder from Comp_ItemManager.EquipOnSpawn: the pickup only) as a
    /// fighter id, -1 for None; `Item::owner` (the Instigator) is not it: a reflect rewrites that.
    pub last_holder: i32,
}

impl Default for Basketball {
    fn default() -> Self {
        Basketball {
            thrown_x: 0.0,
            can_bounce: true,
            pvx: 0.0,
            pvy: 0.0,
            kb: Vec::new(),
            chained: false,
            last_holder: -1,
        }
    }
}

/// `ballAkKey`: the ball's own AkComponent (PF_Post_Event(.., the ball, the level script)).
fn ak_key(seq: i32) -> SoundKey {
    SoundKey { tag: name!("m010Ball"), n: seq }
}

/// `ballBegin`: SpawnBall from BeginPlay (`script::create`, AllowHazards true): the 24 s Delay.
pub fn begin(w: &mut World) {
    stage_delay(w, LEVEL_OWNER, KEY_SPAWN, SPAWN_DELAY, EV_SPAWN);
}

/// `ballTimer`: a due Delay of the ball chain (ev `BALL_EV` and up, owner `LEVEL_OWNER`): the spawn @15-553.
pub fn timer(w: &mut World, ev: i32) {
    if ev != EV_SPAWN && ev != EV_RESPAWN {
        return;
    }
    let Some(id) = crate::world::items::spawn_stage_item(w, name!("Item_Basketball_C"), SPAWN_X, SPAWN_Y) else {
        return;
    };
    let (seq, x, y) = {
        let it = w.actors.items.get_mut(id).expect("the spawned ball");
        // OnDestroyed (bound @439): Basketball.chained, read by the kind's end (the chain's state: CourtState.script.ball)
        if let ItemExt::Basketball(b) = &mut it.ext {
            b.chained = true;
        }
        (it.seq, it.x, it.y)
    };
    own_mut(w).script.ball.item = seq;
    w.events.push(Event::Sound {
        fighter: -1,
        ev: name!("sfx_map_M010_ball_release"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: Some(ak_key(seq)),
    });
}

fn state(w: &mut World, it: ItemId) -> &mut Basketball {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::Basketball(b) => b,
        _ => unreachable!("Item_Basketball_C holds a Basketball"),
    }
}

fn bind(w: &mut World, it: ItemId, f: FighterId) {
    let (id, count) = (w.fighter(f).id, w.fighter(f).attackers.knocked_back_count);
    let b = state(w, it);
    for i in (0..b.kb.len()).step_by(2) {
        if b.kb[i] == id {
            return;
        }
    }
    b.kb.push(id);
    b.kb.push(count);
}

fn unbind(w: &mut World, it: ItemId, f: Option<FighterId>) {
    let Some(f) = f else { return };
    let id = w.fighter(f).id;
    let b = state(w, it);
    for i in (0..b.kb.len()).step_by(2) {
        if b.kb[i] == id {
            b.kb.drain(i..i + 2);
            return;
        }
    }
}

/// DropItem @2472 (a bound KnockedBack): with IsHeld, MulticastThrowItem @10 (-> @2491: the Instigator's
/// Comp_ItemManager.ThrowItem((0, 0, 0), false, (0, 0)) @2676, whatever it holds), the Instigator's DamageManager
/// unbound @24-118, ResetBall((0, 0, 0)) @159 (a zero normal: nothing). Without IsHeld nothing, and the binding stays.
fn drop_item(w: &mut World, it: ItemId) {
    let i = w.actors.items.get(it).expect("a live item");
    if !i.held {
        return;
    }
    let inst = i.owner;
    if let Some(f) = inst {
        crate::world::items::drop_held_item(w, f);
    }
    unbind(w, it, inst);
}

/// `resetBall` (ResetBall(Normal) @2052): BreakVector, Y < -.9 or Y > .9 (strict) -> ThrownFromLocation = (0, 0, 0)
/// @2218. UE Y is the sim's -x: |nx|. A wall's or a backboard's side clears it; the hoops then score from (0, 0, 0):
/// |0 - (+/-2700)| >= 2500, 3 points (`hoop` points_earned).
fn reset_ball(b: &mut Basketball, nx: Real) {
    if (-nx) < -0.9 || (-nx) > 0.9 {
        b.thrown_x = 0.0;
    }
}

/// The Capsule's ComponentHit @422 (BndEvt__Capsule_..._ComponentHitSignature; HasAuthority: every peer runs the
/// sim). With CanBasketballBounce @456, a hit on anything but a BP_FighterCharacter @470-549 (the port's contacts
/// are stage boxes alone): VSize(SpawnedActorParentVelocity) > 1100 @591 -> BasketballBounceMulticast(
/// MirrorVectorByNormal(velocity x BasketballBounceMultiplier (.3), Hit.Normal), Hit.Normal) @802-894:
/// Capsule.AddImpulse(impulse, None, true) @1358 (a velocity change, added to the body's velocity after the parent's
/// ReceiveHit tail wrote its bounce) and ResetBall(normal) @1417; else CanBasketballBounce = false @931 and
/// ResetBall(normal) @1105.
fn component_hit(w: &mut World, it: ItemId, c: crate::actors::item::ItemContact) -> bool {
    let (nx, ny) = (c.nx, c.ny);
    let (can, pvx, pvy) = {
        let b = state(w, it);
        (b.can_bounce, b.pvx, b.pvy)
    };
    if !can {
        return false;
    }
    if crate::math::ue::size2d([pvx, pvy]) > 1100.0 {
        let data = std::sync::Arc::clone(&w.data);
        let src = w.actors.items.get(it).expect("a live item").src;
        let mult = num(ItemDef::at(&data, src).p, "BasketballBounceMultiplier").unwrap_or(0.3);
        let (vx, vy) = ((pvx * mult), (pvy * mult));
        let d = 2.0 * (vx * nx + vy * ny); // MirrorVectorByNormal: V - 2 (V . N) N
        let i = w.actors.items.get_mut(it).expect("a live item");
        i.vx += vx - d * nx;
        i.vy += vy - d * ny;
        reset_ball(state(w, it), nx);
        return false;
    }
    let b = state(w, it);
    b.can_bounce = false;
    reset_ball(b, nx);
    false
}

/// Thrown(Fighter) @1133 after the parent's: CanBasketballBounce = true, ThrownFromLocation, the Instigator's
/// DamageManager unbound @1218-1312 (the thrower: Instigator is LastHolder, `Item::owner`).
fn thrown(w: &mut World, it: ItemId) {
    let (x, owner) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.x, i.owner)
    };
    let b = state(w, it);
    b.can_bounce = true;
    b.thrown_x = x;
    unbind(w, it, owner);
}

/// PickUp(GrabbingActor) @2254 after the parent's: a BaseFighter grabber's DamageManager KnockedBack -> DropItem
/// (LastHolder is written in EquipOnSpawn, inside the parent's PickUp: here in the same call).
fn grabbed(w: &mut World, it: ItemId, f: FighterId) {
    let id = w.fighter(f).id;
    state(w, it).last_holder = id;
    bind(w, it, f);
}

/// Deviation: a bound KnockedBack broadcast is taken at the ball's next step (`Fighter.knockedBackCount`), not inside
/// the hit that broadcasts it; one call per broadcast counted, in bind order.
fn step(w: &mut World, it: ItemId) -> bool {
    let (vx, vy) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.vx, i.vy)
    };
    let n = {
        let b = state(w, it);
        b.pvx = vx;
        b.pvy = vy;
        b.kb.len()
    };
    let mut calls = 0;
    for i in (0..n).step_by(2) {
        let (id, seen) = {
            let b = state(w, it);
            (b.kb[i], b.kb[i + 1])
        };
        let Some(f) = w.by_id(id) else { continue };
        let now = w.fighter(f).attackers.knocked_back_count;
        if now == seen {
            continue;
        }
        state(w, it).kb[i + 1] = now;
        calls += 1;
    }
    while calls > 0 && !w.actors.items.get(it).expect("a live item").dead {
        drop_item(w, it);
        calls -= 1;
    }
    false
}

/// K2_DestroyActor: the release sound's AkComponent goes with the actor (StopWhenAttachedToDestroyed), then
/// OnDestroyed -> Delay(5) @1385. The Lab reset's destruction too (TrainGameMode Destroy All Players).
fn end(w: &mut World, it: ItemId) {
    let seq = w.actors.items.get(it).expect("an item").seq;
    w.events.push(Event::SoundStop { fighter: -1, key: ak_key(seq), fade: None, ev: None, curve: None, cut: false });
    let b = state(w, it);
    if !b.chained {
        return;
    }
    b.chained = false;
    if let Some(c) = super::try_own_mut(w)
        && c.script.ball.item == seq
    {
        c.script.ball.item = -1;
    }
    stage_delay(w, LEVEL_OWNER, KEY_RESPAWN, RESPAWN_DELAY, EV_RESPAWN);
}

/// The `actors({ [BALL]: { item: .. } })` entry: SetTeamColor (@1445-1968: the material by LastHolder's colour, Gray
/// after an ItemThrowHit on a player or an object) is the drawing's.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        sub: Some(|| ItemExt::Basketball(Basketball::default())),
        thrown: Some(thrown),
        component_hit: Some(component_hit),
        grabbed: Some(grabbed),
        step: Some(step),
        end: Some(end),
        ..ItemHooks::default()
    };
    r.actors.enter(
        ItemClass::ItemBasketball.name(),
        ActorEntry::new(ActorClass::Item(ItemClass::ItemBasketball), ActorHooks::Item(hooks)),
    )
}

use crate::actors::ActorClass;
