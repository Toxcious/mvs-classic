//! World actors (`World.actors`; the `SimActor`, `DealerActor` and `PlatformActor` classes): what a fighter
//! spawns that is neither a projectile nor an item, a shockwave, a shield, a zone, an aimer. [`WorldActor`] holds the
//! `SimActor` fields; [`WorldActorState`] the class's own, a variant per class.

use super::WorldActorClass;
use super::projectile::LifetimePause;
use super::smoke_bomb::{SmokeGrenade, SmokeZone};
use crate::Real;
use crate::State;
use crate::chars::shaggy::{Shockwave, SnackBox};
use crate::combat::{ChargeVals, ElemState};
use crate::fighter::FighterId;
use crate::names::Name;

/// A spawned world actor: `SimActor`'s fields.
#[derive(State, Debug)]
pub struct WorldActor {
    pub cls: Name,
    pub class: WorldActorClass,
    /// `id`: the world's count of spawned actors at the spawn.
    pub seq: i32,
    pub dead: bool,
    pub owner: FighterId,
    /// The owner's id, kept for what reads it after the owner is gone.
    pub owner_id: i32,
    pub charge: Option<ChargeVals>,
    pub elem: Option<ElemState>,
    pub lifetime_pause: Option<LifetimePause>,
    /// The root's scale (`SimActor.sizeScale`), `None` at 1.
    pub size_scale: Option<Real>,
    /// A SpawnedPlatformParent child's parent part (`PlatformActor.plat`, its parts the actor's `solids`).
    pub plat: Option<crate::world::spawned_platform::SpawnedPlatform>,
    /// Its Comp_Pawn_HitBoxHandler dodge variables (`DealerActor.dodge`), `None` until
    /// its first dodge.
    pub dodge: Option<crate::fighter::control::DodgeState>,
    /// A pending DestroySpawnedActor Delay (`doom`): the frames left,
    /// `None` while none was asked.
    pub doom: Option<i32>,
    /// The root's Mobility while not Movable (0 Static, 1 Stationary) and StationaryMobilityCount while above 0
    /// (`combat::holds::MobileRoot`, SpawnedActorParent BPI_SetMobilityToStationary): of the kinds only
    /// Jerry_SpawnedActor's own ProjectileMovement reads it.
    pub mobility: Option<u8>,
    pub stationary_count: Option<i32>,
    pub state: WorldActorState,
}

