//! Reindog's Fastball: Fastball_C ([`Fastball`], an item's `ext`), an ItemActorParent
//! spawned by Reindog_AllyItem_Montage's AnimNotify_Spawner_C (0.2749 s to the montage's end, socket None, no
//! direction or speed, DestroyAllActorsOnNotifyEnd: the world destroys it at the notify's end). An ally picks it up
//! (RestrictedToTeam, CanBePickedUpByOwner false: never Reindog) and Reindog rides it, in the ally's hand and in its
//! flight (`fighter::tick` carry_attach / carry_end), until its End lets him go.
//!
//! The class's graph, by entry:
//! - ReceiveBeginPlay @1211 (after the parent's): InstigatingFighter = the Instigator (the spawner),
//!   K2_AttachToActor(it, SnapToTarget), its DamageManager.KnockedBack -> OnInstigatorKnockedBack
//!   (DestroySpawnedActor), its HitstunHandler.HitStunStart -> InstigatorStunned (the same) and its OnIsCarried ->
//!   OnOwnerIsCarried (the same; broadcast by nothing the port has).
//! - PickUp @2891 (after the parent's): MulticastSectionJump(PickedUpMontageSection 'Held') on the Instigator, the
//!   holder's Comp_State_Manager.NewBuffSpawned -> OnHoldingFighterBuffSpawned @7585 (a buff whose StateInfo.Group is
//!   12, NewEnumerator13: Buff_StunnedMoveToPositionThroughGeo, Buff_MoveToPosition_LassoGrapple,
//!   Buff_MoveCompTo_WithStun -> DestroySpawnedActor), Delay(0) -> @121: PickedUp = true and the Instigator welded to
//!   the item (`carry_attach` at the holder's hand socket).
//! - Thrown @3499 (after the parent's): with the movement's Velocity.Y not 0 the Instigator's BPI_SetFacingLogic(
//!   Velocity.Y > 0) @3873, BPI_SetIsBeingHeld(false) @3998; ThrownMontageSection is None: no jump @4036.
//! - Drop @2002 (after the parent's): Delay(0) -> @15: with HitBoxOn? nothing, else DestroySpawnedActor(true) @30 and
//!   MulticastSectionJump(CompleteMontageSection 'End') @97 (after the EndPlay the destroy ran).
//! - HitEnemyPlayer @2515 (after the parent's tail, which spends the item: Durability 1, destroyed there unless it is
//!   clanking) -> the facing write and @706: MulticastSectionJump(HitEnemyMontageSection 'End') @788,
//!   HitEnemyMontageTriggered = true @811.
//! - ReceiveHit @4662 / HitNonPlayer @4112 (after the parent's) -> @4131: the facing write, then unless
//!   HitEnemyMontageTriggered MulticastSectionJump(CompleteMontageSection) @4571.
//! - ReceiveEndPlay @4749: with PickedUp BPI_SetIsBeingHeld(false), the holder's delegate unbound, the item's root
//!   detached from nothing and the facing written back, ClearGravityOverride and Falling while the attach parent is
//!   valid (`carry_end`), and with the Instigator's montage Reindog_AllyItem_Montage in its Held section
//!   MulticastSectionJump('Cancel') @6449-6695. The native detach of the welded pawn follows (`destroy_item`).
//! - ReceiveTick: SetActorScale3D(1). MulticastSectionJump: the Instigator's JumpToSectionLogic(section, null).
//!   BPI_SetItemVisibility: the view's.
//!
//! Model. A subclass of Item (the parent's physics, pickup area, throw and hit); its latent actions and the delegates
//! it binds are taken in its kind's step, once a tick in a hand or not, ahead of its move: the Delay(0) of a pickup or
//! a drop (both made earlier in the frame, the world's pickups and release pass) ends in the same frame's step.
// Unverified: the Delay(0) ends in the same frame's step, as Item.release's.
// Unverified: before the first pickup the item is pinned at the Instigator's actor location (the attach with
// SnapToTarget; a simulated root's attach is taken to follow its parent), its own physics not run.
// Deviation: the Instigator's KnockedBack and HitStunStart, and the holder's NewBuffSpawned, are sampled in the
// kind's step: a KnockedBack by the fighter's knocked-back count, a HitStunStart by a buff stun that began since the
// last step (a knockback's comes with its KnockedBack), a NewBuffSpawned by a live buff of the Move group the holder
// did not have at the bind. The build destroys the item inside the broadcast (the hit, the spawn).
//
// `Fastball`'s method overrides are called from the shared code (`world::items`): `pickUp` past the
// parent's ([`pick_up`]), `release` past the parent's ([`release`]), `step` (the pin before and after the parent's
// until the first hold: [`pinned`], [`pin`]).

