//! Projectiles (`Projectile`). The
//! struct holds every field of `Projectile`; [`ProjectileExt`] the fields a subclass adds.
//!
//! The class's data (`p`, the actor CDO, and `boxes`, the character's box tables) is the bound data's, read by
//! `cls` and the owner's character; it is no state.
// `!(a < b)` is kept as written, true for a NaN: partial_cmp would hide that.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::ProjectileClass;
use crate::Real;
use crate::State;
use crate::actors::projectile_move::{Shapes, blocker, bounce_move, env_hit, hit_move, turn};
use crate::actors::{ActorClass, ActorRef, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::packet::{DataEnum, DataVec};
use crate::combat::{BuffRef, ChargeVals, ElemState, HitPacket};
use crate::consts::{DELTA_SECONDS, GRAVITY_Z};
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::PolyBlock;
use crate::fighter::{DodgeState, FighterId};
use crate::math;
use crate::names::Name;
use crate::registry::entries::{ActorHooks, ProjectileHooks};
use crate::world::BlastBounds;
use crate::world::World;
use indexmap::IndexSet;
use mvs_collision::Ground;
use mvs_data::fighter::ActorDef;
use mvs_data::value::Props;
use mvs_data::value::Value;
use std::sync::{Arc, LazyLock};

/// A spawned projectile (or a projectile-family actor: a shockwave of Bugs, a car). Its data (`p`, the actor CDO;
/// `boxes`, the owner character's box tables) is the class's and the character's.
#[derive(State, Debug)]
pub struct Projectile {
    /// The Blueprint class (`cls`) and its variant.
    pub cls: Name,
    pub class: ProjectileClass,
    /// The world's count of spawned actors at the spawn (`id`, `World.actorSeq`): what events and the trace name.
    pub seq: i32,
    /// The owner (`owner`): `None` for a world-owned one (`Projectile<null>`, an item's or a stage's).
    pub owner: Option<FighterId>,
    /// The owner's id, kept for what reads it after the owner is gone.
    pub owner_id: i32,
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    pub prev_x: Real,
    pub prev_y: Real,
    pub vx: Real,
    pub vy: Real,
    /// The root's pitch in degrees (FRotator's).
    pub pitch: Real,
    pub facing: Real,
    pub age: i32,
    pub dead: bool,
    pub scale: Real,
    pub hit_radius: Real,
    pub hit_on: bool,
    /// HitDamage before the per-stage writes.
    pub hit: HitPacket,
    /// The fighters it hit, by id.
    pub hit_set: IndexSet<i32>,
    /// Its Comp_Pawn_HitBoxHandler dodge variables.
    pub dodge: Option<DodgeState>,
    /// Its Comp_Pawn_ChargeAttack values; `None` at the defaults.
    pub charge: Option<ChargeVals>,
    /// Its Comp_State_Manager's elemental buffs.
    pub elem: Option<ElemState>,
    /// SpawnedActorParent LifetimePausedCount.
    pub lifetime_pause: Option<LifetimePause>,

    // the movement and the turn
    pub rot_x: Real,
    /// bRotationFollowsVelocity.
    pub follows: bool,
    pub max_speed: Real,
    pub grav_scale: Real,
    pub grav_tick: i32,
    pub parent_vx: Real,
    pub parent_vy: Real,
    pub comp_v: Option<(Real, Real)>,
    pub moving: bool,
    /// The mesh's look rotation (C019_CookGrenade RotateGate: FindLookAtRotation), FRotator yaw and pitch in degrees.
    pub look_yaw: Real,
    pub look_pitch: Real,
    pub rotate_gate: bool,
    pub travel_x: Real,
    pub travel_y: Real,
    pub dist_to_target: Real,
    pub tl_pos: Real,
    pub tl_rate: Real,
    pub tl_playing: bool,
    pub attached: bool,
    pub socket: Option<i32>,
    /// `rel`: Y, Z.
    pub rel: (Real, Real),
    pub buff_home: Option<BuffHome>,
    pub sped_up: bool,
    pub mobility: Option<u8>,
    pub stationary_count: Option<i32>,
    pub aim_at: Option<ProjectileId>,
    pub aim_bound: bool,

    // the bounce and the contacts
    pub cap_r: Real,
    pub cap_h: Real,
    pub bounce: Real,
    pub friction: Real,
    pub stop_speed: Real,
    pub angle_friction: bool,
    pub min_friction: Real,
    pub prev_hit_time: Real,
    pub prev_nx: Real,
    pub prev_ny: Real,
    pub sliding: bool,
    pub stopped: bool,
    pub hit_t: Real,
    pub hit_nx: Real,
    pub hit_ny: Real,
    pub hit_pen: bool,
    pub hit_depth: Real,
    /// What the last blocking hit of the move met (the stage block, a box or a polygon solid): the solid whose
    /// actor hears the hit (`projectile_move::solid_dispatch`). Not in the record.
    pub hit_shape: crate::actors::projectile_move::HitShape,
    pub no_dispatch: bool,
    pub mtd_nx: Real,
    pub mtd_ny: Real,
    pub mtd_depth: Real,
    pub avx: Real,
    pub avy: Real,
    pub plat_response: CollisionResponse,
    pub cap_collides: bool,
    pub cap_written: bool,
    pub no_collision: bool,
    pub solids: Option<Vec<ProjectileBox>>,
    /// `MAX_LIFE`: the life a kind set past the class data's, `None` the data's.
    pub max_life: Option<Real>,
    /// `ATTACHED_VEL`: the velocity it had when it was attached.
    pub attached_vel: Option<(Real, Real)>,
    /// `INIT_SPEED`: the spawn's InitialSpeed where a kind changed it.
    pub init_speed: Option<Real>,
    /// `SPAWN_SCALE`: the root's scale at the spawn.
    pub spawn_scale: Option<Real>,
    /// `SPAWN_SIDE`: the side its yaw faced at the spawn.
    pub spawn_side: Option<Real>,
    pub bounce_count: i32,
    pub bounced: bool,
    pub die_on_bounce: bool,
    pub kb_off_on_bounce: bool,
    pub impact_vx: Real,
    pub impact_vy: Real,
    pub impact_x: Real,
    pub impact_y: Real,

    // the hit and hurt boxes
    pub hit_var: bool,
    pub has_hit_a_fighter: bool,
    /// `sphere3D`: the HitBox sphere deals, not HitBox2D.
    pub sphere_3d: bool,
    /// HealedAllies, by fighter id.
    pub healed_allies: IndexSet<i32>,
    pub hurt_radius: Real,
    pub hurt_off_y: Real,
    pub damage_disabled: Option<bool>,
    pub attack_type: i32,
    pub health: Real,
    pub clanking: bool,
    pub queue_destroy: bool,
    pub blocked_destroy: bool,
    /// TrackActorsHitByTag (HitEachActorOnlyOnce).
    pub hit_actors: IndexSet<ActorRef>,
    /// The hurt boxes (and pawn bodies) its hitbox overlaps now.
    pub overlapping: IndexSet<ActorRef>,
    pub ally_over: Option<IndexSet<i32>>,
    pub can_turn_on_hit_box: bool,
    pub hit_gate_tick: i32,
    pub restore_tick: i32,
    pub base_hit: Real,
    pub base_hurt: Real,
    pub destroy_on_enemy_hit: bool,
    pub reflect: Option<ReflectState>,
    pub hold_buff: Option<(FighterId, BuffRef)>,

    // who it counts as
    /// `instOverride`: unset, none, or a fighter.
    pub inst_override: OwnerOverride,
    /// `listOwner`: unset, none, or a fighter.
    pub list_owner: OwnerOverride,
    pub parent_fighter: Option<FighterId>,
    pub lookup_fighter: Option<FighterId>,

    // the life and its timers
    pub doom: Option<i32>,
    /// The frames left of its own Comp_HitPause pause (a class with ReceivesHitPause, the dealer's pause of its hit:
    /// `spawned_hits::dealer_hit_pause`); `None` for none.
    pub hit_pause: Option<i32>,
    /// SpawnedActorParent DestroyAfterHitPause: a DestroySpawnedActor(false) in its own pause, run when the pause ends.
    pub destroy_after_pause: bool,
    pub size_scale: Option<Real>,
    pub destroy_fx_blocked: Option<bool>,
    pub destroy_spawn_blocked: Option<bool>,
    pub life_ticks: i32,
    /// CheckBlastBoxTimer's next expiry, in seconds of counted deltas.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    pub activate_tick: i32,
    pub activated: bool,
    pub split_tick: i32,
    pub slow_tick: i32,
    pub ended: bool,

    // a fused or charged shot
    pub disarmed: bool,
    pub fuse_ticks: i32,
    pub tell_tick: i32,
    pub loop_tick: i32,
    pub detonated: bool,
    pub grenade: bool,
    pub charge_shot: bool,
    pub bomb_count: i32,
    pub bomb_visual: bool,
    pub plasma: bool,

    // a tether or a returning shot
    pub whip: bool,
    pub tether: Option<TetherParams>,
    pub return_socket: Option<i32>,
    pub can_control: bool,
    pub initial_speed: Real,
    pub snap_tick: i32,
    pub return_tick: i32,
    pub end_tick: i32,
    pub return_started: bool,
    pub homing: Real,
    pub returning: bool,
    pub snapped: bool,
    pub tipper_due: bool,
    pub controlled: bool,
    pub kb_seq: i32,

    /// `p` where a graph wrote the instance's own variables over its class defaults (setInstanceVars): the whole
    /// copy, shared by the snapshots until the next write; `None` reads the class's.
    pub own_p: Option<Arc<Props>>,

    pub ext: ProjectileExt,
}

impl Projectile {
    /// `new Projectile(cls, owner, attackId)`'s field initialisers; the constructor's set-up from the class data
    /// (the speed, the boxes, the fuse, the tether, ...) is the projectile port's.
    pub fn new(cls: Name, class: ProjectileClass, owner: Option<FighterId>, owner_id: i32, attack_id: i32) -> Self {
        Projectile {
            cls,
            class,
            seq: -1,
            owner,
            owner_id,
            attack_id,
            x: 0.0,
            y: 0.0,
            prev_x: 0.0,
            prev_y: 0.0,
            vx: 0.0,
            vy: 0.0,
            pitch: 0.0,
            facing: 0.0,
            age: 0,
            dead: false,
            scale: 0.0,
            hit_radius: 0.0,
            hit_on: false,
            hit: HitPacket::default(),
            hit_set: IndexSet::new(),
            dodge: None,
            charge: None,
            elem: None,
            lifetime_pause: None,
            rot_x: 0.0,
            follows: false,
            max_speed: 0.0,
            grav_scale: 0.0,
            grav_tick: 0,
            parent_vx: 0.0,
            parent_vy: 0.0,
            comp_v: None,
            moving: true,
            look_yaw: -180.0,
            look_pitch: 0.0,
            rotate_gate: false,
            travel_x: 0.0,
            travel_y: 0.0,
            dist_to_target: 0.0,
            tl_pos: 0.0,
            tl_rate: 0.0,
            tl_playing: false,
            attached: false,
            socket: None,
            rel: (0.0, 0.0),
            buff_home: None,
            sped_up: false,
            mobility: None,
            stationary_count: None,
            aim_at: None,
            aim_bound: false,
            cap_r: 0.0,
            cap_h: 0.0,
            bounce: 0.0,
            friction: 0.0,
            stop_speed: 0.0,
            angle_friction: false,
            min_friction: 0.0,
            prev_hit_time: 1.0,
            prev_nx: 0.0,
            prev_ny: 1.0,
            sliding: false,
            stopped: false,
            hit_t: 1.0,
            hit_nx: 0.0,
            hit_ny: 0.0,
            hit_pen: false,
            hit_depth: 0.0,
            hit_shape: crate::actors::projectile_move::HitShape::None,
            no_dispatch: false,
            mtd_nx: 0.0,
            mtd_ny: 0.0,
            mtd_depth: 0.0,
            avx: 0.0,
            avy: 0.0,
            plat_response: CollisionResponse::default(),
            cap_collides: false,
            cap_written: false,
            no_collision: false,
            solids: None,
            max_life: None,
            attached_vel: None,
            init_speed: None,
            spawn_scale: None,
            spawn_side: None,
            bounce_count: 0,
            bounced: false,
            die_on_bounce: false,
            kb_off_on_bounce: false,
            impact_vx: 0.0,
            impact_vy: 0.0,
            impact_x: 0.0,
            impact_y: 0.0,
            hit_var: false,
            has_hit_a_fighter: false,
            sphere_3d: false,
            healed_allies: IndexSet::new(),
            hurt_radius: 0.0,
            hurt_off_y: 0.0,
            damage_disabled: None,
            attack_type: 0,
            health: 0.0,
            clanking: false,
            queue_destroy: false,
            blocked_destroy: false,
            hit_actors: IndexSet::new(),
            overlapping: IndexSet::new(),
            ally_over: None,
            can_turn_on_hit_box: true,
            hit_gate_tick: -1,
            restore_tick: -1,
            base_hit: 0.0,
            base_hurt: 0.0,
            destroy_on_enemy_hit: false,
            reflect: None,
            hold_buff: None,
            inst_override: OwnerOverride::default(),
            list_owner: OwnerOverride::default(),
            parent_fighter: None,
            lookup_fighter: None,
            doom: None,
            hit_pause: None,
            destroy_after_pause: false,
            size_scale: None,
            destroy_fx_blocked: None,
            destroy_spawn_blocked: None,
            life_ticks: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: BlastBounds::default(),
            activate_tick: -1,
            activated: false,
            split_tick: -1,
            slow_tick: -1,
            ended: false,
            disarmed: false,
            fuse_ticks: 0,
            tell_tick: -1,
            loop_tick: -1,
            detonated: false,
            grenade: false,
            charge_shot: false,
            bomb_count: 0,
            bomb_visual: false,
            plasma: false,
            whip: false,
            tether: None,
            return_socket: None,
            can_control: false,
            initial_speed: 0.0,
            snap_tick: -1,
            return_tick: -1,
            end_tick: -1,
            return_started: false,
            homing: 0.0,
            returning: false,
            snapped: false,
            tipper_due: false,
            controlled: false,
            kb_seq: 0,
            own_p: None,
            ext: ProjectileExt::default(),
        }
    }
}

/// `instOverride` / `listOwner`: absent (the owner's rule), null (no one), or a fighter.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum OwnerOverride {
    #[default]
    Unset,
    None,
    Fighter(FighterId),
}

/// `CollisionResponse`: ECR_IGNORE, ECR_OVERLAP, ECR_BLOCK.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum CollisionResponse {
    #[default]
    Ignore,
    Overlap,
    Block,
}

/// `ReflectState`.
#[derive(State, Debug, Default)]
pub struct ReflectState {
    pub unlisted: bool,
    /// MulticastReflect's pause: 0 open, 1 its Delay(0.0) runs, 2 its Delay(0.375) runs.
    pub pause: u8,
    pub left: i32,
    pub boxes: bool,
    pub life_span_at: i32,
}

/// What to home on, its `fighter` or its `actor` (one of the two is set).
#[derive(State, Debug)]
pub struct BuffHome {
    pub to: ActorRef,
    pub mag: Real,
}

/// `TetherParams`.
#[derive(State, Debug, Default)]
pub struct TetherParams {
    pub travel: Real,
    pub return_speed: Real,
    pub return_max_speed: Option<Real>,
    pub socket: Option<Name>,
    pub hit_sphere: Option<Name>,
    pub snap: bool,
}

/// `ProjectileBox` (its `PlatformBox` part): a box of the projectile the fighters
/// meet as a platform.
#[derive(State, Debug, Default)]
pub struct ProjectileBox {
    pub dead: bool,
    /// Collision enabled.
    pub on: bool,
    /// The box's centre and sides.
    pub x: Real,
    pub y: Real,
    pub left: Real,
    pub right: Real,
    pub top: Real,
    pub bottom: Real,
    /// Its move since the last placing.
    pub dx: Real,
    pub dy: Real,
    /// The imparted velocity.
    pub vx: Real,
    pub vy: Real,
    /// The fighters (ids below 32) it ignores, as bits.
    pub ignored: i32,
    /// The minions it ignores, by id.
    pub ignored_minions: Option<Vec<i32>>,
    pub placed: bool,
    pub wall: bool,
    pub hx: Real,
    pub hy: Real,
    /// It blocks the ProjectileGroundTrace, PhysicsBody, Item and WorldStaticDetection queries.
    pub trace: bool,
    pub nav: bool,
    pub item: bool,
    pub det: bool,
    /// The half extents in the play plane and the place on the root, unscaled.
    pub ex: Real,
    pub ey: Real,
    pub ox: Real,
    pub oz: Real,
    /// `BOX_TEMPLATE`: the component of the class data it was made from, by name.
    pub template: Name,
    /// `TURNS`: its extents turn with the pitch.
    pub turns: bool,
}

/// `LifetimePause`: SpawnedActorParent's LifetimePausedCount and the paused timer's ticks left.
#[derive(State, Debug, Default)]
pub struct LifetimePause {
    pub count: i32,
    pub left: i32,
}