/// A world actor's class state, a variant per class. A variant without a struct
/// is a class not ported yet; its port gives it one.
#[derive(State, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "one variant per class, held in an arena slot; boxing would move the state off the slot"
)]
pub enum WorldActorState {
    // actors/
    ProjectileBarrier(crate::actors::barrier::ProjectileBarrier),
    ShieldActor(crate::actors::shield_actor::ShieldActor),
    /// `EvadeShield extends ShieldActor`.
    EvadeShield(crate::chars::wonder_woman::perks::EvadeShield),
    SmokeGrenade(SmokeGrenade),
    SmokeZone(SmokeZone),
    // chars/shaggy
    Shockwave(Shockwave),
    SnackBox(SnackBox),
    // chars/morty
    ScrubShield(crate::chars::morty::scrub::ScrubShield),
    // chars/iron-giant
    /// C017_AntenaBolts_C.
    AntenaBolts(crate::chars::iron_giant::actor_state::AntenaBolts),
    /// C017_CannonBall_C.
    CannonBall(crate::chars::iron_giant::actor_state::CannonBall),
    /// C017_CannonAim_C.
    TurretAim(crate::chars::iron_giant::actor_state::TurretAim),
    /// C017_ShockwaveActor_C.
    StompWave(crate::chars::iron_giant::actor_state::StompWave),
    /// C017_DownThrusterAttack_C.
    DownThruster(crate::chars::iron_giant::actor_state::DownThruster),
    /// C017_CannonShotAim_C.
    CannonAim(crate::chars::iron_giant::cannon_aim::CannonAim),
    /// C017_EnergyDiscAimer_C.
    DiscAim(crate::chars::iron_giant::cannon_aim::DiscAim),
    /// C017_PlasmaAimer_C.
    PlasmaAim(crate::chars::iron_giant::cannon_aim::PlasmaAim),
    /// C017_CannonExplotion_C.
    CannonBurst(crate::chars::iron_giant::cannon_aim::CannonBurst),
    /// C017_Pilot_Reticle_C.
    PilotReticle(crate::chars::iron_giant::turret_actors::PilotReticle),
    /// C017_Ally_Turret_Bomb_C.
    TurretBomb(crate::chars::iron_giant::turret_actors::TurretBomb),
    /// SpawnedPlatform_C017_Wall_C.
    WallPlatform(crate::chars::iron_giant::wall::WallPlatform),
    // chars/wonder-woman
    BracerShield(crate::chars::wonder_woman::actors::BracerShield),
    // chars/harley
    /// Harley's `Bomb`.
    HarleyBomb(crate::chars::harley::bomb::Bomb),
    JackBox(crate::chars::harley::actors::JackBox),
    // chars/superman
    /// SupermanFlightHit_C and SupermanGrabListener_C.
    FlightActor(crate::chars::superman::flight::FlightActor),
    IceStream(crate::chars::superman::ice::IceStream),
    LaserDamage(crate::chars::superman::laser::LaserDamage),
    /// SupermanGroundLazer_C and SupermanGroundLazerAir_C.
    LaserSweep(crate::chars::superman::laser::LaserSweep),
    /// SupermanAimPunch_C.
    AimMarker(crate::chars::superman::aim_punch::AimMarker),
    // chars/lebron
    DashToBasketball(crate::chars::lebron::later::DashToBasketball),
    // chars/arya
    DisguiseShock(crate::chars::arya::actors::DisguiseShock),
    KillPie(crate::chars::arya::actors::KillPie),
    // chars/black-adam
    Bolt(crate::chars::black_adam::bolt::Bolt),
    /// C021_EarthSmashIn_C.
    SmashIn(crate::chars::black_adam::bolt::SmashIn),
    ForceField(crate::chars::black_adam::field::ForceField),
    /// `PerkForceField extends ForceField`.
    PerkForceField(crate::chars::black_adam::field::PerkForceField),
    GrabHit(crate::chars::black_adam::grab_hit::GrabHit),
    // chars/bugs
    /// ItemDispenser_Bugs_C.
    Dispenser(crate::chars::bugs::dispenser::Dispenser),
    /// BugsBunnyTunnelParent_C.
    Tunnel(crate::chars::bugs::tunnel::Tunnel),
    // chars/rick
    /// C020PortalAimer_C.
    PortalAimer(crate::chars::rick::aimer::PortalAimer),
    /// MeeSeeks_AirFlap_C.
    AirFlap(crate::chars::rick::air_flap::AirFlap),
    /// PolymorphGrenade_C.
    PolymorphGrenade(crate::chars::rick::grenade::PolymorphGrenade),
    /// AutoTeleportPortal_C, VisualOnlyPortal_C and ExitPortalParent_C.
    PortalActor(crate::chars::rick::portal::PortalActor),
    // chars/taz
    /// C015Dogpile_C.
    Dogpile(crate::chars::taz::dogpile::Dogpile),
    // chars/steven
    /// The Steven_ShieldActor classes.
    StevenShield(crate::chars::steven::barrier::StevenShield),
    /// StevenCollisionBlock_C.
    StevenBlock(crate::chars::steven::shield::StevenBlock),
    /// HealOverTimeZoneBP_C.
    HealZone(crate::chars::steven::watermelon::HealZone),
    // chars/garnet
    /// GarnetShockwaveHitBox_C and its Left and Right children.
    ShockwaveHitBox(crate::chars::garnet::shockwave::ShockwaveHitBox),
    /// GarnetElectricZone_C.
    ElectricZone(crate::chars::garnet::zone::ElectricZone),
    // chars/finn
    /// Finn_Dash_AimerExperiment_C.
    FinnAimer(crate::chars::finn::aimer::FinnAimer),
    /// BMO_FallowActor_C.
    BmoActor(crate::chars::finn::bmo::BmoActor),
    /// Finn_HighFive_Shockwave_C.
    HighFiveShockwave(crate::chars::finn::high_five::HighFiveShockwave),
    /// FinnHighFiveListener_C and FinnKickListener_C: Finn's `Listener`.
    FinnListener(crate::chars::finn::high_five::FinnListener),
    /// FinnGemBoostBlast_C.
    GemBoostBlast(crate::chars::finn::perks::GemBoostBlast),
    /// FinnShockwaveActor_C.
    FinnShockwave(crate::chars::finn::shockwave::FinnShockwave),
    /// Finn_Shop_C.
    FinnShop(crate::chars::finn::shop::FinnShop),
    // chars/jake
    /// JakeAimPunchEatListener_C.
    JakeAimPunchEatListener(crate::chars::jake::aim::JakeAimPunchEatListener),
    /// JakeAimer_C.
    JakeAimer(crate::chars::jake::aim::JakeAimer),
    /// JakeEatAlly_Aim_C.
    JakeEatAllyAim(crate::chars::jake::ally_spit::JakeEatAllyAim),
    /// JakeSpitAllyHitBox_C.
    JakeSpitAllyHitBox(crate::chars::jake::ally_spit::JakeSpitAllyHitBox),
    /// JakeHorse_Actor3_C.
    JakeHorse(crate::chars::jake::horse::JakeHorse),
    /// JakeHorsePlatform_C.
    JakeHorsePlatform(crate::chars::jake::horse::JakeHorsePlatform),
    /// JakeHouse_Actor_C.
    JakeHouse(crate::chars::jake::house::JakeHouse),
    /// ProjectileAimer_SpitJake_C.
    SpitAimer(crate::chars::jake::spit::SpitAimer),
    /// JakeSpitCharacterHitBox_C.
    SpitHitBox(crate::chars::jake::spit::SpitHitBox),
    /// JakeStretch_C.
    Stretch(crate::chars::jake::stretch::Stretch),
    // chars/tom-and-jerry
    /// Jerry_SpawnedActor_C and Jerry_SpawnedActor_TempInvincibility_C.
    JerryActor(crate::chars::tom_and_jerry::jerry::JerryActor),
    /// TomAndJerry_SlingShot_Aim_02_C.
    SlingAim(crate::chars::tom_and_jerry::slingshot::SlingAim),
    /// TomAndJerry_NoJerry_SlingShot_Aim_C.
    NoJerryAim(crate::chars::tom_and_jerry::tennis::NoJerryAim),
    /// MouseTrap_SpawnedActor_C.
    MouseTrap(crate::chars::tom_and_jerry::trap::MouseTrap),
    // chars/velma
    /// VelmaHealBeam_C and VelmaChildBeam_C.
    Beam(crate::chars::velma::beam::Beam),
    /// VelmaCarryRun_Actor_C.
    VelmaCarryRun(crate::chars::velma::carry::VelmaCarryRun),
    /// VelmaEvidence_C and VelmaEvidenceNoGrav_C.
    Evidence(crate::chars::velma::evidence::Evidence),
    /// SlowtoStunGrenade_C.
    SlowToStunGrenade(crate::chars::velma::gadgets::SlowToStunGrenade),
    /// VelmaVialAttack_C.
    VelmaVial(crate::chars::velma::gadgets::VelmaVial),
    /// VelmaJenkinsPin_C.
    JenkinsPin(crate::chars::velma::police::JenkinsPin),
    // chars/marvin
    /// C018DisintegrationRay_C and C018DisintegrationRay_Down_C.
    Ray(crate::chars::marvin::ray::Ray),
    /// C018RocketExplosion_C and C018PerkRocketExplosion_C.
    RocketExplosion(crate::chars::marvin::ufo::RocketExplosion),
    /// C018_Platform_C and C018_PerkPlatform_C: the UFO.
    Ufo(crate::chars::marvin::ufo::Ufo),
    // chars/stripe
    /// C023B_DashListener_C: Stripe's `Listener`.
    StripeListener(crate::chars::stripe::dash::StripeListener),
    /// C023B_AllySaw_C.
    AllySaw(crate::chars::stripe::saw::AllySaw),
    // items/
    /// ItemHelper_BananaGuard_C.
    BananaGuard(crate::items::banana_guard::BananaGuard),
    /// BouncePad_SpawnedActor_C.
    BouncePad(crate::items::bounce_pad::BouncePad),
    /// ItemHelper_Nepter_C.
    Nepter(crate::items::nepter::Nepter),
}

/// `s.dead = true` on one of a fighter's `spawnedActors`: the
/// flag only, the actor's end in its list's pass. An actor gone from its arena is left as it is.
pub fn mark_spawned_dead(w: &mut crate::world::World, a: crate::actors::ActorRef) {
    use crate::actors::ActorRef;
    let w = &mut w.actors;
    let dead = match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => {
            unreachable!("the dead flag of a spawned actor")
        }
        ActorRef::Projectile(id) => w.projectiles.get_mut(id).map(|p| &mut p.dead),
        ActorRef::Item(id) => w.items.get_mut(id).map(|p| &mut p.dead),
        ActorRef::Actor(id) => w.actors.get_mut(id).map(|p| &mut p.dead),
        ActorRef::Marker(id) => w.markers.get_mut(id).map(|p| &mut p.dead),
        ActorRef::Ship(id) => w.ships.get_mut(id).map(|p| &mut p.dead),
        ActorRef::Mortar(id) => w.mortars.get_mut(id).map(|p| &mut p.dead),
        ActorRef::AimPunch(id) => w.aim_punches.get_mut(id).map(|p| &mut p.dead),
        ActorRef::Accelerator(id) => w.accelerators.get_mut(id).map(|p| &mut p.dead),
    };
    if let Some(d) = dead {
        *d = true;
    }
}

