//! The World's hits among spawned actors, items and placed actors:
//! pfgDoDamage for a spawned HitActor (Comp_Pawn_HitBoxHandler), the overlaps a projectile's or a thrown item's
//! hit box begins with another projectile, an item, a World.actors pawn, a placed stage actor and a mortar aim.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::ItemId;
use crate::actors::ProjectileId;
use crate::actors::item as itm;
use crate::actors::projectile as prj;
use crate::combat::HitPacket;
use crate::combat::OnHitBuffRow;
use crate::combat::boxes::{ExtraHurt, WorldSphere, obj, shape_over_box, shape_over_capsule};
use crate::combat::elem;
use crate::combat::hit as fh;
use crate::combat::hits;
use crate::combat::phases;
use crate::combat::reflect::{can_be_reflected, reflect_projectile};
use crate::consts as C;
use crate::events::{Event, SoundHit};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, owner_team, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::traces::{body_info, response_to};
use crate::names::Name;
use crate::registry::ActorHooks;
use crate::registry::entries::{ProjectileHooks, WorldActorHooks};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::world::World;
use crate::world::items;
use crate::world::spawning;
use crate::world::stage_actors as sa;
use indexmap::IndexSet;
use mvs_data::fighter::ActorDef;
use mvs_data::value::{Props, Value};

// ---- the spawned actor's reads, by kind

/// The actor is gone or dead (`a.dead`).
pub(crate) fn dead(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_none_or(|p| p.dead),
        ActorRef::Item(id) => w.actors.items.get(id).is_none_or(|i| i.dead),
        ActorRef::Actor(id) => w.actors.actors.get(id).is_none_or(|x| x.dead),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).is_none_or(|m| m.dead),
        ActorRef::Stage(i) => sa::stage_actor_dead(w, i),
        // (a buff actor as a dealer: gone once its entry is)
        ActorRef::Buff(b) => crate::combat::buffs::buff(w, b).is_none(),
        _ => crate::unported!("spawned hits: a HitActor of this family", w, a),
    }
}

fn cls_of(w: &World, a: ActorRef) -> Name {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).expect("a live projectile").cls,
        ActorRef::Item(id) => w.actors.items.get(id).expect("a live item").cls,
        ActorRef::Actor(id) => w.actors.actors.get(id).expect("a live actor").cls,
        ActorRef::Mortar(id) => w.actors.mortars.get(id).expect("a live mortar").cls,
        // A buff actor's own Comp_Pawn_HitBoxHandler (Buff_Perk_WonderWoman_FastFallSlam's sphere).
        ActorRef::Buff(b) => crate::combat::buffs::buff(w, b).expect("a live buff").cls,
        _ => crate::unported!("spawned hits: the class of a dealer of this family", w, a),
    }
}

/// `a.owner`. A buff actor's: the fighter its class names (`BuffExt::actor_owner`: the FastFallSlam's is the fighter
/// it is on, whose TeamIndex its BeginPlay copies into its own Comp_Actor_Destructible @325).
fn owner(w: &World, a: ActorRef) -> Option<FighterId> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| p.owner),
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|i| i.owner),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|x| x.owner),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map(|m| m.owner),
        ActorRef::Buff(b) => crate::combat::buffs::buff(w, b).and_then(|x| x.ext.actor_owner()),
        _ => crate::unported!("spawned hits: the owner of a dealer of this family", w, a),
    }
}

/// `a.charge`: the actor holds charge values.
fn charged(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.charge.is_some()),
        ActorRef::Item(id) => w.actors.items.get(id).is_some_and(|i| i.charge.is_some()),
        ActorRef::Actor(id) => w.actors.actors.get(id).is_some_and(|x| x.charge.is_some()),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).is_some_and(|m| m.charge.is_some()),
        _ => false,
    }
}

/// The class data (`actorDef(lookupOwner(a), a.cls)`).
fn def(w: &World, a: ActorRef) -> Option<&ActorDef> {
    actor_def(w, a, cls_of(w, a))
}

/// `a.p.<k>` truthy.
fn flag(w: &World, a: ActorRef, k: &str) -> bool {
    def(w, a).is_some_and(|d| hits::truthy(&d.p, k))
}

/// The class's component `k` (`actorDef(...)?.components?.<k>`).
pub(crate) fn comp<'a>(w: &'a World, a: ActorRef, k: &str) -> Option<&'a Props> {
    def(w, a).and_then(|d| hits::obj(&d.components, k))
}

fn hit_radius(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).expect("a live projectile").hit_radius,
        ActorRef::Item(id) => itm::item_hit_radius(w, id),
        _ => crate::unported!("spawned hits: hitRadius of this family", w, a),
    }
}

fn hurt_radius(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).expect("a live projectile").hurt_radius,
        ActorRef::Item(id) => itm::item_hurt_radius(w, id),
        _ => crate::unported!("spawned hits: hurtRadius of this family", w, a),
    }
}

/// BPI_CanProjectileClank: a projectile's, an item's, a World.actors actor's kind hook (`WorldActorHooks::can_clank`,
/// none: false).
fn can_clank(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => prj::can_clank(w, w.actors.projectiles.get(id).expect("a live projectile")),
        ActorRef::Item(id) => itm::item_can_clank(w, id),
        ActorRef::Actor(id) => {
            let cls = w.actors.actors.get(id).expect("a live actor").cls;
            world_hooks(w, cls).and_then(|h| h.can_clank).is_some_and(|f| f(w, id))
        }
        _ => crate::unported!("spawned hits: canClank of this family", w, a),
    }
}

/// `a.attackType` (BPI_GetProjectileAttackType); a World.actors actor's kind hook (`WorldActorHooks::attack_type`,
/// none: Normal).
fn attack_type(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).expect("a live projectile").attack_type,
        ActorRef::Item(id) => itm::item_attack_type(w, id),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).expect("a live mortar").attack_type,
        ActorRef::Actor(id) => {
            let cls = w.actors.actors.get(id).expect("a live actor").cls;
            world_hooks(w, cls).and_then(|h| h.attack_type).map_or(0, |f| f(w, id))
        }
        _ => crate::unported!("spawned hits: attackType of this family", w, a),
    }
}

/// BPI_SetIsProjectileClanking. A World.actors actor keeps no flag: the one that clanks (VelmaHealBeam) reads
/// IsProjectileClanking nowhere in its graph and is never destroyed by a hit.
fn set_clanking(w: &mut World, a: ActorRef, v: bool) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.clanking = v;
            }
        }
        ActorRef::Item(id) => {
            if let Some(i) = w.actors.items.get_mut(id) {
                i.clanking = v;
            }
        }
        ActorRef::Actor(_) => {}
        _ => crate::unported!("spawned hits: clanking of this family", w, a, v),
    }
}

fn queue_destroy(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.queue_destroy),
        ActorRef::Item(id) => w.actors.items.get(id).is_some_and(|i| i.queue_destroy),
        _ => false,
    }
}

/// `a.hitActors`: the handler's TrackActorsHitByTag list (HitEachActorOnlyOnce).
fn hit_actors(w: &mut World, a: ActorRef) -> &mut IndexSet<ActorRef> {
    match a {
        ActorRef::Projectile(id) => &mut w.actors.projectiles.get_mut(id).expect("a live projectile").hit_actors,
        ActorRef::Item(id) => &mut w.actors.items.get_mut(id).expect("a live item").hit_actors,
        _ => crate::unported!("spawned hits: hitActors of this family", a),
    }
}

/// HitEachActorOnlyOnce's track: true when `b` was hit already, else it is added.
fn seen_once(w: &mut World, a: ActorRef, b: ActorRef) -> bool {
    !hit_actors(w, a).insert(b)
}

/// `a.hit`: the actor's HitBoxDamage.
fn own_hit(w: &World, a: ActorRef) -> HitPacket {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).expect("a live projectile").hit.clone(),
        ActorRef::Item(id) => w.actors.items.get(id).expect("a live item").hit.clone(),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).expect("a live mortar").hit.clone(),
        ActorRef::Actor(id) => match &w.actors.actors.get(id).expect("a live actor").state {
            crate::actors::WorldActorState::LaserDamage(d) => d.hit.clone(),
            crate::actors::WorldActorState::HarleyBomb(b) => b.hit.clone(),
            crate::actors::WorldActorState::JackBox(j) => j.hit.clone(),
            crate::actors::WorldActorState::CannonBurst(b) => b.hit.clone(),
            crate::actors::WorldActorState::AntenaBolts(s) => {
                crate::chars::iron_giant::actor_state::antena_hit(s).clone()
            }
            crate::actors::WorldActorState::StompWave(s) => crate::chars::iron_giant::actor_state::stomp_hit(s).clone(),
            crate::actors::WorldActorState::DownThruster(s) => {
                crate::chars::iron_giant::actor_state::thruster_hit(s).clone()
            }
            crate::actors::WorldActorState::TurretBomb(s) => {
                crate::chars::iron_giant::turret_actors::bomb_hit(s).clone()
            }
            crate::actors::WorldActorState::Dogpile(s) => crate::chars::taz::dogpile::hit(s).clone(),
            crate::actors::WorldActorState::ShockwaveHitBox(s) => crate::chars::garnet::shockwave::hit(s).clone(),
            crate::actors::WorldActorState::ElectricZone(s) => crate::chars::garnet::zone::hit(s).clone(),
            crate::actors::WorldActorState::AirFlap(s) => crate::chars::rick::air_flap::hit(s).clone(),
            crate::actors::WorldActorState::PolymorphGrenade(s) => crate::chars::rick::grenade::hit(s).clone(),
            crate::actors::WorldActorState::PortalActor(s) => crate::chars::rick::portal::hit(s).clone(),
            crate::actors::WorldActorState::HighFiveShockwave(s) => crate::chars::finn::high_five::hit(s).clone(),
            crate::actors::WorldActorState::GemBoostBlast(s) => crate::chars::finn::perks::hit(s).clone(),
            crate::actors::WorldActorState::FinnShockwave(s) => crate::chars::finn::shockwave::hit(s).clone(),
            crate::actors::WorldActorState::Beam(s) => crate::chars::velma::beam::hit(s).clone(),
            crate::actors::WorldActorState::VelmaCarryRun(s) => crate::chars::velma::carry::hit(s).clone(),
            crate::actors::WorldActorState::Ray(s) => crate::chars::marvin::ray::hit(s).clone(),
            crate::actors::WorldActorState::RocketExplosion(s) => crate::chars::marvin::ufo::rocket_hit(s).clone(),
            crate::actors::WorldActorState::AllySaw(s) => crate::chars::stripe::saw::hit(s).clone(),
            crate::actors::WorldActorState::Stretch(s) => crate::chars::jake::stretch::hit(s).clone(),
            crate::actors::WorldActorState::JakeHouse(s) => crate::chars::jake::house::hit(s).clone(),
            crate::actors::WorldActorState::JakeHorse(s) => crate::chars::jake::horse::hit(s).clone(),
            crate::actors::WorldActorState::SpitHitBox(s) => crate::chars::jake::spit::hit(s).clone(),
            crate::actors::WorldActorState::JakeSpitAllyHitBox(s) => crate::chars::jake::ally_spit::hit(s).clone(),
            crate::actors::WorldActorState::JerryActor(s) => crate::chars::tom_and_jerry::jerry::hit(s).clone(),
            crate::actors::WorldActorState::MouseTrap(s) => crate::chars::tom_and_jerry::trap::hit(s).clone(),
            crate::actors::WorldActorState::BananaGuard(s) => crate::items::banana_guard::hit(s).clone(),
            _ => crate::unported!("spawned hits: the hit of a world actor of this class", w, a),
        },
        _ => crate::unported!("spawned hits: the hit of a dealer of this family", w, a),
    }
}

/// `a.clankHit` (BPI_GetProjectileClankHitBox): a projectile class's own packet, else HitBoxDamage; an item's
/// HitBoxDamage.
fn clank_hit(w: &World, a: ActorRef) -> HitPacket {
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id).expect("a live projectile");
            // JerryRocket's override: its HitBoxDamage variable (`boxDamage`), not the handler's packet.
            if let crate::actors::ProjectileExt::JerryRocket(s) = &p.ext {
                return crate::chars::tom_and_jerry::rocket::clank_hit(s).clone();
            }
            prj::clank_hit_props(w, p).map_or_else(|| p.hit.clone(), |c| HitPacket::from_props(&w.names, c, None))
        }
        _ => own_hit(w, a),
    }
}

