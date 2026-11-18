//! Shaggy : his state, his hooks, the sandwich projectiles, the raged shockwave, the
//! snack box and his perk buffs.
//!
//! What `mod.rs` enters and where it goes here: the `character('shaggy')` hooks (`state`, `holdingItem`, `reset`,
//! `spawner`), the conditions `ShaggyShirtNotEquippedCondition_C`, `RockEquippedCondition_C`,
//! `IsExtraHungryPerk_C`, the actors `ShaggyShockwaveActor_C` ([`shockwave`]), `ScoobySnackBox_C`
//! ([`snack_box`]), `ShaggySandwichProjectile_C`, `ShaggyRageSandwichProjectile_C`, `Shaggy_AllyTeleportBurst_C`,
//! the buffs `Buff_ShaggyRageSandwichSize_C`, `Buff_ScoobySnack_C` and `perks.rs`'s `Buff_Shaggy_ExtraHungry_C`,
//! `Buff_Shaggy_DyingRage_C` ([`perks`]), and the bot injections.

pub mod perks;
pub mod shockwave;
pub mod snack_box;
pub mod state;

pub use shockwave::Shockwave;
pub use snack_box::SnackBox;
pub use state::ShaggyState;

use crate::actors::{ActorClass, ProjectileClass, ProjectileId, WorldActorClass};
use crate::chars::CharState;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, spawn_buff};
use crate::combat::defense::end_rage;
use crate::combat::hit::hurt_hit;
use crate::combat::{BuffClass, BuffExt, BuffRef};
use crate::events::{Event, NotifyRef, SoundKey};
use crate::fighter::anim::{notify_data, sample_boxes};
use crate::fighter::match_side::TutorialEvent;
use crate::fighter::{FighterId, unequip_object};
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks, ProjectileHooks};
use crate::registry::props::{buff, buff_mut, text};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{ActorEntry, ActorHooks, BuffEntry, CharHooks, Condition, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::tutorial_event;
use state::{rock, set_rock, shaggy_mut};

/// The roster key.
pub const KEY: &str = "shaggy";

/// ShaggyRageSandwichProjectile's own Comp_Pawn_PassiveBuffManager row (data alone: SizeMods MultSizeMod 1.5).
pub const RAGE_SANDWICH_SIZE: &str = "Buff_ShaggyRageSandwichSize_C";

/// `state: (f) => new ShaggyState(f)`. The constructor's `this.rock = f.ch.rock` writes the fighter's equipped list,
/// which the `init` hook right behind it does ([`init`]).
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Shaggy(ShaggyState::default())
}

/// The `ShaggyState` constructor's `this.rock = f.ch.rock` (the fighter's `init` runs right after its `state`).
fn init(w: &mut World, f: FighterId) {
    let key = w.names.str(w.fighter(f).ch);
    let on = w.data.chars.get(key).is_some_and(|c| c.def.rock);
    set_rock(w, f, on);
}

/// Shaggy_AnimBP's BPI_HoldingItem: SandwichEquipped (`ShaggyState.hold_pose` has the callers).
fn holding_item(w: &mut World, f: FighterId, on: bool) {
    if let Some(s) = shaggy_mut(w, f) {
        s.hold_pose = on;
    }
}

/// BPI_Cleanup (Pawn_Shaggy ubergraph @2915, from BaseFighter's ResetFighterLogic): Comp_Spawner.UnequipObject('RockMesh')
/// @2991: the list loses it and the AnimBP gets BPI_HoldingItem(false). A respawned Shaggy has no sandwich: the
/// passive buff only equips one at match start (`World.reset`).
fn reset(w: &mut World, f: FighterId) {
    unequip_object(w, f, name!("RockMesh"));
}

/// The thrown sandwich leaves the hand (ShaggySandwichProjectile_C, socket R_Hand / fml_un_R_wrist).
fn spawner(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if text(p, "ActorClass").unwrap_or("").contains("SandwichProjectile") {
        set_rock(w, f, false);
        holding_item(w, f, false);
    }
}

/// Check: not BPI_GetShaggyShirtMesh.IsVisible() (the Shirt sub-mesh; Shaggy_Taunt_ShirtRip's Equip notifies hide it).
fn shirt_not_equipped(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).pose.sub_hidden.contains(&name!("shirt"))
}

fn rock_equipped(w: &mut World, f: FighterId) -> bool {
    rock(w, f)
}

/// Check (IsExtraHungryPerk @0-402): the actor's BPI_CheckIfHasBuff(Buff_Shaggy_ExtraHungry_C)'s hasBuff.
fn extra_hungry(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_Shaggy_ExtraHungry_C")).is_some()
}