/// What a subclass of `Projectile` adds, a variant per class (in the order of the characters, then the
/// shared ones). `Plain` is `Projectile` itself, which most classes are (Shaggy's sandwiches are). A variant without
/// a struct is a subclass not ported yet; its port gives it one.
#[derive(State, Debug, Default)]
#[expect(clippy::large_enum_variant, reason = "held inline as the other ext enums are: a match has few projectiles")]
pub enum ProjectileExt {
    #[default]
    Plain,
    // the fire wall
    FireWall(crate::actors::firewall::FireWall),
    // chars/morty
    CookGrenade(crate::chars::morty::projectiles::CookGrenade),
    // chars/iron-giant
    Bomb(crate::chars::iron_giant::actor_state::Bomb),
    DiscShot(crate::chars::iron_giant::actor_state::DiscShot),
    Missile(crate::chars::iron_giant::actor_state::Missile),
    ThrownScrap(crate::chars::iron_giant::actors::ThrownScrap),
    // chars/wonder-woman
    BracerClash(crate::chars::wonder_woman::actors::BracerClash),
    Lasso(crate::chars::wonder_woman::actors::Lasso),
    // chars/superman
    IcePellet(crate::chars::superman::ice::IcePellet),
    // chars/lebron
    Ball(crate::chars::lebron::ball::Ball),
    SassBubble(crate::chars::lebron::bubble::SassBubble),
    // chars/arya
    Knife(crate::chars::arya::knife::Knife),
    // chars/batman
    Grapple(crate::chars::batman::grapple::Grapple),
    PerkGrapple(crate::chars::batman::perks_grapple::PerkGrapple),
    // chars/creature
    Firebolt(crate::chars::creature::fire::Firebolt),
    WreckingBall(crate::chars::creature::perks_ball::WreckingBall),
    SeismicReactor(crate::chars::creature::seismic::SeismicReactor),
    TetherProjectile(crate::chars::creature::tether::TetherProjectile),
    // chars/black-adam
    Chain(crate::chars::black_adam::chain::Chain),
    DoubleStrike(crate::chars::black_adam::double_strike::DoubleStrike),
    HomingBall(crate::chars::black_adam::projectiles::HomingBall),
    // chars/bugs
    /// LingeringLoveProjectile_C.
    StillActor(crate::chars::bugs::perks::StillActor),
    /// TunnelShockwave_C: `Shockwave extends StillActor`.
    BugsShockwave(crate::chars::bugs::perks::BugsShockwave),
    /// BugsBunnyRocketFProjectile_C and BugsBunnyRocketUpProjectile_C.
    Rocket(crate::chars::bugs::rockets::Rocket),
    /// BugsBunnySafe_C.
    BugsBunnySafe(crate::chars::bugs::safe::BugsBunnySafe),
    /// BugsBunny_SafeDrop_C.
    SafeDrop(crate::chars::bugs::safe::SafeDrop),
    // chars/rick
    /// FartBomb_SpawnedActor_C.
    FartBomb(crate::chars::rick::fart_bomb::FartBomb),
    /// Meeseeks_SpawnedActor_C.
    MeeseeksSummon(crate::chars::rick::meeseeks::MeeseeksSummon),
    // chars/taz
    /// C015_HealPickup_C.
    HealPickup(crate::chars::taz::chicken::HealPickup),
    /// C015StewSpitAir_C.
    StewSpitAir(crate::chars::taz::projectiles::StewSpitAir),
    // chars/steven
    /// Steven_BubbleCharge_C.
    BubbleCharge(crate::chars::steven::bubble::BubbleCharge),
    /// StevenTetherStunProjectile_C.
    StevenTether(crate::chars::steven::tether::StevenTether),
    /// HealGrenade_C.
    HealGrenade(crate::chars::steven::watermelon::HealGrenade),
    // chars/garnet
    /// GarnetGauntletsProjectile_C and GarnetGauntletsProjectileAir_C.
    GarnetGauntlets(crate::chars::garnet::gauntlets::GarnetGauntlets),
    /// GarnetMarker2_C.
    GarnetMarker2(crate::chars::garnet::marker::GarnetMarker2),
    /// GarnetLightningBolt_C.
    GarnetBolt(crate::chars::garnet::zone::GarnetBolt),
    // chars/finn
    /// FinnTreasure_C.
    FinnTreasure(crate::chars::finn::coins::FinnTreasure),
    // chars/tom-and-jerry
    /// JerryCork_C.
    JerryCork(crate::chars::tom_and_jerry::champagne::JerryCork),
    /// Dynamite_SpawnedActor_C and CantSplitDynamite_SpawnedActor_C: `Dynamite`
    /// (the item of the same name is `ItemExt::Dynamite`).
    Dynamite(crate::chars::tom_and_jerry::dynamite::Dynamite),
    /// FishingLure_C.
    Lure(crate::chars::tom_and_jerry::fishing::Lure),
    /// TomAimRocket1_C.
    AimRocket(crate::chars::tom_and_jerry::kicked::AimRocket),
    /// JerryKickedRocket_C.
    KickedRocket(crate::chars::tom_and_jerry::kicked::KickedRocket),
    /// JerryRocket_C.
    JerryRocket(crate::chars::tom_and_jerry::rocket::JerryRocket),
    /// Tom_and_Jerry_TennisBall_C and Tom_and_Jerry_TennisBall_Reflected_C.
    TennisBall(crate::chars::tom_and_jerry::tennis::TennisBall),
    // chars/velma
    /// VelmaBookProjectile_C.
    VelmaBook(crate::chars::velma::gadgets::VelmaBook),
    /// Velma_SPAIR_CameraFlash_C and Velma_AirSideSp_BookBlast_C.
    VelmaBurst(crate::chars::velma::gadgets::VelmaBurst),
    /// Velma_SassBubblev2_C.
    VelmaSassBubble(crate::chars::velma::gadgets::VelmaSassBubble),
    /// VelmaAimProjectile_C.
    AimProjectile(crate::chars::velma::mark::AimProjectile),
    /// VelmamSassFollowUpProjectile_C.
    FollowUp(crate::chars::velma::mark::FollowUp),
    /// VelmaMarkProjectile_C.
    Mark(crate::chars::velma::mark::Mark),
    // chars/marvin
    /// C018UpBlasterProj_C, C018UpBlasterProjAir_C and C018ComboBlasterProjVersion2_C.
    Blaster(crate::chars::marvin::blaster::Blaster),
    /// C018BubbleProj_C.
    BubbleProj(crate::chars::marvin::bubble::BubbleProj),
    /// C018Flag_C and C018AirFlag_C.
    C018Flag(crate::chars::marvin::flag::C018Flag),
    // chars/stripe
    /// BoomBox_SpawnedActor_C.
    BoomBox(crate::chars::stripe::dash::BoomBox),
    /// C023B_SpawnedGunShot_C.
    GunShot(crate::chars::stripe::gun::GunShot),
    /// C023B_SawBlade_EdgeTrace_C, its _StartOnGround_C child and C023B_SawBlade_FromAlly_EdgeTrace_C.
    Saw(crate::chars::stripe::saw::Saw),
    // chars/gizmo (`CarActor` is abstract: `chars::gizmo::car::car_of` reads the shell of the three cars)
    /// C023A_Arrow_Projectile_C.
    Arrow(crate::chars::gizmo::bow::Arrow),
    /// C023A_Car_SpawnedActor_C.
    GizmoCar(crate::chars::gizmo::car::GizmoCar),
    /// C023A_MusicPickup_C.
    MusicPickup(crate::chars::gizmo::music::MusicPickup),
    /// C023A_Music_Projectile_C.
    MusicProjectile(crate::chars::gizmo::music::MusicProjectile),
    /// C023A_PianoNote_C.
    PianoNote(crate::chars::gizmo::music::PianoNote),
    /// C023A_PopCorn_C.
    Popcorn(crate::chars::gizmo::popcorn::Popcorn),
    // items/
    /// `ItemCar extends CarActor`: Parent_Car_SpawnedActor_C, MysteryMachine_SpawnedCar_C.
    ItemCar(crate::items::car::ItemCar),
    /// `BatMobileCar extends ItemCar`: BatMobile_SpawnedActor_Child_C.
    BatMobileCar(crate::items::car::BatMobileCar),
    /// SelfActivate_Meeseeks_Run_C.
    MeeseeksRun(crate::items::meeseeks::MeeseeksRun),
}

static NO_PROPS: LazyLock<Props> = LazyLock::new(Props::new);

/// The fighter a projectile's class data is read through: its owner, else the fighter it was looked up through
/// (`lookupFighter`).
pub fn data_fighter(pr: &Projectile) -> FighterId {
    pr.owner.or(pr.lookup_fighter).expect("a projectile has an owner or a fighter it was looked up through")
}

/// The class's `ActorDef` (the constructor's `actor`).
pub fn class_def<'a>(w: &'a World, pr: &Projectile) -> Option<&'a ActorDef> {
    crate::fighter::common::actor_class_def(w, data_fighter(pr), pr.cls)
}

/// `p`: the instance's own variables where a graph wrote them (`own_p`), else the actor CDO (empty for a class the
/// data has none of).
pub fn class_p<'a>(w: &'a World, pr: &'a Projectile) -> &'a Props {
    match &pr.own_p {
        Some(p) => p,
        None => class_def(w, pr).map_or(&NO_PROPS, |d| &d.p),
    }
}

/// An instance variable of the projectile that another class's graph
/// sets after the spawn (Buff_BatmanBouncerang's OnHitBuffs, Buff_Creature_Firestorm's ActorClassSpawnedAtDeath and
/// HitBoxDamage @553 / @594). The class defaults are the shared data, so the projectile takes its own copy with the
/// new values. A reader that copied a value out of `p` at the spawn does not see the write: each such reader is
/// checked on its own.
pub fn set_instance_vars(w: &mut World, id: ProjectileId, vars: Vec<(String, Value)>) {
    let mut p = class_p(w, pm_ref(w, id)).clone();
    for (k, v) in vars {
        p.insert(k, v);
    }
    pm(w, id).own_p = Some(Arc::new(p));
}

/// How many dealing boxes the projectile has beside its HitBox (other
/// components on at once, HitBoxesOn with another tag: each with its own HitBoxesToDamage entry, ActivateHitbox
/// @16694, and the hit list of its packet's AttackTag, CheckIfActorHasBeenHit @2906). None but for a class whose graph
/// has two tags on at once; the class's own state holds each box's shapes, packet, hit list and ally overlaps, read
/// by the overlap pass by the box's place.
pub fn extra_hit_boxes(w: &World, id: ProjectileId) -> usize {
    // the one class with extra boxes: Gizmo's popcorn (HitBox beside ShotGunHitBox, OverlapTrigger)
    if matches!(pm_ref(w, id).ext, ProjectileExt::Popcorn(_)) {
        return crate::chars::gizmo::popcorn::extra_hit_boxes(w, id);
    }
    0
}

/// A graph's write of BuffDuration on every row of its own
/// 'On Hit Buffs' whose AppliedBuffs is `cls` (Steven_BubbleCharge @281-426, WonderWoman_BracerClash @338-426): the
/// instance's rows, which each later pfgDoDamage of its handler reads.
pub fn set_hit_buff_duration(w: &mut World, id: ProjectileId, cls: &str, duration: Real) {
    let mut o = obj(Some(class_p(w, pm_ref(w, id))), "On Hit Buffs").cloned().unwrap_or_default();
    let rows: Vec<Value> = o
        .get("OnHitBuffs")
        .and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .map(|e| match e {
            Value::Obj(r) if text(Some(r), "AppliedBuffs") == Some(cls) => {
                let mut r = r.clone();
                r.insert("BuffDuration".to_owned(), Value::Num(f64::from(duration)));
                Value::Obj(r)
            }
            e => e.clone(),
        })
        .collect();
    o.insert("OnHitBuffs".to_owned(), Value::Arr(rows));
    set_instance_vars(w, id, vec![("On Hit Buffs".to_owned(), Value::Obj(o))]);
}

/// The class's `ProjectileKind` hooks (`ACTORS.get(pr.cls)?.projectile`).
pub fn hooks_of(w: &World, id: ProjectileId) -> Option<ProjectileHooks> {
    match crate::actors::world_actor::actor_entry(w, ActorRef::Projectile(id))?.hooks {
        ActorHooks::Projectile(h) => Some(h),
        _ => None,
    }
}

/// JavaScript truthiness of a property (`!!v`): absent, null, false, 0, NaN and "" are false.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `o?.[k]` as a number.
fn num(o: Option<&Props>, k: &str) -> Option<Real> {
    o.and_then(|o| o.get(k)).and_then(Value::as_f64).map(crate::math::narrow)
}

/// `o?.[k]` as an object.
fn obj<'a>(o: Option<&'a Props>, k: &str) -> Option<&'a Props> {
    o.and_then(|o| o.get(k)).and_then(Value::as_obj)
}

/// `o?.[k]` as a string.
fn text<'a>(o: Option<&'a Props>, k: &str) -> Option<&'a str> {
    o.and_then(|o| o.get(k)).and_then(Value::as_str)
}

/// `latentFrames` as a tick count.
fn lf(s: Real) -> i32 {
    math::to_int32(crate::fighter::common::latent_frames(s))
}

/// A fighter's facing (`facing`), place and velocity.
fn fighter_body(w: &World, f: FighterId) -> (Real, Real, Real, Real, Real) {
    let b = &w.fighter(f).body;
    (b.facing_dir, b.x, b.y, b.vx, b.vy)
}

/// The fighter's character definition (`f.ch`).
fn char_def(w: &World, f: FighterId) -> &mvs_data::def::CharDef {
    &w.data.chars[w.names.str(w.fighter(f).ch)].def
}

/// The fighter's box sampler (`f.boxes`).
fn sampler(w: &World, f: FighterId) -> &crate::combat::boxes::BoxSampler {
    &w.fixed.samplers[&w.fighter(f).ch]
}

/// A socket's shape index on a sampler: `byName.get(sn) ?? byLower.get(sn.toLowerCase())`.
fn socket_index(w: &World, s: &crate::combat::boxes::BoxSampler, sn: &str) -> Option<u32> {
    w.names.get(sn).and_then(|n| s.by_name.get(&n).copied()).or_else(|| s.by_lower.get(&sn.to_lowercase()).copied())
}

/// A socket index as the struct keeps it.
fn sock(i: Option<u32>) -> Option<i32> {
    i.and_then(|i| i32::try_from(i).ok())
}

/// The shape offset of a socket index in a sampled frame.
fn offset_of(s: &crate::combat::boxes::BoxSampler, si: i32) -> usize {
    usize::try_from(s.offsets[usize::try_from(si).unwrap_or(0)]).unwrap_or(0)
}

/// Over the whole class definition: BPI_Projectiles_C among its
/// Interfaces, or its Comp_Actor_Destructible's IsProjectile.
fn def_is_projectile(a: &ActorDef) -> bool {
    format!(" {} ", a.ifaces.as_deref().unwrap_or("")).contains(" BPI_Projectiles_C ")
        || truthy(obj(Some(&a.components), "Comp_Actor_Destructible").and_then(|c| c.get("IsProjectile")))
}

// ---- the call surface of the projectiles

/// What `attachParent` gives: the transform of a component's attach parent under the actor's root.
#[derive(Clone, Debug)]
pub struct AttachParentOut {
    pub loc: [Real; 3],
    pub scale: [Real; 3],
    pub rot: Vec<Real>,
}

/// The HurtBox's centre (`hurtX`, `hurtY`) where the class's HurtBox is the base
/// sphere; `None` for a class with a HurtBox of its own shape (one that overrides `hurtDist2`: the fire wall,
/// Gizmo's popcorn). Bugs's Rocket overrides hurtX / hurtY (its port).
pub fn hurt_core(w: &crate::world::World, pr: crate::actors::ProjectileId) -> Option<(Real, Real)> {
    let p = w.actors.projectiles.get(pr).expect("a projectile of the list");
    match &p.ext {
        // its HurtBox is a capsule: the class enters its own body
        ProjectileExt::FireWall(_) => None,
        // their own hurtX / hurtY (Bugs's rocket) or hurtDist2 (Gizmo's popcorn), with their ports
        ProjectileExt::Rocket(r) => Some((r.hurt_x(p), r.hurt_y(p))),
        ProjectileExt::Popcorn(_) => None,
        _ => Some((hurt_x(p), hurt_y(p))),
    }
}

