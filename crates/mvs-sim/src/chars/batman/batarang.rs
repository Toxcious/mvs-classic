//! Batman: Item_BatarangV2_C, his neutral special's batarang. Offsets are its ubergraph's
//! unless named.
//!
//! Item_BatarangV2 is an ItemActorParent child spawned by Comp_Spawner with EnableProjectileMovement (Batman's
//! Attack_N / Attack_Air_N notify: InitialSpeed 2100 / 1900, MaxSpeed 3000, ChargeSpeedMultiplier 2, Scale3D 1.2), so
//! it flies on its ProjectileMovement from the spawn ([`make`], `Item::step`). Its own variables are
//! [`BatarangState`], the item's [`ItemExt::Batarang`]. The spawn triggers ability slot 0 (TriggerAbilityOnSpawnIndex).
//! - ReceiveBeginPlay @1705: Original Scale = GetActorScale3D (the spawn transform's 1.2: the root Capsule has
//!   bAbsoluteScale), rotation 0, IsBouncerang = the owner's Buff_BatmanBouncerang (CheckHasBuff @1880 -> @2049), then
//!   the throw setup @204 ([`throw_setup`]). The flow pushed at 2073 runs Delay(0.2) -> @85: ReturnOverlap and
//!   PickupArea QueryOnly (the construction script turned the PickupArea off).
//! - Thrown @162 (a fighter's thrown release): SetActorScale3D(Original Scale), SetOwner, the setup again. Drop @7740:
//!   the parent's, then SetActorScale3D(Original Scale) (`Item::release`).
//! - OnReturn @8504 ([`on_return`]): not homing -> StartReturn @761; homing -> the 0.65 s timer again.
//! - ReceiveTick @3210 ([`tick`]): SpawnedActorParentVelocity, the loop's RTPC @3298, the Instigator's stick @3110.
//! - PickUp @3402 ([`grabbed`]), ItemThrowHitMulticast @3654 ([`throw_hit`]), HitEnemyPlayer @5847 ([`hit_enemy`]),
//!   the ReturnOverlap @4004 and PickupArea @4379 BeginOverlaps ([`overlaps`]), BatmanCollectBatarangServer @7705
//!   ([`collect`]), ReceiveEndPlay @5665 ([`end`]).
//! - StopBatarang ([`stop`]): Idle, CanControl / CanBeStopped / homing off, the trail destroyed, the loop stopped,
//!   rotation 0. It leaves the velocity, the ReturnTimer and the hitboxes as they are; nothing sets Idle false again.
//! - A fighter's melee on the lying batarang: after the first hit's tail ItemActorParent @4941-4984 sets
//!   Comp_Actor_Destructible.IsProjectile = !CanBeMeleedWhileIdle (false), so a fighter's hit box damages it.
//! - A reflect (Comp_ReflectProjectile): its Reflected @7770 and the end of its MulticastReflect @8682.

// `!(a > b)` is true for a NaN: partial_cmp would hide that.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::State;
use crate::actors::item::{Item, ItemDef, ItemExt, num};
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::OnHitBuffRow;
use crate::combat::buffs::{buff_of, end_buff, spawn_buff};
use crate::combat::elem::clear_homing;
use crate::combat::hit::knockback::timer_ticks;
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::common::latent_frames;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::math::ue::{KINDA_SMALL_NUMBER, SMALL_NUMBER, normal, safe_normal, size2d};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::registry::entries::{ItemHooks, ReflectHooks};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::{ActorList, DelegateCall, DelegateList, broadcast_bound};
use std::sync::Arc;

/// USphereComponent's default radius: ReturnOverlap's template sets none.
// Unverified: the constructor default, not read natively for this build.
const RETURN_RADIUS: Real = 32.0;