/// The `attachedTo` getter of a spawned actor's class (Arya's knife, Batman's grapple, the Creature's tether and
/// wrecking ball, Gizmo's music, Harley's bomb, Steven's tether, Velma's mark), `None` for a class without one.
pub fn attached_to(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<FighterId> {
    use crate::actors::ActorRef;
    use crate::actors::projectile::ProjectileExt as X;
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            match &p.ext {
                X::Grapple(_) | X::PerkGrapple(_) => {
                    crate::chars::batman::grapple::grapple_of(p).expect("a grapple hook").target
                }
                X::Knife(_) => crate::chars::arya::knife::attached_to(p),
                X::WreckingBall(b) => b.ally,
                X::TetherProjectile(t) => t.ally,
                X::StevenTether(t) => crate::chars::steven::tether::attached_to(p, t),
                X::Mark(m) => crate::chars::velma::mark::attached_to(p, m),
                X::MusicProjectile(m) => crate::chars::gizmo::music::attached_to(p, m),
                _ => None,
            }
        }
        ActorRef::Actor(id) => {
            let p = w.actors.actors.get(id)?;
            match &p.state {
                WorldActorState::HarleyBomb(b) => crate::chars::harley::bomb::attached_to(w, p, b),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `s.destroyOnConsume ?? !!s.p?.DestroyOnConsume` of a spawned actor: Arya's knife's
/// getter, else the class data's.
pub fn destroy_on_consume(w: &crate::world::World, a: crate::actors::ActorRef) -> bool {
    use crate::actors::ActorRef;
    use crate::actors::projectile::ProjectileExt;
    if let ActorRef::Projectile(id) = a
        && let Some(p) = w.actors.projectiles.get(id)
        && matches!(p.ext, ProjectileExt::Knife(_))
    {
        return crate::chars::arya::knife::destroy_on_consume(p);
    }
    crate::actors::projectile::truthy(actor_p(w, a).get("DestroyOnConsume"))
}

/// `!!s.p?.['DestroyOnCleanse?']` of a spawned actor's class data.
pub fn destroy_on_cleanse(w: &crate::world::World, a: crate::actors::ActorRef) -> bool {
    crate::actors::projectile::truthy(actor_p(w, a).get("DestroyOnCleanse?"))
}

/// A world actor's `dodge` field: `None` for an actor gone from its arena.
pub fn world_actor_dodge(
    w: &mut crate::world::World,
    a: super::ActorId,
) -> Option<&mut Option<crate::fighter::control::DodgeState>> {
    w.actors.actors.get_mut(a).map(|p| &mut p.dodge)
}

/// A world actor's `useDelayedDodgeText`: the handler template's
/// UseDelayedDodgeText on HarleyJackInTheBox and HarleyBomb (the build's only templates with it false), true (the class
/// default) for the rest and for an actor gone.
pub fn world_actor_delayed_dodge_text(w: &crate::world::World, a: super::ActorId) -> bool {
    let Some(p) = w.actors.actors.get(a) else { return true };
    if !matches!(p.state, WorldActorState::HarleyBomb(_) | WorldActorState::JackBox(_)) {
        return true;
    }
    crate::fighter::common::actor_class_def(w, p.owner, p.cls)
        .and_then(|d| d.components.get("Comp_Pawn_HitBoxHandler"))
        .and_then(mvs_data::value::Value::as_obj)
        .and_then(|h| h.get("UseDelayedDodgeText"))
        .and_then(mvs_data::value::Value::as_bool)
        .unwrap_or(true)
}

/// The root's place and the dead flag of any spawned actor (what a homing target, a follower or a reflect reads
/// of a `SpawnedRoot`): `None` for a fighter, a stage actor, a buff, and a world actor of a class without a place
/// ([`class_has_place`]: the class has no `x`, which its readers take as no place, `?? 0`, or a NaN
/// that meets nothing). An actor no longer in its arena reads as dead at (0, 0): the place of a
/// removed one is not kept here. Unverified: the place the build reads from a destroyed actor.
pub fn actor_place(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<(Real, Real, bool)> {
    use crate::actors::ActorRef;
    let gone = Some((0.0, 0.0, true));
    let w_ = w;
    let w = &w.actors;
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
        ActorRef::Projectile(id) => w.projectiles.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::Item(id) => w.items.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::Marker(id) => w.markers.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::Ship(id) => w.ships.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::Mortar(id) => w.mortars.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::AimPunch(id) => w.aim_punches.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::Accelerator(id) => w.accelerators.get(id).map_or(gone, |p| Some((p.x, p.y, p.dead))),
        ActorRef::Actor(id) => w.actors.get(id).map_or(gone, |p| world_actor_place(w_, p)),
    }
}

/// The dead flag of a spawned actor (`(a as SimActor).dead`) whatever its class: `None` for a fighter, a stage actor
/// or a buff; an actor no longer in its arena is dead. [`actor_place`]'s flag without the place, for a world actor of
/// a class without one.
pub fn spawned_dead(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<bool> {
    use crate::actors::ActorRef;
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
        ActorRef::Actor(id) => Some(w.actors.actors.get(id).is_none_or(|p| p.dead)),
        a => actor_place(w, a).map(|(_, _, dead)| dead),
    }
}

/// The place and the dead flag of a world actor whose class has a place of its own (`typeof o.x ===
/// 'number'`): `None` for one gone from its arena or of a class without one.
pub fn world_actor_place_of(w: &crate::world::World, id: crate::actors::ActorId) -> Option<(Real, Real, bool)> {
    world_actor_place(w, w.actors.actors.get(id)?)
}

fn world_actor_place(w: &crate::world::World, p: &WorldActor) -> Option<(Real, Real, bool)> {
    class_place(w, p).map(|(x, y)| (x, y, p.dead))
}

/// A world actor's place (`x`, `y`), by its class's state: every class is named, `None` for the
/// classes without one ([`class_has_place`], which this agrees with).
fn class_place(w: &crate::world::World, p: &WorldActor) -> Option<(Real, Real)> {
    use WorldActorState as S;
    let at = match &p.state {
        // the place in the class's state
        S::Shockwave(s) => Some((s.x, s.y)),
        S::LaserDamage(s) => Some((s.x, s.y)),
        S::SnackBox(s) => Some((s.x, s.y)),
        S::SmokeGrenade(s) => Some((s.x, s.y)),
        S::SmokeZone(s) => Some((s.x, s.y)),
        S::ProjectileBarrier(s) => Some((s.x, s.y)),
        S::DisguiseShock(s) => Some((s.x, s.y)),
        S::ForceField(_) | S::PerkForceField(_) => {
            crate::chars::black_adam::field::dealer_parts(&p.state).map(|(x, y, _, _)| (x, y))
        }
        S::Bolt(s) => Some((s.x, s.y)),
        S::KillPie(s) => Some((s.x, s.y)),
        S::AimMarker(s) => Some((s.x, s.y)),
        S::CannonBurst(s) => Some((s.x, s.y)),
        // attached: where its owner is
        S::FlightActor(_) | S::GrabHit(_) => Some((w.fighter(p.owner).body.x, crate::fighter::actor_y(w, p.owner))),
        // the place its class's module answers
        S::ShieldActor(s) => crate::actors::shield_actor::place(w, p, s),
        S::BouncePad(s) => crate::items::bounce_pad::place(w, p, s),
        S::EvadeShield(s) => crate::chars::wonder_woman::perks::evade_place(w, p, s),
        S::BracerShield(s) => crate::chars::wonder_woman::actors::bracer_place(w, p, s),
        S::ScrubShield(s) => crate::chars::morty::scrub::place(w, p, s),
        S::HarleyBomb(s) => crate::chars::harley::bomb::place(w, p, s),
        S::JackBox(s) => crate::chars::harley::actors::jack_place(w, p, s),
        S::AntenaBolts(s) => crate::chars::iron_giant::actor_state::antena_place(w, p, s),
        S::CannonBall(s) => crate::chars::iron_giant::actor_state::ball_place(w, p, s),
        S::StompWave(s) => crate::chars::iron_giant::actor_state::stomp_place(w, p, s),
        S::DownThruster(s) => crate::chars::iron_giant::actor_state::thruster_place(w, p, s),
        S::CannonAim(s) => crate::chars::iron_giant::cannon_aim::cannon_aim_place(w, p, s),
        S::PilotReticle(s) => crate::chars::iron_giant::turret_actors::reticle_place(w, p, s),
        S::TurretBomb(s) => crate::chars::iron_giant::turret_actors::bomb_place(w, p, s),
        S::WallPlatform(s) => crate::chars::iron_giant::wall::place(w, p, s),
        S::Dogpile(s) => crate::chars::taz::dogpile::place(w, p, s),
        S::StevenShield(s) => crate::chars::steven::barrier::place(w, p, s),
        S::HealZone(s) => crate::chars::steven::watermelon::heal_zone_place(w, p, s),
        S::ShockwaveHitBox(s) => crate::chars::garnet::shockwave::place(w, p, s),
        S::ElectricZone(s) => crate::chars::garnet::zone::place(w, p, s),
        S::Tunnel(s) => crate::chars::bugs::tunnel::place(w, p, s),
        S::PortalAimer(s) => crate::chars::rick::aimer::place(w, p, s),
        S::AirFlap(s) => crate::chars::rick::air_flap::place(w, p, s),
        S::PolymorphGrenade(s) => crate::chars::rick::grenade::place(w, p, s),
        S::PortalActor(s) => crate::chars::rick::portal::place(w, p, s),
        S::FinnAimer(s) => crate::chars::finn::aimer::place(w, p, s),
        S::BmoActor(s) => crate::chars::finn::bmo::place(w, p, s),
        S::HighFiveShockwave(s) => crate::chars::finn::high_five::place(w, p, s),
        S::GemBoostBlast(s) => crate::chars::finn::perks::place(w, p, s),
        S::FinnShockwave(s) => crate::chars::finn::shockwave::place(w, p, s),
        S::FinnShop(s) => crate::chars::finn::shop::place(w, p, s),
        S::Beam(s) => crate::chars::velma::beam::place(w, p, s),
        S::VelmaCarryRun(s) => crate::chars::velma::carry::place(w, p, s),
        S::Evidence(s) => crate::chars::velma::evidence::place(w, p, s),
        S::SlowToStunGrenade(s) => crate::chars::velma::gadgets::grenade_place(w, p, s),
        S::VelmaVial(s) => crate::chars::velma::gadgets::vial_place(w, p, s),
        S::JenkinsPin(s) => crate::chars::velma::police::place(w, p, s),
        S::Ray(s) => crate::chars::marvin::ray::place(w, p, s),
        S::RocketExplosion(s) => crate::chars::marvin::ufo::rocket_place(w, p, s),
        S::Ufo(s) => crate::chars::marvin::ufo::place(w, p, s),
        S::AllySaw(s) => crate::chars::stripe::saw::place(w, p, s),
        S::JerryActor(s) => crate::chars::tom_and_jerry::jerry::place(w, p, s),
        S::SlingAim(s) => crate::chars::tom_and_jerry::slingshot::place(w, p, s),
        S::NoJerryAim(s) => crate::chars::tom_and_jerry::tennis::aim_place(w, p, s),
        S::MouseTrap(s) => crate::chars::tom_and_jerry::trap::place(w, p, s),
        S::Nepter(s) => crate::items::nepter::place(w, p, s),
        S::JakeAimer(s) => crate::chars::jake::aim::aimer_place(w, p, s),
        S::JakeEatAllyAim(s) => crate::chars::jake::ally_spit::aim_place(w, p, s),
        S::JakeSpitAllyHitBox(s) => crate::chars::jake::ally_spit::hit_box_place(w, p, s),
        S::JakeHorse(s) => crate::chars::jake::horse::horse_place(w, p, s),
        S::JakeHorsePlatform(s) => crate::chars::jake::horse::platform_place(w, p, s),
        S::JakeHouse(s) => crate::chars::jake::house::place(w, p, s),
        S::SpitAimer(s) => crate::chars::jake::spit::aimer_place(w, p, s),
        S::SpitHitBox(s) => crate::chars::jake::spit::hit_box_place(w, p, s),
        S::Stretch(s) => crate::chars::jake::stretch::place(w, p, s),
        S::BananaGuard(s) => crate::items::banana_guard::place(w, p, s),
        // No place: the class has no `x`, `y`. None of these is a dealer or a victim (no hit packet, no hurt
        // or pawn body, no Comp_Actor_Destructible, no body a hit body meets: `place_tests`), so no hit reads a place
        // of theirs; a reader that is no hit takes the missing place as none ([`actor_place`]).
        // - Iron Giant's aimers (TurretAim, PlasmaAim, DiscAim):
        //   an aim vector in the owner's frame; their shots are the owner's CompSpawnActor.
        S::TurretAim(_) | S::DiscAim(_) | S::PlasmaAim(_) => None,
        // - Superman's IceBreathStream (a particle's pitch) and ground laser sweep
        //   (LaserSweep: the beam's ends; its LaserDamage child deals the hits, at a place of its own).
        S::IceStream(_) | S::LaserSweep(_) => None,
        // - LeBron's DashToBasketball (moves its owner to a ball) and Black Adam's
        //   EarthSmashIn (gives the enemies their pillar buffs at its spawn).
        S::DashToBasketball(_) | S::SmashIn(_) => None,
        // - Steven's collision block (StevenBlock): its Box follows the owner and is met as
        //   a Platform body (the box's own place), never its root.
        S::StevenBlock(_) => None,
        // - the listeners, whose graphs act through their binds alone (Finn's and Stripe's listeners,
        //   JakeAimPunchEatListener).
        S::FinnListener(_) | S::StripeListener(_) | S::JakeAimPunchEatListener(_) => None,
        // - Bugs's ItemDispenser: its items spawn at the owner.
        S::Dispenser(_) => None,
    };
    debug_assert_eq!(
        at.is_some(),
        class_has_place(p.class),
        "class_has_place disagrees with the state of {:?}",
        p.class
    );
    at
}

/// Whether a world actor class has a place ([`class_place`], by the class rather than its state): every class is
/// named. A class without one is never a dealer or a victim (`place_tests` holds this against the registry).
pub const fn class_has_place(c: super::WorldActorClass) -> bool {
    use super::WorldActorClass as C;
    match c {
        // the classes without a place, as [`class_place`] groups them
        C::C017CannonAim | C::C017EnergyDiscAimer | C::C017PlasmaAimer => false,
        C::SupermanIceBreathStream | C::SupermanGroundLazer | C::SupermanGroundLazerAir => false,
        C::DashToBasketball | C::C021EarthSmashIn => false,
        C::StevenCollisionBlock => false,
        C::FinnKickListener | C::FinnHighFiveListener | C::C023BDashListener | C::JakeAimPunchEatListener => false,
        C::ItemDispenserBugs => false,
        // Never a world actor: entered with no `spawn` (C016AimPunchSlam: dormant; the
        // SpawnedPlatformParent_C parent's own entry, which its children name) or not entered (the tunnel buff's
        // OnJumpPressed listener, `chars/bugs/tunnel.rs` Deviation (n)).
        C::C016AimPunchSlam | C::SpawnedPlatformParent | C::BuffTunnelingOnJumpPressed => false,
        // the rest: a place in its state, its module's or its owner's
        C::SmokeGrenade
        | C::SmokeGrenadeOcclusionBP
        | C::ItemHelperBananaGuard
        | C::ItemHelperNepter
        | C::BouncePadSpawnedActor
        | C::ShaggyShockwaveActor
        | C::ScoobySnackBox
        | C::C019ScrubShield
        | C::C017AntenaBolts
        | C::C017CannonBall
        | C::C017ShockwaveActor
        | C::C017DownThrusterAttack
        | C::C017PilotReticle
        | C::C017AllyTurretBomb
        | C::C017CannonShotAim
        | C::C017CannonExplotion
        | C::SpawnedPlatformC017Wall
        | C::WWEvadeProjectileBlock
        | C::WWShieldActor
        | C::WWShieldActorUSpecial
        | C::WWBracerShield
        | C::HarleyBomb
        | C::HarleyJackInTheBox
        | C::SupermanFlightHit
        | C::SupermanGrabListener
        | C::SupermanGroundLazerDamage
        | C::SupermanAimPunch
        | C::C016ProjectileBarrier
        | C::C016DunkShockwave
        | C::AryaDisguiseShock
        | C::AryaKillPie
        | C::C021ForceField
        | C::C021ForceFieldPerk
        | C::C021GrabHit
        | C::C021Bolt
        | C::BugsBunnyTunnelParent
        | C::MeeSeeksAirFlap
        | C::PolymorphGrenade
        | C::AutoTeleportPortal
        | C::VisualOnlyPortal
        | C::ExitPortalParent
        | C::C020PortalAimer
        | C::C015Dogpile
        | C::StevenShieldActor
        | C::StevenShieldActorUp
        | C::StevenShieldActorUSpecialPlatform
        | C::HealOverTimeZoneBP
        | C::GarnetShockwaveHitBox
        | C::GarnetShockwaveHitBoxLeft
        | C::GarnetShockwaveHitBoxRight
        | C::GarnetElectricZone
        | C::FinnDashAimerExperiment
        | C::FinnGemBoostBlast
        | C::FinnShop
        | C::BMOFallowActor
        | C::FinnHighFiveShockwave
        | C::FinnShockwaveActor
        | C::JakeStretch
        | C::JakeHouseActor
        | C::JakeHorseActor3
        | C::JakeHorsePlatform
        | C::JakeEatAllyAim
        | C::JakeSpitAllyHitBox
        | C::ProjectileAimerSpitJake
        | C::JakeSpitCharacterHitBox
        | C::JakeAimer
        | C::TomAndJerryNoJerrySlingShotAim
        | C::JerrySpawnedActor
        | C::JerrySpawnedActorTempInvincibility
        | C::TomAndJerrySlingShotAim02
        | C::MouseTrapSpawnedActor
        | C::VelmaProjectileShieldActor
        | C::VelmaEvidence
        | C::VelmaEvidenceNoGrav
        | C::VelmaJenkinsPin
        | C::VelmaHealBeam
        | C::VelmaChildBeam
        | C::VelmaCarryRunActor
        | C::VelmaVialAttack
        | C::SlowtoStunGrenade
        | C::C018Platform
        | C::C018RocketExplosion
        | C::C018DisintegrationRay
        | C::C018DisintegrationRayDown
        | C::C018PerkPlatform
        | C::C018PerkRocketExplosion
        | C::C023BAllySaw
        | C::ProjectileBarrier
        | C::C023AProjectileBarrier => true,
    }
}

/// The Blueprint class (`cls`) of any spawned actor; `None` for a fighter, a stage actor or one no longer in its
/// arena.
pub fn actor_cls(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<Name> {
    use crate::actors::ActorRef;
    let (w_, w) = (w, &w.actors);
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) => None,
        ActorRef::Buff(b) => crate::combat::buffs::buff(w_, b).map(|x| x.cls),
        ActorRef::Projectile(id) => w.projectiles.get(id).map(|p| p.cls),
        ActorRef::Item(id) => w.items.get(id).map(|p| p.cls),
        ActorRef::Actor(id) => w.actors.get(id).map(|p| p.cls),
        ActorRef::Marker(id) => w.markers.get(id).map(|p| p.cls),
        ActorRef::Ship(id) => w.ships.get(id).map(|p| p.cls),
        ActorRef::Mortar(id) => w.mortars.get(id).map(|p| p.cls),
        ActorRef::AimPunch(id) => w.aim_punches.get(id).map(|p| p.cls),
        ActorRef::Accelerator(id) => w.accelerators.get(id).map(|p| p.cls),
    }
}

/// The class's `ACTORS` entry (`ACTORS.get(a.cls)`).
pub fn actor_entry(
    w: &crate::world::World,
    a: crate::actors::ActorRef,
) -> Option<&'static crate::registry::entries::ActorEntry> {
    let cls = actor_cls(w, a)?;
    let r: &'static crate::registry::Registry = w.registry;
    r.actors.get(w.names.str(cls))
}

/// The `owner` of any spawned actor (`None`: a world-owned one, a fighter, a stage actor, one no longer in its
/// arena).
pub fn actor_owner(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<FighterId> {
    use crate::actors::ActorRef;
    let (w_, w) = (w, &w.actors);
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) => None,
        ActorRef::Buff(b) => crate::combat::buffs::buff(w_, b).and_then(|x| x.ext.actor_owner()),
        ActorRef::Projectile(id) => w.projectiles.get(id).and_then(|p| p.owner),
        ActorRef::Item(id) => w.items.get(id).and_then(|p| p.owner),
        ActorRef::Actor(id) => w.actors.get(id).map(|p| p.owner),
        ActorRef::Marker(id) => w.markers.get(id).map(|p| p.owner),
        ActorRef::Ship(id) => w.ships.get(id).map(|p| p.owner),
        ActorRef::Mortar(id) => w.mortars.get(id).map(|p| p.owner),
        ActorRef::AimPunch(id) => w.aim_punches.get(id).map(|p| p.owner),
        ActorRef::Accelerator(id) => w.accelerators.get(id).map(|p| p.owner),
    }
}