use crate::Real;
use crate::State;
use crate::actors::classes::ItemClass;
use crate::actors::item::{Item, ItemDef, ItemExt};
use crate::actors::{ActorClass, ActorRef, ItemId};
use crate::fighter::common::buff_def;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::match_side::hold_point;
use crate::fighter::tick::{carry_attach, carry_end, face_along, in_section, jump_section_logic, set_is_being_held};
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ItemHooks};
use crate::registry::{Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::items::{destroy_item, resolve_def};
use std::sync::Arc;

const MONTAGE: Name = name!("Reindog_AllyItem_Montage");
/// EStateGroups 12 (OnHoldingFighterBuffSpawned @7585).
const MOVE_GROUP: &str = "StateGroups::NewEnumerator13";

/// A holder whose Comp_State_Manager.NewBuffSpawned is bound (PickUp @3379, once per holder: the multicast delegate
/// adds a handler once), with the Move group buffs it had at the bind. EndPlay @7054 unbinds through the current
/// Instigator (the last holder) alone; an earlier holder's binding stays while the item lives.
#[derive(State, Debug, Default)]
pub struct Hold {
    pub id: i32,
    pub had: Vec<Name>,
}

/// `Fastball extends Item`: what the class adds.
#[derive(State, Debug)]
pub struct Fastball {
    /// PickedUp.
    pub picked: bool,
    /// The parent's PickUp ran once (the attach to the Instigator is over).
    pub ever_held: bool,
    /// PickUp's Delay(0) is pending.
    pub pick_due: bool,
    /// Drop's Delay(0) is pending.
    pub drop_due: bool,
    /// HitEnemyMontageTriggered.
    pub triggered: bool,
    /// HitEnemyPlayer's part behind the destroy it waits for.
    pub end_tail: bool,
    /// The Instigator's knocked-back count at the bind.
    pub kb_seq: i32,
    /// The Instigator's buff stun at the last step, and its age then.
    pub stun_was: bool,
    pub stun_age: Real,
    pub holds: Vec<Hold>,
    /// The Instigator PickUp set (the last holder; -1: none).
    pub hold_last: i32,
}

impl Default for Fastball {
    /// The field initial values.
    fn default() -> Self {
        Fastball {
            picked: false,
            ever_held: false,
            pick_due: false,
            drop_due: false,
            triggered: false,
            end_tail: false,
            kb_seq: 0,
            stun_was: false,
            stun_age: 0.0,
            holds: Vec::new(),
            hold_last: -1,
        }
    }
}

fn fb(w: &World, it: ItemId) -> &Fastball {
    match &w.actors.items.get(it).expect("a live item").ext {
        ItemExt::Fastball(s) => s,
        _ => panic!("item {it:?} is not a Fastball"),
    }
}

fn fb_w(w: &mut World, it: ItemId) -> &mut Fastball {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::Fastball(s) => s,
        _ => panic!("item {it:?} is not a Fastball"),
    }
}

/// `this.spawner`: spawned by a fighter (its Comp_Spawner), never a stage item with no instigator.
fn spawner(w: &World, it: ItemId) -> FighterId {
    w.actors.items.get(it).expect("a live item").spawner.expect("a Fastball is spawned by a fighter")
}