/// The Item_BatarangV2 variables the parent's do not hold.
#[derive(State, Debug, PartialEq)]
pub struct BatarangState {
    /// Comp_Actor_Destructible TeamIndex: the spawner's, then the grabber's (ItemActorParent PickUp), then the
    /// reflector's (Comp_ReflectProjectile @1596-1913).
    pub team: i32,
    /// Creator once a reflect wrote it (Reflected @8239-8415; `Some(None)`: none, every test on it fails); `None`
    /// until then: the spawner (BeginPlay's).
    pub creator_ov: Option<Option<FighterId>>,
    /// CanControl.
    pub can_control: bool,
    /// CanBeStopped.
    pub can_be_stopped: bool,
    /// Idle (StopBatarang; never false again).
    pub idle: bool,
    /// ThrownAlready (PickUp).
    pub thrown_already: bool,
    /// BombEquiped? (and the PS_Bombarang BombVFX with it).
    pub bomb: bool,
    /// AudioLooping.
    pub looping: bool,
    /// ProjectileMovement.bIsHomingProjectile.
    pub homing: bool,
    /// HomingAccelerationMagnitude.
    pub home_mag: Real,
    /// ProjectileMovement.InitialSpeed (the spawn's launch speed, a throw's VSize).
    pub initial_speed: Real,
    /// Original Scale.
    pub orig_scale: Real,
    /// ReturnTimer: timer passes until OnReturn, 0: none.
    pub return_left: Real,
    /// ReturnOverlap and PickupArea QueryOnly (the Delay(0.2) of BeginPlay).
    pub area_on: bool,
    /// ReturnOverlap.bGenerateOverlapEvents (the template default true).
    pub ret_gen: bool,
    /// ReturnOverlap / PickupArea overlap the Creator's capsule.
    pub in_ret: bool,
    pub in_area: bool,
    /// The DoOnce of BatmanCollectBatarangServer.
    pub collected: bool,
    /// SpawnedActorParentVelocity.
    pub parent_vx: Real,
    pub parent_vy: Real,
    /// For the drawing: SpawnedEffect (PS_Batarang) alive and shown (hidden while a bomb rides), the
    /// TeamColorPickupVFX (HitSpawnPickupVisuals) alive, and the age at which the Delay(0.0) ahead of it ends (-1).
    pub trail: bool,
    pub trail_shown: bool,
    pub pickup_vfx: bool,
    pub pickup_at: i32,
    /// IsBouncerang (BeginPlay @2049).
    pub is_bouncerang: bool,
    /// StartReturn's write of On Hit Buffs with IsBouncerang: the rows are the one Buff_Weakened row ([`own_rows`]).
    pub weakened_rows: bool,
}

impl Default for BatarangState {
    fn default() -> Self {
        BatarangState {
            team: 0,
            creator_ov: None,
            can_control: false,
            can_be_stopped: false,
            idle: false,
            thrown_already: false,
            bomb: false,
            looping: false,
            homing: false,
            home_mag: 0.0,
            initial_speed: 0.0,
            orig_scale: 1.0,
            return_left: 0.0,
            area_on: false,
            ret_gen: true,
            in_ret: false,
            in_area: false,
            collected: false,
            parent_vx: 0.0,
            parent_vy: 0.0,
            trail: false,
            trail_shown: false,
            pickup_vfx: false,
            pickup_at: -1,
            is_bouncerang: false,
            weakened_rows: false,
        }
    }
}

fn item(w: &World, it: ItemId) -> &Item {
    w.actors.items.get(it).expect("a live item")
}

fn item_mut(w: &mut World, it: ItemId) -> &mut Item {
    w.actors.items.get_mut(it).expect("a live item")
}

fn st(i: &Item) -> &BatarangState {
    match &i.ext {
        ItemExt::Batarang(b) => b,
        _ => unreachable!("an Item_BatarangV2 without its state"),
    }
}

fn st_mut(i: &mut Item) -> &mut BatarangState {
    match &mut i.ext {
        ItemExt::Batarang(b) => b,
        _ => unreachable!("an Item_BatarangV2 without its state"),
    }
}

fn bs(w: &mut World, it: ItemId) -> &mut BatarangState {
    st_mut(item_mut(w, it))
}

/// The Instigator: spawned by a fighter (its Comp_Spawner), never an item with no owner.
fn owner(w: &World, it: ItemId) -> FighterId {
    item(w, it).owner.expect("an Item_BatarangV2 with an owner")
}

/// Creator: the reflect's write, else the spawner.
fn creator(w: &World, it: ItemId) -> Option<FighterId> {
    let i = item(w, it);
    st(i).creator_ov.unwrap_or(i.spawner)
}

/// The 'Batarang Return' component's height over the Creator's actor location (`None`: the pawn has none): attached
/// to the root (no AttachParent), the root's rotation leaves Z alone.
fn return_z(w: &World, f: FighterId) -> Option<Real> {
    let pawn = &crate::fighter::common::data_of(w, f).fighter.components.pawn;
    pawn.values().find_map(|c| {
        let p = c.p.as_obj()?;
        let tags = p.get("ComponentTags").and_then(|t| t.as_arr())?;
        if !tags.iter().any(|t| t.as_str() == Some("Batarang Return")) {
            return None;
        }
        Some(p.get("RelativeLocation").and_then(|v| v.as_obj()).and_then(|l| num(l, "Z")).unwrap_or(0.0))
    })
}