/// A warning of the port's dev console (`warnOnce`'s text): not state, not shown in a match.
pub fn warn(msg: &str) {
    let _ = msg;
}

/// The class data (`p`, the actor CDO) of any spawned actor, read through its owner's data (a projectile: its owner
/// or the fighter it was looked up through); empty for a fighter, a stage actor, one with no owner or one gone.
pub fn actor_p(w: &crate::world::World, a: crate::actors::ActorRef) -> &mvs_data::value::Props {
    use crate::actors::ActorRef;
    static NONE: std::sync::LazyLock<mvs_data::value::Props> = std::sync::LazyLock::new(Default::default);
    if let ActorRef::Projectile(id) = a {
        return w.actors.projectiles.get(id).map_or(&NONE, |p| crate::actors::projectile::class_p(w, p));
    }
    match (actor_owner(w, a), actor_cls(w, a)) {
        (Some(own), Some(cls)) => crate::fighter::common::actor_class_def(w, own, cls).map_or(&NONE, |d| &d.p),
        _ => &NONE,
    }
}

/// The Comp_Pawn_ChargeAttack values an actor holds (`ChargeHolder.charge`): `None` for a kind without them.
pub fn actor_charge(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<&Option<crate::combat::ChargeVals>> {
    use crate::actors::ActorRef;
    let w = &w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get(id).map(|p| &p.charge),
        ActorRef::Item(id) => w.items.get(id).map(|p| &p.charge),
        ActorRef::Actor(id) => w.actors.get(id).map(|p| &p.charge),
        ActorRef::Marker(id) => w.markers.get(id).map(|p| &p.charge),
        ActorRef::Mortar(id) => w.mortars.get(id).map(|p| &p.charge),
        _ => None,
    }
}