/// The transform of a component's attach parent under the actor's
/// root, from the construction script's tree (the data's AttachParent chain): location, rotation and scale in the
/// root's space, before the root's own scale. USceneComponent composes FTransforms (RelativeTransform x
/// ParentToWorld): the scales multiply per axis whatever the rotations (no shear: C019_WhipProjectile's Cylinder
/// (0.5, 0.5, 1) and its children's (2, 2, 1) give 1), the rotation is the parent's after the child's, the location
/// Parent.Rot(Parent.Scale x Child.Loc) + Parent.Loc. rot: rows = the root-space directions of the parent's X, Y
/// and Z; a child at RelativeLocation L with scale S sits at loc + (scale x L) through rot, with scale x S. The
/// rotations written here are the construction script's: a parent turned at run time is the caller's.
pub fn attach_parent(cs: &Props, name: &str) -> AttachParentOut {
    let mut loc = [0.0; 3];
    let mut scale = [1.0; 3];
    let mut rot = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let parent_of = |n: &str| text(obj(Some(cs), n), "AttachParent").filter(|s| !s.is_empty()).map(str::to_owned);
    let mut p = parent_of(name);
    while let Some(pn) = p.clone() {
        let Some(next) = parent_of(&pn) else { break };
        let c = obj(Some(cs), &pn);
        let l = obj(c, "RelativeLocation");
        let s = obj(c, "RelativeScale3D");
        let q = obj(c, "RelativeRotation");
        let pl = [num(l, "X").unwrap_or(0.0), num(l, "Y").unwrap_or(0.0), num(l, "Z").unwrap_or(0.0)];
        let ps = [num(s, "X").unwrap_or(1.0), num(s, "Y").unwrap_or(1.0), num(s, "Z").unwrap_or(1.0)];
        let turned = q.is_some_and(|q| truthy(q.get("Pitch")) || truthy(q.get("Yaw")) || truthy(q.get("Roll")));
        if turned {
            // The parent's RelativeRotation as the component transform holds it: FRotator::Quaternion of the
            // rotator in degrees; the rows are its turn of the unit axes (FQuat::RotateVector).
            // Unverified: the composition below is the port's matrix product, not FTransform's quaternion product.
            let (pi, ya, ro) =
                (num(q, "Pitch").unwrap_or(0.0), num(q, "Yaw").unwrap_or(0.0), num(q, "Roll").unwrap_or(0.0));
            let qq = math::ue::rotator_quaternion([pi, ya, ro]);
            let [rx, ry, rz] =
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|e| math::ue::quat_rotate_vector(qq, e));
            let pr = [rx[0], rx[1], rx[2], ry[0], ry[1], ry[2], rz[0], rz[1], rz[2]];
            let v = [ps[0] * loc[0], ps[1] * loc[1], ps[2] * loc[2]];
            let mut out = Vec::with_capacity(9);
            for i in 0..3 {
                for c in 0..3 {
                    out.push(rot[3 * i] * pr[c] + rot[3 * i + 1] * pr[3 + c] + rot[3 * i + 2] * pr[6 + c]);
                }
            }
            rot = out;
            for k in 0..3 {
                loc[k] = pl[k] + v[0] * pr[k] + v[1] * pr[3 + k] + v[2] * pr[6 + k];
                scale[k] *= ps[k];
            }
        } else {
            for k in 0..3 {
                loc[k] = pl[k] + ps[k] * loc[k];
                scale[k] *= ps[k];
            }
        }
        p = Some(next);
    }
    AttachParentOut { loc, scale, rot }
}

/// The one component of a class's SCS whose ComponentTags hold `tag`
/// (lower case), with its sphere's or capsule's radius; `None` for none, for several, or for a box.
fn tagged_hit_box<'a>(cs: &'a Props, tag: &str) -> Option<(&'a str, Real)> {
    let mut out = None;
    for (name, c) in cs {
        let Some(tags) = c.as_obj().and_then(|c| c.get("ComponentTags")).and_then(Value::as_arr) else { continue };
        if !tags.iter().any(|t| t.as_str().is_some_and(|t| t.to_lowercase() == tag)) {
            continue;
        }
        let r = num(c.as_obj(), "SphereRadius").or_else(|| num(c.as_obj(), "CapsuleRadius"));
        let (Some(r), None) = (r, out) else { return None };
        out = Some((name.as_str(), r));
    }
    out
}

/// The smallest axis of a world scale (USphereComponent::GetScaledSphereRadius's).
fn min_axis(s: &[Real; 3]) -> Real {
    math::min(math::min(s[0].abs(), s[1].abs()), s[2].abs())
}

/// The side of the yaw a notify's spawn transform gives the actor
/// (Comp_Spawner @1385-2417: MakeRotator(0, 0, FacingLeft ? 90 : -90), then the notify rotation's yaw and roll
/// added mirrored for a right-facing owner and its pitch added), as K2_GetActorRotation reads it back from the
/// root's quaternion: 1 for a yaw below 0 (ComingFromLeft), -1 else.
// Unverified: at the singularity (a pitch of +-90) the yaw is taken as 0 (not below 0); the build's float SinCos
// and the quaternion's rounding decide it there (not read).
fn spawn_yaw_side(q: &Props, facing: Real) -> Real {
    let left = facing < 0.0;
    let qq = [
        num(Some(q), "X").unwrap_or(0.0),
        num(Some(q), "Y").unwrap_or(0.0),
        num(Some(q), "Z").unwrap_or(0.0),
        num(Some(q), "W").unwrap_or(1.0),
    ];
    // FQuat::Rotator 14136e550 and FRotator::Quaternion 14136a6d0, float32.
    let [p, y, r] = math::ue::quat_rotator(qq);
    let out = math::ue::quat_rotator(math::ue::rotator_quaternion([
        p,
        (if left { 90.0 } else { -90.0 }) + (if left { y } else { -y }),
        if left { r } else { -r },
    ]));
    let yaw = if out[0].abs() == 90.0 { 0.0 } else { out[1] };
    if yaw < 0.0 { 1.0 } else { -1.0 }
}

/// Comp_HoldCharge.GetCHargeValue(0) of a spawner's owner, for
/// the notifies with HoldChargeOverrideSpeedMod: its ChargePercent when the owner has the component and HasCharge,
/// else NaN (the ChargeAttack value).
fn hold_charge_percent(w: &World, f: FighterId) -> Real {
    let v = crate::combat::hits::get_charge_value(w, f, 0);
    if v.has { v.pct } else { Real::NAN }
}

/// `isOwnedProjectile`.
pub fn is_owned_projectile(w: &World, pr: ProjectileId) -> bool {
    w.actors.projectiles.get(pr).is_some_and(|p| p.owner.is_some())
}

/// The root's world scale at PostActorConstruction (the spawn
/// transform's, ahead of a BeginPlay that scales it).
pub fn spawn_root_scale(w: &World, pr: ProjectileId) -> Real {
    w.actors.projectiles.get(pr).map_or(0.0, |p| p.spawn_scale.unwrap_or(p.scale))
}

/// What the class's kind brings to the constructor (its `make`): the tether family's values; `charge_scale`: its
/// BeginPlay's MulticastSetProjectileScale(GetChargeValue(4)); `life`: its own GetSpawnMaxLifetime in place of
/// MaxLifetime; `ridden`: a SpawnedActorParent child with neither a ProjectileMovement nor a root collision profile
/// whose kind places it on what it is attached to (the two warnings about those are not given); `lookup`: with no
/// owner, a fighter of the match (`lookupFighter`).
#[derive(Debug, Default)]
pub struct MakeOpts {
    pub tether: Option<TetherParams>,
    pub charge_scale: bool,
    pub life: Option<Real>,
    pub by_fighter: bool,
    pub ridden: bool,
    pub lookup: Option<FighterId>,
}