/// A string of the class defaults (`this.p.<key>`).
fn class_str(w: &World, it: ItemId, key: &str) -> Option<Name> {
    let s = crate::world::items::item_def(w, it)?.get(key)?.as_str()?;
    Some(w.names.get(s).unwrap_or_else(|| panic!("Fastball_C {key} {s:?} is not a data string")))
}

/// A section name that is set and not None.
fn real_section(w: &World, s: Option<Name>) -> Option<Name> {
    s.filter(|&n| w.names.str(n) != "None")
}

/// `isMove`: a buff class whose StateInfo.Group is the Move group.
fn is_move(w: &World, f: FighterId, cls: Name) -> bool {
    buff_def(w, ActorRef::Fighter(f), cls)
        .and_then(|d| d.get("StateInfo"))
        .and_then(|s| s.as_obj())
        .and_then(|s| s.get("Group"))
        .and_then(|g| g.as_str())
        == Some(MOVE_GROUP)
}

/// The classes of a fighter's live Move group buffs.
fn move_buffs(w: &World, f: FighterId) -> Vec<Name> {
    let list: Vec<Name> = w.fighter(f).buffs.buffs.iter().filter(|b| b.frames > 0.0).map(|b| b.cls).collect();
    list.into_iter().filter(|&c| is_move(w, f, c)).collect()
}

/// `Fastball.pickUp` past `super.pickUp(f)`.
pub fn pick_up(w: &mut World, it: ItemId, f: FighterId) {
    let s = spawner(w, it);
    fb_w(w, it).ever_held = true;
    if let Some(sec) = real_section(w, class_str(w, it, "PickedUpMontageSection")) {
        jump_section_logic(w, s, sec);
    }
    let id = w.fighter(f).id;
    let had = move_buffs(w, f);
    let x = fb_w(w, it);
    x.hold_last = id;
    if !x.holds.iter().any(|h| h.id == id) {
        x.holds.push(Hold { id, had });
    }
    x.pick_due = true;
}

/// `Fastball.release` past `super.release(...)`.
pub fn release(w: &mut World, it: ItemId, thrown: bool) {
    if !thrown {
        fb_w(w, it).drop_due = true;
        return;
    }
    let s = spawner(w, it);
    face_along(w, s, it);
    set_is_being_held(w, s, false);
    if let Some(sec) = real_section(w, class_str(w, it, "ThrownMontageSection")) {
        jump_section_logic(w, s, sec);
    }
}

/// `Fastball.step`'s test: `!everHeld && !dead` (the pin runs around the parent's step).
pub fn pinned(w: &World, it: ItemId) -> bool {
    !fb(w, it).ever_held && !w.actors.items.get(it).expect("a live item").dead
}

/// `Fastball.pin`: at the spawner's root, at rest.
pub fn pin(w: &mut World, it: ItemId) {
    let s = spawner(w, it);
    let (x, y) = (w.fighter(s).body.x, actor_y(w, s));
    let i = w.actors.items.get_mut(it).expect("a live item");
    (i.x, i.prev_x, i.y, i.prev_y) = (x, x, y, y);
    (i.vx, i.vy) = (0.0, 0.0);
}

/// @706 / @4131's facing write and section jump; true when the section is not None (the jump ran).
fn section_tail(w: &mut World, it: ItemId, section: Option<Name>) -> bool {
    let s = spawner(w, it);
    face_along(w, s, it);
    let Some(sec) = real_section(w, section) else { return false };
    jump_section_logic(w, s, sec);
    true
}