/// A sphere at (x, y) against a fighter's root capsule (as `Item::overlaps_fighter`).
fn over_capsule(w: &World, x: Real, y: Real, r: Real, f: FighterId) -> bool {
    let (half, rad, ay) =
        (crate::fighter::capsule_half(w, f), crate::fighter::cap_radius(w, f), crate::fighter::actor_y(w, f));
    let fx = w.fighter(f).body.x;
    let seg = half - rad;
    let dy = y - max(ay - seg, min(y, ay + seg));
    let rr = r + rad;
    (x - fx) * (x - fx) + dy * dy <= rr * rr
}

/// ComputeHomingAcceleration toward the Creator's 'Batarang Return' component: the move's class homing
/// (`Item::step`), `None` while bIsHomingProjectile is off.
pub fn home_accel(w: &World, it: ItemId) -> Option<V2> {
    let i = item(w, it);
    let b = st(i);
    let (true, Some(c)) = (b.homing, creator(w, it)) else { return None };
    let z = return_z(w, c)?;
    let dx = w.fighter(c).body.x - i.x;
    let dy = crate::fighter::actor_y(w, c) + z - i.y;
    let [_, nx, ny] = safe_normal([0.0, dx, dy], SMALL_NUMBER);
    Some(V2 { x: nx * b.home_mag, y: ny * b.home_mag })
}

/// The On Hit Buffs rows StartReturn @828-1094 wrote with IsBouncerang (the Bouncerang perk), an instance variable of
/// this batarang: one row (Buff_Weakened_C, self false, on hit true, ally false, enemy true,
/// RequireHitToApplyBuffToSelf false, StackCount BouncerangWeakenedStacks, BuffDuration 0.0). `None`: the class's.
pub fn own_rows(w: &World, it: ItemId) -> Option<Vec<OnHitBuffRow>> {
    let i = item(w, it);
    if !st(i).weakened_rows {
        return None;
    }
    let stacks = num(i.def(&w.data).p, "BouncerangWeakenedStacks").unwrap_or(0.0);
    Some(vec![OnHitBuffRow {
        applied_buffs: name!("Buff_Weakened_C"),
        apply_buff_self: false,
        apply_buff_hit: true,
        apply_buff_ally: false,
        apply_buff_enemy: true,
        require_hit_to_apply_buff_to_self: false,
        stack_count: Some(to_int32(stacks)),
        buff_duration: Some(0.0),
    }])
}

/// An event of the actor's own calls (flushed with the spawner's at the end of the frame).
fn ev(w: &mut World, it: ItemId, e: Event) {
    let s = item(w, it).spawner.expect("an Item_BatarangV2 with a spawner");
    w.fighter_mut(s).events.push(e);
}

fn loop_key(w: &World, it: ItemId) -> SoundKey {
    SoundKey { tag: name!("batarang:"), n: item(w, it).seq }
}

fn owner_fid(w: &World, it: ItemId) -> i32 {
    w.fighter(owner(w, it)).id
}

fn stop_loop(w: &mut World, it: ItemId) {
    if !st(item(w, it)).looping {
        return;
    }
    let (fighter, key) = (owner_fid(w, it), loop_key(w, it));
    let stop = Some(name!("sfx_shared_item_batarang_loop_stop"));
    ev(w, it, Event::SoundStop { fighter, key, fade: None, ev: stop, curve: None, cut: false });
    bs(w, it).looping = false;
}

/// The setup @204-756 (BeginPlay and Thrown): ThrowHitBox overlaps, CanBeStopped false, ProjectileGravityScale 0,
/// AttachBatarangBomb, CanControl = !ThrownAlready @355, MulticastCreateVisuals, the loop unless AudioLooping
/// @462 / @680, ReturnOverlap overlaps @509, ReturnTimer = K2_SetTimerDelegate(OnReturn, InitialMoveDuration) @595.
fn throw_setup(w: &mut World, it: ItemId) {
    bs(w, it).can_be_stopped = false;
    item_mut(w, it).grav_scale = 0.0;
    attach_bomb(w, it);
    let b = bs(w, it);
    b.can_control = !b.thrown_already;
    // MulticastCreateVisuals @9257: a live SpawnedEffect is shown again, a new one is shown without a bomb.
    b.trail_shown = b.trail || !b.bomb;
    b.trail = true;
    if !b.looping {
        let (fighter, key) = (owner_fid(w, it), loop_key(w, it));
        ev(w, it, Event::SoundStart { fighter, ev: name!("sfx_shared_item_batarang_loop"), key });
        bs(w, it).looping = true;
    }
    let dur = num(item(w, it).def(&w.data).p, "InitialMoveDuration").unwrap_or(0.65);
    let b = bs(w, it);
    b.ret_gen = true;
    b.return_left = timer_ticks(Some(dur)) + 1.0; // set outside the timer pass
}