/// `new Projectile(cls, owner, attackId, actor, notify, boxes, origin, opts)`: the
/// actor's class data is read through the owner (or `opts.lookup`), the box sampler is that fighter's. `notify`: the
/// spawn notify's properties (not borrowed from the World: the caller holds its own `Arc` of the data). `origin`: a
/// spawn by another actor (Comp_Spawner.CompSpawnActor with a world transform), the location.
#[allow(clippy::too_many_arguments, reason = "the constructor's arguments")]
pub fn make_projectile(
    w: &mut World,
    cls: Name,
    class: ProjectileClass,
    owner: Option<FighterId>,
    attack_id: i32,
    notify: &Props,
    origin: Option<V2>,
    opts: MakeOpts,
) -> Projectile {
    let cls_s = w.names.str(cls).to_owned();
    if owner.is_none() && opts.lookup.is_none() {
        panic!("{cls_s}: a projectile with no owner needs a fighter of the match to look its class up");
    }
    if owner.is_none() && origin.is_none() {
        panic!("{cls_s}: a projectile with no owner and no spawn location");
    }
    let owner_id = owner.map_or(-1, |f| w.fighter(f).id);
    let mut pr = Projectile::new(cls, class, owner, owner_id, attack_id);
    if owner.is_none() {
        pr.lookup_fighter = opts.lookup;
    }
    let n = Some(notify);
    let charge = owner.map_or(0.0, |f| w.fighter(f).charge.charge_pct);
    // ApplyProjectileSpeedBuffsToInputs (HandleActorPostSpawn @5329 / @7255, ahead of the charge Lerps): the
    // spawner's owner has a Comp_State_Manager and the actor is a projectile (BPI_Projectiles from the class's
    // Interfaces, or its Comp_Actor_Destructible's IsProjectile).
    // Unverified: a spawn from another actor's Comp_Spawner (`origin`) takes no buff, as its owner is taken to have
    // no Comp_State_Manager; the classes whose graphs spawn with speeds of their own are not each read.
    let is_proj = class_def(w, &pr).is_some_and(def_is_projectile);
    let buffed = owner.is_some() && (origin.is_none() || opts.by_fighter) && is_proj;
    let n_initial = num(n, "InitialSpeed").unwrap_or(0.0);
    let initial = match owner {
        Some(f) if buffed => crate::fighter::moves::projectile_speed_buffed(w, f, n_initial),
        _ => n_initial,
    };
    let held = match owner {
        Some(f) if truthy(notify.get("HoldChargeOverrideSpeedMod")) => hold_charge_percent(w, f),
        _ => Real::NAN,
    };
    let n_max = num(n, "MaxSpeed").unwrap_or(0.0);
    let max = match owner {
        Some(f) if buffed => crate::fighter::moves::projectile_speed_buffed(w, f, n_max),
        _ => n_max,
    };
    let w: &World = w;
    let def = class_def(w, &pr).unwrap_or_else(|| panic!("{cls_s}: no class data"));
    let p = &def.p;
    let cs = &def.components;
    let pp = Some(p);
    pr.hit = crate::combat::HitPacket::from_props(&w.names, obj(pp, "HitBoxDamage").unwrap_or(&NO_PROPS), None);
    pr.attached = truthy(notify.get("AttachToActor"));
    // HandleActorPostSpawn: K2_SetActorRelativeTransform(notify Transforms) after attaching; the root Capsule has
    // bAbsoluteScale, so the notify Scale3D is its world scale. Not attached: FinishSpawningActor (142a7c3e0) runs
    // AActor::FinishSpawning with bIsDefaultTransform false, and USCS_Node::ExecuteNodeOnActor (142d4a790) gives the
    // root node the spawn transform whole: its world scale is the transform's.
    let capsule = obj(Some(cs), "Capsule");
    let tr = obj(n, "Transforms");
    let n_scale = num(obj(tr, "Scale3D"), "X").unwrap_or(1.0);
    let mut scale = if pr.attached {
        if truthy(capsule.and_then(|c| c.get("bAbsoluteScale"))) {
            n_scale
        } else {
            num(obj(capsule, "RelativeScale3D"), "X").unwrap_or(1.0)
        }
    } else {
        n_scale
    };
    pr.spawn_scale = Some(scale);
    // C019_SplitterProjectile / C019_SplitterFollowup BeginPlay: MulticastSetProjectileScale(GetChargeValue(4)) ->
    // Capsule.SetWorldScale3D(VLerp(NoChargeScale, FullChargeScale, alpha)).
    let split = cls_s.contains("SplitterProjectile");
    let followup = cls_s.contains("SplitterFollowup");
    // CreatureSpit_C BeginPlay: the same charge scale (@68-123) and GetSpawnMaxLifetime (Lerp(MinLifetimeClamp,
    // MaxLifetimeClamp, GetChargeValue(4))) as the follow-up's (`chars/creature/fire.rs`).
    let spit = cls_s == "CreatureSpit_C";
    // The root Capsule's response to the Platform channel, from the class's own BodyInstance (Platform's default is
    // Block). ProjectileNavNoPlatform (ParentProjectilePhysics) lists Platform Ignore, ProjectileNavPlatform does
    // not; NoCollision ignores all.
    let body = obj(capsule, "BodyInstance");
    let profile = truthy(body.and_then(|b| b.get("CollisionProfileName")));
    if !profile && !opts.ridden {
        crate::actors::world_actor::warn("projectile: no collision profile in the class data, platforms passed");
    }
    let plat = obj(body, "CollisionResponses")
        .and_then(|c| c.get("ResponseArray"))
        .and_then(Value::as_arr)
        .and_then(|a| a.iter().find(|r| text(r.as_obj(), "Channel") == Some("Platform")))
        .and_then(|r| text(r.as_obj(), "Response"))
        .unwrap_or("ECR_Block");
    pr.cap_collides = profile && text(body, "CollisionEnabled") != Some("ECollisionEnabled::NoCollision");
    pr.plat_response = match plat {
        "ECR_Block" => CollisionResponse::Block,
        "ECR_Overlap" => CollisionResponse::Overlap,
        _ => CollisionResponse::Ignore,
    };
    if (split || followup || spit || opts.charge_scale) && truthy(p.get("NoChargeScale")) {
        let nc = num(obj(pp, "NoChargeScale"), "X").unwrap_or(Real::NAN);
        scale = nc + (num(obj(pp, "FullChargeScale"), "X").unwrap_or(scale) - nc) * charge;
    }
    pr.scale = scale;
    pr.cap_r = num(capsule, "CapsuleRadius").unwrap_or(22.0);
    pr.cap_h = num(capsule, "CapsuleHalfHeight").unwrap_or(44.0);
    // SpawnedActorParent ReceiveBeginPlay @3777-3792: without EnableProjectileMovement ProjectileMovement.Deactivate().
    // Unverified: an attached actor's movement is left active in the port; the classes that detach one write its
    // velocity themselves.
    let moves = truthy(p.get("EnableProjectileMovement"));
    if !pr.attached && !moves {
        pr.moving = false;
    }
    if moves && !cs.contains_key("ProjectileMovement") && !opts.ridden {
        crate::actors::world_actor::warn(
            "projectile: no ProjectileMovement template in the class data, the native constructor values taken",
        );
    }
    let pmv = obj(Some(cs), "ProjectileMovement").unwrap_or(&NO_PROPS);
    let pmo = Some(pmv);
    pr.follows = truthy(pmv.get("bRotationFollowsVelocity"));
    // CompSpawnActor hands the notify's InitialSpeed / MaxSpeed to the ProjectileMovement (HandleActorPostSpawn
    // @8407 BPI_SetInitialMovement -> SpawnedActorParent @20742-20840); a field the notify leaves out is the
    // AnimNotify_Spawner CDO's 0. Comp_Spawner CompSpawnActor with ChargeAffectsInitialSpeed: Lerp(speed, speed x
    // ChargeSpeedMultiplier, clamp(the instigator's PercentCharged, 0, 1)), the hold charge's ChargePercent in its
    // place with HoldChargeOverrideSpeedMod (HandleActorPostSpawn @4494-4684).
    let mut speed = initial;
    let csm = num(n, "ChargeSpeedMultiplier").unwrap_or(1.0);
    let charge_alpha = if truthy(notify.get("ChargeAffectsInitialSpeed")) {
        math::min(math::max(if held.is_nan() { charge } else { held }, 0.0), 1.0)
    } else {
        0.0
    };
    speed += (speed * csm - speed) * charge_alpha;
    // HandleActorPostSpawn 5559-6268: the launch speed is VSize(InitialMovementDirection x InitialSpeed); the velocity
    // is Normal(direction) x it.
    let dir = obj(n, "InitialMovementDirection");
    let (dir_y, dir_z) = match dir {
        Some(_) => (num(dir, "Y").unwrap_or(0.0), num(dir, "Z").unwrap_or(0.0)),
        None => (1.0, 0.0),
    };
    let mut launch = math::ue::size2d([dir_y * speed, dir_z * speed]);
    // AddMomentumAlpha (4722-5029, 5614) and MovementSpeedAddsToInitialSpeed (5836, 6314-6424).
    let momentum = if owner.is_some() { num(n, "AddMomentumAlpha").unwrap_or(0.0) } else { 0.0 };
    let side = owner.map_or(1.0, |f| fighter_body(w, f).0);
    let mut lvx = dir_y * speed * side;
    let mut lvy = dir_z * speed;
    if momentum != 0.0 {
        let (_, _, _, ovx, ovy) = fighter_body(w, owner.expect("an owner"));
        let base = initial;
        let my = dir_y * base + ovx * side * momentum;
        let mz = dir_z * base + ovy * momentum;
        let size = math::ue::size2d([my, mz]);
        launch = size + (size * csm - size) * charge_alpha;
        // the Normal node (tolerance 0.0001)
        let u = math::ue::normal([0.0, my, mz], math::ue::KINDA_SMALL_NUMBER);
        lvx = u[1] * launch * side;
        lvy = u[2] * launch;
    }
    let adds = truthy(notify.get("MovementSpeedAddsToInitialSpeed"));
    if adds && let Some(f) = owner {
        let (_, _, _, ovx, ovy) = fighter_body(w, f);
        lvx += ovx;
        lvy += ovy;
        launch = math::max(launch, math::ue::size2d([lvx, lvy]));
    }
    pr.max_speed =
        math::max(if adds { math::ue::size2d([lvx, lvy]) } else { launch }, max + (max * csm - max) * charge_alpha);
    pr.init_speed = Some(launch);
    // Which component deals: Comp_Spawner HandleActorPostSpawn (12145-12532) BPI_Enable2DHitboxes(the actor's
    // Supports2DHitboxes and the owner fighter's BPI_Use2DHitboxes).
    // Unverified: an attached actor is taken to stay on the plane (the box data carries no socket depth, and the
    // attach translation's X is dropped).
    pr.sphere_3d = !(truthy(p.get("Supports2DHitboxes")) && owner.is_some_and(|f| char_def(w, f).use_2d));
    let tether = opts.tether;
    pr.whip = tether.is_some();
    pr.die_on_bounce = matches!(class, ProjectileClass::C017CannonShot | ProjectileClass::C017DiscShot);
    // C019_WhipProjectile has no HitBox2D: its HitBox is a sphere (radius 110) under Cylinder (scale (0.5, 0.5, 1),
    // pitch 90) under DamageBox under the root; GetScaledSphereRadius takes the smallest axis: 110 x 0.5 = 55.
    let sphere = tether.as_ref().and_then(|t| t.hit_sphere).map(|s| w.names.str(s).to_owned());
    let wscale = sphere.as_deref().map_or(1.0, |s| min_axis(&attach_parent(cs, s).scale));
    // HitBoxesOn binds the components tagged with HitBoxTags ([HitBox]) in 3D and HitBox2D in 2D (FNames: the tag's
    // case is not compared): in 3D the one component tagged HitBox, a class's HitBox2D notwithstanding
    // (FinnGemBoostUseProjectile's HitBox sphere 100 over its HitBox2D 42, C018CannonBlast's CapsuleHitbox); in 2D a
    // class with no HitBox2D component takes the one tagged HitBox2D. Its sphere's or capsule's radius at its attach
    // parent's scale.
    let tagged = if sphere.is_some() {
        None
    } else if pr.sphere_3d {
        tagged_hit_box(cs, "hitbox")
    } else if !truthy(cs.get("HitBox2D")) {
        tagged_hit_box(cs, "hitbox2d")
    } else {
        None
    };
    let hbs = num(obj(pp, "HitBoxScale"), "X").unwrap_or(1.0);
    pr.hit_radius = match tagged {
        Some((name, r)) => r * min_axis(&attach_parent(cs, name).scale) * pr.scale * hbs,
        None => {
            let r = num(obj(Some(cs), "HitBox2D"), "CapsuleRadius")
                .or_else(|| match &sphere {
                    Some(s) => num(obj(Some(cs), s), "SphereRadius"),
                    None => Some(0.0),
                })
                .unwrap_or(0.0);
            r * wscale * pr.scale * hbs
        }
    };
    if let Some(t) = &tether {
        pr.can_control = truthy(p.get("CanControl"));
        pr.initial_speed = launch;
        pr.snap_tick = lf(t.travel);
        pr.kb_seq = w.fighter(owner.expect("a tether has an owner")).attackers.knocked_back_count;
    }
    pr.hit_on = truthy(p.get("HitBoxOn?"));
    pr.hit_var = pr.hit_on;
    // USphereComponent::GetScaledSphereRadius: the radius x the smallest axis of the world scale.
    let hp = attach_parent(cs, "HurtBox");
    let hurt = obj(Some(cs), "HurtBox");
    // A begin overlap needs both components' bGenerateOverlapEvents: a HurtBox whose template has it off counts as
    // none (GrappleHookProjectileV2 and GrappleHookProjectile_Perk: ParentProjectilePhysics turns it on only in
    // BPI_OnInitFromPool @2209, and neither class is pooled). ShaggySandwichProjectile's BeginPlay turns it on (@1394,
    // ahead of the spawn's UpdateOverlaps), its Rage child's through the parent's BeginPlay.
    let hurt_on = hurt.and_then(|h| h.get("bGenerateOverlapEvents")) != Some(&Value::Bool(false))
        || matches!(
            pr.class,
            ProjectileClass::ShaggySandwichProjectile | ProjectileClass::ShaggyRageSandwichProjectile
        );
    let hr = if truthy(cs.get("HurtBox")) && hurt_on { num(hurt, "SphereRadius").unwrap_or(32.0) } else { 0.0 };
    pr.hurt_radius = hr * min_axis(&hp.scale) * pr.scale;
    // The centre: the parent's place plus the HurtBox's own RelativeLocation through the parent's scale and turn.
    // Its Z alone is read: of the classes with a HurtBox offset along X each overrides hurtX / hurtY.
    let hl = obj(hurt, "RelativeLocation");
    let hs = [
        hp.scale[0] * num(hl, "X").unwrap_or(0.0),
        hp.scale[1] * num(hl, "Y").unwrap_or(0.0),
        hp.scale[2] * num(hl, "Z").unwrap_or(0.0),
    ];
    pr.hurt_off_y = (hp.loc[2] + hs[0] * hp.rot[2] + hs[1] * hp.rot[5] + hs[2] * hp.rot[8]) * pr.scale;
    pr.attack_type = math::to_int32(num(pp, "ProjectileAttackType").unwrap_or(0.0));
    if cls_s == "C017_Plasma_Shot_C" {
        pr.plasma = true;
        pr.base_hit = pr.hit_radius;
        pr.base_hurt = pr.hurt_radius;
        pr.slow_tick = lf(num(pp, "SlowDownDelay").unwrap_or(0.0));
    }
    pr.health = num(pp, "Health").unwrap_or(0.0);
    // ProjectileMovement.ProjectileGravityScale at the spawn is the class's template value, else the native
    // constructor's 1.0 (UProjectileMovementComponent 142983a00 +0x10c). ShaggySandwichProjectile's graph alone
    // (its child the Rage sandwich with it) has BeginPlay's Delay(DurationToGravity) @346 ->
    // ProjectileGravityScale = 'Projectile Gravity Scale' @72: the classes that have that variable.
    pr.grav_scale = num(pmo, "ProjectileGravityScale").unwrap_or(1.0);
    pr.grav_tick =
        if p.contains_key("Projectile Gravity Scale") { lf(num(pp, "DurationToGravity").unwrap_or(0.0)) } else { -1 };
    if cls_s.contains("CookGrenade") {
        // ReceiveBeginPlay: K2_SetTimer(DetonateTimerFunction, GrenadeLifetime), Delay(GrenadeLifetime - 0.5) -> the
        // tell sound, Delay(0.0667) then Delay(0.2) -> StartUpDelayArm and the fuse loop.
        let fuse = num(pp, "GrenadeLifetime").unwrap_or(0.0);
        let mut k = 0;
        while !(crate::consts::clock(crate::real(k)) > f64::from(fuse)) {
            k += 1;
        }
        pr.fuse_ticks = k + 1;
        pr.tell_tick = lf(fuse - 0.5);
        pr.loop_tick = lf(0.0667) + lf(0.2);
        pr.rotate_gate = truthy(p.get("RotateGate"));
        pr.grenade = true;
    }
    if cls_s.contains("BlasterChargeProjectile") {
        pr.charge_shot = true;
    }
    // C019_SplitterFollowup GetSpawnMaxLifetime: Lerp(MinLifetimeClamp, MaxLifetimeClamp, GetChargeValue(4)).
    let life = match opts.life {
        Some(l) => l,
        None => {
            if followup || spit {
                let lo = num(pp, "MinLifetimeClamp").unwrap_or(0.0);
                lo + (num(pp, "MaxLifetimeClamp").unwrap_or(0.0) - lo) * charge
            } else {
                num(pp, "MaxLifetime").unwrap_or(0.0)
            }
        }
    };
    pr.bounce = if truthy(pmv.get("bShouldBounce")) && !truthy(p.get("DestroyOnCollideWithEnvironment")) {
        num(pmo, "Bounciness").unwrap_or(0.6)
    } else {
        -1.0
    };
    pr.friction = num(pmo, "Friction").unwrap_or(0.2);
    pr.stop_speed = num(pmo, "BounceVelocityStopSimulatingThreshold").unwrap_or(5.0);
    pr.angle_friction = truthy(pmv.get("bBounceAngleAffectsFriction"));
    pr.min_friction = num(pmo, "MinFrictionFraction").unwrap_or(0.0);
    if split {
        // BeginPlay: without CanSplit the hit is replaced first (UpdateHitBoxDamage: 3.5 damage, no knockback,
        // HitReaction 6, no hitpause or hitstun); then Delay(DelayBeforeSplit).
        if !truthy(p.get("CanSplit")) {
            pr.hit = crate::combat::HitPacket {
                sparse: true,
                damage: Some(3.5),
                base_knock_back: Some(0.0),
                knock_back_scaling: Some(0.0),
                knock_back_direction_vector: Some(DataVec { x: Some(0.0), y: Some(0.0), z: None }),
                attack_tag: Some(crate::name!("None")),
                hit_reaction: Some(DataEnum::Value(6)),
                hit_pause_time: Some(0.0),
                hit_stun_mult: Some(0.0),
                play_on_hit_effects: Some(true),
                apply_hit_pause_to_damage_dealer: Some(true),
                hit_each_actor_only_once: Some(true),
                account_for_velocity_with_vacuum_knockback: Some(true),
                kbi_multiplier: Some(1.0),
                ..Default::default()
            };
        }
        pr.split_tick = lf(num(pp, "DelayBeforeSplit").unwrap_or(0.0));
        pr.destroy_on_enemy_hit = true;
    }
    pr.life_ticks = lifetime_at(&pr, life, 1);
    if life > 0.0 {
        pr.max_life = Some(life);
    }

    // Location: the socket on the owner's pose at the end of its step, plus the notify translation (local Y is the
    // facing direction).
    // Unverified: an actor the world spawned with an identity rotation faces sim +x (its forward is UE X, off the
    // play plane); it has an origin and no socket.
    pr.facing = owner.map_or(1.0, |f| fighter_body(w, f).0);
    let t = obj(tr, "Translation");
    let (t_y, t_z) = (num(t, "Y").unwrap_or(0.0), num(t, "Z").unwrap_or(0.0));
    // Socket names are FNames, compared without case (Creature_Firewall_Spit_Montage names 'snout' for Snout).
    let df = data_fighter(&pr);
    let bs = sampler(w, df);
    let n_socket = text(n, "Socket");
    let si = socket_index(w, bs, &format!("socket:{}", n_socket.unwrap_or("")));
    pr.socket = sock(si);
    let t_socket = tether.as_ref().and_then(|t| t.socket);
    pr.return_socket = match t_socket {
        Some(s) if !w.names.str(s).is_empty() => sock(socket_index(w, bs, &format!("socket:{}", w.names.str(s)))),
        _ => sock(si),
    };
    pr.tether = tether;
    pr.rel = (t_y, t_z);
    if pr.attached {
        follow(w, &mut pr);
        pr.prev_x = pr.x;
        pr.prev_y = pr.y;
        pr.vx = 0.0;
        pr.vy = 0.0;
        pr.attached_vel = Some((lvx, lvy));
        return pr;
    }
    let no_socket = n_socket.is_none_or(|s| s.is_empty() || s == "None");
    if origin.is_none()
        && let Some(rot) = obj(tr, "Rotation")
    {
        pr.spawn_side = Some(spawn_yaw_side(rot, fighter_body(w, df).0));
    }
    let (mut fwd, mut up) = (0.0, 0.0);
    let pose = match (pr.socket, origin) {
        (Some(_), None) => crate::fighter::anim::sampled_boxes(w, df),
        _ => None,
    };
    if let (Some(pose), Some(s)) = (pose, pr.socket) {
        let o = offset_of(bs, s);
        fwd = crate::real(pose[o]);
        up = crate::real(pose[o + 1]);
        crate::fighter::anim::put_pose(w, pose);
    } else if origin.is_none() && !no_socket {
        crate::actors::world_actor::warn("spawn socket missing");
    }
    // The socket is on the Mesh component: it carries the mesh's relative offset. With socket None Comp_Spawner takes
    // the attach actor's location - (0, 0, 100) in its place (CompSpawnActor, ubergraph 978-2417).
    let (x, y) = match origin {
        Some(o) => (o.x, o.y),
        None => {
            let (facing, fx, _, _, _) = fighter_body(w, df);
            if no_socket {
                (fx + t_y * facing, crate::fighter::actor_y(w, df) - 100.0 + t_z)
            } else {
                (crate::fighter::mesh_wx(w, df) + (fwd + t_y) * facing, crate::fighter::mesh_wy(w, df) + up + t_z)
            }
        }
    };
    pr.x = x;
    pr.prev_x = x;
    pr.y = y;
    pr.prev_y = y;
    pr.vx = lvx;
    pr.vy = lvy;
    pr.pitch = if pr.follows { math::ue::vector_rotation([0.0, pr.vx.abs(), pr.vy])[0] } else { 0.0 };
    pr
}

// ---- the getters

/// `instigator`: GetInstigator of the actor (what a hit's credit, damage record, juggle count, taken-damage `by`,
/// knockback socket and death VFX colour read); GetOwner (`owner`) stays the team, decay, hit buffs and hit pause.
pub fn instigator(p: &Projectile) -> Option<FighterId> {
    // A safe the world spawned keeps no instigator once a hit gives it an owner.
    if matches!(&p.ext, ProjectileExt::BugsBunnySafe(s) if s.orig_inst < 0) {
        return None;
    }
    match p.inst_override {
        OwnerOverride::Unset | OwnerOverride::None => p.owner,
        OwnerOverride::Fighter(f) => Some(f),
    }
}

/// `spawner`: whose SpawnedActors the actor is in.
pub fn spawner(p: &Projectile) -> Option<FighterId> {
    if matches!(&p.ext, ProjectileExt::BugsBunnySafe(s) if s.orig_inst < 0) {
        return None;
    }
    match p.list_owner {
        OwnerOverride::Unset | OwnerOverride::None => p.owner,
        OwnerOverride::Fighter(f) => Some(f),
    }
}

/// `hurtY`: the root's place plus the HurtBox's own and its parents' Z offsets. `hurtX`: the root's. A child class
/// whose HurtBox sits ahead of the root has its own (its `ProjectileExt` arm, with its port).
pub fn hurt_y(p: &Projectile) -> Real {
    match &p.ext {
        // FireWall.hurtY: the HurtBox capsule's centre
        ProjectileExt::FireWall(f) => p.y + f.hurt_z,
        ProjectileExt::Rocket(r) => r.hurt_y(p),
        _ => p.y + p.hurt_off_y,
    }
}

/// `hurtX`.
pub fn hurt_x(p: &Projectile) -> Real {
    if let ProjectileExt::Rocket(r) = &p.ext {
        return r.hurt_x(p);
    }
    p.x
}

/// `hitX` / `hitY`: the dealing hit box's centre (HitBox, or HitBox2D in 2D): the root's. Creature_WreckingBall_C's
/// override: its ally's actor location while attached (`perks_ball::hit_xy`).
pub fn hit_xy(w: &World, p: &Projectile) -> (Real, Real) {
    if matches!(p.ext, ProjectileExt::WreckingBall(_)) {
        return crate::chars::creature::perks_ball::hit_xy(w, p);
    }
    if matches!(p.ext, ProjectileExt::BoomBox(_)) {
        return (p.x, (p.y + (crate::chars::stripe::dash::HIT_Z * p.scale)));
    }
    if matches!(p.ext, ProjectileExt::MeeseeksRun(_)) {
        return crate::items::meeseeks::hit_xy(p);
    }
    if let ProjectileExt::FartBomb(b) = &p.ext {
        return (p.x, crate::chars::rick::fart_bomb::hit_y(p, b));
    }
    if let ProjectileExt::Rocket(r) = &p.ext {
        return r.hit_xy(p);
    }
    // C023A_Music_Projectile_C: its ally's actor location while attached
    if matches!(p.ext, ProjectileExt::MusicProjectile(_)) {
        return crate::chars::gizmo::music::hit_xy(w, p);
    }
    // C023A_PopCorn_C: ShotGunHitBox's place while the shot is on
    if matches!(p.ext, ProjectileExt::Popcorn(_)) {
        return crate::chars::gizmo::popcorn::hit_xy(p);
    }
    (p.x, p.y)
}

/// `hitShapes`: the hit box's shapes against a fighter's hurt capsules: the sphere on the play plane (sphere3D) or
/// HitBox2D's circle, at hitX / hitY.
pub fn hit_shapes(w: &World, p: &Projectile, out: &mut Vec<WorldSphere>) {
    if matches!(p.ext, ProjectileExt::StewSpitAir(_)) {
        return crate::chars::taz::projectiles::hit_shapes(w, p, out);
    }
    if matches!(p.ext, ProjectileExt::MeeseeksSummon(_)) {
        return crate::chars::rick::meeseeks::hit_shapes(w, p, out);
    }
    if let ProjectileExt::Rocket(r) = &p.ext {
        return r.hit_shapes(p, out);
    }
    if matches!(p.ext, ProjectileExt::BugsShockwave(_)) {
        return crate::chars::bugs::perks::shockwave_shapes(p, out);
    }
    if matches!(p.ext, ProjectileExt::Blaster(_)) {
        return crate::chars::marvin::blaster::hit_shapes(w, p, out);
    }
    let (x, y) = hit_xy(w, p);
    let seg = tagged_capsule_axis(w, p).map(|[ax, ay]| [x + ax, y + ay, 0.0]);
    let (x, y) = match seg {
        Some([bx, by, _]) => (x - (bx - x), y - (by - y)),
        None => (x, y),
    };
    out.push(WorldSphere { x, y, z: p.sphere_3d.then_some(0.0), r: p.hit_radius, seg, ..Default::default() });
}

/// In 3D, where the dealing component (the one tagged HitBox) is a capsule (C018CannonBlast's CapsuleHitbox, radius
/// 100, half height 275): half its core in the play plane, from the centre to one end. Its long axis is the
/// component's Z through its RelativeRotation, then the actor's pitch and yaw; the core's half length is
/// GetScaledCapsuleHalfHeight less the radius, at the scale the radius has (`hit_radius`).
// Unverified: the axis's depth part is dropped and the component's RelativeLocation (CapsuleHitbox's X 1) is not
// read: the core is about the hit place.
fn tagged_capsule_axis(w: &World, p: &Projectile) -> Option<[Real; 2]> {
    if !p.sphere_3d {
        return None;
    }
    let cs = &class_def(w, p)?.components;
    let (name, r) = tagged_hit_box(cs, "hitbox")?;
    let c = obj(Some(cs), name);
    let half = num(c, "CapsuleHalfHeight")?;
    if !(half > r) || !(r > 0.0) {
        return None;
    }
    let k = p.hit_radius / r * (half - r);
    // The component's Z axis turned by its RelativeRotation: its X (the actor's forward) and Z (up) parts.
    let rot = obj(c, "RelativeRotation");
    let deg = |n: &str| num(rot, n).unwrap_or(0.0);
    let q = math::ue::rotator_quaternion([deg("Pitch"), deg("Yaw"), deg("Roll")]);
    let z = math::ue::quat_rotate_vector(q, [0.0, 0.0, 1.0]);
    let [ax, ay] = math::ue::turn_by_pitch(p.pitch, [z[0], z[2]]);
    Some([ax * yaw_x(p) * k, ay * k])
}