/// `actor_charge`, to write.
pub fn actor_charge_mut(
    w: &mut crate::world::World,
    a: crate::actors::ActorRef,
) -> Option<&mut Option<crate::combat::ChargeVals>> {
    use crate::actors::ActorRef;
    let w = &mut w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get_mut(id).map(|p| &mut p.charge),
        ActorRef::Item(id) => w.items.get_mut(id).map(|p| &mut p.charge),
        ActorRef::Actor(id) => w.actors.get_mut(id).map(|p| &mut p.charge),
        ActorRef::Marker(id) => w.markers.get_mut(id).map(|p| &mut p.charge),
        ActorRef::Mortar(id) => w.mortars.get_mut(id).map(|p| &mut p.charge),
        _ => None,
    }
}

/// The Comp_State_Manager buff list an actor holds (`ElemHost.elem`): `None` for a kind without one (a fighter, a
/// stage actor, a ship, an aim punch, an accelerator) or one no longer in its arena.
pub fn actor_elem(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<&Option<crate::combat::ElemState>> {
    use crate::actors::ActorRef;
    let w = &w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get(id).map(|p| &p.elem),
        ActorRef::Item(id) => w.items.get(id).map(|p| &p.elem),
        ActorRef::Actor(id) => w.actors.get(id).map(|p| &p.elem),
        ActorRef::Marker(id) => w.markers.get(id).map(|p| &p.elem),
        ActorRef::Mortar(id) => w.mortars.get(id).map(|p| &p.elem),
        _ => None,
    }
}