/// `a.x`, `a.y` of a projectile, an item, a mortar or a world actor.
fn at_of(w: &World, a: ActorRef) -> V2 {
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id).expect("a live projectile");
            V2 { x: p.x, y: p.y }
        }
        ActorRef::Item(id) => {
            let i = w.actors.items.get(id).expect("a live item");
            V2 { x: i.x, y: i.y }
        }
        ActorRef::Mortar(id) => {
            let m = w.actors.mortars.get(id).expect("a live mortar");
            V2 { x: m.x, y: m.y }
        }
        ActorRef::Actor(_) => {
            let (x, y, _) = crate::actors::world_actor::actor_place(w, a).expect("a world actor has a place");
            V2 { x, y }
        }
        // A buff actor sits on the fighter it is on (`hits::dealer`'s place).
        ActorRef::Buff(_) => {
            let d = hits::dealer(w, a);
            V2 { x: d.x, y: d.y }
        }
        _ => crate::unported!("spawned hits: the place of a dealer of this family", w, a),
    }
}

/// `a.hitNear(x, y)` of a projectile, the item's own place for an item.
fn near(w: &World, a: ActorRef, x: Real, y: Real) -> V2 {
    match a {
        ActorRef::Projectile(id) => prj::hit_near(w, w.actors.projectiles.get(id).expect("a live projectile"), x, y),
        _ => at_of(w, a),
    }
}

/// The hit shape's depth: 0 for a sphere (an item's ThrowHitBox, a projectile's HitBox), none for HitBox2D.
fn sphere_z(w: &World, a: ActorRef) -> Option<Real> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).expect("a live projectile").sphere_3d.then_some(0.0),
        _ => Some(0.0),
    }
}

/// An item's hit box can deal: `it.boxOn && !it.dead && it.holder < 0`.
fn item_deals(w: &World, it: ItemId) -> bool {
    itm::item_box_on(w, it) && w.actors.items.get(it).is_some_and(|i| !i.dead && i.holder < 0)
}

pub(crate) fn projectile_hooks(w: &World, cls: Name) -> Option<&'static ProjectileHooks> {
    match &w.registry.actors.get(w.names.str(cls))?.hooks {
        ActorHooks::Projectile(h) => Some(h),
        _ => None,
    }
}

pub(crate) fn world_hooks(w: &World, cls: Name) -> Option<&'static WorldActorHooks> {
    match &w.registry.actors.get(w.names.str(cls))?.hooks {
        ActorHooks::World(h) => Some(h),
        _ => None,
    }
}

fn hooks_of(w: &World, a: ActorRef) -> Option<&'static ProjectileHooks> {
    match a {
        ActorRef::Projectile(id) => projectile_hooks(w, w.actors.projectiles.get(id)?.cls),
        _ => None,
    }
}

/// The dealer's packet with the dealer's buffs (`dealtPass`, pfgDoDamage @5617-5904: the dealer's own
/// ApplyBuffDamageMods with the HitActor as Victim).
fn dealt_pass(w: &mut World, a: ActorRef, b: ActorRef, hit: &HitPacket) -> HitPacket {
    elem::actor_dealt_damage_mods_hit(w, a, b, hit, true)
}

/// The Damage x the dealer's GetChargeDamageMultiplier (DamageAndLaunch @7835, a victim with no Comp_Armor).
fn charged_damage(w: &World, a: ActorRef, hit: &HitPacket) -> Real {
    let dmg = hit.damage.unwrap_or(0.0);
    if charged(w, a) { dmg * elem::charge_dmg(w, a) } else { dmg }
}

/// A Comp_Actor_Destructible's CanBeHitBy fields.
pub(crate) fn can_be_hit_by_d(d: Option<&Props>) -> Option<sa::CanBeHitByD> {
    d.map(|d| sa::CanBeHitByD {
        damage_disabled: hits::truthy(d, "DamageDisabled"),
        friendly_fire: hits::truthy(d, "FriendlyFire"),
    })
}

/// Comp_HitPause GetHitPauseTime of a spawned actor: its pause's frames left in seconds.
// Unverified: the remaining time is counted from the frames left, not from the timer's start as a fighter's
// (`hit_pause_remaining`).
pub fn actor_hit_pause_remaining(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w
            .actors
            .projectiles
            .get(id)
            .and_then(|p| p.hit_pause)
            .map_or(0.0, |n| crate::real(n) * crate::consts::DELTA_SECONDS),
        _ => 0.0,
    }
}

/// Whether the actor's class has a Comp_HitPause with ReceivesHitPause (false in the component's default).
/// Only the spawned families hold a class here: the four classes with ReceivesHitPause are projectiles, so another
/// family (a stage actor, a fighter) and a dead dealer answer false.
pub fn receives_hit_pause(w: &World, a: ActorRef) -> bool {
    let spawned = matches!(
        a,
        ActorRef::Projectile(_) | ActorRef::Item(_) | ActorRef::Actor(_) | ActorRef::Mortar(_) | ActorRef::Buff(_)
    );
    spawned && !dead(w, a) && comp(w, a, "Comp_HitPause").is_some_and(|c| hits::truthy(c, "ReceivesHitPause"))
}

/// pfgDoDamage's dealer pause (@16860-17442) on a spawned dealer `a`, ahead of its owner's: HitPauseTime > 0,
/// ApplyHitPauseToDamageDealer (true by default), the dealer's Comp_HitPause valid with ReceivesHitPause and its
/// remaining pause below HitPauseTime; then SetHitPauseTime(LocalScalingHitPause `sec`) unless the hit queues its
/// pause until the poses (QueueHitPauseUntilPoses and not InHitPausePose, which nothing sets on a spawned actor: the
/// queue path @23160 gives it none). The pause is the actor's CustomTimeDilation 0.005 (StartPause @870) until the
/// HitPauseTimer, a world timer, ends it (EndPause; OnHitPauseEnded is bound to the component's own
/// HandleQueuedVacuumPause and to the actor's BPI_HitPauseEnd, which runs a DestroySpawnedActor asked in the pause).
// Deviation: the paused actor is held still (its move, its tick, its latent actions, its mesh), not run at 0.005 of
// the frame, as a fighter's pause; the world timers on its age (MaxLifetime, the blast-box check) are held too.
pub fn dealer_hit_pause(w: &mut World, a: ActorRef, hit: &HitPacket, sec: Real) {
    let raw = hit.hit_pause_time.unwrap_or(0.0);
    if raw <= 0.0
        || !hit.apply_hit_pause_to_damage_dealer.unwrap_or(true)
        || hit.queue_hit_pause_until_poses.unwrap_or(false)
        || !receives_hit_pause(w, a)
        || actor_hit_pause_remaining(w, a) >= raw
    {
        return;
    }
    // Comp_HitPause SetHitPauseTime: a pause of 0.01 s at least (@1084 -> SetHitPauseTimeMulticast @4893).
    let s = if sec > 0.0 { sec } else { 0.01 };
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.hit_pause = Some(fh::timer_count(s).max(1));
            }
        }
        _ => crate::fighter::common::warn_text(w, "Comp_HitPause on a spawned actor that is no projectile: no pause"),
    }
}

/// A paused projectile's frame: true while its pause holds it (the frame counted; the pause ends with its last).
pub fn projectile_pause_tick(w: &mut World, id: crate::actors::ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get_mut(id) else { return false };
    let Some(n) = p.hit_pause else { return false };
    p.hit_pause = if n > 1 { Some(n - 1) } else { None };
    true
}

/// The dealer's ApplyHitPauseToOwner pause on its owner (@16603-18248), x `scale`, behind the dealer's own.
fn owner_pause(w: &mut World, a: ActorRef, hit: &HitPacket) {
    let raw = hit.hit_pause_time.unwrap_or(0.0);
    dealer_hit_pause(w, a, hit, raw * C::HIT_PAUSE_MULT);
    if raw <= 0.0 || !flag(w, a, "ApplyHitPauseToOwner") {
        return;
    }
    give_owner_pause(w, a, raw * C::HIT_PAUSE_MULT, raw);
}

/// The dealer's owner's pause of the spawned hit paths (`raw > 0 && a.p.ApplyHitPauseToOwner`: a NaN pause
/// gives none, where [`owner_pause`]'s gives one); `scale`: the victim's HitPauseMultiplier, read only when the
/// pause is given.
fn spawned_owner_pause(w: &mut World, a: ActorRef, hit: &HitPacket, scale: impl FnOnce(&mut World) -> Real) {
    let raw = hit.hit_pause_time.unwrap_or(0.0);
    let owner = raw > 0.0 && flag(w, a, "ApplyHitPauseToOwner");
    if !owner && !(raw > 0.0 && receives_hit_pause(w, a)) {
        return;
    }
    let k = scale(w);
    dealer_hit_pause(w, a, hit, raw * C::HIT_PAUSE_MULT * k);
    if owner {
        give_owner_pause(w, a, raw * C::HIT_PAUSE_MULT * k, raw);
    }
}

/// `a.owner.ownerHitPause(timerCount(sec), sec, raw)`.
fn give_owner_pause(w: &mut World, a: ActorRef, sec: Real, raw: Real) {
    let Some(o) = owner(w, a) else { return };
    fh::owner_hit_pause(w, o, crate::real(fh::timer_count(sec)), sec, raw);
}

// ---- the hit body's type

/// `hitBodyComp`: the body a projectile or an item deals with (HitBox2D or the HitBox sphere; an item's
/// ThrowHitBox) and its template.
fn hit_body_comp(w: &World, a: ActorRef) -> (&'static str, Option<&Props>) {
    let comps = def(w, a).map(|d| &d.components);
    let name = match a {
        ActorRef::Item(_) => "ThrowHitBox",
        _ if sphere_z(w, a).is_none() && comps.is_some_and(|c| c.get("HitBox2D").is_some()) => "HitBox2D",
        _ => "HitBox",
    };
    (name, comps.and_then(|c| hits::obj(c, name)))
}

/// `hitBodyType`: the object type of that body (`bodyInfo`); a class whose hit body has another name or comes from
/// a parent template is warned and taken as HitBox.
fn hit_body_type(w: &World, a: ActorRef) -> i32 {
    let (name, c) = hit_body_comp(w, a);
    match body_info(c, None) {
        Some(b) => b.kind,
        None => {
            let cls = w.names.str(cls_of(w, a));
            warn_text(w, &format!("hit body type unknown: no {name} in {cls}'s data"));
            obj::HIT_BOX
        }
    }
}

/// The object types `hitBoxTypes` asks about.
const ASKED: [i32; 7] = [
    obj::WORLD_DYNAMIC,
    obj::PAWN,
    obj::PHYSICS_BODY,
    obj::DAMAGE_DESTRUCTIBLE,
    obj::PROJECTILE_DESTRUCTIBLE,
    obj::ITEM,
    obj::ITEM_DESTRUCTIBLE,
];

/// `hitBoxTypes`: the gathered object types a hit box's template answers with
/// Overlap or Block (its serialized ResponseArray).
pub fn hit_box_types(comp: Option<&Props>) -> Vec<Real> {
    let Some(c) = comp else { return Vec::new() };
    ASKED.iter().map(|&k| crate::real(k)).filter(|&k| response_to(Some(c), k) != crate::name!("ECR_Ignore")).collect()
}

/// `hit_box_types` with HurtBox when the template answers it: the object types a hit body or a hit box meets on one
/// world actor's trace bodies (`hit_body_over_bodies`, the sass bubbles' Box). The gather of every actor
/// (`hits::hit_box_actor_hits`) keeps `hit_box_types`.
// A HurtBox is asked too (not the fighters' alone, which the dealer's fighter test meets): VelmaEvidence's
// Destructible (ECC_GameTraceChannel17, Overlap to HitBox only) is a world actor's HurtBox body the port gathers.
pub fn actor_body_types(comp: Option<&Props>) -> Vec<Real> {
    let mut t = hit_box_types(comp);
    if comp.is_some() && response_to(comp, crate::real(obj::HURT_BOX)) != crate::name!("ECR_Ignore") {
        t.push(crate::real(obj::HURT_BOX));
    }
    t
}

// ---- pfgDoDamage's gates

/// `spawnedRefuses`: pfgDoDamage @3952-4598 for a spawned HitActor: the dealer is BPI_CanBeReflected and the
/// HitActor's handler IsReflectingProjectiles (an item while its hit box is on, a projectile's class's own entry or
/// its hit while its hit box is on): the call pops.
fn spawned_refuses(w: &World, a: ActorRef, b: ActorRef) -> bool {
    let on = match b {
        ActorRef::Item(it) => itm::item_box_on(w, it) && own_hit(w, b).reflect_projectiles.unwrap_or(false),
        ActorRef::Projectile(id) => match hooks_of(w, b).and_then(|h| h.reflecting_projectiles) {
            Some(own) => own(w, id),
            None => phases::hitbox_active(w, id) && own_hit(w, b).reflect_projectiles.unwrap_or(false),
        },
        _ => crate::unported!("spawned hits: spawnedRefuses on this family", w, a, b),
    };
    on && can_be_reflected(w, a)
}