/// The delegates bound in BeginPlay (the Instigator's) and in PickUp (the holder's): true when one fired.
fn signalled(w: &mut World, it: ItemId) -> bool {
    let s = spawner(w, it);
    let (stunned, age, kb) = {
        let fi = w.fighter(s);
        (fi.buffs.buff_stunned, fi.buffs.buff_stun_age, fi.attackers.knocked_back_count)
    };
    let x = fb_w(w, it);
    let stun_new = stunned && (!x.stun_was || age <= x.stun_age);
    x.stun_was = stunned;
    x.stun_age = age;
    if kb != x.kb_seq || stun_new {
        return true;
    }
    let holds: Vec<(i32, Vec<Name>)> = fb(w, it).holds.iter().map(|h| (h.id, h.had.clone())).collect();
    for (id, had) in holds {
        let Some(h) = w.by_id(id) else { continue };
        if move_buffs(w, h).iter().any(|c| !had.contains(c)) {
            return true;
        }
    }
    false
}

/// AdjustIfPossibleButAlwaysSpawn: FindTeleportSpot for the Capsule (80 / 80) against what blocks an item comes in
/// `add_item` (the push-out).
fn make(w: &mut World, f: FighterId, _r: &SpawnRequest, x: Real, y: Real) -> ItemId {
    let src = resolve_def(w, Some(f), ItemClass::Fastball.name()).expect("Fastball_C in its spawner's data");
    let data = Arc::clone(&w.data);
    let sb = crate::world::blast::spawn_box(w);
    let facing = crate::fighter::facing(w, f);
    let mut it = Item::make(
        w,
        name!("Fastball_C"),
        ItemClass::Fastball,
        Some(f),
        src,
        ItemDef::at(&data, src),
        x,
        y,
        0.0,
        facing,
        0.0,
        0.0,
        &sb,
        false,
    );
    it.ext = ItemExt::Fastball(Fastball::default());
    w.actors.items.insert(it)
}

/// The class's own variables (an item made outside [`make`]).
fn sub() -> ItemExt {
    ItemExt::Fastball(Fastball::default())
}

/// ReceiveBeginPlay @1211: the binds on the Instigator, and the attach.
fn begin(w: &mut World, it: ItemId) -> bool {
    let s = spawner(w, it);
    let (kb, stunned, age) = {
        let fi = w.fighter(s);
        (fi.attackers.knocked_back_count, fi.buffs.buff_stunned, fi.buffs.buff_stun_age)
    };
    let x = fb_w(w, it);
    x.kb_seq = kb;
    x.stun_was = stunned;
    x.stun_age = age;
    pin(w, it);
    false
}

/// The kind's step: the delegates, ReceiveTick's scale, the Delay(0)s. True: DestroySpawnedActor.
fn step(w: &mut World, it: ItemId) -> bool {
    if signalled(w, it) {
        return true;
    }
    w.actors.items.get_mut(it).expect("a live item").scale = 1.0; // ReceiveTick: SetActorScale3D(1)
    let s = spawner(w, it);
    if fb(w, it).pick_due {
        // @121.
        let x = fb_w(w, it);
        x.pick_due = false;
        x.picked = true;
        let holder = w.actors.items.get(it).expect("a live item").holder;
        if holder >= 0
            && let Some(h) = w.by_id(holder)
        {
            let (hx, hy) = hold_point(w, h);
            carry_attach(w, s, it, hx, hy);
        }
    }
    if fb(w, it).drop_due {
        // @15.
        fb_w(w, it).drop_due = false;
        if !w.actors.items.get(it).expect("a live item").hit_on {
            destroy_item(w, it);
            if let Some(sec) = real_section(w, class_str(w, it, "CompleteMontageSection")) {
                jump_section_logic(w, s, sec);
            }
        }
    }
    false
}

/// HitEnemyPlayer @2515: the parent's tail destroys the item next (`item_hits`): the rest runs behind its EndPlay.
fn hit_enemy(w: &mut World, it: ItemId, _vic: FighterId) -> bool {
    let i = w.actors.items.get(it).expect("a live item");
    if i.durability < 1.0 && !i.clanking {
        fb_w(w, it).end_tail = true;
        return false;
    }
    let sec = class_str(w, it, "HitEnemyMontageSection");
    if section_tail(w, it, sec) {
        fb_w(w, it).triggered = true; // @778 -> @788, @811
    }
    false
}