/// `hitNear`: the point of the hit box's core nearest (x, y): its centre, or the nearest of a tagged capsule's core.
pub fn hit_near(w: &World, p: &Projectile, x: Real, y: Real) -> V2 {
    if matches!(p.ext, ProjectileExt::MeeseeksSummon(_)) {
        return crate::chars::rick::meeseeks::hit_near(w, p, x, y);
    }
    if let ProjectileExt::Rocket(r) = &p.ext {
        return r.hit_near(p, x, y);
    }
    if matches!(p.ext, ProjectileExt::BugsShockwave(_)) {
        return crate::chars::bugs::perks::shockwave_near(p, x, y);
    }
    let (cx, cy) = hit_xy(w, p);
    // A tagged capsule's core: the point of its segment nearest (x, y).
    if let Some([ax, ay]) = tagged_capsule_axis(w, p) {
        let l2 = ax * ax + ay * ay;
        let t = if l2 > 0.0 { math::max(-1.0, math::min(1.0, ((x - cx) * ax + (y - cy) * ay) / l2)) } else { 0.0 };
        return V2 { x: cx + ax * t, y: cy + ay * t };
    }
    V2 { x: cx, y: cy }
}

/// `capsuleRadius` / `capsuleHalfHeight`: the root Capsule's scaled size.
pub fn capsule_radius(p: &Projectile) -> Real {
    p.cap_r * p.scale
}

pub fn capsule_half_height(p: &Projectile) -> Real {
    p.cap_h * p.scale
}

/// `capsuleOn`: a query meets the root Capsule.
pub fn capsule_on(p: &Projectile) -> bool {
    p.cap_collides && !p.dead && !p.no_collision
}

/// `hurtDist2`: the squared distance in the play plane from (x, y) to the HurtBox's core (its sphere's centre).
pub fn hurt_dist2(p: &Projectile, x: Real, y: Real) -> Real {
    if let ProjectileExt::FireWall(f) = &p.ext {
        // FireWall.hurtDist2: the distance to the capsule's axis, the segment of half length `hurt_half` about hurtY
        let dx = x - p.x;
        let dy = math::max((y - hurt_y(p)).abs() - f.hurt_half, 0.0);
        return dx * dx + dy * dy;
    }
    // C023A_PopCorn_C: its HurtBox capsule
    if matches!(p.ext, ProjectileExt::Popcorn(_)) {
        return crate::chars::gizmo::popcorn::hurt_dist2(p, x, y);
    }
    let dx = x - hurt_x(p);
    let dy = y - hurt_y(p);
    dx * dx + dy * dy
}

/// `hitboxActive`.
/// C016_SassBubble's override: false (its HitBox sphere generates no overlaps; its Box lives inside ActivateObject).
/// TetherProjectile's: false (no component carries the HitBox tag). JerryRocket's: the parent's while its hit boxes
/// begin overlaps (`rocket::overlaps_live`).
pub fn hitbox_active(p: &Projectile) -> bool {
    if matches!(p.ext, ProjectileExt::MeeseeksSummon(_)) {
        return crate::chars::rick::meeseeks::hitbox_active(p);
    }
    let on = p.hit_on
        && !p.dead
        && !p.no_collision
        && !matches!(
            p.ext,
            ProjectileExt::SassBubble(_) | ProjectileExt::TetherProjectile(_) | ProjectileExt::StevenTether(_)
        );
    match &p.ext {
        ProjectileExt::JerryRocket(s) => on && crate::chars::tom_and_jerry::rocket::overlaps_live(p, s),
        _ => on,
    }
}

/// `hitCollision`: the hitbox component's collision (UpdateHitbox follows HitBoxOn?), whatever HasHitAFighter says.
/// C016_SassBubble's and TetherProjectile's overrides: false.
pub fn hit_collision(p: &Projectile) -> bool {
    p.hit_var
        && !p.dead
        && !p.no_collision
        && !matches!(
            p.ext,
            ProjectileExt::SassBubble(_) | ProjectileExt::TetherProjectile(_) | ProjectileExt::StevenTether(_)
        )
}

/// `canClank`: BPI_CanProjectileClank (SpawnedActorParent): the HitBoxOn? variable && CanProjectileClank;
/// C019_CookGrenade's override: CanProjectileClank alone; TetherProjectile's: false.
pub fn can_clank(w: &World, p: &Projectile) -> bool {
    if matches!(p.ext, ProjectileExt::TetherProjectile(_) | ProjectileExt::StevenTether(_)) {
        return false;
    }
    (p.grenade || p.hit_var) && truthy(class_p(w, p).get("CanProjectileClank"))
}

/// `clankHit`: BPI_GetProjectileClankHitbox, C019_CookGrenade's own ProjectileClankHitbox (`Some`), else HitBoxDamage
/// (`None`: `p.hit`).
pub fn clank_hit_props<'a>(w: &'a World, p: &'a Projectile) -> Option<&'a Props> {
    if p.grenade { obj(Some(class_p(w, p)), "ProjectileClankHitbox") } else { None }
}

/// `clankRows`: its OnHitBuffs output, the class's own 'On Hit Buffs' rows (SpawnedActorParent @54);
/// C019_CookGrenade's override returns none (@73).
pub fn clank_rows<'a>(w: &'a World, p: &'a Projectile) -> &'a [Value] {
    if p.grenade {
        return &[];
    }
    obj(Some(class_p(w, p)), "On Hit Buffs").and_then(|o| o.get("OnHitBuffs")).and_then(Value::as_arr).unwrap_or(&[])
}

/// `receiveDamage`: ParentProjectilePhysics ReceiveAnyDamage (UGameplayStatics::ApplyDamage skips a damage of 0). A
/// Heavy projectile only takes damage from a Heavy one, and a Light causer only damages a Light one. True when the
/// actor is destroyed now (ReleaseToPool); while clanking the destroy is queued instead.
/// A basketball's ReceiveAnyDamage runs its hit response behind the parent's when the parent did not destroy it
/// (`ball::damaged`).
pub fn receive_damage(w: &mut World, id: ProjectileId, dmg: Real, causer_type: i32) -> bool {
    if dmg == 0.0 {
        return false;
    }
    // Arya's knife's ReceiveAnyDamage @9609 has no parent call and is never reached (`knife::damaged`).
    if matches!(pm_ref(w, id).ext, ProjectileExt::Knife(_)) {
        return crate::chars::arya::knife::damaged();
    }
    // Velma's mark's ReceiveAnyDamage @4208 has no parent call (`mark::damaged`).
    if matches!(pm_ref(w, id).ext, ProjectileExt::Mark(_)) {
        return crate::chars::velma::mark::damaged();
    }
    // A Firebolt's ReceiveAnyDamage @6014 has no parent call: its gate (`fire::damaged`).
    if matches!(pm_ref(w, id).ext, ProjectileExt::Firebolt(_)) {
        return crate::chars::creature::fire::damaged(w, id);
    }
    if receive_damage_parent(w, id, dmg, causer_type) {
        return true;
    }
    matches!(pm_ref(w, id).ext, ProjectileExt::Ball(_)) && crate::chars::lebron::ball::damaged(w, id)
}

/// ParentProjectilePhysics ReceiveAnyDamage (`dmg` not 0).
fn receive_damage_parent(w: &mut World, id: ProjectileId, dmg: Real, causer_type: i32) -> bool {
    let takes = truthy(class_p(w, pm_ref(w, id)).get("TakesDamage"));
    let p = pm(w, id);
    // C019_CookGrenade ReceiveAnyDamage: Detonate Server, or QueueDestroyAfterClank while clanking.
    if p.grenade {
        return destroy_or_queue(p);
    }
    if p.attack_type == 1 && causer_type != 1 {
        return false;
    }
    if causer_type == 2 && p.attack_type != 2 {
        return false;
    }
    if !takes {
        return false;
    }
    p.health -= dmg;
    p.health <= 0.0 && destroy_or_queue(p)
}

/// The otherType HitNonPlayer takes when the HitActor is no SpawnedActorParent.
pub const NOT_SPAWNED_ACTOR: i32 = -1;

/// `hitNonPlayer`: ParentProjectilePhysics HitNonPlayer with another spawned actor: DestroyOnHitHeavyProjectile
/// against a Heavy one; DestroyOnHitProjectile against the rest, except that a Light one only stops another Light
/// one. A HitActor that is no SpawnedActorParent (NOT_SPAWNED_ACTOR) @520: DestroyOnHitDestructibleEnvironment.
/// A basketball's OnHitAnything runs its hit response behind the parent's for a SpawnedActorParent
/// (`ball::hit_non_player`). A Firebolt's HitNonPlayer runs its gate behind the parent's whatever the parent said
/// (`fire::hit_non_player`).
pub fn hit_non_player(w: &mut World, id: ProjectileId, other_type: i32) -> bool {
    if matches!(pm_ref(w, id).ext, ProjectileExt::Firebolt(_)) {
        let parent = hit_non_player_parent(w, id, other_type);
        return crate::chars::creature::fire::hit_non_player(w, id, other_type, parent);
    }
    if hit_non_player_parent(w, id, other_type) {
        return true;
    }
    matches!(pm_ref(w, id).ext, ProjectileExt::Ball(_)) && crate::chars::lebron::ball::hit_non_player(w, id, other_type)
}

/// ParentProjectilePhysics HitNonPlayer (and C019BlasterChargeProjectile's override).
fn hit_non_player_parent(w: &mut World, id: ProjectileId, other_type: i32) -> bool {
    // C019BlasterChargeProjectile's override (no parent call): StopAimMoveMulticast and UpdateProjectileVelocity.
    if pm_ref(w, id).charge_shot {
        stop_aim(pm(w, id));
        fly_on(w, id);
        return false;
    }
    let cp = class_p(w, pm_ref(w, id));
    if other_type == NOT_SPAWNED_ACTOR {
        return truthy(cp.get("DestroyOnHitDestructibleEnvironment"));
    }
    let heavy = truthy(cp.get("DestroyOnHitHeavyProjectile"));
    let any = truthy(cp.get("DestroyOnHitProjectile"));
    let p = pm(w, id);
    let destroy = if other_type == 1 && heavy { true } else { (other_type != 2 || p.attack_type == 2) && any };
    destroy && destroy_or_queue(p)
}