/// `spawnedCanBeHitBy`: DamageAndLaunch @608-659 for a spawned victim: its Comp_Actor_Destructible.CanBeHitBy(the
/// dealer's team) (`canBeHitBy`).
fn spawned_can_be_hit_by(w: &World, a: ActorRef, b: ActorRef) -> bool {
    sa::can_be_hit_by(owner_team(w, a), owner_team(w, b), can_be_hit_by_d(comp(w, b, "Comp_Actor_Destructible")))
}

/// The packet the call deals is the dealer's own HitBoxDamage (`hit === a.hit`).
// Unverified: the packets are values here, so a packet equal to the dealer's own in
// every field is taken as it. No ported packet a caller passes is a copy of the dealer's with other rows.
// A buff actor's handler keeps no HitBoxDamage of its own: its packet is the caller's.
fn is_own_hit(w: &World, a: ActorRef, hit: &HitPacket) -> bool {
    if matches!(a, ActorRef::Buff(_)) {
        return false;
    }
    crate::state::hash_of(hit) == crate::state::hash_of(&own_hit(w, a))
}

/// OnHitBuffsParam of the call: the caller's `rows` (`DamageCall::rows`), else the handler's own (`victimRows`' rule:
/// the dealer's class rows for its own packet, else the packet's).
fn call_rows(w: &World, a: ActorRef, packet: &HitPacket, rows: Option<&[OnHitBuffRow]>) -> Vec<OnHitBuffRow> {
    match rows {
        Some(r) => r.to_vec(),
        None if is_own_hit(w, a, packet) => class_rows(w, a),
        None => packet.on_hit_buffs.clone(),
    }
}

/// pfgDoDamage's ally rows @5931-6835 on a HitActor that is no fighter, behind the dealt pass (`hit_buffs_ally_actor`
/// with the call's rows). True: a row was given and the dealer or the HitActor is gone after it: the call ends.
// Every route of a HitActor that is no fighter makes them, as the build's one pfgDoDamage does.
// Unverified: a dealer with no Instigator makes none (the gate @6352 reads the Instigator's Destructible).
fn ally_rows(w: &mut World, a: ActorRef, b: ActorRef, packet: &HitPacket, rows: Option<&[OnHitBuffRow]>) -> bool {
    if owner(w, a).is_none() || dead(w, b) || owner_team(w, a) != owner_team(w, b) {
        return false;
    }
    let rows = call_rows(w, a, packet, rows);
    if !rows.iter().any(|e| e.apply_buff_hit && e.apply_buff_ally) {
        return false;
    }
    hit_buffs_ally_actor(w, a, b, &rows);
    dead(w, a) || dead(w, b)
}

/// The dealer implements BPI_SpawnedActor (SpawnedActorParent): a projectile, an item, a World.actors actor.
fn spawned_actor(a: ActorRef) -> bool {
    matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_) | ActorRef::Actor(_))
}

/// BPI_GetProjectileClankHitBox's OnHitBuffs: a projectile class's clank rows, else the class's.
fn clank_rows_of(w: &World, a: ActorRef) -> Vec<OnHitBuffRow> {
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id).expect("a live projectile");
            prj::clank_rows(w, p)
                .iter()
                .filter_map(Value::as_obj)
                .map(|r| OnHitBuffRow::from_props(&w.names, r))
                .collect()
        }
        _ => class_rows(w, a),
    }
}

/// pfgDoDamage @20004-20664 for a call that reached @15084 (IsClankParam or an IsProjectile HitActor) without
/// ReflectProjectiles: both actors BPI_SpawnedActor and BPI_CanProjectileClank of different TeamIndex, or IsClankParam,
/// make the clank: HitPauseTime 0, then Handle Projectile Clank (HitBoxHandler fn @0-2460): unless the call is the
/// reciprocal one, both SetIsProjectileClanking(true) and the HitActor's handler deals its clank hit
/// (BPI_GetProjectileClankHitBox: the packet and its OnHitBuffs) back on the dealer with IsClankParam (@582); then
/// the Damage zeroes by the two attack types. True: the call goes on to DamageAndLaunch @15514 with `hit` changed.
// Every BPI_SpawnedActor dealer clanks an enemy clankable spawned
// HitActor: a projectile or item through `damage_spawned_actor` (the bolts, rockets, car, shockwave, mine, dynamite and
// the other spawned hit boxes), and a World.actors dealer (VelmaHealBeam).
fn clank(w: &mut World, a: ActorRef, b: ActorRef, hit: &mut HitPacket, is_clank: bool) -> bool {
    let both = || {
        spawned_actor(a)
            && spawned_actor(b)
            && can_clank(w, a)
            && can_clank(w, b)
            && owner_team(w, a) != owner_team(w, b)
    };
    if !is_clank && !both() {
        return false;
    }
    hit.hit_pause_time = Some(0.0); // @20555
    if !is_clank {
        set_clanking(w, a, true);
        set_clanking(w, b, true);
        let back = clank_hit(w, b);
        let rows = clank_rows_of(w, b);
        let call = hits::DamageCall { hit_box: false, comp: None, clank: true, rows: Some(&rows) };
        hits::do_damage(w, b, a, &back, &call);
    }
    if zeroed(attack_type(w, a), attack_type(w, b)) {
        hit.damage = Some(0.0);
    }
    true
}

/// The end of a clanking dealer's pfgDoDamage @21484: BPI_SetIsProjectileClanking(false), whose destroy a hit queued
/// meanwhile then runs (as `spawned_hit`'s end).
fn clank_end(w: &mut World, a: ActorRef) {
    set_clanking(w, a, false);
    if queue_destroy(w, a) && !dead(w, a) {
        spawning::destroy_spawned(w, a);
    }
}

/// The class's `On Hit Buffs`.OnHitBuffs rows (`rowsOf`), or an item's own where its graph wrote them.
pub(crate) fn class_rows(w: &World, a: ActorRef) -> Vec<OnHitBuffRow> {
    if let ActorRef::Item(id) = a
        && let Some(rows) = crate::actors::item::item_own_rows(w, id)
    {
        return rows;
    }
    // A projectile's own `p` where a graph wrote its instance variables (setInstanceVars: the bracer clash's
    // BuffDuration), else the class's.
    let p = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|pr| prj::class_p(w, pr)),
        _ => def(w, a).map(|d| &d.p),
    };
    let rows = p.and_then(|p| hits::obj(p, "On Hit Buffs")).and_then(|o| o.get("OnHitBuffs"));
    rows.map_or_else(Vec::new, |r| OnHitBuffRow::rows(&w.names, Some(r)))
}

/// pfgDoDamage's entry (@842-2821, ahead of every gate) for a call of the spawned dealer `a`'s own handler with its
/// HitBoxDamage (the begin of an overlap, @7502: OnHitBuffsParam is the handler's OnHitBuffs, the class's rows): the
/// rows that need no hit, on the Instigator and its team mates (`hit_buffs_call`). False: `a` or `b` is gone after
/// them and the call ends.
/// These rows run on every route (the body, stage and damage-box calls too), not on the projectile pairs alone.
pub(crate) fn entry_rows(w: &mut World, a: ActorRef, b: ActorRef) -> bool {
    let rows = class_rows(w, a);
    if !rows.is_empty()
        && let Some(o) = owner(w, a)
    {
        hits::hit_buffs_call(w, o, &rows);
        return !dead(w, a) && !dead(w, b);
    }
    true
}

/// `victimRows`: the OnHitBuffs rows pfgDoDamage gives the HitActor on a Hit (@18293-19121: `hitActorRows`), behind
/// the hit pause: the call's rows (`call_rows`). None with no owner.
fn victim_rows(w: &mut World, a: ActorRef, b: ActorRef, packet: &HitPacket, rows: Option<&[OnHitBuffRow]>) {
    let Some(o) = owner(w, a) else { return };
    if dead(w, b) {
        return;
    }
    let rows = call_rows(w, a, packet, rows);
    let mult = elem::charge_buff(w, a);
    elem::hit_actor_rows_typed(w, o, b, &rows, mult);
}

// ---- an item on the fighters

/// `itemHits`: a thrown item's hit box against the fighters.
pub fn item_hits(w: &mut World, it: ItemId) {
    let mut nf = 0;
    while let Some(vic) = hits::fighter_at(w, nf) {
        nf += 1;
        // (an item a hit of this loop destroyed reaches no other fighter: Buff_DodgeReflect's hit from attackDodged too)
        let Some(i) = w.actors.items.get(it) else { return };
        if i.dead {
            return;
        }
        let Some(own) = i.owner else { return };
        let (x, y) = (i.x, i.y);
        // (the port's own Dodgeball spares an opponent what a fighter's move made, not an item the stage dropped)
        let no_hit = if i.spawner.is_some() { phases::no_hit_by(w, own, vic) } else { phases::no_hit(w, vic) };
        let v = w.fighter(vic);
        if vic == own || !phases::alive(w, vic) || no_hit || i.hit_set.contains(&v.id) {
            continue;
        }
        let Some(vp) = phases::victim_pose(w, vic) else { continue };
        let s = [WorldSphere { x, y, r: itm::item_hit_radius(w, it), ..WorldSphere::default() }];
        let met = fh::hurt_hit(w, vic, &s, &vp).is_some();
        phases::give_vbuf(w, vp);
        if !met {
            continue;
        }
        let d = hits::dealer(w, ActorRef::Item(it));
        if hits::projectile_blocks(w, &d, vic) {
            continue;
        }
        let vid = w.fighter(vic).id;
        let p = own_hit(w, ActorRef::Item(it));
        if w.fighter(vic).team.team == w.fighter(own).team.team {
            // (an invulnerable ally: pfgDoDamage's dodge branch @7381 pops ahead of NotifyAllyOfHit)
            if hits::invulnerable(w.fighter(vic)) {
                continue;
            }
            w.actors.items.get_mut(it).expect("a live item").hit_set.insert(vid);
            // pfgDoDamage's entry rows (@842-2821), the rows for the hit ally (@5931-6835) and NotifyAllyOfHit
            // @21344: the ally's HitByAlly @584, then the handler's HitAlly @886 (the item's elemental, then
            // ItemActorParent's HitAlly -> ItemThrowHit(true, false) @7501), as `phases::run_projectile_ally`.
            // The build's order and rows.
            let rows = d.buffs(w);
            hits::hit_buffs_call(w, own, &rows);
            let mult = elem::charge_buff(w, ActorRef::Item(it));
            hits::hit_buffs_ally(w, own, vic, &rows, mult);
            hits::item_ally_hit(w, &d, vic, &p);
            continue;
        }
        if hits::invulnerable(w.fighter(vic)) {
            let bkb = p.base_knock_back.unwrap_or(0.0);
            let tick = w.flow.tick;
            attack_dodged(w, ActorRef::Item(it), vic, d.attack_id, bkb, tick, None, false); // pfgDoDamage @7381
            continue;
        }
        dodge_over(w, ActorRef::Item(it), vic);
        w.actors.items.get_mut(it).expect("a live item").hit_set.insert(vid);
        let rows = d.buffs(w);
        let pct = elem::charge_pct(w, ActorRef::Item(it));
        let at = V2 { x: d.x, y: d.y };
        if !hits::apply_projectile_hit(w, &d, vic, &p, &rows, at, None, &[], pct) {
            continue; // Hit false (Light Projectile Armor): no HitEnemyPlayer
        }
        // The buffs the item was granted (bound ahead of the class's part) and those given later, around the
        // class's listeners. An item its own listeners destroy takes its buffs with it: no late signal then.
        // Unverified: the order against ItemActorParent's own spend is not read.
        let arg = ActorBuffArg { by: vic, kx: 0.0, ky: 0.0 };
        if has_elem(w, ActorRef::Item(it)) {
            elem::actor_buff_signal(w, ActorRef::Item(it), ActorBuffSignal::HitEnemyPlayerFirst, Some(&arg));
        }
        let spent = itm::item_spend(w, it, true);
        // (the class's own listeners of HitEnemyPlayer: ItemHooks::hit_enemy)
        let cls = cls_of(w, ActorRef::Item(it));
        let own_hook = w.registry.actors.get(w.names.str(cls)).and_then(|e| match &e.hooks {
            ActorHooks::Item(h) => h.hit_enemy,
            _ => None,
        });
        let gone = own_hook.is_some_and(|h| h(w, it, vic)) || spent;
        if !gone && !dead(w, ActorRef::Item(it)) && has_elem(w, ActorRef::Item(it)) {
            elem::actor_buff_signal(w, ActorRef::Item(it), ActorBuffSignal::HitEnemyPlayer, Some(&arg));
        }
        if gone {
            items::destroy_item(w, it);
        }
        return;
    }
}