/// AttachBatarangBomb: the owner's Buff_BatmanOnHitApplyBuff is used up (K2_DestroyActor). No buff leaves the flag.
fn attach_bomb(w: &mut World, it: ItemId) {
    let o = owner(w, it);
    let Some(b) = buff_of(w, o, name!("Buff_BatmanOnHitApplyBuff_C")) else { return };
    bs(w, it).bomb = true;
    end_buff(w, o, b);
}

/// StopBatarang.
fn stop(w: &mut World, it: ItemId) {
    let b = bs(w, it);
    b.idle = true;
    b.can_control = false;
    b.can_be_stopped = false;
    clear_homing(w, ActorRef::Item(it)); // bIsHomingProjectile = false @33, a buff's homing with it
    let b = bs(w, it);
    b.trail = false;
    b.trail_shown = false;
    stop_loop(w, it);
    item_mut(w, it).pitch = 0.0;
}

/// OnReturn (the ReturnTimer, in the timer pass). StartReturn: MaxSpeed 3000, CanBeStopped, CanControl false @817;
/// with a 'Batarang Return' scene component on the Creator (Pawn_BatmanV2's, RelativeLocation (0, 0, 50) on the
/// root): Velocity 0, HomingAccelerationMagnitude = InitialSpeed x 5, bIsHomingProjectile, HomingTargetComponent.
fn on_return(w: &mut World, it: ItemId) {
    if st(item(w, it)).homing {
        let dur = num(item(w, it).def(&w.data).p, "InitialMoveDuration").unwrap_or(0.65);
        bs(w, it).return_left = timer_ticks(Some(dur)); // set inside the timer pass
        return;
    }
    let i = item_mut(w, it);
    i.max_speed = 3000.0;
    let b = st_mut(i);
    b.can_be_stopped = true;
    b.can_control = false;
    if b.is_bouncerang {
        bouncerang(w, it);
    }
    let Some(c) = creator(w, it) else { return };
    if return_z(w, c).is_none() {
        return;
    }
    let i = item_mut(w, it);
    // ProjectileMovement.Velocity: the body's velocity is another one while the movement is off.
    if i.flying {
        i.vx = 0.0;
        i.vy = 0.0;
    }
    // HomingTargetComponent and the magnitude over a buff's (none here: a buff's write sets the flag, and this runs
    // only while it is off).
    i.buff_home = None;
    let b = st_mut(i);
    b.home_mag = b.initial_speed * 5.0;
    b.homing = true;
}

/// StartReturn @828-1094 with IsBouncerang: On Hit Buffs.OnHitBuffs = the one Buff_Weakened row ([`own_rows`]), then
/// UpdateHitbox() @1094 (SpawnedActorParent @18336 -> @15092): HitBoxOn? -> the handler's HitBoxesOn(... On Hit
/// Buffs), which takes the new rows and clears nothing; else HitBoxesOff, every box off (a reflect's too) and the
/// actors already hit forgotten (ClearActorsHitByTag). Neither turns HitBoxOn? on: a batarang that has hit does not
/// hit again on the way back; one still in its flight carries the row.
fn bouncerang(w: &mut World, it: ItemId) {
    let i = item_mut(w, it);
    st_mut(i).weakened_rows = true;
    if i.hit_on {
        return;
    }
    if let Some(r) = &mut i.reflect {
        r.boxes = false;
    }
    i.hit_set.clear();
    i.hit_actors.clear();
}

/// DestroyVFX @6382 -> @5684 (as ReceiveEndPlay).
fn destroy_vfx(w: &mut World, it: ItemId) {
    bs(w, it).pickup_vfx = false;
    stop_loop(w, it);
}