/// ReceiveHit @4662: the facing write, then unless HitEnemyMontageTriggered the Complete section.
fn receive_hit(w: &mut World, it: ItemId, _destroyed: bool) -> bool {
    let sec = if fb(w, it).triggered { None } else { class_str(w, it, "CompleteMontageSection") };
    section_tail(w, it, sec);
    false
}

/// HitNonPlayer @4112 -> @4131: as ReceiveHit.
fn hit_non_player(w: &mut World, it: ItemId, _destroyed: bool) -> bool {
    receive_hit(w, it, false)
}

/// ReceiveEndPlay @4749.
fn end(w: &mut World, it: ItemId) {
    let last = fb(w, it).hold_last;
    fb_w(w, it).holds.retain(|h| h.id != last); // @7258
    // (not in the Lab reset's pass: an old pawn there)
    if crate::world::flow::reset_pass(w) {
        return;
    }
    let s = spawner(w, it);
    if fb(w, it).picked {
        carry_end(w, s, it);
        if in_section(w, s, MONTAGE, name!("Held")) {
            jump_section_logic(w, s, name!("Cancel"));
        }
    }
    if fb(w, it).end_tail {
        fb_w(w, it).end_tail = false;
        let sec = class_str(w, it, "HitEnemyMontageSection");
        if section_tail(w, it, sec) {
            fb_w(w, it).triggered = true;
        }
    }
}

fn never(_w: &mut World, _f: FighterId) -> bool {
    false
}

/// This file's registry entries (the item's kind, the three notifies, the condition).
///
/// The fastball attack of Reindog's pawn (Comp_FastBallSpecial2_C, his component; Creature_Attack_Air_FastBall /
/// FastBallStartupIntro_Montage) has no route in this build: BranchGroup_RexCreatureFastball, the one group that
/// names Creature_Attack_Air_FastBall_Montage, is reached from no montage, no start table and no other group, and
/// nothing starts Creature_Attack_Air_FastBallStartupIntro_Montage. Their notifies and the group's condition are
/// entered and do nothing. Arya_Attack_Special_Up_Montage names ClearFastBall (0.0001 s), FastBallReadyCondition
/// (its FastBallChecking section's jump) and FastBallMoveCommand too; her pawn has no Comp_FastBallSpecial2, where
/// both reads are exact: the Check's GetComponentByClass is not valid @123 -> false @266, ClearFastBall's End skips
/// @122 -> @281 (Begin returns false), so her FastBall section (FastBallMoveCommand) is not reached. On Reindog's
/// pawn the Check is the component's GetFastBallDirection. FastBallReady @188 and ClearFastBall's End calls its
/// ClearFastBall @229: nothing sets FastBallReady while his move is unreachable. AnimNotify_FastBallMoveCommand is
/// an AnimNotify_MoveCommand child (Distance 2250) and AnimNotify_RecieveFastBallVelocity's Begin launches by the
/// component's direction: neither is reached. Comp_FastBallSpecial2_C itself (ReadPlayerInput false, its Listen*
/// events bound by nothing reachable) is not ported: the port has no registry of pawn components.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        make: Some(make),
        sub: Some(sub),
        begin: Some(begin),
        step: Some(step),
        hit_enemy: Some(hit_enemy),
        receive_hit: Some(receive_hit),
        hit_non_player: Some(hit_non_player),
        end: Some(end),
        ..ItemHooks::default()
    };
    r.actors.enter(
        ItemClass::Fastball.name(),
        ActorEntry::new(ActorClass::Item(ItemClass::Fastball), ActorHooks::Item(hooks)),
    )?;
    for n in ["AnimNotify_ClearFastBall_C", "AnimNotify_FastBallMoveCommand_C", "AnimNotify_RecieveFastBallVelocity_C"]
    {
        r.notifies.enter(n, NotifyHandler::default())?;
    }
    r.conditions.enter("FastBallReadyCondition_C", Condition::Fn(never))?;
    Ok(())
}