fn has_elem(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.elem.is_some()),
        ActorRef::Item(id) => w.actors.items.get(id).is_some_and(|i| i.elem.is_some()),
        ActorRef::Actor(id) => w.actors.actors.get(id).is_some_and(|x| x.elem.is_some()),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).is_some_and(|m| m.elem.is_some()),
        _ => false,
    }
}

// ---- the overlap sets

/// Where an overlap is remembered (a `set`): a projectile's `overlapping`, an item's `overlapping` or
/// `overlappedBy`.
#[derive(Clone, Copy, Debug)]
enum Set {
    Overlapping(ActorRef),
    OverlappedBy(ItemId),
}

fn set_of(w: &mut World, s: Set) -> &mut IndexSet<ActorRef> {
    match s {
        Set::Overlapping(ActorRef::Projectile(id)) => {
            &mut w.actors.projectiles.get_mut(id).expect("a live projectile").overlapping
        }
        Set::Overlapping(ActorRef::Item(id)) => &mut w.actors.items.get_mut(id).expect("a live item").overlapping,
        Set::OverlappedBy(id) => &mut w.actors.items.get_mut(id).expect("a live item").overlapped_by,
        Set::Overlapping(a) => crate::unported!("spawned hits: the overlaps of this family", a),
    }
}

/// The overlap with `key` ended (`set.delete`), when the actor holding the set is still there.
fn set_delete(w: &mut World, s: Set, key: ActorRef) {
    let holder = match s {
        Set::Overlapping(a) => a,
        Set::OverlappedBy(it) => ActorRef::Item(it),
    };
    let there = match holder {
        ActorRef::Projectile(id) => w.actors.projectiles.contains(id),
        ActorRef::Item(id) => w.actors.items.contains(id),
        _ => true,
    };
    if there {
        set_of(w, s).shift_remove(&key);
    }
}

/// The overlap with `key` begins now (`!set.has(key)`, then `set.add(key)`).
fn set_begin(w: &mut World, s: Set, key: ActorRef) -> bool {
    set_of(w, s).insert(key)
}

// ---- an item among the other spawned actors

/// `itemSpawnedHits`: a thrown item among the other spawned actors: a
/// projectile's hitbox or another item's ThrowHitBox beginning to overlap its Sphere, and its own ThrowHitBox
/// beginning to overlap a projectile's HurtBox, another item's Sphere, a mortar's Hurtbox, a World.actors pawn and
/// a placed stage actor.
// Deviation: tested once a step after the items have moved, spheres as circles in the play plane (the order against
// the actors' own ticks is not read); the projectiles and items are taken in list order.
// The lists are walked by place (`nth`), read anew each turn: a destroyed actor stays in its list (dead) until the
// step's sweep, and one spawned inside the loop is met, as a `for of` over a growing array.
pub fn item_spawned_hits(w: &mut World) {
    let mut n = 0;
    while let Some(it) = w.actors.items.nth(n) {
        n += 1;
        // A stage item no one has picked up: idle since its spawn, its Sphere and ThrowHitBox off, so it has no overlap
        // to begin or end here.
        if !itm::is_owned(w, it) {
            continue;
        }
        let me = ActorRef::Item(it);
        let mut k = 0;
        while let Some(a) = w.actors.projectiles.nth(k) {
            k += 1;
            let on = phases::hitbox_active(w, a);
            spawned_overlap(w, ActorRef::Projectile(a), me, Set::OverlappedBy(it), ActorRef::Projectile(a), on);
        }
        let mut k = 0;
        while let Some(b) = w.actors.projectiles.nth(k) {
            k += 1;
            let on = item_deals(w, it);
            spawned_overlap(w, me, ActorRef::Projectile(b), Set::Overlapping(me), ActorRef::Projectile(b), on);
        }
        let mut k = 0;
        while let Some(b) = w.actors.items.nth(k) {
            k += 1;
            if b != it && itm::is_owned(w, b) {
                let on = item_deals(w, it);
                spawned_overlap(w, me, ActorRef::Item(b), Set::Overlapping(me), ActorRef::Item(b), on);
            }
        }
        let on = item_deals(w, it);
        crate::chars::morty::mortar_hurt(w, me, on);
        let mut k = 0;
        while let Some(b) = w.actors.actors.nth(k) {
            k += 1;
            let on = item_deals(w, it);
            actor_body_overlap(w, me, b, Set::Overlapping(me), on);
        }
        for i in 0..stage_actor_count(w) {
            let on = item_deals(w, it);
            stage_body_overlap(w, me, i, Set::Overlapping(me), on);
        }
    }
}

pub(crate) fn stage_actor_count(w: &World) -> u32 {
    w.stage_run.as_ref().map_or(0, |r| u32::try_from(r.actors.len()).expect("a stage of fewer than 2^32 actors"))
}

/// `spawnedOverlap`: `a`'s hit box against `b`'s HurtBox (or Sphere, or its damage box); on: a's hitbox is on.
fn spawned_overlap(w: &mut World, a: ActorRef, b: ActorRef, set: Set, key: ActorRef, on: bool) {
    if let ActorRef::Projectile(bp) = b
        && let Some(bx) = hooks_of(w, b).and_then(|h| h.damage_box)
    {
        spawned_damage_box_overlap(w, a, bp, bx, set, on);
        return;
    }
    let hurt = if dead(w, b) { 0.0 } else { hurt_radius(w, b) };
    let r = if on && hurt > 0.0 { hit_radius(w, a) + hurt } else { 0.0 };
    // the victim's hurt centre read only for a projectile dealer's hitNear
    let n = match (a, b) {
        (ActorRef::Projectile(_), ActorRef::Projectile(id)) => {
            let p = w.actors.projectiles.get(id).expect("a live projectile");
            let (bx, by) = (prj::hurt_x(p), prj::hurt_y(p));
            near(w, a, bx, by)
        }
        (ActorRef::Projectile(_), _) => {
            let v = at_of(w, b);
            near(w, a, v.x, v.y)
        }
        _ => at_of(w, a),
    };
    let (ax, ay) = (n.x, n.y);
    let bat = at_of(w, b);
    let (dx, dy) = (ax - bat.x, ay - bat.y);
    let d2 = |w: &World| match b {
        ActorRef::Projectile(id) => prj::hurt_dist2(w.actors.projectiles.get(id).expect("a live projectile"), ax, ay),
        _ => dx * dx + dy * dy,
    };
    if r <= 0.0 || d2(w) > r * r {
        set_delete(w, set, key);
        return;
    }
    if !set_begin(w, set, key) {
        return;
    }
    if let ActorRef::Projectile(ap) = a
        && let Some(begin) = hooks_of(w, a).and_then(|h| h.hitbox_begin_overlap)
    {
        begin(w, ap, b);
        if dead(w, a) || dead(w, b) {
            return;
        }
    }
    if !entry_rows(w, a, b) {
        return;
    }
    let hit = own_hit(w, a);
    hits::do_damage(w, a, b, &hit, &hits::DamageCall { hit_box: true, comp: None, clank: false, rows: None });
}

/// `spawnedHit`: pfgDoDamage of a spawned actor's hitbox handler on another
/// spawned actor when one of the two is an item: the rules of `projectileHitsProjectile` (clank, DestroyProjectiles,
/// DamageAndLaunch on another team, the dealer's HitNonPlayer). Returns Hit (CanBeHitBy passed on the
/// DamageAndLaunch path). `packet`: `None` is the default (`clankHit` for a clank, else `hit`).
/// `rows`: OnHitBuffsParam (`DamageCall::rows`).
pub fn spawned_hit(
    w: &mut World,
    a: ActorRef,
    b: ActorRef,
    is_clank: bool,
    packet: &HitPacket,
    rows: Option<&[OnHitBuffRow]>,
) -> bool {
    spawned_hit_with(w, a, b, is_clank, Some(packet), rows)
}

fn spawned_hit_with(
    w: &mut World,
    a: ActorRef,
    b: ActorRef,
    is_clank: bool,
    packet: Option<&HitPacket>,
    rows: Option<&[OnHitBuffRow]>,
) -> bool {
    if spawned_refuses(w, a, b) {
        return false;
    }
    let packet = match packet {
        Some(p) => p.clone(),
        None if is_clank => clank_hit(w, a),
        None => own_hit(w, a),
    };
    if packet.hit_each_actor_only_once.unwrap_or(false) && seen_once(w, a, b) {
        return false;
    }
    let hit = dealt_pass(w, a, b, &packet);
    if ally_rows(w, a, b, &packet, rows) {
        return false;
    }
    let enemy = owner_team(w, a) != owner_team(w, b);
    let mut dmg = hit.damage.unwrap_or(0.0);
    // @15158 (spawnedReflects): with ReflectProjectiles the clank test (@20004) is not reached.
    let reflects = hit.reflect_projectiles.unwrap_or(false) && (is_clank || victim_is_projectile(w, b));
    if !is_clank && matches!(b, ActorRef::Item(it) if !itm::item_is_projectile(w, it)) {
        // @15084 -> 15514: DamageAndLaunch at once
    } else if reflects && spawned_reflects(w, a, &hit, b) {
        return false;
    } else if !reflects && ((can_clank(w, a) && can_clank(w, b) && enemy) || is_clank) {
        // An item no fighter holds or threw clanks one way: the dealer takes the clank, the item no Handle Projectile
        // Clank of its own.
        // Unverified: the build's path for an ownerless clank victim was not traced.
        let owned = match b {
            ActorRef::Projectile(_) => true,
            ActorRef::Item(it) => itm::is_owned(w, it),
            _ => false,
        };
        if !is_clank && owned {
            set_clanking(w, a, true);
            set_clanking(w, b, true);
            let back = clank_rows_of(w, b);
            spawned_hit_with(w, b, a, true, None, Some(&back));
        }
        if zeroed(attack_type(w, a), attack_type(w, b)) {
            dmg = 0.0;
        }
    } else if !hit.destroy_projectiles.unwrap_or(false) {
        if let ActorRef::Projectile(bp) = b {
            let d = hits::dealer(w, a);
            let hd = hits::projectile_hit_dir(w, &d, &hit, Some(victim_x(w, b)));
            let at = attack_type(w, a);
            hits::hit_by(w, bp, !enemy, hd.x, hd.y, hd.bkb, at, Some(a));
        }
        return false;
    }
    if charged(w, a) {
        dmg *= elem::charge_dmg(w, a); // DamageAndLaunch @7835 (no Comp_Armor)
    }
    let can = spawned_can_be_hit_by(w, a, b); // DamageAndLaunch @659
    if can {
        if !dead(w, b) {
            let gone = match b {
                ActorRef::Item(it) => itm::item_receive_damage(w, it, dmg, false),
                ActorRef::Projectile(bp) => {
                    let at = attack_type(w, a);
                    projectile_any_damage(w, bp, dmg, at, a)
                }
                _ => false,
            };
            if gone {
                items::item_throw_hits(w); // an item's ItemThrowHit inside its ReceiveAnyDamage, ahead of its destroy
                spawning::destroy_spawned(w, b);
            }
            // OnTakeAnyDamage after ReceiveAnyDamage, the spawned actor the causer
            crate::world::flow::broadcast_take_any_damage(w, b, dmg, Some(a));
        }
        if !is_clank {
            victim_rows(w, a, b, &packet, rows);
        }
        let bt = attack_type(w, b);
        dealer_hit_non_player(w, a, crate::real(bt), b);
        hits::hit_anything(w, a, b);
    }
    if let ActorRef::Projectile(bp) = b
        && w.actors.projectiles.contains(bp)
    {
        let d = hits::dealer(w, a);
        let hd = hits::projectile_hit_dir(w, &d, &hit, Some(victim_x(w, b)));
        let at = attack_type(w, a);
        hits::hit_by(w, bp, !can, hd.x, hd.y, hd.bkb, at, Some(a));
        force_knockback_tail(w, b, can, &hit, hd, at, a);
    }
    // NotifyAllyOfHit @21344: nothing to give (`damage_spawned_actor`).
    set_clanking(w, a, false);
    if queue_destroy(w, a) && !dead(w, a) {
        spawning::destroy_spawned(w, a);
    }
    can
}

/// Handle Projectile Clank's damage by type: Normal against Heavy / Radial / Transcendent, Heavy against Radial /
/// Transcendent, Light against anything but Light deal nothing.
fn zeroed(a: i32, o: i32) -> bool {
    match a {
        0 => o == 1 || o == 3 || o == 4,
        1 => o == 3 || o == 4,
        2 => o != 2,
        _ => false,
    }
}

/// The victim's x the hit's KnockBackMirror reads (`projectileHitDir`'s `b`).
fn victim_x(w: &World, b: ActorRef) -> Real {
    at_of(w, b).x
}