fn sound(fighter: i32, ev: crate::names::Name, at: Option<(crate::Real, crate::Real)>) -> Event {
    Event::Sound { fighter, ev, x: at.map(|a| a.0), y: at.map(|a| a.1), hit: None, pooled: false, key: None }
}

/// The owner's id of a projectile.
fn proj_owner(w: &World, pr: ProjectileId) -> Option<(FighterId, i32)> {
    let p = w.actors.projectiles.get(pr)?;
    Some((p.owner?, p.owner_id))
}

/// The sandwich loop's key, `'proj' + pr.id`.
fn loop_key(w: &World, pr: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("proj"), n: w.actors.projectiles.get(pr).map_or(-1, |p| p.seq) }
}

// ShaggySandwichProjectile's own sounds. ReceiveBeginPlay (ubergraph 122, after the parent's):
// PF_Post_Event(sfx_cha_shaggy_sandwich_loop, self) @281; the loop ends with the actor. HitPlayer (ubergraph 1428):
// PF_Post_Event_At_Location(sfx_cha_shaggy_sandwich_hit, Capsule.K2_GetComponentLocation()) @1590.
// Deviation: the loop is keyed to the owner's game object (the port's projectile loops: 'proj' + id), so its 3D
// position is the thrower's, not the sandwich's; it is stopped by its key with the port's default fade (the native
// stop of a destroyed actor's sounds is not read, nor whether the class is pooled); cha_locally_controlled
// (PF_Post_Event @357, by the Instigator) is not set for it.
fn sandwich_begin(w: &mut World, pr: ProjectileId) {
    tutorial_event(w, TutorialEvent::UniqueCharacterMechanic2); // ReceiveBeginPlay @242
    let Some((_, owner)) = proj_owner(w, pr) else { return };
    let ev = Event::SoundStart { fighter: owner, ev: name!("sfx_cha_shaggy_sandwich_loop"), key: loop_key(w, pr) };
    w.events.push(ev); // @281
}

/// ShaggySandwichProjectile HandleFighterOverlap (ubergraph 2506-3323): a fighter on the Instigator's team that is
/// not the Instigator and not in HealedAllies is added to it and gets SpawnBuffActor(Buff_HealOverTime, the ally, the
/// Instigator, stacks 1, duration 1.5); the returned actor's HealingOnInterval is then set to 2.0.
// Unverified: the ally's shapes that begin the overlap are taken as its hurtboxes, as for the enemy hit, and an ally
// without them (noHit) or waiting for its respawn is left out; the box is on while HitBoxOn? is.
fn sandwich_heal(w: &mut World, pr: ProjectileId, vic: FighterId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let Some(owner) = p.owner else { return };
    let hit_collision = p.hit_on && !p.dead && !p.no_collision;
    let sphere =
        WorldSphere { x: p.x, y: p.y, z: p.sphere_3d.then_some(0.0), r: p.hit_radius, ..WorldSphere::default() };
    let vf = w.fighter(vic);
    if !hit_collision
        || vic == owner
        || vf.team.team != w.fighter(owner).team.team
        || vf.life.dead != 0.0
        || crate::fighter::no_hit(w, vic)
        || p.healed_allies.contains(&vf.id)
    {
        return;
    }
    let vid = vf.id;
    let mut vp = Vec::new();
    if !sample_boxes(w, vic, &mut vp) {
        return;
    }
    if hurt_hit(w, vic, &[sphere], &vp).is_none() {
        return;
    }
    if let Some(p) = w.actors.projectiles.get_mut(pr) {
        p.healed_allies.insert(vid);
    }
    let cls = name!("Buff_HealOverTime_C");
    if let Some(b) = spawn_buff(w, vic, cls, 1.5, Some(1), Some(owner), true, None)
        && let Some(e) = buff_mut(w, b)
    {
        e.heal_amt = Some(2.0);
    }
}

fn sandwich_hit_enemy(w: &mut World, pr: ProjectileId, _vic: FighterId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (owner, at) = (p.owner_id, (p.x, p.y));
    w.events.push(sound(owner, name!("sfx_cha_shaggy_sandwich_hit"), Some(at))); // @1590
}

fn sandwich_end(w: &mut World, pr: ProjectileId) {
    let Some((_, owner)) = proj_owner(w, pr) else { return };
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStop { fighter: owner, key, fade: None, ev: None, curve: None, cut: false });
}

fn sandwich() -> ProjectileHooks {
    ProjectileHooks {
        begin: Some(sandwich_begin),
        overlap: Some(sandwich_heal),
        hit_enemy: Some(sandwich_hit_enemy),
        end: Some(sandwich_end),
        ..ProjectileHooks::default()
    }
}