fn destroy_or_queue(p: &mut Projectile) -> bool {
    if !p.clanking {
        return true;
    }
    p.queue_destroy = true;
    false
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn pm_ref(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

/// `follow`: attached, AttachToComponent(Mesh, socket, SnapToTarget location) and the notify's relative location in
/// the actor frame (Z up, Y forward), as a relative location under the mesh, so times the mesh's scale (game memory:
/// K2_GetActorLocation of Shaggy's burst at Z 190.66 with Shaggy standing at 210.412, Root (-193) + 150 x 1.155).
/// Gives the facing and the place.
fn follow_at(w: &World, p: &Projectile) -> (Real, Real, Real) {
    let f = p.parent_fighter.or(p.owner).expect("an attached projectile has an owner");
    let s = char_def(w, f).mesh_scale;
    let (facing, _, _, _, _) = fighter_body(w, f);
    // The sampler the socket was looked up on: the parent fighter's, else the class data's fighter's.
    let bs = sampler(w, if p.parent_fighter.is_some() { f } else { data_fighter(p) });
    let pose = p.socket.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
    let (mut fwd, mut up) = (0.0, 0.0);
    if let (Some(pose), Some(si)) = (pose, p.socket) {
        let o = offset_of(bs, si);
        fwd = crate::real(pose[o]);
        up = crate::real(pose[o + 1]);
        crate::fighter::anim::put_pose(w, pose);
    } else {
        crate::actors::world_actor::warn("spawn socket missing (attached)");
    }
    (
        facing,
        crate::fighter::mesh_wx(w, f) + (fwd + p.rel.0 * crate::math::narrow(s)) * facing,
        crate::fighter::mesh_wy(w, f) + up + p.rel.1 * crate::math::narrow(s),
    )
}

fn follow(w: &World, p: &mut Projectile) {
    let (facing, x, y) = follow_at(w, p);
    p.facing = facing;
    p.x = x;
    p.y = y;
}

fn follow_id(w: &mut World, id: ProjectileId) {
    let (facing, x, y) = follow_at(w, pm_ref(w, id));
    let p = pm(w, id);
    p.facing = facing;
    p.x = x;
    p.y = y;
}

/// `attachToSocket`: K2_AttachToComponent(the Mesh of f, socket, SnapToTarget) and K2_SetActorRelativeLocation((0,
/// relY, relZ)) on an actor that is not attached (C023A_Car_SpawnedActor UpdateDriverInfo @327-888, a new driver):
/// it rides the socket of f's pose (its own sampler's for a fighter other than the owner), its movement deactivated
/// with the Velocity it had (the attached actor's `attached_vel`).
pub fn attach_to_socket(w: &mut World, id: ProjectileId, f: FighterId, socket: &str, rel_y: Real, rel_z: Real) {
    let own = pm_ref(w, id).owner == Some(f);
    let bs = sampler(w, if own { data_fighter(pm_ref(w, id)) } else { f });
    let si = sock(socket_index(w, bs, &format!("socket:{socket}")));
    let p = pm(w, id);
    p.parent_fighter = if own { None } else { Some(f) };
    p.socket = si;
    p.rel = (rel_y, rel_z);
    p.attached_vel = Some((p.vx, p.vy));
    p.attached = true;
    p.vx = 0.0;
    p.vy = 0.0;
    follow_id(w, id);
}

/// `activate`: BPI_ActivateObject (Shaggy_AllyTeleportBurst ActivateObject): Activated, visuals and the blast, then
/// Delay(0.02) -> HitBoxOn? and UpdateHitbox. The owner's notify calls it before the actor's tick of that frame, so
/// the Delay counts that frame (the Delay rule): 2 actor ticks.
/// C016_SassBubble's ActivateObject has no Delay of its own: Activated alone. The electric chains' override does
/// nothing (`chars::black_adam::chain`: their kind's activate does the work).
pub fn activate(p: &mut Projectile) {
    if matches!(p.ext, ProjectileExt::Chain(_)) {
        return;
    }
    if matches!(p.ext, ProjectileExt::SassBubble(_)) {
        p.activated = true;
        return;
    }
    // Velma_SassBubblev2's ActivateObject has no parent call either (`gadgets::bubble_activate`).
    if matches!(p.ext, ProjectileExt::VelmaSassBubble(_)) {
        crate::chars::velma::gadgets::bubble_activate(p);
        return;
    }
    if p.activate_tick >= 0 {
        return;
    }
    p.activated = true;
    p.activate_tick = p.age + lf(0.02);
}

/// `hitVelocity`: the velocity pfgDoDamage reads from the hitbox's owner: BPI_GetParentVelocity when it is not zero,
/// else GetVelocity. AActor::GetVelocity is the root's ComponentVelocity, which a ProjectileMovement writes at the
/// end of its own tick (UpdateComponentVelocity) and nothing else does: for C019BlasterChargeProjectile, whose
/// movement is deactivated or has its Velocity set from Blueprint between ticks, it is not the movement's Velocity.
/// Its SpawnedActorParentVelocity is a copy of that ComponentVelocity from the first actor tick on, so a zero one has
/// a zero GetVelocity behind it.
pub fn hit_velocity(p: &Projectile) -> (Real, Real) {
    // HealGrenade: GetVelocity of the simulating root: its body's.
    if matches!(p.ext, ProjectileExt::HealGrenade(_)) {
        return (p.vx, p.vy);
    }
    if matches!(p.ext, ProjectileExt::MeeseeksRun(_)) {
        return crate::items::meeseeks::hit_velocity(p);
    }
    // Meeseeks_SpawnedActor (chars/rick/meeseeks.rs): GetVelocity of the simulating body while it runs.
    if let Some(v) = crate::chars::rick::meeseeks::hit_velocity(p) {
        return v;
    }
    // BugsBunnySafe: GetVelocity of a simulating root is its body's.
    // Unverified: BPI_GetParentVelocity (the parent's tick copy) is taken as the body's velocity of this frame.
    if let ProjectileExt::BugsBunnySafe(s) = &p.ext
        && s.sim
    {
        return (s.bvx, s.bvy);
    }
    if p.charge_shot || p.parent_vx != 0.0 || p.parent_vy != 0.0 {
        return (p.parent_vx, p.parent_vy);
    }
    p.comp_v.unwrap_or((p.vx, p.vy))
}

/// `launchFromHit`: C019_CookGrenade LaunchFromHitMulticast (OnHitByEnemy / OnHitByAlly, ubergraph 631): MaxSpeed
/// 3500, RotateGate, ProjectileGravityScale 3, Friction 0, Bounciness 1 and the new velocity; RetriggerableDelay(0.2)
/// then sets Bounciness 0.5 and Friction 0.75. OnHitByAlly first (behind CanTurnOnHitBox, checked by the world): the
/// gate closes for Delay(0.25), TurnOffKB is added to the movement's bounce delegate, HitBoxOn? = true with
/// UpdateHitbox (HitBoxesOn) and the PS_DamageVisual component shown; TurnOffKB at the next bounce removes itself and
/// turns the hitbox and the visual off again. The launch writes ProjectileMovement.Velocity alone (ubergraph 8792):
/// GetVelocity keeps the velocity of the movement's last tick (zero before the first) until its next.
// Unverified: the Delays count from this frame. Zero before the first movement tick: the movement's constructor
// Velocity (1, 0, 0) turned by the root's rotation in InitializeComponent is the root's ComponentVelocity at the
// spawn, so a launch and hit in the spawn frame would take the spawn's forward in the build.
pub fn launch_from_hit(p: &mut Projectile, vx: Real, vy: Real, ally: bool) {
    if p.comp_v.is_none() {
        p.comp_v = Some(if p.age == 0 { (0.0, 0.0) } else { (p.vx, p.vy) });
    }
    if ally {
        p.can_turn_on_hit_box = false;
        p.hit_gate_tick = p.age + lf(0.25);
        p.kb_off_on_bounce = true;
        set_hitbox(p, true); // ubergraph 8475
    }
    p.max_speed = 3500.0;
    p.rotate_gate = true;
    p.grav_scale = 3.0;
    p.friction = 0.0;
    p.bounce = 1.0;
    p.vx = vx;
    p.vy = vy;
    p.restore_tick = p.age + lf(0.2);
}

/// `startMoving`: C019BlasterChargeProjectile ReceiveBeginPlay without a target (ubergraph 2420):
/// UpdateProjectileVelocity activates the movement; the spawner's BPI_SetInitialMovement then sets the notify's
/// velocity again.
pub fn start_moving(p: &mut Projectile) {
    activate_movement(p);
}

/// `activateMovement`: ProjectileMovement.Activate(true) on a movement BeginPlay deactivated (`moving`). A
/// StopSimulating (`stopped`) is not undone: it clears the UpdatedComponent, which Activate does not restore.
pub fn activate_movement(p: &mut Projectile) {
    p.moving = true;
}

/// `initialMovement`: its BPI_SetInitialMovement (called by Comp_Spawner after BeginPlay): with a valid AimAtActor
/// the velocity and both speeds times FullChargeSpeedMultiplier; the parent's SetInitialMovementMulticast stores the
/// velocity in the (deactivated) movement and in SpawnedActorParentVelocity, which lasts until the actor's first tick
/// (ParentProjectilePhysics.ReceiveTick 796 -> SpawnedActorParent.ReceiveTick 8771).
pub fn initial_movement(w: &mut World, id: ProjectileId) {
    let m = num(Some(class_p(w, pm_ref(w, id))), "FullChargeSpeedMultiplier").unwrap_or(1.0);
    let p = pm(w, id);
    if p.aim_at.is_some() {
        p.vx *= m;
        p.vy *= m;
        p.max_speed *= m;
    }
    p.parent_vx = p.vx;
    p.parent_vy = p.vy;
}

/// `aimAtGrenade`: AimAtActor = the target, AimActorEnd bound, BounceCount = 0, MoveToCookedGrenade(actor location)
/// (ubergraph 2262, 4541): TravelStart Location, DistanceToTarget, MoveToTimeline (length 1, Alpha linear 0 -> 1)
/// play rate = 1 / (distance / TravelSpeed); with BombCount > 0 the hit is updated, here with
/// KnockBackDirectionVector = (Y, Z) of unit(TravelStart -> target) (ubergraph 161-449); PlayFromStart.
pub fn aim_at_grenade(w: &mut World, id: ProjectileId, g: ProjectileId) {
    let (gx, gy) = {
        let g = pm_ref(w, g);
        (g.x, g.y)
    };
    let speed = num(Some(class_p(w, pm_ref(w, id))), "TravelSpeed").unwrap_or(0.0);
    let p = pm(w, id);
    p.aim_at = Some(g);
    p.aim_bound = true;
    p.bounce_count = 0;
    p.travel_x = p.x;
    p.travel_y = p.y;
    p.dist_to_target = math::ue::size2d([gx - p.x, gy - p.y]);
    p.tl_rate = 1.0 / (p.dist_to_target / speed);
    if p.bomb_count > 0 {
        // GetDirectionUnitVector(TravelStart, the target) (ubergraph 211)
        let u = math::ue::safe_normal([0.0, gx - p.x, gy - p.y], math::ue::SMALL_NUMBER);
        bomb_hit(p);
        p.hit.knock_back_direction = Some(DataVec { x: Some(-u[1]), y: Some(u[2]), z: None });
    }
    p.tl_pos = 0.0;
    p.tl_playing = true;
}

/// `stopAim`: StopAimMove: MoveToTimeline.Stop and AimActorEnd unbound (AimAtActor is kept).
pub fn stop_aim(p: &mut Projectile) {
    p.tl_playing = false;
    p.aim_bound = false;
}

/// `flyOn`: UpdateProjectileVelocity(unit(TravelStart -> location) x TravelSpeed): MaxSpeed = InitialSpeed =
/// TravelSpeed, ProjectileMovement.Activate, Velocity. GetDirectionUnitVector of two equal points is zero. The
/// timeline is not stopped by this.
pub fn fly_on(w: &mut World, id: ProjectileId) {
    let s = num(Some(class_p(w, pm_ref(w, id))), "TravelSpeed").unwrap_or(0.0);
    let p = pm(w, id);
    let dx = p.x - p.travel_x;
    let dy = p.y - p.travel_y;
    let u = math::ue::safe_normal([0.0, dx, dy], math::ue::SMALL_NUMBER);
    p.max_speed = s;
    activate_movement(p);
    p.vx = u[1] * s;
    p.vy = u[2] * s;
}

/// `timelineTick`: one tick of MoveToTimeline (ubergraph 4921): with a live AimAtActor,
/// K2_SetActorLocation(VLerp(location + unit(target -> location) x Lerp(1, DistanceToTarget, Alpha), target, Alpha))
/// without a sweep. True when the timeline finishes.
// Unverified: the timeline is ticked in the spawn frame too and after the actor's other work of the frame.
pub fn timeline_tick(w: &mut World, id: ProjectileId) -> bool {
    if !pm_ref(w, id).tl_playing {
        return false;
    }
    let target = pm_ref(w, id).aim_at.and_then(|g| w.actors.projectiles.get(g)).filter(|g| !g.dead).map(|g| (g.x, g.y));
    let p = pm(w, id);
    p.tl_pos += DELTA_SECONDS * p.tl_rate;
    let a = math::min(p.tl_pos, 1.0);
    if let Some((gx, gy)) = target {
        // GetDirectionUnitVector(the target, location) (ubergraph 5198)
        let [_, ux, uy] = math::ue::safe_normal([0.0, p.x - gx, p.y - gy], math::ue::SMALL_NUMBER);
        let back = 1.0 + (p.dist_to_target - 1.0) * a;
        let ax = p.x + ux * back;
        let ay = p.y + uy * back;
        p.x = ax + (gx - ax) * a;
        p.y = ay + (gy - ay) * a;
    }
    if p.tl_pos < 1.0 {
        return false;
    }
    p.tl_playing = false;
    true
}

/// `bombArrived`: MoveToTimeline finished, after the target's Detonate Server (ubergraph 818):
/// UpdateVisualsMulticast, BombCount + 1 and the hit update.
pub fn bomb_arrived(p: &mut Projectile) {
    p.bomb_visual = true;
    p.bomb_count += 1;
    bomb_hit(p);
}

/// `bombHit`: HitBoxDamage: Damage = BombCount + 2, BaseKnockBack 550, KnockBackScaling 10, HitPauseTime 0.1,
/// HitStunMult 1, then UpdateHitBoxDamage, which keeps the AttackTag of the first HitBoxesToDamage entry
/// (Comp_Pawn_HitBoxHandler 11146), turns the unused boxes off without ClearActorsHitByTag (DeactivateHitbox(box,
/// false), 3243) and never touches ActorsHitByAttacks: the actors already hit stay hit.
fn bomb_hit(p: &mut Projectile) {
    p.hit.damage = Some(crate::real(p.bomb_count) + 2.0);
    p.hit.base_knock_back = Some(550.0);
    p.hit.knock_back_scaling = Some(10.0);
    p.hit.hit_pause_time = Some(0.1);
    p.hit.hit_stun_mult = Some(1.0);
}

/// `hitboxesOff`: HitOnlyOneFighter: the hitboxes go off after the first fighter hit.
pub fn hitboxes_off(p: &mut Projectile) {
    p.hit_on = false;
}

/// `reflectBoxesOn`: a reflect's UpdateHitBoxDamage (`combat/reflect.rs` activate_boxes): the boxes' collision on,
/// HitBoxOn? and the handler's HasHitAFighter as they were (hit_on off with hit_var on is that refusal: it stays).
pub fn reflect_boxes_on(p: &mut Projectile) {
    if !p.hit_var {
        p.hit_on = true;
    }
}

/// `setHitbox`: HitBoxOn? = on and UpdateHitbox. SpawnedActorParent UpdateHitbox @18336 -> @15092: with HitBoxOn? the
/// handler's HitBoxesOn (7284 -> 12258 -> 11984 -> 8624), which clears nothing; without it @15461
/// Comp_Pawn_HitBoxHandler.HitBoxesOff(HitBoxTags) 7813 -> 6913 -> DeactivateHitbox(box, true) 653 ->
/// ClearActorsHitByTag 17495 for each box, and HasHitAFighter = false 7250. So the off forgets the actors already hit
/// and the next on starts a new set.
pub fn set_hitbox(p: &mut Projectile, on: bool) {
    p.hit_on = on;
    p.hit_var = on;
    if on {
        return;
    }
    p.hit_set.clear();
    p.hit_actors.clear();
    p.has_hit_a_fighter = false;
}

/// `setCapsuleCollision`: a child graph's Capsule.SetCollisionEnabled(on ? QueryAndPhysics : NoCollision)
/// (C023A_MusicPickup UpdateCapsuleCollision @6835): the root's blocking, its traces' body and its moves against the
/// stage and the platforms, from the next move.
pub fn set_capsule_collision(p: &mut Projectile, on: bool) {
    p.cap_written = true;
    p.cap_collides = on;
}

/// `setPlatformResponse`: a child graph's Capsule.SetCollisionResponseToChannel(20, r). True when the value changed: a
/// write of the value already set does nothing in the build (FBodyInstance::SetResponseToChannel 142c7d810), and a
/// changed one runs the component's overlap update at once (142d02f60 -> 142cfe1e0 -> UpdateOverlapsImpl 14297ec80).
pub fn set_platform_response(p: &mut Projectile, r: CollisionResponse) -> bool {
    if p.plat_response == r {
        return false;
    }
    p.plat_response = r;
    true
}

/// The platforms a move of the root Capsule may meet (`platBoxes`'s input): every platform, and those of them that
/// are stage solids of object type WorldStatic (`World.blockers('nav')`'s, whose collision blocks PhysicsBody), met
/// as the stage block is whatever the response to Platform; the polygon solids beside them likewise (`polys`, and the
/// WorldStatic ones `static_polys`: `fighter::traces::solids::blocker_polys`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Blockers<'a> {
    pub all: &'a [Ground],
    pub statics: &'a [Ground],
    pub polys: &'a [PolyBlock],
    pub static_polys: &'a [PolyBlock],
}

impl<'a> Blockers<'a> {
    /// `NO_BOXES`.
    pub const NONE: Blockers<'static> = Blockers { all: &[], statics: &[], polys: &[], static_polys: &[] };

    /// `platBoxes`: the platforms a move of the root Capsule meets as blockers, with the polygon solids.
    pub fn of(&self, p: &Projectile) -> Shapes<'a> {
        if !p.cap_collides {
            return Shapes::NONE;
        }
        if p.plat_response == CollisionResponse::Block {
            Shapes { boxes: self.all, polys: self.polys }
        } else {
            Shapes { boxes: self.statics, polys: self.static_polys }
        }
    }
}

// ---- the lifetime

/// `clearMaxLifetime`: SpawnedActorParent UpdateMaxLifetimeWithAuthority(0) (ubergraph 4231-4405): the MaxLifetime
/// timer is cleared and, the new lifetime not being > 0, none is set again.
pub fn clear_max_lifetime(p: &mut Projectile) {
    update_max_lifetime(p, 0.0, 1);
}

/// `updateMaxLifetime`: UpdateMaxLifetimeWithAuthority(NewMaxLifetime) on a live actor (ubergraph 4231-4405,
/// authority): K2_ClearAndInvalidateTimerHandle(MaxLifetimeTimer) @4261, then for NewMaxLifetime > 0 @4281 a new
/// one-shot timer of that rate from now (K2_SetTimerDelegate(OnMaxLifetimeExpired) @4348). `ahead` 0 for a call
/// behind the projectile pass (a kind's step), 1 ahead of it. With LifetimePausedCount > 0 @4432 the new timer is
/// paused at once (K2_PauseTimerHandle @4476): its ticks go to the pause, which BPI_SetLifetimePaused(false) gives
/// back (`chars::marvin::reverse::set_lifetime_paused`).
// Unverified: the call is taken to come in the same place of a frame as a spawn.
pub fn update_max_lifetime(p: &mut Projectile, life: Real, ahead: i32) {
    p.life_ticks = lifetime_at(p, life, ahead);
    if p.lifetime_pause.as_ref().is_some_and(|x| x.count > 0) {
        let left = pause_max_lifetime(p);
        if let Some(x) = p.lifetime_pause.as_mut() {
            x.left = left;
        }
    }
}

/// `maxLifetimeAt`: the age the MaxLifetimeTimer ends the actor at (0: no timer).
pub fn max_lifetime_at(p: &Projectile) -> i32 {
    p.life_ticks
}

/// `pauseMaxLifetime`: K2_PauseTimerHandle(MaxLifetimeTimer) (Buff_ProjectileThroughTunnel @633): the ticks the timer
/// had left (0: none was set), and no timer runs until unpause_max_lifetime gives them back from the age the call
/// finds (K2_UnPauseTimerHandle @1071). UnPauseTimer 142e46bc0: unpaused ahead of the frame's timer tick, that frame's
/// delta is not counted.
pub fn pause_max_lifetime(p: &mut Projectile) -> i32 {
    let left = if p.life_ticks > 0 { (p.life_ticks - p.age).max(0) } else { 0 };
    p.life_ticks = 0;
    left
}

/// `unpauseMaxLifetime`.
pub fn unpause_max_lifetime(p: &mut Projectile, left: i32) {
    if left > 0 && p.life_ticks == 0 {
        p.life_ticks = p.age + left + 1;
    }
}

/// `restartMaxLifetime`: MulticastReflect's pause end @342-470: with MaxLifetime > 0, K2_SetTimerDelegate(
/// OnMaxLifetimeExpired, MaxLifetime) from now, in the actor's tick behind its move of the frame.
pub fn restart_max_lifetime(p: &mut Projectile) {
    if let Some(s) = p.max_life {
        p.life_ticks = lifetime_at(p, s, 0);
    }
}

/// `lifetimeAt`: the age a MaxLifetimeTimer set now for `life` seconds ends the actor at (0: none): the timer's tick
/// count k (the first k with k x DELTA_SECONDS > rate) on from the age, plus `ahead` for a call ahead of the actor's
/// move of the frame (step counts the age up before its expiry test).
pub fn lifetime_at(p: &Projectile, life: Real, ahead: i32) -> i32 {
    let t = life;
    if !(t > 0.0) {
        return 0;
    }
    let mut k = 0;
    while !(crate::consts::clock(crate::real(k)) > f64::from(t)) {
        k += 1;
    }
    p.age + k + ahead
}

/// `writeHoming`: bIsHomingProjectile as a graph outside the class writes it (`combat/elem.rs` clear_homing, which
/// drops buff_home; MulticastReflect @23793-23862 through it). Off: the tether family's own homing stops with the
/// flag; on: the class's own target and magnitude stay the class's to set.
/// TetherProjectile's, C021_Homing_Projectile_C's, Tom_and_Jerry_TennisBall's and TomAimRocket1's overrides run behind
/// the parent's.
pub fn write_homing(p: &mut Projectile, on: bool) {
    if !on {
        p.homing = 0.0;
    }
    match p.ext {
        ProjectileExt::TetherProjectile(_) => crate::chars::creature::tether::write_homing(p, on),
        ProjectileExt::HomingBall(_) => crate::chars::black_adam::projectiles::write_homing(p, on),
        ProjectileExt::TennisBall(_) => crate::chars::tom_and_jerry::tennis::write_homing(p, on),
        ProjectileExt::AimRocket(_) => crate::chars::tom_and_jerry::kicked::write_homing(p, on),
        ProjectileExt::VelmaBook(_) => crate::chars::velma::gadgets::book_write_homing(p, on),
        _ => {}
    }
}

/// `setGravityScale`: a child graph's write of ProjectileMovement.ProjectileGravityScale.
pub fn set_gravity_scale(p: &mut Projectile, s: Real) {
    p.grav_scale = s;
}

/// `gravityScale`.
pub fn gravity_scale(p: &Projectile) -> Real {
    p.grav_scale
}