/// `dealerIsProjectile(b)` of a spawned victim.
fn victim_is_projectile(w: &World, b: ActorRef) -> bool {
    match b {
        ActorRef::Projectile(_) | ActorRef::Item(_) => hits::dealer_is_projectile(w, &hits::dealer(w, b)),
        _ => false,
    }
}

/// `spawnedReflects`: pfgDoDamage @15084-15462 for a spawned actor's handler: the hit has ReflectProjectiles and the
/// HitActor is IsProjectile or the call a clank's: the dealer's own Comp_ReflectProjectile where its class has one,
/// else its owner's; ReflectProjectile(HitActor, the hit) with the hit's direction. True: reflected (the call ends).
fn spawned_reflects(w: &mut World, a: ActorRef, hit: &HitPacket, b: ActorRef) -> bool {
    let d = hits::dealer(w, a);
    let hd = hits::projectile_hit_dir(w, &d, hit, Some(victim_x(w, b)));
    let own_comp = comp(w, a, "Comp_ReflectProjectile").is_some();
    // Unverified: an actor with no owner reflects nothing (the new owner reflectProjectile writes would be none).
    let Some(o) = owner(w, a) else { return false };
    if !reflect_projectile(w, o, b, V2 { x: -hd.x, y: hd.y }, own_comp.then_some(a)) {
        return false;
    }
    if let ActorRef::Projectile(ap) = a
        && flag(w, a, "DestroyOnReflectingProjectile")
        && !dead(w, a)
    {
        set_clanking(w, a, false);
        spawning::destroy_projectile(w, ap, false);
    }
    true
}

/// `spawnedDamageBoxOverlap`: a projectile's hit box or a thrown item's ThrowHitBox against another actor's damage
/// box (ProjectileHooks::damage_box): the hit body of object type HitBox makes the overlap; its begin is
/// `spawnedDamageBoxHit`.
// Deviation: tested where the projectiles' and items' other overlaps are, at the end of the moves; the hit shape as
// against a fighter but at depth 0 for a sphere, HitBox2D extruded along the depth; the box as the class gives it.
fn spawned_damage_box_overlap(
    w: &mut World,
    a: ActorRef,
    b: ProjectileId,
    bx: fn(&World, ProjectileId) -> Option<WorldSphere>,
    set: Set,
    on: bool,
) {
    let bref = ActorRef::Projectile(b);
    let db = if on && !dead(w, bref) && !dead(w, a) { bx(w, b) } else { None };
    let mut over = false;
    if let Some(db) = db {
        let n = near(w, a, db.x, db.y);
        let s = WorldSphere { x: n.x, y: n.y, r: hit_radius(w, a), z: sphere_z(w, a), ..WorldSphere::default() };
        over = s.r > 0.0 && shape_over_box(&s, &db) && hit_body_type(w, a) == obj::HIT_BOX;
    }
    if !over {
        set_delete(w, set, bref);
        return;
    }
    if !set_begin(w, set, bref) {
        return;
    }
    spawned_damage_box_hit(w, a, b);
}

/// `spawnedDamageBoxHit`: pfgDoDamage of a projectile's or an item's own hit on an actor met at its damage box: the
/// entry rows first (`entry_rows`).
fn spawned_damage_box_hit(w: &mut World, a: ActorRef, b: ProjectileId) {
    let bref = ActorRef::Projectile(b);
    if !entry_rows(w, a, bref) || spawned_refuses(w, a, bref) {
        return;
    }
    let hit = own_hit(w, a);
    if hit.hit_each_actor_only_once.unwrap_or(false) && seen_once(w, a, bref) {
        return;
    }
    damage_spawned_actor(w, a, b, &hit);
}

/// `damageSpawnedActor`: pfgDoDamage of a spawned actor's (or a buff actor's)
/// handler on a spawned actor: an IsProjectile victim takes the reflect / DestroyProjectiles branch @15084 first;
/// then DamageAndLaunch (CanTakeDamage, CanBeHitBy, ApplyDamage, HitByEnemy / HitByAlly, the owner's pause, the
/// rows, HitNonPlayer and HitAnything). Returns Hit.
/// NotifyAllyOfHit @21344, which every route of a spawned victim reaches (this one, `spawned_hit`,
/// `projectile_hits_projectile`, `damage_item`, `damage_mortar`, `damage_actor`), gives nothing on a spawned
/// victim: its branch @704-850 (DestroyProjectiles or ReflectProjectiles, a team mate's BPI_SpawnedActor) tracks the
/// victim @392 (TrackActorsHitByTag, done @5290 on every path past @8400), writes the dealer handler's
/// MostRecentHitActor @433 (overwritten @21532 ahead of its one read @22812; no spawned class or actor buff reads it),
/// sends ElementalBuffHitAlly @472 (not modelled on any route, ledger "HitByAlly"), and broadcasts the victim
/// handler's HitByAlly @584 (bound by Buff_Ice on a fighter's handler alone) and the dealer handler's
/// HitAllyProjectile @657 (bound by Pawn_C015 on its own handler alone).
pub fn damage_spawned_actor(w: &mut World, a: ActorRef, b: ProjectileId, packet: &HitPacket) -> bool {
    damage_spawned_actor_call(w, a, b, packet, &hits::DamageCall::default())
}

/// [`damage_spawned_actor`] with the call's OnHitBuffsParam and IsClankParam (`ctx`): the ally rows behind the
/// dealt pass (`ally_rows`) and the clank of a BPI_SpawnedActor dealer (`clank`) at @15084.
pub(crate) fn damage_spawned_actor_call(
    w: &mut World,
    a: ActorRef,
    b: ProjectileId,
    packet: &HitPacket,
    ctx: &hits::DamageCall<'_>,
) -> bool {
    let bref = ActorRef::Projectile(b);
    if dead(w, bref) {
        return false;
    }
    let mut hit = dealt_pass(w, a, bref, packet);
    if ally_rows(w, a, bref, packet, ctx.rows) {
        return false;
    }
    let is_proj = comp(w, bref, "Comp_Actor_Destructible").is_some_and(|c| hits::truthy(c, "IsProjectile"));
    let spawned = matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_));
    // the dealer's attack type read at each HitBy call (`sp ? sp.attackType : -1`)
    let sp_type = |w: &World| if spawned { attack_type(w, a) } else { -1 };
    let mut clanked = false;
    if is_proj || ctx.clank {
        let reflect = hit.reflect_projectiles.unwrap_or(false);
        if spawned && reflect && spawned_reflects(w, a, &hit, bref) {
            return false; // @15158-15462
        }
        clanked = !reflect && clank(w, a, bref, &mut hit, ctx.clank);
        if !clanked && !hit.destroy_projectiles.unwrap_or(false) {
            // @19122: HitByAlly / HitByEnemy alone
            let d = hits::dealer(w, a);
            let hd = hits::projectile_hit_dir(w, &d, &hit, Some(victim_x(w, bref)));
            let ally = owner_team(w, a) == owner_team(w, bref);
            let t = sp_type(w);
            hits::hit_by(w, b, ally, hd.x, hd.y, hd.bkb, t, Some(a));
            return false;
        }
    }
    // DamageAndLaunch @51: without the victim's CanTakeDamage Hit = false @6097 and the end.
    if comp(w, bref, "Comp_Pawn_DamageManager").is_some_and(|c| c.get("CanTakeDamage") == Some(&Value::Bool(false))) {
        if clanked {
            clank_end(w, a);
        }
        return false;
    }
    let dmg = charged_damage(w, a, &hit);
    let can = spawned_can_be_hit_by(w, a, bref);
    // ApplyDamage(HitActor, Damage, null, DamageDealer, null) @2591: the causer is the dealer actor itself.
    if can {
        let gone = if is_proj {
            let t = if spawned { attack_type(w, a) } else { 0 };
            projectile_any_damage(w, b, dmg, t, a)
        } else {
            let any = hooks_of(w, bref).and_then(|h| h.any_damage);
            dmg != 0.0 && any.is_some_and(|f| f(w, b, dmg, a))
        };
        if gone && !dead(w, bref) {
            spawning::destroy_projectile(w, b, false);
        }
    }
    let d = hits::dealer(w, a);
    let hd = hits::projectile_hit_dir(w, &d, &hit, Some(victim_x_or(w, bref, d.x)));
    // HitByEnemy @3723 / HitByAlly @8047 after ApplyDamage @2591: the broadcast skips a handler whose object is
    // pending kill (ProcessMulticastDelegate 14079aa20), so a victim that ApplyDamage destroyed gets no hitBy.
    if !dead(w, bref) {
        let t = sp_type(w);
        hits::hit_by(w, b, !can, hd.x, hd.y, hd.bkb, t, Some(a));
    }
    let t = sp_type(w);
    force_knockback_tail(w, bref, can, &hit, hd, t, a);
    if can {
        spawned_owner_pause(w, a, &hit, |_| 1.0);
        victim_rows(w, a, bref, packet, ctx.rows);
        if spawned {
            let bt = attack_type(w, bref);
            dealer_hit_non_player(w, a, crate::real(bt), bref);
        }
        hits::hit_anything(w, a, bref);
    }
    if clanked {
        clank_end(w, a);
    }
    can
}

/// A refused hit with ForceKnockback (DamageAndLaunch @8516 goto 3222): the allowed hit's tail after its HitByAlly
/// @8047, without ApplyDamage: HitByEnemy @3723 on a projectile victim still there (no rumble: the dealer is no
/// BPI_BaseFighter), Hit false. An item victim's HitByEnemy has no listener the port binds (no item class's, as on
/// the allowed hit). The reaction's light-hit sound and KnockedBack are not made for a spawned victim, as on the
/// allowed hit.
fn force_knockback_tail(
    w: &mut World,
    b: ActorRef,
    can: bool,
    hit: &HitPacket,
    hd: hits::ProjectileHitDirOut,
    attack_type: i32,
    by: ActorRef,
) {
    if can || !hit.force_knockback.unwrap_or(false) {
        return;
    }
    if let ActorRef::Projectile(bp) = b
        && !dead(w, b)
    {
        hits::hit_by(w, bp, false, hd.x, hd.y, hd.bkb, attack_type, Some(by));
    }
}

/// The victim's x, or `fallback` once it is gone (the dead actor's last place is read).
fn victim_x_or(w: &World, b: ActorRef, fallback: Real) -> Real {
    match b {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(fallback, |p| p.x),
        ActorRef::Item(id) => w.actors.items.get(id).map_or(fallback, |i| i.x),
        _ => victim_x(w, b),
    }
}

/// `dealerHitNonPlayer`: the hit box handler's HitNonPlayer broadcast on the
/// dealer `a`: the parent's handler (true: its destroy) or a projectile class's own override in its place, then an
/// item class's own after it.
pub fn dealer_hit_non_player(w: &mut World, a: ActorRef, other_type: Real, victim: ActorRef) {
    if dead(w, a) {
        return;
    }
    let gone = spawned_hit_non_player(w, a, other_type, victim);
    items::item_throw_hits(w); // an item's ItemThrowHit (HitNonPlayer @9354), ahead of the destroy
    if gone {
        spawning::destroy_spawned(w, a);
    }
    if let ActorRef::Item(it) = a {
        let cls = w.actors.items.get(it).map(|i| i.cls);
        let own = cls.and_then(|c| w.registry.actors.get(w.names.str(c))).and_then(|e| match &e.hooks {
            ActorHooks::Item(h) => h.hit_non_player,
            _ => None,
        });
        if own.is_some_and(|h| h(w, it, gone)) && !dead(w, a) {
            spawning::destroy_spawned(w, a);
        }
    }
}

/// `spawnedHitNonPlayer`: HitNonPlayer on the dealer: a projectile class's own override (no parent call: false) or
/// the parent's handler (true: its destroy).
fn spawned_hit_non_player(w: &mut World, a: ActorRef, other_type: Real, victim: ActorRef) -> bool {
    let t = crate::math::to_int32(other_type);
    match a {
        ActorRef::Projectile(id) => match hooks_of(w, a).and_then(|h| h.hit_non_player) {
            Some(own) => {
                own(w, id, victim);
                false
            }
            None => prj::hit_non_player(w, id, t),
        },
        ActorRef::Item(it) => itm::item_hit_non_player(w, it, t),
        _ => crate::unported!("spawned hits: HitNonPlayer of this family", w, a, other_type, victim),
    }
}

// ---- a World.actors pawn and a placed stage actor