/// `actor_elem`, to write.
pub fn actor_elem_mut(
    w: &mut crate::world::World,
    a: crate::actors::ActorRef,
) -> Option<&mut Option<crate::combat::ElemState>> {
    use crate::actors::ActorRef;
    let w = &mut w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get_mut(id).map(|p| &mut p.elem),
        ActorRef::Item(id) => w.items.get_mut(id).map(|p| &mut p.elem),
        ActorRef::Actor(id) => w.actors.get_mut(id).map(|p| &mut p.elem),
        ActorRef::Marker(id) => w.markers.get_mut(id).map(|p| &mut p.elem),
        ActorRef::Mortar(id) => w.mortars.get_mut(id).map(|p| &mut p.elem),
        _ => None,
    }
}

/// The dead flag of any spawned actor (`(a as { dead?: boolean }).dead`): true for one no longer in its arena,
/// false for a fighter or a stage actor.
pub fn actor_dead(w: &crate::world::World, a: crate::actors::ActorRef) -> bool {
    use crate::actors::ActorRef;
    let (w_, w) = (w, &w.actors);
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) => false,
        ActorRef::Buff(b) => crate::combat::buffs::buff(w_, b).is_none(),
        ActorRef::Projectile(id) => w.projectiles.get(id).is_none_or(|p| p.dead),
        ActorRef::Item(id) => w.items.get(id).is_none_or(|p| p.dead),
        ActorRef::Actor(id) => w.actors.get(id).is_none_or(|p| p.dead),
        ActorRef::Marker(id) => w.markers.get(id).is_none_or(|p| p.dead),
        ActorRef::Ship(id) => w.ships.get(id).is_none_or(|p| p.dead),
        ActorRef::Mortar(id) => w.mortars.get(id).is_none_or(|p| p.dead),
        ActorRef::AimPunch(id) => w.aim_punches.get(id).is_none_or(|p| p.dead),
        ActorRef::Accelerator(id) => w.accelerators.get(id).is_none_or(|p| p.dead),
    }
}