/// `setBounciness`: a child graph's write of ProjectileMovement.Bounciness (Tom_and_Jerry_TennisBall @1612, @2500,
/// @4356, @5733), read by the next impact's ComputeBounceResult. bShouldBounce is not written: a movement without it
/// keeps none.
pub fn set_bounciness(p: &mut Projectile, b: Real) {
    if p.bounce >= 0.0 {
        p.bounce = b;
    }
}

/// `setFriction`: a child graph's write of ProjectileMovement.Friction (C023A_Car_SpawnedActor UpdateMovementInfo
/// @115, UpdateFriction @39).
pub fn set_friction(p: &mut Projectile, v: Real) {
    p.friction = v;
}

/// `movementActive`: ProjectileMovement.IsActive(): false for a deactivated movement component (an attached actor, a
/// charge shot before its launch). The root Capsule of a projectile never simulates physics.
pub fn movement_active(p: &Projectile) -> bool {
    // GarnetMarker2_C: ProjectileMovement.Deactivate() at its BeginPlay (@6181), never activated again.
    // HealGrenade: the deactivated movement, forced false.
    // C018Flag: no ProjectileMovement.
    if let ProjectileExt::BugsBunnySafe(s) = &p.ext {
        return s.pm_active;
    }
    !(p.attached
        || !p.moving
        || matches!(
            p.ext,
            ProjectileExt::GarnetMarker2(_) | ProjectileExt::HealGrenade(_) | ProjectileExt::C018Flag(_)
        ))
}

/// `speedBuff`: Buff_SlowProjectile ApplySpeedBuff (Buff_SpeedUpProjectile: modifier 2): an active
/// ProjectileMovement's Velocity and MaxSpeed times the modifier; nothing for a deactivated one.
pub fn speed_buff(p: &mut Projectile, m: Real) {
    p.sped_up = true;
    scale_speed(p, m, false);
}

/// `setActorScale`: AActor::SetActorScale3D(s) (Comp_SizeManager SetSizeScale @566): the root's world scale becomes s,
/// and its children's world scale follows by s over the old one: the hit and hurt spheres, the hurt box's offset, the
/// plasma shot's grow bases.
// Deviation: a root at scale 0 has no ratio to scale its children by: they are left alone.
pub fn set_actor_scale(p: &mut Projectile, s: Real) {
    if p.scale == 0.0 {
        p.scale = s;
        crate::actors::world_actor::warn("SetActorScale3D on a projectile at scale 0: its boxes left alone");
        return;
    }
    let r = s / p.scale;
    p.scale = s;
    p.hit_radius *= r;
    p.hurt_radius *= r;
    p.hurt_off_y *= r;
    p.base_hit *= r;
    p.base_hurt *= r;
}

/// `scaleSpeed`: the two writes of ApplySpeedBuff @147 / @287 and of RemoveSpeedBuff @110 / @301: Velocity and
/// MaxSpeed times `m`. False (nothing written) for a deactivated movement component unless `always` is set
/// (RemoveSpeedBuff does not ask).
pub fn scale_speed(p: &mut Projectile, m: Real, always: bool) -> bool {
    if !always && !movement_active(p) {
        return false;
    }
    p.vx *= m;
    p.vy *= m;
    p.max_speed *= m;
    true
}

// ---- the tick

/// Why a step ended the projectile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepEnd {
    Env,
    Blast,
    Life,
    Split,
    Fuse,
    Bounce,
    Whip,
}

/// `step`: one tick of UProjectileMovementComponent plus the actor's timers; why it died, if it did. `host`: a bounce
/// inside the move is heard by the class's kind (false: no kind handler runs). `hit_boxes`: for a class with a
/// ReceiveHit of its own (`ProjectileHooks::receive_hit`), what its root capsule meets besides the stage block;
/// `None` for every other class (its move is not swept).
#[allow(clippy::too_many_arguments, reason = "the signature the call site fills")]
pub fn step(
    w: &mut World,
    id: ProjectileId,
    ground: &Ground,
    blast: &Ground,
    platforms: &Blockers,
    host: bool,
    hit_boxes: Option<Shapes>,
) -> Option<StepEnd> {
    // A basketball's override, ahead of the parent's: the velocity its pass starts with.
    if matches!(pm_ref(w, id).ext, ProjectileExt::Ball(_)) {
        crate::chars::lebron::ball::pass_start(w, id);
    }
    if pm_ref(w, id).dead {
        return None;
    }
    // Creature_WreckingBall_C's override: it rides its ally's actor location, the parent's timers run on.
    if matches!(pm_ref(w, id).ext, ProjectileExt::WreckingBall(_)) {
        return crate::chars::creature::perks_ball::step(w, id);
    }
    // VelmaMarkProjectile_C's override: on a fighter it rides his actor location, its timers run on.
    if matches!(pm_ref(w, id).ext, ProjectileExt::Mark(_))
        && let Some(e) = crate::chars::velma::mark::step_override(w, id)
    {
        return e;
    }
    // GarnetMarker2_C's override: the movement does not tick; it stands or rides its attach parent.
    if matches!(pm_ref(w, id).ext, ProjectileExt::GarnetMarker2(_)) {
        return crate::chars::garnet::marker::step_override(w, id);
    }
    // C018Flag's override: no ProjectileMovement; it rides its floor's move, then the timers.
    if matches!(pm_ref(w, id).ext, ProjectileExt::C018Flag(_)) {
        return crate::chars::marvin::flag::step_override(w, id);
    }
    // TetherProjectile's override: on its ally it rides the ally's actor location.
    if matches!(pm_ref(w, id).ext, ProjectileExt::TetherProjectile(_)) && crate::chars::creature::tether::ride(w, id) {
        return None;
    }
    // StevenTether's override: the same ride.
    if matches!(pm_ref(w, id).ext, ProjectileExt::StevenTether(_)) && crate::chars::steven::tether::ride(w, id) {
        return None;
    }
    // The class's own override (GrappleHookProjectileV2's: on a fighter it rides its actor location).
    if let Some(over) = hooks_of(w, id).and_then(|h| h.step_override)
        && let Some(end) = over(w, id)
    {
        return end;
    }
    // Arya's knife's override: on a fighter it rides its actor location.
    if crate::chars::arya::knife::ride(w, id) {
        return None;
    }
    // C023A_Music_Projectile_C's override: it rides its ally, the parent's expiry runs on.
    if matches!(pm_ref(w, id).ext, ProjectileExt::MusicProjectile(_)) {
        return crate::chars::gizmo::music::step_override(w, id);
    }
    let ps = platforms.of(pm_ref(w, id));
    {
        let p = pm(w, id);
        p.snapped = false;
        p.tipper_due = false;
        p.controlled = false;
        p.prev_x = p.x;
        p.prev_y = p.y;
    }
    if pm_ref(w, id).attached {
        pm(w, id).age += 1;
        follow_id(w, id);
        let p = pm(w, id);
        if p.age == p.activate_tick {
            p.hit_on = true;
            p.hit_var = true;
        }
        return None;
    }
    if pm_ref(w, id).charge_shot && !pm_ref(w, id).moving {
        // The deactivated ProjectileMovement does not tick: only the actor's timers run, and its ReceiveTick
        // (SpawnedActorParentVelocity = GetVelocity() @8771). The spawner's BPI_SetInitialMovement writes
        // ProjectileMovement.Velocity and SpawnedActorParentVelocity (SpawnedActorParent @20742 / @20889), but a write
        // of the property leaves the root's ComponentVelocity alone, the movement has never ticked and
        // MoveToTimeline's K2_SetActorLocation gives the root no velocity: GetVelocity is zero from the first
        // ReceiveTick on, so a hit on the way to the grenade takes the hit's own vector and the actor's yaw.
        // Unverified: the engine side of this is the stock 4.26 source, not read in the exe; a hit in the spawn's own
        // frame (ahead of the first ReceiveTick) would read the spawner's velocity, which the port does not keep.
        let p = pm(w, id);
        p.parent_vx = 0.0;
        p.parent_vy = 0.0;
        p.age += 1;
        p.bounced = false;
        return expiry(w, id);
    }
    let cp = class_p(w, pm_ref(w, id));
    let grav_value = num(Some(cp), "Projectile Gravity Scale").unwrap_or(Real::NAN);
    let env_kill = truthy(cp.get("DestroyOnCollideWithEnvironment"));
    {
        let p = pm(w, id);
        if p.age == 0 && !p.charge_shot {
            p.parent_vx = p.vx;
            p.parent_vy = p.vy;
        }
        if p.age == p.restore_tick {
            p.bounce = 0.5;
            p.friction = 0.75;
        }
        if p.age == p.hit_gate_tick {
            p.can_turn_on_hit_box = true;
        }
        // Delay(DurationToGravity) then ProjectileGravityScale = 'Projectile Gravity Scale'.
        if p.age == p.grav_tick {
            p.grav_scale = grav_value;
        }
        if p.age == p.slow_tick {
            p.max_speed = 350.0;
        }
        p.age += 1;
        if p.plasma {
            let a = math::min((crate::real(p.age) * DELTA_SECONDS) / 0.35, 1.0);
            // The cubic between the two keys: Hermite with the leave tangent x the key gap.
            let g = (a * a * a - 2.0 * a * a + a) * (4.917_337_4 * 0.35) + (3.0 * a * a - 2.0 * a * a * a);
            let s = 0.2 + 0.8 * g;
            p.hit_radius = p.base_hit * s;
            p.hurt_radius = p.base_hurt * s;
        }
    }
    let dt = 1.0 / 60.0;
    // ComputeMoveDelta: v dt + (v' - v) dt / 2 with v' = ComputeVelocity(v, dt). A stopped component
    // (StopSimulating) does not tick. A root that is not Movable: the component's TickComponent returns before
    // ComputeVelocity and the move, so no sweep, no hit, no bounce and no turn; the actor's own tick and timers below
    // run on. A deactivated movement (`moving` false: EnableProjectileMovement off) does not tick either.
    let (ovx, ovy) = (pm_ref(w, id).vx, pm_ref(w, id).vy);
    let (mut dx, mut dy) = (0.0, 0.0);
    let still = {
        let p = pm_ref(w, id);
        p.stopped || p.mobility.is_some() || !p.moving
    };
    if !still {
        let (ax, ay) = compute_velocity(w, pm_ref(w, id), ovx, ovy, dt);
        let p = pm(w, id);
        p.comp_v = None; // UMovementComponent::UpdateComponentVelocity at the end of the tick
        p.avx = ax;
        p.avy = ay;
        dx = ovx * dt + (ax - ovx) * 0.5 * dt;
        dy = ovy * dt + (ay - ovy) * 0.5 * dt;
        p.vx = ax;
        p.vy = ay;
        turn(p, ovx, ovy);
    }
    // The root capsule blocks WorldStatic (the floor) and ignores platforms and pawns. A blocking hit runs
    // OnComponentHit -> DestroySpawnedActor. The sweep is sampled every 10 u and bisected to the contact. A root
    // without collision (NoCollision) has no blocking hit.
    {
        let p = pm(w, id);
        if env_kill && p.cap_collides && p.mobility.is_none() && !p.no_collision {
            let (x0, y0) = (p.x, p.y);
            let hits = |p: &Projectile, x: Real, y: Real| blocker(p, ground, ps, x, y, 0, 0.0).is_some();
            if hits(p, x0, y0) {
                p.dead = true; // a sweep that starts in penetration is a blocking hit
                env_hit(w, id, ground, ps, x0, y0);
                return Some(StepEnd::Env);
            }
            let n = math::max(1.0, (math::ue::size2d([dx, dy]) / 10.0).ceil());
            let mut i = 1.0;
            while i <= n {
                if hits(p, x0 + (dx * i) / n, y0 + (dy * i) / n) {
                    let mut lo = (i - 1.0) / n;
                    let mut hi = i / n;
                    for _ in 0..12 {
                        let m = (lo + hi) / 2.0;
                        if hits(p, x0 + dx * m, y0 + dy * m) {
                            hi = m;
                        } else {
                            lo = m;
                        }
                    }
                    p.x = x0 + dx * lo;
                    p.y = y0 + dy * lo;
                    p.dead = true;
                    env_hit(w, id, ground, ps, x0 + dx * hi, y0 + dy * hi);
                    return Some(StepEnd::Env);
                }
                i += 1.0;
            }
        }
    }
    pm(w, id).bounced = false;
    let (no_collision, cap_off, bounces) = {
        let p = pm_ref(w, id);
        (p.no_collision, !p.cap_collides && p.cap_written, p.bounce >= 0.0)
    };
    if still {
        // nothing moves it
    } else if no_collision || cap_off {
        // The actor's collision off (the tunnel carry), or the root's by its graph (set_capsule_collision): the sweep
        // meets nothing, so no hit and no bounce.
        let p = pm(w, id);
        p.x += dx;
        p.y += dy;
    } else if bounces {
        bounce_move(w, id, ground, platforms, ovx, ovy, host);
    } else if let Some(hb) = hit_boxes {
        hit_move(w, id, ground, hb, dx, dy, host);
    } else {
        let p = pm(w, id);
        p.x += dx;
        p.y += dy;
    }
    // Destroyed inside the move by a hit at a bounce's place (the world's hitbox_overlaps_here): ended already.
    let p = pm(w, id);
    if p.dead {
        return None;
    }
    if p.bounced && p.kb_off_on_bounce {
        p.kb_off_on_bounce = false;
        set_hitbox(p, false); // TurnOffKB, ubergraph 9076-9087
    }
    // A bounce handler's K2_DestroyActor (bounce_impact: BounceEvent, OnBounce, a kind's handler).
    if p.ended {
        p.dead = true;
        return Some(StepEnd::Bounce);
    }
    // SpawnedActorParent ReceiveTick (8771; every class here reaches it). The movement component ticks before its
    // owner (bTickBeforeOwner), so the actor reads the velocity this frame's move ended with.
    // Unverified: a hit is tested after this, so one made by this frame's own move already has this frame's velocity.
    if !p.grenade || (p.age - 1) % 6 == 0 {
        p.parent_vx = p.vx;
        p.parent_vy = p.vy;
    }
    // C019_CookGrenade ReceiveTick (PrimaryActorTick.TickInterval 0.1 s: every 6th frame at the fixed step), with
    // RotateGate: the mesh's world rotation = FindLookAtRotation(location, location + Velocity) in the (Y, Z) plane.
    // Unverified: the first tick is taken in the spawn frame and, like the Delay links, after that frame's move.
    if p.rotate_gate {
        if (p.age - 1) % 6 == 0 {
            // FindLookAtRotation 142f6cad0 (MakeFromX + FMatrix::Rotator) of (0, -vx, vy).
            let r = math::ue::rot_from_x([0.0, -p.vx, p.vy]);
            p.look_yaw = r[1];
            p.look_pitch = r[0];
        }
        if p.bounced {
            p.rotate_gate = false;
        }
    }
    if pm_ref(w, id).tether.is_some() && whip_tick(w, id, blast) {
        return Some(StepEnd::Whip);
    }
    let p = pm(w, id);
    // The splitter's Delay: SplitVisuals, the follow-ups (world) and K2_DestroyActor.
    if p.age == p.split_tick {
        p.dead = true;
        return Some(StepEnd::Split);
    }
    // C019_CookGrenade DetonateTimerFunction -> Detonate Server (the world's detonate).
    if p.fuse_ticks > 0 && p.age >= p.fuse_ticks {
        p.dead = true;
        return Some(StepEnd::Fuse);
    }
    expiry(w, id)
}

/// `whipTick`: the tether family's actor tick (a class's kind gives its TetherParams: Morty's C019_WhipProjectile,
/// Wonder Woman's LassoProjectileFastV3), written from C019_WhipProjectile's graph: ReceiveTick (ubergraph 12108 ->
/// 7671) StartReturn within 300 of the sides of the BlastBoxPlayerExit actor's bounds (4929-5447: GetActorBounds with
/// SM_LevelBorder01 scaled to 3.875 x the extent, 5235-6162); then (5681) with CanControl and MoveYAxisValue != 0,
/// Velocity = Normal(0, (FacingLeft ? 1 : -1) x IS, (MoveY > 0 ? 0.5 : -0.5) x IS) x IS. BeginPlay's Delay(0.2): with
/// CanControl StartReturn, then SpawnSnapEffect (HitBoxOn? = false, UpdateHitbox). StartReturn: CanControl = false,
/// Delay(0.033) -> ReturnMovementServer (MaxSpeed 6000, Velocity = unit(location -> the instigator's mesh socket) x
/// 6000, Bounciness 0.1, homing at InitialSpeed x 10 on the instigator's root, no more blocking hits),
/// ActivateLassoTipper, and Delay(0.36) -> K2_DestroyActor. True when it ends the actor.
// Unverified: the Delays count from the tick that starts them; AActor::GetActorBounds counting primitives without
// collision is the stock 4.26 code, not read from the exe.
fn whip_tick(w: &mut World, id: ProjectileId, blast: &Ground) -> bool {
    let f = pm_ref(w, id).owner.expect("a tether has an owner");
    let border = (blast.right - blast.left) * 0.5 * 3.875;
    {
        let p = pm(w, id);
        if p.x <= math::min(crate::math::narrow(blast.left), crate::math::narrow(-border)) + 300.0
            || p.x >= math::max(crate::math::narrow(blast.right), crate::math::narrow(border)) - 300.0
        {
            start_return(p);
        }
    }
    let (facing, _, _, _, _) = fighter_body(w, f);
    let iy = w.fighter(f).controls.inp.y;
    {
        let p = pm(w, id);
        if p.can_control && iy != 0.0 {
            // Normal(MakeVector, 0.0001) x InitialSpeed (C019_WhipProjectile @6311)
            let u = math::ue::normal([0.0, facing, if iy > 0.0 { 0.5 } else { -0.5 }], math::ue::KINDA_SMALL_NUMBER);
            p.vx = u[1] * p.initial_speed;
            p.vy = u[2] * p.initial_speed;
            p.controlled = true;
        }
        if p.age == p.snap_tick && p.can_control {
            start_return(p);
            if p.tether.as_ref().is_some_and(|t| t.snap) {
                set_hitbox(p, false);
                p.snapped = true;
            }
        }
    }
    if pm_ref(w, id).age == pm_ref(w, id).return_tick {
        let p = pm_ref(w, id);
        let si = p.return_socket;
        let bs = sampler(w, data_fighter(p));
        let pose = si.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
        let mut tx = crate::fighter::mesh_wx(w, f);
        let mut ty = crate::fighter::mesh_wy(w, f);
        if let (Some(pose), Some(si)) = (pose, si) {
            let o = offset_of(bs, si);
            tx += crate::real(pose[o]) * facing;
            ty += crate::real(pose[o + 1]);
            crate::fighter::anim::put_pose(w, pose);
        }
        let p = pm(w, id);
        let t = p.tether.as_ref().expect("a tether");
        let (return_max, return_speed) = (t.return_max_speed, t.return_speed);
        if let Some(m) = return_max {
            p.max_speed = m;
        }
        let dx = tx - p.x;
        let dy = ty - p.y;
        // GetDirectionUnitVector(location, the socket) x the return speed (C019_WhipProjectile @2904 / @4056)
        let u = math::ue::safe_normal([0.0, dx, dy], math::ue::SMALL_NUMBER);
        p.vx = u[1] * return_speed;
        p.vy = u[2] * return_speed;
        p.homing = p.initial_speed * 10.0;
        // ReturnMovementServer writes HomingTargetComponent (the instigator's root) over a buff's target.
        p.buff_home = None;
        p.bounce = -1.0;
        p.returning = true;
        p.tipper_due = true;
        p.end_tick = p.age + lf(0.36);
    }
    let p = pm(w, id);
    if p.age == p.end_tick {
        p.dead = true;
        return true;
    }
    false
}