/// `actorBodyOverlap`: a projectile's HitBox2D / HitBox or an item's ThrowHitBox over a World.actors pawn's root
/// capsule (WorldActorHooks::pawn_body); the begin of the overlap is pfgDoDamage: the entry rows (`entry_rows`),
/// TrackActorsHitByTag, then `damageActor`.
// Unverified: only a hit body of object type HitBox is taken.
// Deviation: tested at the end of the moves, as the projectiles' other overlaps.
fn actor_body_overlap(w: &mut World, a: ActorRef, b: crate::actors::ActorId, set: Set, on: bool) {
    let bref = ActorRef::Actor(b);
    let live = on && !dead(w, bref) && !dead(w, a);
    let cls = cls_of(w, bref);
    let body: Option<ExtraHurt> =
        if live { world_hooks(w, cls).and_then(|h| h.pawn_body).and_then(|f| f(w, b)) } else { None };
    let meet_bodies = live && w.registry.actors.get(w.names.str(cls)).is_some_and(|k| k.spawned_meet_bodies);
    let mut over = false;
    if body.is_none() && meet_bodies {
        over = hit_body_over_bodies(w, a, b);
    } else if let Some(body) = body {
        let n = near(w, a, body.x, (body.y0 + body.y1) / 2.0);
        let s = WorldSphere { x: n.x, y: n.y, r: hit_radius(w, a), z: sphere_z(w, a), ..WorldSphere::default() };
        over = s.r > 0.0 && shape_over_capsule(&s, &body) && hit_body_type(w, a) == obj::HIT_BOX;
    }
    if !over {
        set_delete(w, set, bref);
        return;
    }
    if !set_begin(w, set, bref) || !entry_rows(w, a, bref) {
        return;
    }
    let hit = own_hit(w, a);
    if hit.hit_each_actor_only_once.unwrap_or(false) && seen_once(w, a, bref) {
        return;
    }
    damage_actor(w, a, b, &hit, None);
}

/// `hitBodyOverBodies`: the hit body's sphere over the actor's trace bodies of the object types the hit body's
/// template answers with Overlap or Block, HurtBox among them (`ActorEntry::spawned_meet_bodies`: C018_Platform's
/// Hurtbox1 / Hurtbox2, VelmaEvidence's Destructible, WW_ShieldActor's BlockCollision).
/// Deviation: tested at the end of the moves in the play plane, as the projectiles' other overlaps.
fn hit_body_over_bodies(w: &mut World, a: ActorRef, b: crate::actors::ActorId) -> bool {
    let r = hit_radius(w, a);
    if r.is_nan() || r <= 0.0 {
        return false;
    }
    let types = actor_body_types(hit_body_comp(w, a).1);
    if types.is_empty() {
        return false;
    }
    // A class without a place meets nothing (hitNear(undefined, undefined): NaN): no class with
    // spawned_meet_bodies is one (`world_actor::class_has_place`).
    let Some((bx, by, _)) = crate::actors::world_actor::world_actor_place_of(w, b) else { return false };
    let n = near(w, a, bx, by);
    let responder = crate::real(hit_body_type(w, a));
    crate::fighter::traces::sphere_overlaps_actor(w, n.x, n.y, r, &types, &[Some(a)], responder, ActorRef::Actor(b))
}

/// `lightHitSound`: DamageAndLaunch's light path on a victim that is no fighter
/// (@3222 -> @3346: BaseKnockBack and HitStunMult not above 0): with a positive Damage LightHitReactionMulticast
/// @3468 -> PlayHitSound at the hit, none when the dealer is a fighter in hitstun (@140-329). `instigator`: the
/// victim's owner.
// Unverified: GetVictimPhysmat is taken as 6 (flesh, no PhysMaterialOverride) for every spawned victim.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn light_hit_sound(
    w: &mut World,
    hit: &HitPacket,
    dmg: Real,
    owner: FighterId,
    at: V2,
    by_fighter: bool,
    instigator: crate::events::Fid,
) {
    let Some(ev) = hit.on_hit_sound.or(hit.on_hit_audio) else { return };
    if hit.base_knock_back.unwrap_or(0.0) > 0.0
        || hit.hit_stun_mult.unwrap_or(0.0) > 0.0
        || dmg.is_nan()
        || dmg <= 0.0
        || (by_fighter && hits::in_hitstun(w.fighter(owner)))
    {
        return;
    }
    let charge = by_fighter.then(|| crate::fighter::percent_charged(w, owner));
    let sh = SoundHit { victim: -1, z_floor: None, charge, surface: Some(6), instigator: Some(instigator) };
    let fighter = w.fighter(owner).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(at.x), y: Some(at.y), hit: Some(sh), pooled: false, key: None });
}

/// `actorDamageAndLaunch`: Comp_Pawn_DamageManager DamageAndLaunch on a
/// World.actors pawn: CanTakeDamage @51, CanBeHitBy @659 (false and no ForceKnockback: no hit), ApplyDamage ->
/// the class's ReceiveAnyDamage, the light hit's sound, a fighter dealer's ControllerRumble (@3589-3668). Returns
/// Hit (CanBeHitBy).
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn actor_damage_and_launch(
    w: &mut World,
    b: crate::actors::ActorId,
    dealer_team: i32,
    hit: &HitPacket,
    dmg: Real,
    causer: ActorRef,
    owner: FighterId,
    at: V2,
    by_fighter: bool,
) -> bool {
    let bref = ActorRef::Actor(b);
    if comp(w, bref, "Comp_Pawn_DamageManager").is_some_and(|c| c.get("CanTakeDamage") == Some(&Value::Bool(false))) {
        return false;
    }
    let cls = cls_of(w, bref);
    let kind = world_hooks(w, cls);
    let b_owner = w.actors.actors.get(b).expect("a live actor").owner;
    let d = match kind.and_then(|k| k.destructible) {
        Some(f) => can_be_hit_by_d(f(w, b)),
        None => can_be_hit_by_d(comp(w, bref, "Comp_Actor_Destructible")),
    };
    let can = sa::can_be_hit_by(dealer_team, w.fighter(b_owner).team.team, d);
    if !can && !hit.force_knockback.unwrap_or(false) {
        return false;
    }
    if can
        && dmg != 0.0
        && let Some(any) = kind.and_then(|k| k.any_damage)
    {
        any(w, b, dmg, causer);
    }
    let inst = w.fighter(b_owner).id;
    light_hit_sound(w, hit, dmg, owner, at, by_fighter, inst);
    // @3589-3668: a BPI_BaseFighter dealer's BPI_ControllerRumble(ControllerRumble), on the refused ForceKnockback
    // branch too (@8516 goto 3222).
    if by_fighter {
        crate::fighter::tick::controller_rumble(w, owner, hit.controller_rumble);
    }
    can
}

/// `stageBodyOverlap`: a projectile's or an item's hit body over a placed stage actor's hurt shape, as
/// `actorBodyOverlap`: the begin of the overlap, the entry rows, HitEachActorOnlyOnce, IsVulnerableTo (@4608), then
/// DamageAndLaunch (`stageDamageAndLaunch`); on a Hit the owner's pause (x the victim's scale), the dealer's
/// HitNonPlayer and HitAnything.
// Deviation: the hit body's object type must be HitBox (as actorBodyOverlap); tested at the end of the moves.
fn stage_body_overlap(w: &mut World, a: ActorRef, i: u32, set: Set, on: bool) {
    let bref = ActorRef::Stage(i);
    let kind = stage_kind(w, i);
    let mut body = Vec::new();
    let hb = match kind {
        Some(k) if on && !dead(w, a) => sa::stage_hurt(w, k, bref, &mut body),
        _ => None,
    };
    let mut over = false;
    if hb.is_some() {
        let n = match a {
            ActorRef::Projectile(_) => {
                let c = sa::stage_hurt_centre(&body);
                near(w, a, c.x, c.y)
            }
            _ => at_of(w, a),
        };
        let s = WorldSphere { x: n.x, y: n.y, r: hit_radius(w, a), z: sphere_z(w, a), ..WorldSphere::default() };
        over = s.r > 0.0 && sa::stage_hurt_over(&s, &body) && hit_body_type(w, a) == obj::HIT_BOX;
    }
    if !over {
        set_delete(w, set, bref);
        return;
    }
    if !set_begin(w, set, bref) || !entry_rows(w, a, bref) {
        return;
    }
    let hit = own_hit(w, a);
    if hit.hit_each_actor_only_once.unwrap_or(false) && seen_once(w, a, bref) {
        return;
    }
    let k = kind.expect("an overlap of a stage actor with a kind");
    // Deviation: a dealer with no owner (an actor the world spawned) deals no hit here; the one class that has such an
    // actor (BugsBunnySafe) has no hit box of the parent's and hits fighters alone from its own test.
    let Some(o) = owner(w, a) else { return };
    if !sa::stage_vulnerable(w, k, bref) {
        return;
    }
    let dmg = charged_damage(w, a, &hit);
    let team = owner_team(w, a);
    let at = at_of(w, a);
    if !sa::stage_damage_and_launch(w, bref, team, &hit, dmg, a, o, at, false) {
        return;
    }
    spawned_owner_pause(w, a, &hit, |w| sa::stage_hit_pause_scale(w, k, bref));
    dealer_hit_non_player(w, a, crate::real(prj::NOT_SPAWNED_ACTOR), bref);
    hits::hit_anything(w, a, bref);
}

/// The registry's entry of the stage's actor `i` (`STAGE_ACTORS.get(b.cls)`).
pub(crate) fn stage_kind(w: &World, i: u32) -> Option<&'static crate::registry::entries::StageActorEntry> {
    let cls = w.stage_run.as_ref()?.actors.get(usize::try_from(i).ok()?)?;
    w.registry.stage_actors.get(cls.cls.name())
}

/// `damageActor`: pfgDoDamage of a spawned actor's or an actor's own handler on
/// a World.actors pawn: DamageAndLaunch (`actorDamageAndLaunch`), then on a Hit the dealer's owner's pause, the
/// rows, the dealer's HitNonPlayer and HitAnything. Returns Hit. `at`: pfgDoDamage's LocalHitLocation when the caller
/// passes no HitBox; `None`: the dealer actor's location.
pub fn damage_actor(w: &mut World, a: ActorRef, b: crate::actors::ActorId, packet: &HitPacket, at: Option<V2>) -> bool {
    actor_route(w, a, b, packet, at, &hits::DamageCall::default(), false)
}

/// [`damage_actor`] with the call's OnHitBuffsParam and IsClankParam (`ctx`): a query dealer's
/// pfgDoDamage(null, actor) that passes its own rows (`actorVictimHit`).
pub(crate) fn damage_actor_call(
    w: &mut World,
    a: ActorRef,
    b: crate::actors::ActorId,
    packet: &HitPacket,
    at: Option<V2>,
    ctx: &hits::DamageCall<'_>,
) -> bool {
    actor_route(w, a, b, packet, at, ctx, false)
}

/// [`damage_actor`] with the call's OnHitBuffsParam and IsClankParam (`ctx`); `gate`: the HitActor's
/// Comp_Actor_Destructible IsProjectile is read (@15084, `damage_world_actor`). The ally rows come behind the dealt
/// pass (`ally_rows`). A call with IsClankParam (Handle Projectile Clank's reciprocal one, on VelmaHealBeam) goes to
/// @20004 whatever the flag: HitPauseTime 0, the Damage zeroed by the two attack types, then DamageAndLaunch.
// Unverified: the reflect @15158 of an IsClankParam call on a World.actors HitActor is not made (no clank hit of a
// ported class has ReflectProjectiles); a dealer's own clank on a World.actors HitActor (@20004 without IsClankParam)
// is not made: no ported World.actors kind with BPI_CanProjectileClank has a body a hit box meets.
fn actor_route(
    w: &mut World,
    a: ActorRef,
    b: crate::actors::ActorId,
    packet: &HitPacket,
    at: Option<V2>,
    ctx: &hits::DamageCall<'_>,
    gate: bool,
) -> bool {
    let hit_ok = actor_route_body(w, a, b, packet, at, ctx, gate);
    // @21484: a clanking dealer's call ends with SetIsProjectileClanking(false) on every way out (the ally loop's exit
    // @8400 included).
    if ctx.clank {
        clank_end(w, a);
    }
    hit_ok
}

/// [`actor_route`] up to the clank's end.
fn actor_route_body(
    w: &mut World,
    a: ActorRef,
    b: crate::actors::ActorId,
    packet: &HitPacket,
    at: Option<V2>,
    ctx: &hits::DamageCall<'_>,
    gate: bool,
) -> bool {
    let bref = ActorRef::Actor(b);
    // Deviation: a dealer with no owner (an actor the world spawned) deals no hit here; the one class that has such an
    // actor (BugsBunnySafe) has no hit box of the parent's and hits fighters alone from its own test.
    let Some(o) = owner(w, a) else { return false };
    if dead(w, bref) {
        return false;
    }
    let mut hit = dealt_pass(w, a, bref, packet);
    if ally_rows(w, a, bref, packet, ctx.rows) {
        return false;
    }
    if ctx.clank {
        hit.hit_pause_time = Some(0.0); // @20555
        if zeroed(attack_type(w, a), attack_type(w, bref)) {
            hit.damage = Some(0.0);
        }
    } else if gate && world_actor_is_projectile(w, b) && !hit.destroy_projectiles.unwrap_or(false) {
        // @19122: the HitBy broadcasts alone (no World.actors kind ported binds them)
        return false;
    }
    actor_tail(w, a, b, o, packet, &hit, at, ctx.rows)
}

