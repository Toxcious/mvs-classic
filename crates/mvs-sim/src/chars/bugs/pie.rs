//! Bugs Bunny's pie: PieProjectile and the Lingering Kiss conditions
//! (BugsHasLingeringKiss, BugsNoLingeringKiss). The pie's equip and holster are data (AnimNotify_Equip / the throw's
//! Unequip, PieEquippedCondition in `combat::conditions`).
//!
//! PieProjectile: a ParentProjectilePhysics child. What the parent and the data give with no code here: InitialSpeed /
//! MaxSpeed 3250 (the template's; the throw's notify may write its own), gravity 0.2, AimDownProjectileMovement,
//! MaxLifetime 8, DestroyOnExitedArena?, the HitBox (sphere r100, tag HitBox) on from the spawn, the packet 4 / 1200 /
//! 16.5 / (0.5, 0.5) with HitOnlyOneFighter, DestroyOnHitFighter (its HitEnemyPlayer @779 calls the parent's first,
//! which destroys: ParentProjectilePhysics @229), DestroyOnHitProjectile, CanBeReflected, the ability slot 3
//! (TriggerAbilityIndex), 'Ak Death Location' sfx_cha_bugs_piethrow_hit and DeathVFX PS_BugsBunnyPie_Impact.

use crate::actors::{ActorClass, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, spawn_buff};
use crate::combat::hit::hurt_hit;
use crate::events::{Event, SoundKey};
use crate::fighter::anim::sample_boxes;
use crate::fighter::match_side::post_vo;
use crate::fighter::{FighterId, no_hit};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::buff_mut;
use crate::registry::{Condition, Registry, RegistryError};
use crate::world::World;

/// The Lingering Love perk's buff.
const LINGERING: crate::names::Name = name!("Buff_BugsBunny_LingeringLove_C");

/// The pie loop's key, `'proj' + pr.id`.
fn loop_key(w: &World, pr: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("proj"), n: w.actors.projectiles.get(pr).map_or(-1, |p| p.seq) }
}

/// The heal: HitBox BeginOverlap (@1299-1694) with authority, OtherActor a BP_Fighter_Character that is not the Owner
/// and of the Owner's Comp_Actor_Destructible TeamIndex, then a DoOnce (@15-50: Has_Been_Initd / IsClosed, closed
/// first): the Owner's Comp_State_Manager.SpawnBuffActor(Buff_HealOverTime_C, the ally, the Owner, stacks 1, duration
/// 1.5, true, false) @132, and the cast's HealingOnInterval = 2.0 @291 (the BeginPlay heal is the class default's, as
/// for Shaggy's sandwich). The gate opens again only at OnReleasedToPool -> ResetHeal (@1699 -> @374 -> @419); the
/// class is not pooled (no CanBePooled), so one heal a pie, for the first team mate it meets. The pie flies on.
// Unverified (as the sandwich's): the ally's shapes that begin the overlap are taken as its hurtboxes; an ally
// without them (noHit) or waiting for its respawn is left out.
// The once-gate is the actor's HealedAllies set (state the snapshot covers): closed once it holds anyone.
fn pie_heal(w: &mut World, pr: ProjectileId, vic: FighterId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let Some(owner) = p.owner else { return };
    let hit_collision = p.hit_on && !p.dead && !p.no_collision;
    let sphere =
        WorldSphere { x: p.x, y: p.y, z: p.sphere_3d.then_some(0.0), r: p.hit_radius, ..WorldSphere::default() };
    let vf = w.fighter(vic);
    if !p.healed_allies.is_empty()
        || !hit_collision
        || vic == owner
        || vf.team.team != w.fighter(owner).team.team
        || vf.life.dead != 0.0
        || no_hit(w, vic)
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
    if let Some(b) = spawn_buff(w, vic, name!("Buff_HealOverTime_C"), 1.5, Some(1), Some(owner), true, None)
        && let Some(e) = buff_mut(w, b)
    {
        e.heal_amt = Some(2.0);
    }
}

/// ReceiveActorBeginOverlap (@435): the parent's, then PF_Post_Event_On_AkComponent(sfx_cha_bugs_piethrow_loop) @454,
/// at every begin overlap of the actor.
// Deviation: the loop is started once, at the spawn: the actor's first begin overlap is taken as the one it has at its
// spawn place (the thrower's own bodies around his hand), and a later begin overlap's post (a new instance of the loop
// on the same AkComponent) is not made. Keyed per projectile; stopped at its end with the port's default fade (the
// native stop of a destroyed actor's AkComponent is not read, as the sandwich's).
// The tick's PF_Velocity_Ak and the cha_bugs_pie_velocity RTPC (@519-700) are not ported.
fn pie_begin(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let fighter = p.owner_id;
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_bugs_piethrow_loop"), key });
}

/// HitEnemyPlayer @779: the parent's (DestroyOnHitFighter: the world's), Pie Hit Player = the victim, then Pie Hit @841:
/// PF_Post_VO_Event(Instigator, ability_generic, Pie Hit Player, 0.3).
fn pie_hit_enemy(w: &mut World, pr: ProjectileId, vic: FighterId) {
    let Some(owner) = w.actors.projectiles.get(pr).and_then(|p| p.owner) else { return };
    post_vo(w, owner, name!("ability_generic"), Some(vic), 0.3);
}

fn pie_end(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let fighter = p.owner_id;
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
}

// ---- Lingering Kiss
// BugsHasLingeringKiss Check @0-269: the actor's BPI_CheckIfHasBuff(Buff_BugsBunny_LingeringLove_C)'s hasBuff; without
// the fighter interface false (@293). BugsNoLingeringKiss Check @0-58: Not of its parent's Check. The buff is the
// Lingering Love perk's (Perks/Buffs/BugsBunny/LingeringLove; the perk's buff applied at the match's start, nothing
// else gives it): with the perk BugsBunny_Attack_BlowLingeringKiss_Montage is reached, and its Spawner makes
// LingeringLoveProjectile_C (perks.rs).
// Unverified: IsBuffOfClass is not read (as HasEducatedBuff's, `combat::conditions`): the exact class is tested.
fn has_kiss(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, LINGERING).is_some()
}

fn no_kiss(w: &mut World, f: FighterId) -> bool {
    !has_kiss(w, f)
}

/// This file's registry entries (PieProjectile_C and the two conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let pie = ProjectileHooks {
        begin: Some(pie_begin),
        overlap: Some(pie_heal),
        hit_enemy: Some(pie_hit_enemy),
        end: Some(pie_end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "PieProjectile_C",
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::PieProjectile), ActorHooks::Projectile(pie)),
    )?;
    r.conditions.enter("BugsHasLingeringKiss_C", Condition::Fn(has_kiss))?;
    r.conditions.enter("BugsNoLingeringKiss_C", Condition::Fn(no_kiss))
}