/// Shaggy_AllyTeleportBurst ReceiveBeginPlay (authority): the owner's Comp_Pawn_Rage.EndRageMulticast, then
/// BPI_ProcessTutorialEvent(UniqueCharacterMechanic1) @1129 (not yet Activated).
fn burst_begin(w: &mut World, pr: ProjectileId) {
    if let Some((owner, _)) = proj_owner(w, pr) {
        end_rage(w, owner);
    }
    tutorial_event(w, TutorialEvent::UniqueCharacterMechanic1);
}

/// The burst's BlastVisuals (PS_ShaggyRageSPN_Explosion) and FX_Blast (sfx_cha_shaggy_teleport_blast at the actor;
/// the GrowthScale branch picks the same event either way).
fn burst_activate(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (owner, at) = (p.owner_id, (p.x, p.y));
    w.events.push(sound(owner, name!("sfx_cha_shaggy_teleport_blast"), Some(at)));
}

// Buff_ScoobySnack. BPI After Status Update Multicast (ubergraph 1304): with BuffApplied @88, SpawnVFX @98 and
// PF_Post_Event(sfx_cha_shaggy_rage_ally_buff, GetOwner()) @132, then the pop text @733. BPI On Fighter Owner Hit Enemy
// Player @980-1153: the HitPlayer's SpawnBuffActor(Buff_Weakened_C, HitPlayer, GetInstigator(), CurrentStackAmount,
// 0.0, BreakIfBlocked true) @1051, RemoveVFX @1139, K2_DestroyActor @1153.
fn snack_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<crate::Real>) {
    if let Some(e) = buff_mut(w, b) {
        e.ext = BuffExt::ScoobySnack { by: from }; // GetInstigator()
    }
}

fn snack_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let (BuffSignal::HitEnemy, Some(victim)) = (s, a.victim) else { return false };
    let Some(e) = buff(w, b) else { return false };
    let by = match e.ext {
        BuffExt::ScoobySnack { by } => by,
        _ => None,
    };
    let stacks = crate::math::to_int32(e.stacks);
    spawn_buff(w, victim, name!("Buff_Weakened_C"), 0.0, Some(stacks), by, true, Some(victim));
    true
}

fn snack_status(w: &mut World, f: FighterId, _cls: BuffClass, _stacks: crate::Real, applied: bool, _removed: bool) {
    if applied {
        let id = w.fighter(f).id;
        // (pushed to the fighter's own `events`, which the frame merges in its own place)
        w.fighter_mut(f).events.push(sound(id, name!("sfx_cha_shaggy_rage_ally_buff"), None)); // @132
    }
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.chars.enter(
        KEY,
        CharHooks {
            state: Some(new_state),
            init: Some(init),
            holding_item: Some(holding_item),
            reset: Some(reset),
            spawner: Some(spawner),
            ..CharHooks::default()
        },
    )?;
    r.conditions.enter("ShaggyShirtNotEquippedCondition_C", Condition::Fn(shirt_not_equipped))?;
    r.conditions.enter("RockEquippedCondition_C", Condition::Fn(rock_equipped))?;
    r.conditions.enter("IsExtraHungryPerk_C", Condition::Fn(extra_hungry))?;
    r.actors.enter("ShaggyShockwaveActor_C", shockwave::entry(WorldActorClass::ShaggyShockwaveActor))?;
    r.actors.enter(snack_box::SNACK_BOX, snack_box::entry())?;
    // foreignOwner (Taz's burp: a sandwich he ate is spawned again with him as its owner): the class's code reads of
    // the owner only what any fighter answers. ShaggyRageSandwichProjectile overrides nothing but its passive buff row.
    for (cls, class) in [
        ("ShaggySandwichProjectile_C", ProjectileClass::ShaggySandwichProjectile),
        ("ShaggyRageSandwichProjectile_C", ProjectileClass::ShaggyRageSandwichProjectile),
    ] {
        let mut e = ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(sandwich()));
        e.foreign_owner = true;
        r.actors.enter(cls, e)?;
    }
    let burst =
        ProjectileHooks { begin: Some(burst_begin), activate: Some(burst_activate), ..ProjectileHooks::default() };
    r.actors.enter(
        "Shaggy_AllyTeleportBurst_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::ShaggyAllyTeleportBurst),
            ActorHooks::Projectile(burst),
        ),
    )?;
    let mut size = BuffEntry::new(BuffClass::BuffShaggyRageSandwichSize, BuffHooks::default());
    size.actor = Some(ActorBuffHooks::default());
    r.buffs.enter(RAGE_SANDWICH_SIZE, size)?;
    let snack =
        BuffHooks { begin: Some(snack_begin), on: Some(snack_on), status: Some(snack_status), ..BuffHooks::default() };
    r.buffs.enter("Buff_ScoobySnack_C", BuffEntry::new(BuffClass::BuffScoobySnack, snack))?;
    perks::register(r)?;
    Ok(())
}