/// [`actor_route`] from DamageAndLaunch on.
#[allow(clippy::too_many_arguments, reason = "the route's parts")]
fn actor_tail(
    w: &mut World,
    a: ActorRef,
    b: crate::actors::ActorId,
    o: FighterId,
    packet: &HitPacket,
    hit: &HitPacket,
    at: Option<V2>,
    rows: Option<&[OnHitBuffRow]>,
) -> bool {
    let bref = ActorRef::Actor(b);
    let at = at.unwrap_or_else(|| at_of(w, a));
    let dmg = charged_damage(w, a, hit);
    let team = owner_team(w, a);
    if !actor_damage_and_launch(w, b, team, hit, dmg, a, o, at, false) {
        return false;
    }
    spawned_owner_pause(w, a, hit, |_| 1.0);
    victim_rows(w, a, bref, packet, rows);
    if matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        // A HitActor with BPI_GetProjectileAttackType (its kind's hook: VelmaHealBeam) is a SpawnedActorParent: its
        // type; any other NOT_SPAWNED_ACTOR.
        // Unverified: the other World.actors classes that are SpawnedActorParent children pass NOT_SPAWNED_ACTOR.
        let cls = cls_of(w, bref);
        let bt = world_hooks(w, cls).and_then(|h| h.attack_type).map_or(prj::NOT_SPAWNED_ACTOR, |f| f(w, b));
        dealer_hit_non_player(w, a, crate::real(bt), bref);
    }
    hits::hit_anything(w, a, bref);
    true
}

/// A World.actors HitActor's Comp_Actor_Destructible has IsProjectile (the kind's `destructible`, else its template).
fn world_actor_is_projectile(w: &World, b: crate::actors::ActorId) -> bool {
    let bref = ActorRef::Actor(b);
    let cls = cls_of(w, bref);
    match world_hooks(w, cls).and_then(|k| k.destructible) {
        Some(f) => f(w, b).is_some_and(|d| hits::truthy(d, "IsProjectile")),
        None => comp(w, bref, "Comp_Actor_Destructible").is_some_and(|d| hits::truthy(d, "IsProjectile")),
    }
}

// ---- the routes of pfgDoDamage

/// `damageItem`: a World.actors dealer's or a buff handler's pfgDoDamage on an item.
/// The ally rows behind the dealt pass (`ally_rows`). An IsProjectile item (or a call with IsClankParam) takes @15084:
/// the clank (`clank`), else without DestroyProjectiles @19122's HitByAlly / HitByEnemy alone (SpawnedActorParent's,
/// empty). Else DamageAndLaunch @15630: CanTakeDamage @51, CanBeHitBy @659 (the dealer's TeamIndex: -1 with no
/// owner), ApplyDamage of Damage x the dealer's charge multiplier (@7835) -> the item's ReceiveAnyDamage (true: its
/// destroy), the dealer's owner's pause, the victim's OnHitBuffs rows and the dealer's HitAnything. The item takes no
/// hit pause of its own.
// Unverified: the item's class data is taken by actorDef's lookup (the owner's character, then the match's), where
// not the item's own definition.
pub(crate) fn damage_item(
    w: &mut World,
    a: ActorRef,
    b: ItemId,
    packet: &HitPacket,
    ctx: &hits::DamageCall<'_>,
) -> bool {
    let bref = ActorRef::Item(b);
    if dead(w, bref) {
        return false;
    }
    let mut hit = dealt_pass(w, a, bref, packet);
    if ally_rows(w, a, bref, packet, ctx.rows) {
        return false;
    }
    let mut clanked = false;
    if itm::item_is_projectile(w, b) || ctx.clank {
        // (no dealer of this path has ReflectProjectiles: the reflect @15158 is not reached)
        clanked = !hit.reflect_projectiles.unwrap_or(false) && clank(w, a, bref, &mut hit, ctx.clank);
        if !clanked && !hit.destroy_projectiles.unwrap_or(false) {
            return false;
        }
    }
    let hit_ok = item_tail(w, a, b, packet, &hit, ctx.rows);
    if clanked {
        clank_end(w, a);
    }
    hit_ok
}

/// [`damage_item`] from DamageAndLaunch on.
fn item_tail(
    w: &mut World,
    a: ActorRef,
    b: ItemId,
    packet: &HitPacket,
    hit: &HitPacket,
    rows: Option<&[OnHitBuffRow]>,
) -> bool {
    let bref = ActorRef::Item(b);
    if dead(w, bref) {
        return false;
    }
    if comp(w, bref, "Comp_Pawn_DamageManager").is_some_and(|c| c.get("CanTakeDamage") == Some(&Value::Bool(false))) {
        return false;
    }
    // The dealer's own TeamIndex, -1 with no owner.
    let team = owner_team(w, a);
    let d = can_be_hit_by_d(comp(w, bref, "Comp_Actor_Destructible"));
    if !sa::can_be_hit_by(team, itm::item_destructible_team(w, b), d) {
        return false;
    }
    let dmg = charged_damage(w, a, hit);
    if itm::item_receive_damage(w, b, dmg, false) {
        items::item_throw_hits(w);
        items::destroy_item(w, b);
    }
    owner_pause(w, a, hit);
    victim_rows(w, a, bref, packet, rows);
    hits::hit_anything(w, a, bref);
    true
}

/// `damageMortar`: pfgDoDamage on C019_MortarAim (IsProjectile, no ReceiveAnyDamage,
/// CanBeHitBy false until ActivateObject clears DamageDisabled): the ally rows behind the dealt pass (`ally_rows`);
/// only a DestroyProjectiles hit of another team on an active one is a Hit: the dealer's owner's pause, the aim's
/// OnHitBuffs rows, the dealer's HitNonPlayer and HitAnything.
// Unverified: the clank @20004 is not made on a mortar aim (its BPI_CanProjectileClank is not read).
pub(crate) fn damage_mortar(
    w: &mut World,
    a: ActorRef,
    b: crate::actors::MortarId,
    packet: &HitPacket,
    ctx: &hits::DamageCall<'_>,
) -> bool {
    let bref = ActorRef::Mortar(b);
    if dead(w, bref) {
        return false;
    }
    let hit = dealt_pass(w, a, bref, packet);
    if ally_rows(w, a, bref, packet, ctx.rows) {
        return false;
    }
    let m = w.actors.mortars.get(b).expect("a live mortar");
    // (the dealer's own TeamIndex, -1 with no owner)
    if !hit.destroy_projectiles.unwrap_or(false) || owner_team(w, a) == w.fighter(m.owner).team.team || !m.active {
        return false;
    }
    owner_pause(w, a, &hit);
    victim_rows(w, a, bref, packet, ctx.rows);
    if matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        let bt = attack_type(w, bref);
        dealer_hit_non_player(w, a, crate::real(bt), bref);
    }
    hits::hit_anything(w, a, bref);
    true
}

/// `damageWorldActor`: pfgDoDamage on a World.actors entry: one whose
/// Comp_Actor_Destructible has IsProjectile takes @15084: without DestroyProjectiles the HitBy broadcasts alone (no
/// World.actors kind ported binds them), with it DamageAndLaunch as one without the flag (`damageActor`). `at`: as
/// [`damage_actor`]'s.
// The dealt pass and the ally rows come first, as @5617-6835 precede @15084.
pub(crate) fn damage_world_actor(
    w: &mut World,
    a: ActorRef,
    b: crate::actors::ActorId,
    hit: &HitPacket,
    at: Option<V2>,
    ctx: &hits::DamageCall<'_>,
) -> bool {
    actor_route(w, a, b, hit, at, ctx, true)
}

// ---- the projectiles among themselves

/// The rest of `detectProjectileHits`: a projectile's hit box against another
/// projectile's HurtBox (the pair of one with itself too), a World.actors pawn's root capsule, a placed stage
/// actor's hurt shape; the mortars' overlaps of dead actors dropped, then the mortars' Hurtbox.
// The lists by place, as item_spawned_hits'.
pub fn projectile_spawned_hits(w: &mut World) {
    let mut i = 0;
    while let Some(a) = w.actors.projectiles.nth(i) {
        i += 1;
        let mut k = 0;
        while let Some(b) = w.actors.projectiles.nth(k) {
            k += 1;
            projectile_pair(w, a, b);
        }
    }
    let mut i = 0;
    while let Some(b) = w.actors.actors.nth(i) {
        i += 1;
        let mut k = 0;
        while let Some(a) = w.actors.projectiles.nth(k) {
            k += 1;
            let on = phases::hitbox_active(w, a);
            let ar = ActorRef::Projectile(a);
            actor_body_overlap(w, ar, b, Set::Overlapping(ar), on);
        }
    }
    for i in 0..stage_actor_count(w) {
        let mut k = 0;
        while let Some(a) = w.actors.projectiles.nth(k) {
            k += 1;
            let on = phases::hitbox_active(w, a);
            let ar = ActorRef::Projectile(a);
            stage_body_overlap(w, ar, i, Set::Overlapping(ar), on);
        }
    }
    let mut i = 0;
    while let Some(mo) = w.actors.mortars.nth(i) {
        i += 1;
        let m = w.actors.mortars.get(mo).expect("a listed mortar");
        let gone: Vec<ActorRef> = m.overlapped_by.iter().copied().filter(|&o| dead(w, o)).collect();
        let m = w.actors.mortars.get_mut(mo).expect("a listed mortar");
        for o in gone {
            m.overlapped_by.shift_remove(&o);
        }
    }
    let mut i = 0;
    while let Some(a) = w.actors.projectiles.nth(i) {
        i += 1;
        let on = phases::hitbox_active(w, a);
        crate::chars::morty::mortar_hurt(w, ActorRef::Projectile(a), on);
    }
}

/// `projectilePair`: `a`'s hit box against `b`'s HurtBox (or its buff's AdditionalHurtBox, or its own damage box):
/// the overlap's begin.
fn projectile_pair(w: &mut World, a: ProjectileId, b: ProjectileId) {
    if !w.actors.projectiles.contains(a) || !w.actors.projectiles.contains(b) {
        return;
    }
    let (ar, br) = (ActorRef::Projectile(a), ActorRef::Projectile(b));
    let is_self = a == b;
    // A class with a damage box of its own is met there. Not ported: its own hit box over its own damage box (none of
    // those classes has an OnHitBuffs row).
    if let Some(bx) = hooks_of(w, br).and_then(|h| h.damage_box) {
        if !is_self {
            let on = phases::hitbox_active(w, a);
            spawned_damage_box_overlap(w, ar, b, bx, Set::Overlapping(ar), on);
        }
        return;
    }
    let (a_hit, b_hurt, b_x, b_y, b_hx, b_hy) = {
        let pa = w.actors.projectiles.get(a).expect("a live projectile");
        let pb = w.actors.projectiles.get(b).expect("a live projectile");
        (pa.hit_radius, pb.hurt_radius, pb.x, pb.y, prj::hurt_x(pb), prj::hurt_y(pb))
    };
    let r = a_hit + b_hurt;
    let br_r = if has_elem(w, br) && !dead(w, br) { elem::actor_hurt_sphere(w, br) } else { 0.0 };
    let rb = a_hit + br_r;
    let n = prj::hit_near(w, w.actors.projectiles.get(a).expect("a live projectile"), b_x, b_y);
    let (ex, ey) = (n.x - b_x, n.y - b_y);
    let in_buff = br_r > 0.0 && ex * ex + ey * ey <= rb * rb;
    let n = prj::hit_near(w, w.actors.projectiles.get(a).expect("a live projectile"), b_hx, b_hy);
    // hurtRadius 0: an actor without a HurtBox is no target of a hit box.
    let misses = |w: &World| {
        b_hurt <= 0.0 || prj::hurt_dist2(w.actors.projectiles.get(b).expect("a live projectile"), n.x, n.y) > r * r
    };
    if !phases::hitbox_active(w, a) || dead(w, br) || (misses(w) && !in_buff) {
        set_delete(w, Set::Overlapping(ar), br);
        return;
    }
    if !set_begin(w, Set::Overlapping(ar), br) {
        return;
    }
    if is_self {
        self_hit(w, a);
        return;
    }
    // The class's own begin handler ahead of the hit box handler's.
    if let Some(begin) = hooks_of(w, ar).and_then(|h| h.hitbox_begin_overlap) {
        begin(w, a, br);
        if dead(w, ar) || dead(w, br) {
            return;
        }
    }
    projectile_hits_projectile(w, a, b, false);
}