/// The attack id of a SpawnedActorParent the port deals with (`attackId`): a projectile, an item, a world actor of
/// a dealer kind, a marker, a mortar aim; `None` for another kind.
pub fn actor_attack_id(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<i32> {
    use crate::actors::ActorRef;
    let (w_, w) = (w, &w.actors);
    match a {
        ActorRef::Projectile(id) => w.projectiles.get(id).map(|p| p.attack_id),
        ActorRef::Item(id) => w.items.get(id).map(|p| p.attack_id),
        ActorRef::Marker(id) => w.markers.get(id).map(|p| p.attack_id),
        ActorRef::Mortar(id) => w.mortars.get(id).map(|p| p.attack_id),
        ActorRef::Actor(id) => {
            let p = w.actors.get(id)?;
            match &p.state {
                WorldActorState::Shockwave(s) => Some(s.attack_id),
                WorldActorState::FlightActor(s) => Some(s.attack_id),
                WorldActorState::GrabHit(s) => Some(s.attack_id),
                WorldActorState::LaserDamage(s) => Some(s.attack_id),
                WorldActorState::DisguiseShock(s) => Some(s.attack_id),
                WorldActorState::ForceField(_) | WorldActorState::PerkForceField(_) => {
                    crate::chars::black_adam::field::dealer_parts(&p.state).map(|(.., id)| id)
                }
                WorldActorState::Bolt(s) => Some(s.attack_id),
                WorldActorState::HarleyBomb(s) => Some(s.attack_id),
                WorldActorState::JackBox(s) => Some(s.attack_id),
                WorldActorState::CannonBurst(s) => Some(s.attack_id),
                WorldActorState::AntenaBolts(s) => {
                    Some(crate::chars::iron_giant::actor_state::antena_dealer_parts(w_, p, s).3)
                }
                WorldActorState::StompWave(s) => {
                    Some(crate::chars::iron_giant::actor_state::stomp_dealer_parts(w_, p, s).3)
                }
                WorldActorState::DownThruster(s) => {
                    Some(crate::chars::iron_giant::actor_state::thruster_dealer_parts(w_, p, s).3)
                }
                WorldActorState::TurretBomb(s) => {
                    Some(crate::chars::iron_giant::turret_actors::bomb_dealer_parts(w_, p, s).3)
                }
                WorldActorState::Dogpile(s) => Some(crate::chars::taz::dogpile::dealer_parts(w_, p, s).3),
                WorldActorState::ShockwaveHitBox(s) => Some(crate::chars::garnet::shockwave::dealer_parts(w_, p, s).3),
                WorldActorState::ElectricZone(s) => Some(crate::chars::garnet::zone::dealer_parts(w_, p, s).3),
                WorldActorState::AirFlap(s) => Some(crate::chars::rick::air_flap::dealer_parts(w_, p, s).3),
                WorldActorState::PolymorphGrenade(s) => Some(crate::chars::rick::grenade::dealer_parts(w_, p, s).3),
                WorldActorState::PortalActor(s) => Some(crate::chars::rick::portal::dealer_parts(w_, p, s).3),
                WorldActorState::HighFiveShockwave(s) => Some(crate::chars::finn::high_five::dealer_parts(w_, p, s).3),
                WorldActorState::GemBoostBlast(s) => Some(crate::chars::finn::perks::dealer_parts(w_, p, s).3),
                WorldActorState::FinnShockwave(s) => Some(crate::chars::finn::shockwave::dealer_parts(w_, p, s).3),
                WorldActorState::Beam(s) => Some(crate::chars::velma::beam::dealer_parts(w_, p, s).3),
                WorldActorState::VelmaCarryRun(s) => Some(crate::chars::velma::carry::dealer_parts(w_, p, s).3),
                WorldActorState::Ray(s) => Some(crate::chars::marvin::ray::dealer_parts(w_, p, s).3),
                WorldActorState::RocketExplosion(s) => Some(crate::chars::marvin::ufo::rocket_dealer_parts(w_, p, s).3),
                WorldActorState::AllySaw(s) => Some(crate::chars::stripe::saw::dealer_parts(w_, p, s).3),
                WorldActorState::Stretch(s) => Some(crate::chars::jake::stretch::dealer_parts(w_, p, s).3),
                WorldActorState::JakeHouse(s) => Some(crate::chars::jake::house::dealer_parts(w_, p, s).3),
                WorldActorState::JakeHorse(s) => Some(crate::chars::jake::horse::dealer_parts(w_, p, s).3),
                WorldActorState::SpitHitBox(s) => Some(crate::chars::jake::spit::dealer_parts(w_, p, s).3),
                WorldActorState::JakeSpitAllyHitBox(s) => Some(crate::chars::jake::ally_spit::dealer_parts(w_, p, s).3),
                WorldActorState::JerryActor(s) => Some(crate::chars::tom_and_jerry::jerry::dealer_parts(w_, p, s).3),
                WorldActorState::MouseTrap(s) => Some(crate::chars::tom_and_jerry::trap::dealer_parts(w_, p, s).3),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The HitBoxDamage packet of a dealer actor (`hit`), for the kinds `actor_attack_id` names.
pub fn actor_hit(w: &crate::world::World, a: crate::actors::ActorRef) -> Option<&crate::combat::HitPacket> {
    use crate::actors::ActorRef;
    let w = &w.actors;
    match a {
        ActorRef::Projectile(id) => w.projectiles.get(id).map(|p| &p.hit),
        ActorRef::Item(id) => w.items.get(id).map(|p| &p.hit),
        ActorRef::Marker(id) => w.markers.get(id).map(|p| &p.hit),
        ActorRef::Mortar(id) => w.mortars.get(id).map(|p| &p.hit),
        ActorRef::Actor(id) => {
            let p = w.actors.get(id)?;
            match &p.state {
                WorldActorState::Shockwave(s) => Some(&s.hit),
                WorldActorState::LaserDamage(s) => Some(&s.hit),
                WorldActorState::DisguiseShock(s) => Some(&s.hit),
                WorldActorState::HarleyBomb(s) => Some(&s.hit),
                WorldActorState::JackBox(s) => Some(&s.hit),
                WorldActorState::CannonBurst(s) => Some(&s.hit),
                WorldActorState::AntenaBolts(s) => Some(crate::chars::iron_giant::actor_state::antena_hit(s)),
                WorldActorState::StompWave(s) => Some(crate::chars::iron_giant::actor_state::stomp_hit(s)),
                WorldActorState::DownThruster(s) => Some(crate::chars::iron_giant::actor_state::thruster_hit(s)),
                WorldActorState::TurretBomb(s) => Some(crate::chars::iron_giant::turret_actors::bomb_hit(s)),
                WorldActorState::Dogpile(s) => Some(crate::chars::taz::dogpile::hit(s)),
                WorldActorState::ShockwaveHitBox(s) => Some(crate::chars::garnet::shockwave::hit(s)),
                WorldActorState::ElectricZone(s) => Some(crate::chars::garnet::zone::hit(s)),
                WorldActorState::AirFlap(s) => Some(crate::chars::rick::air_flap::hit(s)),
                WorldActorState::PolymorphGrenade(s) => Some(crate::chars::rick::grenade::hit(s)),
                WorldActorState::PortalActor(s) => Some(crate::chars::rick::portal::hit(s)),
                WorldActorState::HighFiveShockwave(s) => Some(crate::chars::finn::high_five::hit(s)),
                WorldActorState::GemBoostBlast(s) => Some(crate::chars::finn::perks::hit(s)),
                WorldActorState::FinnShockwave(s) => Some(crate::chars::finn::shockwave::hit(s)),
                WorldActorState::Beam(s) => Some(crate::chars::velma::beam::hit(s)),
                WorldActorState::VelmaCarryRun(s) => Some(crate::chars::velma::carry::hit(s)),
                WorldActorState::Ray(s) => Some(crate::chars::marvin::ray::hit(s)),
                WorldActorState::RocketExplosion(s) => Some(crate::chars::marvin::ufo::rocket_hit(s)),
                WorldActorState::AllySaw(s) => Some(crate::chars::stripe::saw::hit(s)),
                WorldActorState::Stretch(s) => Some(crate::chars::jake::stretch::hit(s)),
                WorldActorState::JakeHouse(s) => Some(crate::chars::jake::house::hit(s)),
                WorldActorState::JakeHorse(s) => Some(crate::chars::jake::horse::hit(s)),
                WorldActorState::SpitHitBox(s) => Some(crate::chars::jake::spit::hit(s)),
                WorldActorState::JakeSpitAllyHitBox(s) => Some(crate::chars::jake::ally_spit::hit(s)),
                WorldActorState::JerryActor(s) => Some(crate::chars::tom_and_jerry::jerry::hit(s)),
                WorldActorState::MouseTrap(s) => Some(crate::chars::tom_and_jerry::trap::hit(s)),
                WorldActorState::ForceField(_) | WorldActorState::PerkForceField(_) => {
                    crate::chars::black_adam::field::hit_of(&p.state)
                }
                WorldActorState::GrabHit(_) => Some(&crate::chars::black_adam::grab_hit::GRAB_PACKET),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A world actor's own `attackType` where its class keeps one (Velma's beam, reset to 0
/// at its end), `None` where it has none (the class defaults'
/// ProjectileAttackType is read then).
pub fn instance_attack_type(w: &crate::world::World, id: super::ActorId) -> Option<i32> {
    let p = w.actors.actors.get(id)?;
    match &p.state {
        WorldActorState::Beam(s) => Some(crate::chars::velma::beam::attack_type(s)),
        _ => None,
    }
}

/// A world actor's place along sim x (`a.x`, read by faceTowards); `None` for an actor gone
/// from its arena or of a class without a place ([`class_has_place`]).
pub fn world_actor_x(w: &crate::world::World, a: super::ActorId) -> Option<Real> {
    let p = w.actors.actors.get(a)?;
    class_place(w, p).map(|(x, _)| x)
}

/// A world actor's pending DestroySpawnedActor Delay (`SimActor.doom`): the frames
/// left, `None` while none was asked or for an actor gone from its arena.
pub fn actor_doom(w: &crate::world::World, a: crate::actors::ActorId) -> Option<i32> {
    w.actors.actors.get(a).and_then(|p| p.doom)
}

/// The write of `SimActor.doom`.
pub fn set_actor_doom(w: &mut crate::world::World, a: crate::actors::ActorId, frames: Option<i32>) {
    w.actors.actors.get_mut(a).expect("a live world actor").doom = frames;
}

#[cfg(test)]
mod place_tests {
    use super::class_has_place;
    use crate::actors::{ActorClass, WorldActorClass};
    use crate::registry::{ActorHooks, Registry};

    /// Every World.actors class through the place: each entered class is entered as itself, and one a hit can
    /// deal from or land on (a hit packet or hit shapes, a hurt or pawn body, a Comp_Actor_Destructible, a box a
    /// projectile meets, a mesh's hit boxes, bodies a hit body meets, damage taken) has a place.
    #[test]
    fn every_dealer_and_victim_class_has_a_place() {
        let r = Registry::build().expect("the registry");
        let mut entered = 0;
        for c in WorldActorClass::ALL {
            let Some(e) = r.actors.get(c.name()) else {
                assert!(!class_has_place(c), "{c:?} is not entered but is given a place");
                continue;
            };
            entered += 1;
            assert!(matches!(e.class, ActorClass::World(x) if x == c), "{} is entered as {:?}", c.name(), e.class);
            let ActorHooks::World(h) = e.hooks else { panic!("{} is entered with another family's hooks", c.name()) };
            let mut why = Vec::new();
            for (on, what) in [
                (h.hits.is_some(), "hits"),
                (h.hit_shapes.is_some(), "hit_shapes"),
                (h.pawn_body.is_some(), "pawn_body"),
                (h.hurt_box.is_some(), "hurt_box"),
                (h.destructible.is_some(), "destructible"),
                (h.hit_box_on.is_some(), "hit_box_on"),
                (!h.damage_box.is_empty(), "damage_box"),
                (h.any_damage.is_some(), "any_damage"),
                (e.spawned_meet_bodies, "spawned_meet_bodies"),
                (e.mesh.is_some_and(|m| m.hit_boxes_on.is_some()), "mesh hit boxes"),
            ] {
                if on {
                    why.push(what);
                }
            }
            assert!(why.is_empty() || class_has_place(c), "{} deals or takes hits ({why:?}) without a place", c.name());
        }
        assert!(entered > 90, "only {entered} World.actors classes entered");
    }
}