/// `startReturn`: StartReturn's return sequence behind its do-once (also a kind's call: Wonder Woman's lasso perk).
pub fn start_return(p: &mut Projectile) {
    if p.return_started {
        return;
    }
    p.return_started = true;
    p.can_control = false;
    p.return_tick = p.age + lf(0.033);
}

/// `expiry`: the MaxLifetime timer (OnMaxLifetimeExpired); the LifeSpan timer a reflect set (Comp_ReflectProjectile
/// SetLifeSpan @1950); the 0.1 s looping timer SpawnedActorParent CheckBlastBoxExit @20219-20685 (InRange(location,
/// the stored BlastBoxBoundsMin / Max, inclusive) per axis), whose expiry adds its rate.
// Unverified: AActor's LifeSpanExpired (Destroy) is taken as the MaxLifetime end, as the Firebolt child's own
// LifeSpan is.
pub fn expiry(w: &mut World, id: ProjectileId) -> Option<StepEnd> {
    let exits = truthy(class_p(w, pm_ref(w, id)).get("DestroyOnExitedArena?"));
    let p = pm(w, id);
    if p.life_ticks > 0 && p.age >= p.life_ticks {
        p.dead = true;
        return Some(StepEnd::Life);
    }
    if let Some(rs) = &p.reflect
        && rs.life_span_at >= 0
        && p.age >= rs.life_span_at
    {
        p.dead = true;
        return Some(StepEnd::Life);
    }
    if exits && crate::consts::clock(crate::real(p.age - 1)) > p.blast_at {
        p.blast_at += f64::from(0.1_f32);
        if crate::world::blast::check_blast_box_exit(p.x, p.y, &p.blast_bounds) {
            p.dead = true;
            return Some(StepEnd::Blast);
        }
    }
    None
}

/// `computeVelocity`: ComputeVelocity(v, t): v + a t with a = gravity (+ the homing acceleration), then LimitVelocity
/// to MaxSpeed.
pub fn compute_velocity(w: &World, p: &Projectile, vx: Real, vy: Real, dt: Real) -> (Real, Real) {
    let g = -GRAVITY_Z * p.grav_scale;
    let mut nvx = vx;
    let mut nvy = vy + g * dt;
    let h = match &p.buff_home {
        Some(b) => crate::actors::homing::home_toward(w, p.x, p.y, b),
        None => home_accel(w, p),
    };
    if let Some(h) = h {
        nvx += h.x * dt;
        nvy += h.y * dt;
    }
    crate::actors::projectile_move::limit_velocity(nvx, nvy, p.max_speed)
}

/// `homeAccel`: ComputeHomingAcceleration: (the target component's location - the location).GetSafeNormal() x
/// HomingAccelerationMagnitude, or none (no bIsHomingProjectile, no HomingTargetComponent; a zero vector gives zero). Here the
/// tether family's: the target is the instigator's root. A class with another target has its own (its
/// `ProjectileExt` arm, with its port): the Creature's TetherProjectile's (`tether::home_accel`), Tom and Jerry's
/// tennis ball's (toward Jerry's HomingAim) and TomAimRocket1's (toward Jerry's root).
pub fn home_accel(w: &World, p: &Projectile) -> Option<V2> {
    match &p.ext {
        ProjectileExt::TetherProjectile(_) => return crate::chars::creature::tether::home_accel(w, p),
        ProjectileExt::TennisBall(s) => return crate::chars::tom_and_jerry::tennis::home_accel(w, p, s),
        ProjectileExt::AimRocket(s) => return crate::chars::tom_and_jerry::kicked::home_accel(w, p, s),
        _ => {}
    }
    if matches!(p.ext, ProjectileExt::VelmaBook(_)) {
        return crate::chars::velma::gadgets::book_home_accel(w, p);
    }
    if !(p.homing > 0.0) {
        return None;
    }
    let o = p.owner.expect("a homing tether has an owner");
    let hx = w.fighter(o).body.x - p.x;
    let hy = crate::fighter::actor_y(w, o) - p.y;
    // ComputeHomingAcceleration 14298c730: GetSafeNormal (1e-8, the == 1 shortcut) x HomingAccelerationMagnitude.
    let u = math::ue::safe_normal([0.0, hx, hy], math::ue::SMALL_NUMBER);
    Some(V2 { x: u[1] * p.homing, y: u[2] * p.homing })
}

/// `aimDownTick`: ParentProjectilePhysics ReceiveTick @796 -> @10-166: with AimDownProjectileMovement
/// K2_SetActorRotation(FindLookAtRotation(loc, loc + ProjectileMovement.Velocity)) every tick, after its movement
/// component's (the world calls this behind step, ahead of the child's own tick). The look-at of (0, -vx, vy): pitch
/// atan2(vy, |vx|), yaw -90 for vx > 0 (rot_x 1), +90 for vx < 0, 0 for vx = 0 (rot_x -1: the zero vector's rotator is
/// zero, read in the exe). An attached actor looks along the velocity the spawner wrote on its deactivated movement
/// (`attached_vel`).
// Unverified: that Deactivate leaves Velocity alone is the stock 4.26 source (UActorComponent::Deactivate, no
// override in the movement classes), not read in the exe.
pub fn aim_down_tick(w: &mut World, id: ProjectileId) {
    if !aim_down_on(w, pm_ref(w, id)) {
        return;
    }
    let p = pm(w, id);
    let v = if p.attached { attached_velocity(p) } else { None };
    let (vx, vy) = v.unwrap_or((p.vx, p.vy));
    // FindLookAtRotation 142f6cad0: MakeFromX + FMatrix::Rotator, the pitch of (0, -vx, vy).
    p.pitch = math::ue::rot_from_x([0.0, -vx, vy])[0];
    p.rot_x = if vx > 0.0 { 1.0 } else { -1.0 };
}

/// `aimDownOn`: AimDownProjectileMovement as the actor holds it: the class data's (a child graph that writes the
/// variable has its own: TomAimRocket1's; JerryCork's aimDownTick runs the parent's only while its own variable holds).
pub fn aim_down_on(w: &World, p: &Projectile) -> bool {
    let data = matches!(class_p(w, p).get("AimDownProjectileMovement"), Some(Value::Bool(true)));
    match &p.ext {
        ProjectileExt::AimRocket(s) => crate::chars::tom_and_jerry::kicked::aim_down_on(p, s),
        ProjectileExt::JerryCork(s) => data && crate::chars::tom_and_jerry::champagne::aim_down_on(p, s),
        _ => data,
    }
}

/// `attachedVelocity`: the Velocity of the deactivated movement of an attached actor. The electric chains' override:
/// their launch velocity (`chain::attached_velocity`).
pub fn attached_velocity(p: &Projectile) -> Option<(Real, Real)> {
    if matches!(p.ext, ProjectileExt::Chain(_)) {
        return crate::chars::black_adam::chain::attached_velocity(p);
    }
    if matches!(p.ext, ProjectileExt::StewSpitAir(_)) {
        return crate::chars::taz::projectiles::attached_velocity(p);
    }
    p.attached_vel
}

/// `pmInitialSpeed`: the ProjectileMovement's InitialSpeed (the tether family's own write once it made one).
pub fn pm_initial_speed(p: &Projectile) -> Real {
    if p.tether.is_some() { p.initial_speed } else { p.init_speed.unwrap_or(0.0) }
}

/// `yawX`: the sign of a facing hit's knockback X (see rot_x): before the first turn the spawn transform's side.
pub fn yaw_x(p: &Projectile) -> Real {
    // C017_Bomb_C overrides yawX with its facing
    if matches!(p.ext, ProjectileExt::Bomb(_)) {
        return p.facing;
    }
    // C023B_SpawnedGunShot_C: its rotation is the spawn's rot 0 (actor yaw 0): -1
    if matches!(p.ext, ProjectileExt::GunShot(_)) {
        return -1.0;
    }
    if matches!(p.ext, ProjectileExt::MeeseeksSummon(_)) {
        return crate::chars::rick::meeseeks::yaw_x(p);
    }
    // C023A_PopCorn_C: -1 while hidden
    if matches!(p.ext, ProjectileExt::Popcorn(_)) {
        return crate::chars::gizmo::popcorn::yaw_x(p);
    }
    if p.rot_x != 0.0 { p.rot_x } else { p.spawn_side.unwrap_or(p.facing) }
}

/// `handlerFacingLeft`: Comp_Pawn_HitBoxHandler.HitBoxHandlerFacingLeft, read by pfgDoDamage's KnockBackMirror
/// (@11892): written only by BaseFighter, BugsBunnySafe and the Meeseeks classes (xref writes), so false for every
/// other spawned actor.
pub fn handler_facing_left(p: &Projectile) -> bool {
    if matches!(p.ext, ProjectileExt::MeeseeksSummon(_)) {
        return crate::chars::rick::meeseeks::handler_facing_left(p);
    }
    matches!(&p.ext, ProjectileExt::BugsBunnySafe(s) if s.facing_left)
}

/// `stopSimulating`: StopSimulating: Velocity = 0 and no UpdatedComponent. OnProjectileStop has no listener in the
/// ported classes.
pub fn stop_simulating(p: &mut Projectile) {
    p.vx = 0.0;
    p.vy = 0.0;
    p.stopped = true;
}

/// `new Projectile(cls, null, 0, a, r.p, look.boxes, r.origin, { lookup: look })` or the class's `makeWorld`
///: an ownerless world projectile of the items part's class at (x, y), InitialSpeed
/// and MaxSpeed 0, looked up through `look`; the caller's spawn inserts it into the World's list, not yet begun.
/// The class's constructor tail (`construct`) runs for the no-owner case as a world spawn does: the safe's
/// `new BugsBunnySafe(null, r, a, l)` is `construct_safe` on a projectile with no owner.
// Deviation: `construct` runs for every class, where a class's constructor tail only runs through a world spawn
// (a plain Projectile otherwise); only the safe reaches this today.
pub fn make_world_at_rest(w: &mut World, cls: Name, look: crate::fighter::FighterId, x: Real, y: Real) -> ProjectileId {
    let entry = *w
        .registry
        .actors
        .get(w.names.str(cls))
        .unwrap_or_else(|| panic!("{}: a world-spawned class with no registry entry", w.names.str(cls)));
    let ActorClass::Projectile(class) = entry.class else {
        panic!("{}: a world-spawned class that is not a projectile", w.names.str(cls));
    };
    let hooks = match entry.hooks {
        ActorHooks::Projectile(h) => h,
        _ => ProjectileHooks::default(),
    };
    let dir: Props = ["X", "Y", "Z"].into_iter().map(|k| (k.to_owned(), Value::Num(0.0))).collect();
    let mut notify = Props::new();
    notify.insert("InitialMovementDirection".to_owned(), Value::Obj(dir));
    notify.insert("InitialSpeed".to_owned(), Value::Num(0.0));
    notify.insert("MaxSpeed".to_owned(), Value::Num(0.0));
    let opts = MakeOpts { lookup: Some(look), ..MakeOpts::default() };
    let mut p = make_projectile(w, cls, class, None, 0, &notify, Some(V2 { x, y }), opts);
    if let Some(construct) = hooks.construct {
        construct(w, &mut p);
    }
    w.actors.projectiles.insert(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Projectile {
        Projectile::new(Name::default(), ProjectileClass::ShaggySandwichProjectile, None, -1, 0)
    }

    /// The rows attach_parent takes from the rotator's quaternion are FRotationMatrix's rows (UE's sign convention).
    #[test]
    fn rotator_quaternion_rows_are_the_rotation_matrix() {
        for r in [[30.0_f32, 45.0, 60.0], [-10.0, 170.0, 5.0], [0.0, 90.0, 0.0], [89.0, -33.0, -120.0]] {
            let q = math::ue::rotator_quaternion(r);
            let [p, y, ro] = r.map(math::ue::deg_to_rad);
            let (sp, cp) = (math::ue::sinf(p), math::ue::cosf(p));
            let (sy, cy) = (math::ue::sinf(y), math::ue::cosf(y));
            let (sr, cr) = (math::ue::sinf(ro), math::ue::cosf(ro));
            let m = [
                [cp * cy, cp * sy, sp],
                [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp],
                [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp],
            ];
            // a tolerance on purpose: the quaternion route against the rotation matrix, two different float32 computations
            for (i, e) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].into_iter().enumerate() {
                let v = math::ue::quat_rotate_vector(q, e);
                for k in 0..3 {
                    assert!((v[k] - m[i][k]).abs() < 1e-5, "{r:?} row {i}: {v:?} {:?}", m[i]);
                }
            }
        }
    }

    #[test]
    fn lifetime_counts_the_first_tick_past_the_rate() {
        let mut p = plain();
        p.age = 3;
        assert_eq!(lifetime_at(&p, 0.0, 1), 0);
        // 0.1 s: 6 x fround(0.0166668) = 0.1000008 > fround(0.1)
        assert_eq!(lifetime_at(&p, 0.1, 1), 3 + 6 + 1);
        assert_eq!(lifetime_at(&p, 0.1, 0), 3 + 6);
    }

    #[test]
    fn pause_and_unpause_give_the_ticks_back_from_the_next_frame() {
        let mut p = plain();
        p.age = 10;
        p.life_ticks = 25;
        assert_eq!(pause_max_lifetime(&mut p), 15);
        assert_eq!(p.life_ticks, 0);
        p.age = 40;
        unpause_max_lifetime(&mut p, 15);
        assert_eq!(p.life_ticks, 56);
    }

    #[test]
    fn a_lifetime_set_while_paused_waits_for_the_unpause() {
        let mut p = plain();
        p.age = 3;
        p.lifetime_pause = Some(LifetimePause { count: 1, left: 0 });
        update_max_lifetime(&mut p, 0.1, 1);
        assert_eq!(p.life_ticks, 0);
        assert_eq!(p.lifetime_pause.as_ref().map(|x| x.left), Some(6 + 1));
        // a count of 0 (the pause given back) sets the timer as before
        p.lifetime_pause = Some(LifetimePause { count: 0, left: 0 });
        update_max_lifetime(&mut p, 0.1, 1);
        assert_eq!(p.life_ticks, 3 + 6 + 1);
    }

    #[test]
    fn set_hitbox_off_forgets_the_hits() {
        let mut p = plain();
        p.hit_set.insert(3);
        set_hitbox(&mut p, true);
        assert!(p.hit_on && p.hit_var && p.hit_set.contains(&3));
        set_hitbox(&mut p, false);
        assert!(!p.hit_on && !p.hit_var && p.hit_set.is_empty());
    }

    #[test]
    fn blockers_follow_the_platform_response() {
        let a = [Ground { left: 0.0, right: 1.0, bottom: 0.0, top: 1.0 }];
        let b = Blockers { all: &a, ..Blockers::NONE };
        let mut p = plain();
        assert!(b.of(&p).boxes.is_empty());
        p.cap_collides = true;
        assert!(b.of(&p).boxes.is_empty());
        p.plat_response = CollisionResponse::Block;
        assert_eq!(b.of(&p).boxes.len(), 1);
    }

    #[test]
    fn yaw_x_falls_back_to_the_spawn_side() {
        let mut p = plain();
        p.facing = 1.0;
        assert_eq!(yaw_x(&p), 1.0);
        p.spawn_side = Some(-1.0);
        assert_eq!(yaw_x(&p), -1.0);
        p.rot_x = 1.0;
        assert_eq!(yaw_x(&p), 1.0);
    }
}