/// `spawnHurtBegins`: the other dealers' boxes against a new projectile's
/// HurtBox (another projectile's hit box, a thrown item's ThrowHitBox), begun in its FinishSpawningActor; an overlap
/// begun here is held.
pub fn spawn_hurt_begins(w: &mut World, pr: ProjectileId) {
    let mut k = 0;
    while let Some(other) = w.actors.projectiles.nth(k) {
        if dead(w, ActorRef::Projectile(pr)) {
            break;
        }
        if other != pr {
            projectile_pair(w, other, pr);
        }
        k += 1;
    }
    let mut n = 0;
    while let Some(it) = w.actors.items.nth(n) {
        n += 1;
        if dead(w, ActorRef::Projectile(pr)) {
            return;
        }
        if itm::is_owned(w, it) {
            let me = ActorRef::Item(it);
            let on = item_deals(w, it);
            spawned_overlap(w, me, ActorRef::Projectile(pr), Set::Overlapping(me), ActorRef::Projectile(pr), on);
        }
    }
}

/// `spawnPlaceHits`: a new projectile's own hit box at its spawn place against
/// the other projectiles' HurtBox and its own, a thrown item's Sphere, a World.actors pawn, a stage actor and a
/// mortar's Hurtbox. An overlap begun here is held.
pub fn spawn_place_hits(w: &mut World, pr: ProjectileId) {
    let me = ActorRef::Projectile(pr);
    let mut k = 0;
    while let Some(other) = w.actors.projectiles.nth(k) {
        if dead(w, me) {
            break;
        }
        projectile_pair(w, pr, other);
        k += 1;
    }
    let mut n = 0;
    while let Some(it) = w.actors.items.nth(n) {
        n += 1;
        if dead(w, me) {
            return;
        }
        if itm::is_owned(w, it) {
            let on = phases::hitbox_active(w, pr);
            spawned_overlap(w, me, ActorRef::Item(it), Set::OverlappedBy(it), me, on);
        }
    }
    let mut n = 0;
    while let Some(b) = w.actors.actors.nth(n) {
        n += 1;
        if !dead(w, me) {
            let on = phases::hitbox_active(w, pr);
            actor_body_overlap(w, me, b, Set::Overlapping(me), on);
        }
    }
    for i in 0..stage_actor_count(w) {
        if !dead(w, me) {
            let on = phases::hitbox_active(w, pr);
            stage_body_overlap(w, me, i, Set::Overlapping(me), on);
        }
    }
    if !dead(w, me) {
        let on = phases::hitbox_active(w, pr);
        crate::chars::morty::mortar_hurt(w, me, on);
    }
}

/// `projectileAnyDamage`: ApplyDamage @2591 on a projectile HitActor of the
/// IsProjectile branch: the class's own ReceiveAnyDamage (none for a Damage of 0) in place of
/// ParentProjectilePhysics'. True: the actor is destroyed now.
pub fn projectile_any_damage(w: &mut World, b: ProjectileId, dmg: Real, causer_type: i32, causer: ActorRef) -> bool {
    match hooks_of(w, ActorRef::Projectile(b)).and_then(|h| h.any_damage) {
        Some(any) => dmg != 0.0 && any(w, b, dmg, causer),
        None => prj::receive_damage(w, b, dmg, causer_type),
    }
}

/// `hitBuffsAllyActor`: pfgDoDamage @6124-6835 with a spawned HitActor of the Instigator's TeamIndex: each row with
/// ApplyBuffHit? and ApplyBuffAlly? is SpawnBuffActor on the actor (`spawnActorBuff`), BreakIfBlocked true.
fn hit_buffs_ally_actor(w: &mut World, a: ActorRef, b: ActorRef, rows: &[OnHitBuffRow]) {
    if dead(w, b) || owner_team(w, a) != owner_team(w, b) {
        return;
    }
    let mult = elem::charge_buff(w, a);
    let from = owner(w, a);
    for e in rows {
        if e.apply_buff_hit && e.apply_buff_ally {
            let stacks = hits::stacks_times(e, mult);
            elem::spawn_actor_buff(w, b, e.applied_buffs, stacks, e.buff_duration.unwrap_or(0.0), true, from);
            if dead(w, b) {
                return;
            }
        }
    }
}

/// `selfHit`: a projectile's hit box beginning to overlap its own HurtBox: the class's own begin-overlap handler,
/// then pfgDoDamage with the actor as its own HitActor (`projectile_hits_projectile`): the entry rows to the
/// Instigator and its team, the reflect pop, HitEachActorOnlyOnce, the ally rows on the actor itself, then the tail
/// of a team mate's actor: a ReflectProjectiles hit's reflect of itself, @19122's HitByAlly without DestroyProjectiles,
/// else DamageAndLaunch, whose CanBeHitBy (@659) refuses its own team: the class's OnHitByAlly (@8047, the
/// DamageManager broadcast SpawnedActorParent binds), with ForceKnockback its OnHitByEnemy (@3723). NotifyAllyOfHit's
/// branch for a spawned victim (@704-850) has no listener (`damage_spawned_actor`).
/// The rest of the call runs after the begin handler too, not the On Hit Buffs rows alone.
// Deviation: one call when the hit box turns on (or the HurtBox comes back); the build makes one per begin of an
// overlap of the hit box with each of the actor's own components and with the owner's.
fn self_hit(w: &mut World, a: ProjectileId) {
    let ar = ActorRef::Projectile(a);
    if let Some(begin) = hooks_of(w, ar).and_then(|h| h.hitbox_begin_overlap) {
        begin(w, a, ar);
        if dead(w, ar) {
            return;
        }
    }
    projectile_hits_projectile(w, a, a, false);
}

/// `projectileHitsProjectile`: pfgDoDamage with a projectile as HitActor: the
/// entry rows, the reflect pop, HitEachActorOnlyOnce, the ally rows, then the clank (both BPI_CanProjectileClank and
/// different teams, or the reciprocal call) or DestroyProjectiles, DamageAndLaunch on another team, HitNonPlayer on
/// the dealer, and BPI_SetIsProjectileClanking(false) at the end. The clank's reciprocal call takes the clank rows.
pub fn projectile_hits_projectile(w: &mut World, a: ProjectileId, b: ProjectileId, is_clank: bool) {
    let (ar, br) = (ActorRef::Projectile(a), ActorRef::Projectile(b));
    let rows = if is_clank {
        let p = w.actors.projectiles.get(a).expect("a live projectile");
        prj::clank_rows(w, p)
            .iter()
            .filter_map(Value::as_obj)
            .map(|r| OnHitBuffRow::from_props(&w.names, r))
            .collect::<Vec<_>>()
    } else {
        class_rows(w, ar)
    };
    if !rows.is_empty()
        && let Some(o) = owner(w, ar)
    {
        hits::hit_buffs_call(w, o, &rows);
        if dead(w, ar) || dead(w, br) {
            return;
        }
    }
    if spawned_refuses(w, ar, br) {
        return;
    }
    let packet = if is_clank { clank_hit(w, ar) } else { own_hit(w, ar) };
    if packet.hit_each_actor_only_once.unwrap_or(false) && seen_once(w, ar, br) {
        return;
    }
    if !rows.is_empty() {
        hit_buffs_ally_actor(w, ar, br, &rows);
        if dead(w, ar) || dead(w, br) {
            return;
        }
    }
    let hit = dealt_pass(w, ar, br, &packet);
    let enemy = owner_team(w, ar) != owner_team(w, br);
    let mut dmg = hit.damage.unwrap_or(0.0);
    // @15158 (spawnedReflects): with ReflectProjectiles the clank test (@20004) is not reached.
    let reflects = hit.reflect_projectiles.unwrap_or(false) && (is_clank || victim_is_projectile(w, br));
    if reflects && spawned_reflects(w, ar, &hit, br) {
        return;
    }
    let a_type = attack_type(w, ar);
    if !reflects && ((can_clank(w, ar) && can_clank(w, br) && enemy) || is_clank) {
        if !is_clank {
            set_clanking(w, ar, true);
            set_clanking(w, br, true);
            projectile_hits_projectile(w, b, a, true);
        }
        if zeroed(a_type, attack_type(w, br)) {
            dmg = 0.0;
        }
    } else if !hit.destroy_projectiles.unwrap_or(false) {
        // Ubergraph 19122: only the victim's HitByAlly / HitByEnemy.
        let d = hits::dealer(w, ar);
        let hd = hits::projectile_hit_dir(w, &d, &hit, Some(victim_x(w, br)));
        hits::hit_by(w, b, !enemy, hd.x, hd.y, hd.bkb, a_type, Some(ar));
        return;
    }
    // DamageAndLaunch: a hit only when the victim's CanBeHitBy passes; HitByEnemy after the damage, HitByAlly (@8047)
    // for the dealer's own team and for a DamageDisabled victim of any team.
    let d = hits::dealer(w, ar);
    let hd = hits::projectile_hit_dir(w, &d, &hit, Some(victim_x(w, br)));
    if charged(w, ar) {
        dmg *= elem::charge_dmg(w, ar); // DamageAndLaunch @7835: no Comp_Armor
    }
    let can = spawned_can_be_hit_by(w, ar, br);
    if can {
        if !dead(w, br) && projectile_any_damage(w, b, dmg, a_type, ar) {
            spawning::destroy_projectile(w, b, false);
        }
        if !dead(w, br)
            && !rows.is_empty()
            && let Some(o) = owner(w, ar)
        {
            let mult = elem::charge_buff(w, ar);
            elem::hit_actor_rows_typed(w, o, br, &rows, mult); // @18293-19121
        }
        if !dead(w, ar) {
            let bt = attack_type(w, br);
            if spawned_hit_non_player(w, ar, crate::real(bt), br) {
                spawning::destroy_projectile(w, a, false);
            }
        }
        hits::hit_anything(w, ar, br);
    }
    if w.actors.projectiles.contains(b) {
        hits::hit_by(w, b, !can, hd.x, hd.y, hd.bkb, a_type, Some(ar));
    }
    force_knockback_tail(w, br, can, &hit, hd, a_type, ar);
    // NotifyAllyOfHit @21344: nothing to give (`damage_spawned_actor`).
    set_clanking(w, ar, false);
    if queue_destroy(w, ar) && !dead(w, ar) {
        spawning::destroy_projectile(w, a, false);
    }
}

/// Comp_Actor_Destructible.IsVulnerableTo(TeamIndex): not invulnerable, damage not disabled, another team (as
/// `chars/tom_and_jerry/kicked.rs` reads it). A class without the component is passed over (@531).
// Unverified: IsVulnerableTo @0-233's FriendlyFire and a non-fighter's IsInvulnerable? are not read here.
pub(crate) fn is_vulnerable_to(w: &World, o: ActorRef, team: i32) -> bool {
    match o {
        ActorRef::Fighter(f) => {
            !crate::fighter::invulnerable(w, f)
                && !w.fighter(f).invuln.damage_disabled
                && w.fighter(f).team.team != team
        }
        ActorRef::Projectile(b) => {
            let Some(bp) = w.actors.projectiles.get(b) else { return false };
            let Some(c) = comp(w, o, "Comp_Actor_Destructible") else { return false };
            let off = bp.damage_disabled.unwrap_or_else(|| hits::truthy(c, "DamageDisabled"));
            !off && owner_team(w, o) != team
        }
        ActorRef::Item(i) => {
            let Some(c) = comp(w, o, "Comp_Actor_Destructible") else { return false };
            !hits::truthy(c, "DamageDisabled") && crate::actors::item::item_destructible_team(w, i) != team
        }
        ActorRef::Actor(a) => {
            let Some(wa) = w.actors.actors.get(a) else { return false };
            let (cls, own) = (wa.cls, wa.owner);
            let hooked = world_hooks(w, cls).and_then(|k| k.destructible).and_then(|f| f(w, a));
            let Some(c) = hooked.or_else(|| comp(w, o, "Comp_Actor_Destructible")) else { return false };
            !hits::truthy(c, "DamageDisabled") && w.fighter(own).team.team != team
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::zeroed;

    #[test]
    fn clank_zeroes_by_attack_type() {
        // Normal against Heavy / Radial / Transcendent, Heavy against Radial / Transcendent, Light against all but
        // Light.
        assert!(zeroed(0, 1) && zeroed(0, 3) && zeroed(0, 4) && !zeroed(0, 0) && !zeroed(0, 2));
        assert!(zeroed(1, 3) && zeroed(1, 4) && !zeroed(1, 0) && !zeroed(1, 1));
        assert!(zeroed(2, 0) && zeroed(2, 1) && !zeroed(2, 2));
        assert!(!zeroed(3, 0) && !zeroed(4, 1));
    }
}