/// BatmanCollectBatarangServer @7705 -> DoOnce @6387: the Creator's UpdateAbilityCooldownServer(0, false, 0,
/// CooldownPercentReturnedOnPickup) @6558, slot 2 too while BombEquiped @6679, CollectVisuals (PS_BatmanPickup at the
/// actor, scale 1.6; sfx_shared_item_batarang_despawn). True: ReleaseToPool.
fn collect(w: &mut World, it: ItemId) -> bool {
    if st(item(w, it)).collected {
        return false;
    }
    let Some(c) = creator(w, it) else { return false };
    bs(w, it).collected = true;
    let pct = num(item(w, it).def(&w.data).p, "CooldownPercentReturnedOnPickup").unwrap_or(0.675);
    crate::fighter::control::update_ability_cooldown(w, c, 0, false, 0.0, pct);
    if st(item(w, it)).bomb {
        crate::fighter::control::update_ability_cooldown(w, c, 2, false, 0.0, pct);
    }
    let (fighter, x, y) = (w.fighter(c).id, item(w, it).x, item(w, it).y);
    w.events.push(Event::ProjectileDeath {
        cls: name!("Item_BatarangV2_C"),
        fighter,
        vfx: Some(name!("PS_BatmanPickup")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.6,
        tint: None,
        to: None,
    });
    let ev = name!("sfx_shared_item_batarang_despawn");
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    true
}

/// Comp_Spawner HandleActorPostSpawn with EnableProjectileMovement (as Projectile's constructor): the speed with the
/// charge Lerp, the velocity along the notify's direction mirrored by the facing (no impulse, no half of the
/// fighter's velocity), MaxSpeed max(launch, the Lerp of the notify's), SpawnedActorParentVelocity = the velocity.
/// The notifies set neither AddMomentumAlpha nor MovementSpeedAddsToInitialSpeed.
fn make(w: &mut World, f: FighterId, r: &SpawnRequest, x: Real, y: Real) -> ItemId {
    let n = crate::world::spawning::request_props(w, f, r);
    let obj = |k: &str| n.get(k).and_then(|v| v.as_obj());
    let (dy, dz) = match obj("InitialMovementDirection") {
        Some(d) => (num(d, "Y").unwrap_or(0.0), num(d, "Z").unwrap_or(0.0)),
        None => (1.0, 0.0),
    };
    let alpha = if n.get("ChargeAffectsInitialSpeed").and_then(|v| v.as_bool()).unwrap_or(false) {
        min(max(crate::fighter::percent_charged(w, f), 0.0), 1.0)
    } else {
        0.0
    };
    let mult = num(&n, "ChargeSpeedMultiplier").unwrap_or(1.0);
    let mut speed = num(&n, "InitialSpeed").unwrap_or(0.0);
    speed += (speed * mult - speed) * alpha;
    let launch = speed * size2d([dy, dz]);
    let facing = crate::fighter::facing(w, f);
    let (vx, vy) = (dy * speed * facing, dz * speed);
    let scale = obj("Transforms").and_then(|t| t.get("Scale3D")).and_then(|s| s.as_obj()).and_then(|s| num(s, "X"));
    let scale = scale.unwrap_or(1.0);
    let max_n = num(&n, "MaxSpeed").unwrap_or(0.0);
    let src = crate::world::items::resolve_def(w, Some(f), ItemClass::ItemBatarangV2.name())
        .expect("Item_BatarangV2_C in its spawner's data");
    let data = Arc::clone(&w.data);
    let sb = crate::world::blast::spawn_box(w);
    let (cls, class) = (name!("Item_BatarangV2_C"), ItemClass::ItemBatarangV2);
    let mut b = Item::make(w, cls, class, Some(f), src, ItemDef::at(&data, src), x, y, 0.0, facing, vx, vy, &sb, false);
    // EnableProjectileMovement: the ProjectileMovement flies from the spawn; IsThrown and HitBoxOn? are on in the
    // class defaults.
    b.flying = true;
    b.thrown = true;
    b.hit_on = true;
    b.max_speed = max(launch, max_n + (max_n * mult - max_n) * alpha);
    b.scale = scale;
    b.attack_id = r.attack_id;
    b.ext = ItemExt::Batarang(BatarangState {
        team: w.fighter(f).team.team,
        initial_speed: launch,
        orig_scale: scale,
        parent_vx: vx,
        parent_vy: vy,
        ..BatarangState::default()
    });
    w.actors.items.insert(b)
}

/// The class's own variables (an item made outside [`make`]).
fn sub() -> ItemExt {
    ItemExt::Batarang(BatarangState::default())
}

/// ReceiveBeginPlay @1809-2049: GetOwner's Comp_State_Manager CheckHasBuff(Buff_BatmanBouncerang) -> IsBouncerang;
/// then the throw setup.
fn begin(w: &mut World, it: ItemId) -> bool {
    let o = owner(w, it);
    if buff_of(w, o, name!("Buff_BatmanBouncerang_C")).is_some() {
        bs(w, it).is_bouncerang = true;
    }
    throw_setup(w, it);
    false
}

/// Thrown @162: the release's SetMovementMulticast InitialSpeed (ThrowItem's VSize), then the setup @204. The
/// SetActorScale3D(Original Scale) of Thrown and Drop is `Item::release`'s.
fn thrown(w: &mut World, it: ItemId) {
    let i = item(w, it);
    let s = size2d([i.vx, i.vy]);
    bs(w, it).initial_speed = s;
    throw_setup(w, it);
}

/// PickUp @3402, after ItemActorParent's: MaxSpeed 3000, scale Original / 1.2, ReturnOverlap stops generating
/// overlaps, DestroyVFX, ThrownAlready, DetachBatarangBomb, StopBatarang.
fn grabbed(w: &mut World, it: ItemId, f: FighterId) {
    let team = w.fighter(f).team.team;
    let i = item_mut(w, it);
    i.max_speed = 3000.0;
    i.scale = st(i).orig_scale / 1.2;
    let b = st_mut(i);
    b.team = team;
    b.ret_gen = false;
    destroy_vfx(w, it);
    let b = bs(w, it);
    b.thrown_already = true;
    b.bomb = false;
    stop(w, it);
    let b = bs(w, it);
    // The actor's collision is off in a hand: its overlaps end.
    b.in_ret = false;
    b.in_area = false;
    // Unverified: whether ReceiveTick keeps writing SpawnedActorParentVelocity (GetVelocity) while it is held is not
    // read; taken as 0 in a hand.
    b.parent_vx = 0.0;
    b.parent_vy = 0.0;
}

/// ItemThrowHitMulticast @3654 after the parent's: anything but a fighter @3800 -> sfx_shared_item_batarang_hit at
/// the actor, DetachBatarangBomb, StopBatarang, then @3999 goto 3767 -> @3795 goto 15: Delay(0.0) ahead of
/// HitSpawnPickupVisuals @70. An enemy's is [`hit_enemy`]'s; an ally's does nothing.
fn throw_hit(w: &mut World, it: ItemId, hit_player: bool, _enemy: bool, _vx: Real, _vy: Real) {
    if hit_player {
        return;
    }
    let (fighter, x, y) = (owner_fid(w, it), item(w, it).x, item(w, it).y);
    let hit = name!("sfx_shared_item_batarang_hit");
    ev(w, it, Event::Sound { fighter, ev: hit, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    bs(w, it).bomb = false;
    stop(w, it);
    let i = item_mut(w, it);
    let at = i.age + to_int32(latent_frames(0.0));
    st_mut(i).pickup_at = at;
}

/// ReceiveTick @3210, after its move and hits: the parent's (SpawnedActorParentVelocity = GetVelocity),
/// SetRTPCValue(cha_batarang_velocity, VSize(GetVelocity), 0 ms) @3298, the Delay(0.2) @85 and the Delay(0.0) ahead
/// of HitSpawnPickupVisuals @4510 ending, then with authority @3110 the Instigator's stick: CanControl and
/// MoveYAxisValue != 0 -> Velocity = Normal(facing x InitialSpeed, (Y > 0 ? 0.5 : -0.5) x InitialSpeed) x
/// InitialSpeed, halved when both parts exceed 4000 @2719-2992.
fn tick(w: &mut World, it: ItemId) -> bool {
    let i = item_mut(w, it);
    let (vx, vy, age) = (i.vx, i.vy, i.age);
    let b = st_mut(i);
    b.parent_vx = vx;
    b.parent_vy = vy;
    // Deviation: the build's game object of the RTPC is the batarang itself; the port's is the owner's, where it plays
    // the loop: two batarangs of one owner in flight at once would share the value here.
    let fighter = owner_fid(w, it);
    let value = size2d([vx, vy]);
    w.events.push(Event::Rtpc { fighter, name: name!("cha_batarang_velocity"), value, interp_ms: 0.0 });
    let b = bs(w, it);
    if f64::from(age) == f64::from(latent_frames(0.2)) {
        b.area_on = true; // @85
    }
    if age == b.pickup_at {
        b.pickup_at = -1;
        b.pickup_vfx = true; // HitSpawnPickupVisuals @4510 (once: IsValid(TeamColorPickupVFX))
    }
    let f = owner(w, it);
    let my = w.fighter(f).controls.inp.y;
    let b = st(item(w, it));
    if !b.can_control || my == 0.0 {
        return false;
    }
    let is = b.initial_speed;
    let vy = (if my > 0.0 { 0.5 } else { -0.5 }) * is;
    let vx = crate::fighter::facing(w, f) * is;
    // The Normal node (tolerance 0.0001): zero below it, the vector at a sum of 1, else times InvSqrt.
    let [_, nx, ny] = normal([0.0, vx, vy], KINDA_SMALL_NUMBER);
    let i = item_mut(w, it);
    let k = if (ny * is).abs() > 4000.0 && (nx * is).abs() > 4000.0 { 0.5 } else { 1.0 };
    // ProjectileMovement.Velocity is an FVector: the written parts are float32.
    i.vx = nx * k * is;
    i.vy = ny * k * is;
    false
}

/// The ReturnTimer.
fn timer(w: &mut World, it: ItemId) -> bool {
    let b = bs(w, it);
    if b.return_left > 0.0 {
        b.return_left -= 1.0;
        if b.return_left == 0.0 {
            on_return(w, it);
        }
    }
    false
}

/// ReturnOverlap BeginOverlap @4004 (authority): the Creator, not IsInvulnerable?, and (HitBoxOn? and CanBeStopped)
/// or Idle -> BatmanCollectBatarangServer. PickupArea BeginOverlap @4379: Idle and the Creator -> the same. Only the
/// Creator's overlaps act. True: ReleaseToPool.
fn overlaps(w: &mut World, it: ItemId, only: Option<FighterId>) -> bool {
    let Some(c) = creator(w, it) else { return false };
    if only.is_some_and(|o| o != c) {
        return false;
    }
    // A tunnel carry's SetActorEnableCollision(false) (Buff_ProjectileThroughTunnel @189) ends both overlaps; @830
    // turns collision on and an overlap that holds then begins again.
    let i = item(w, it);
    let off = i.no_collision == Some(true);
    let b = st(i);
    let in_r = !off && b.area_on && b.ret_gen && over_capsule(w, i.x, i.y, RETURN_RADIUS * i.scale, c);
    if in_r != b.in_ret {
        bs(w, it).in_ret = in_r;
        let i = item(w, it);
        let b = st(i);
        if in_r && !crate::fighter::invulnerable(w, c) && ((i.hit_on && b.can_be_stopped) || b.idle) && collect(w, it) {
            return true;
        }
    }
    // Unverified: the two components' begin events of one update are taken ReturnOverlap first (the DoOnce makes the
    // order matter only for which test passes).
    let i = item(w, it);
    let d = i.def(&w.data);
    let in_a = !off && i.pickup_on() && over_capsule(w, i.pickup_x(d), i.pickup_y(d), i.pickup_radius(d), c);
    if in_a != st(i).in_area {
        bs(w, it).in_area = in_a;
        if in_a {
            let idle = st(item(w, it)).idle;
            let gone = idle && collect(w, it);
            // The PickupArea's later binds (a tutorial minigame's, bound after the batarang's own BeginPlay) in the
            // same broadcast, the item released to its pool only after it.
            // Unverified: only the Creator's begin is dispatched, as above (no other pawn's overlap is tracked).
            let at = DelegateList::Actor(ActorRef::Item(it), ActorList::PickupOverlap);
            broadcast_bound(w, at, &DelegateCall::PickupOverlap { other: ActorRef::Fighter(c), idle });
            if gone {
                return true;
            }
        }
    }
    false
}

/// HitEnemyPlayer @5847 (bound ahead of the parent's): ApplyBombOnHitServer @6773 (BombEquiped -> the victim gets
/// Buff_DelayKnockUp from the Instigator, 1 stack, 5.0 s; DetachBatarangBomb), BatarangHit @5956
/// (sfx_shared_item_batarang_hit, the Instigator's VO ability_batarang at 0.3, the loop stopped), then the parent's
/// (spent by the world ahead of this) -> ItemThrowHitMulticast's enemy branch: DetachBatarangBomb, StopBatarang,
/// Delay(0.0) -> HitSpawnPickupVisuals.
fn hit_enemy(w: &mut World, it: ItemId, vic: FighterId) -> bool {
    let o = owner(w, it);
    if st(item(w, it)).bomb {
        spawn_buff(w, vic, name!("Buff_DelayKnockUp_C"), 5.0, Some(1), Some(o), true, None);
        bs(w, it).bomb = false;
    }
    let (fighter, x, y) = (w.fighter(o).id, item(w, it).x, item(w, it).y);
    let hit = name!("sfx_shared_item_batarang_hit");
    w.events.push(Event::Sound { fighter, ev: hit, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    crate::fighter::match_side::post_vo(w, o, name!("ability_batarang"), Some(vic), 0.3);
    stop_loop(w, it);
    bs(w, it).bomb = false;
    stop(w, it);
    let i = item_mut(w, it);
    let at = i.age + to_int32(latent_frames(0.0));
    st_mut(i).pickup_at = at;
    false
}

/// ReceiveEndPlay @5665: TeamColorPickupVFX destroyed, the loop stopped.
fn end(w: &mut World, it: ItemId) {
    destroy_vfx(w, it);
}

fn item_of(a: ActorRef) -> ItemId {
    match a {
        ActorRef::Item(id) => id,
        _ => unreachable!("an Item_BatarangV2 hook on another actor"),
    }
}

/// Reflected @7770, after the parent's (empty): the component wrote the TeamIndex ahead of it (@1596-1913). With a
/// GetOwner that is a BP_FighterCharacter with a valid Comp_Actor_Destructible @7780-7955 and a Creator that is not in
/// its GetAllies @8016-8150 (the other fighters of its team not knocked out): Creator = the owner when its
/// BPI_GetFighterCharacter is 2 (Pawn_BatmanV2) or 1 (Pawn_Arya), else null @8160-8499. Of the 28
/// BPI_GetFighterCharacter definitions only Pawn_Arya returns 1 and only Pawn_BatmanV2 2.
/// Unverified: GetAllies' list as the port keeps it (its order). An owner that is its own Creator is not in its
/// GetAllies: the rule runs.
/// Deviation: an ally's reflect leaves the port's owner the old one, where the build's GetOwner read here is the
/// reflector (SetOwner @1385): `o` is the thrower.
fn reflected(w: &mut World, a: ActorRef) {
    let it = item_of(a);
    let o = owner(w, it);
    let c = creator(w, it);
    let team = w.fighter(o).team.team;
    bs(w, it).team = team;
    let ally = c.is_some_and(|c| c != o && w.fighter(c).team.team == team && !(w.fighter(c).life.dead > 0.0));
    if ally {
        return;
    }
    let key = w.names.str(w.fighter(o).ch);
    let ov = if key == "batman" || key == "arya" { Some(o) } else { None };
    bs(w, it).creator_ov = Some(ov);
}

/// MulticastReflect @8682, after the parent's: CanControl false @8710; on authority with ShouldPause and the
/// ReturnTimer active, the ReturnTimer cleared @8721-8801.
fn multicast(w: &mut World, a: ActorRef, _by: FighterId, pause: bool) {
    let b = bs(w, item_of(a));
    b.can_control = false;
    if pause && b.return_left > 0.0 {
        b.return_left = 0.0;
    }
}

/// Item_BatarangV2's actor entry.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        make: Some(make),
        sub: Some(sub),
        reflect: ReflectHooks { reflected: Some(reflected), multicast: Some(multicast), ..ReflectHooks::default() },
        begin: Some(begin),
        thrown: Some(thrown),
        throw_hit: Some(throw_hit),
        grabbed: Some(grabbed),
        hit_enemy: Some(hit_enemy),
        tick: Some(tick),
        timer: Some(timer),
        overlaps: Some(overlaps),
        end: Some(end),
        ..ItemHooks::default()
    };
    r.actors.enter(
        ItemClass::ItemBatarangV2.name(),
        ActorEntry {
            class: ActorClass::Item(ItemClass::ItemBatarangV2),
            parents: &[],
            foreign_owner: false,
            ownerless: false,
            keep_at_lab_reset: false,
            spawned_meet_bodies: false,
            calls_parent_hit_anything: false,
            elemental: None,
            hooks: ActorHooks::Item(hooks),
            carry: None,
            mesh: None,
            hazard: None,
            follower: None,
        },
    )
}
