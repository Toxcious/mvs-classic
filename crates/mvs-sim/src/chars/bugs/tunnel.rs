//! Bugs Bunny's tunnels: Comp_Pawn_Tunnel (the pawn's component: the spawn with the
//! MaxTunnels walk, TeleportTunnel, FindClosestTunnel, RotateFighter, TeleportFighter, Handle Clear Tunnels),
//! BugsBunnyTunnelParent_C (the tunnel: the crouch teleport of its Tick, the ProjectileOverlap carry, TransitionOut,
//! CheckForGround; [`Tunnel`], a world actor), Buff_Tunneling_C ([`TunnelingExt`]: the dig of
//! BugsBunny_Attack_TunnelDown_Montage / _Air_DiveDown_TunnelDown_ / _Att_AirToGroundTunnelDown_, its ApplyBuff at
//! 0.8147 s, 2 s) and Buff_TunnelTeleport_C ([`TeleportExt`]: the teleport of a fighter between two tunnels). Offsets are
//! of their ubergraphs unless a function is named. An actor carried from one tunnel to the next is
//! `actors::tunnel_carry::through_tunnel`'s (Buff_ProjectileThroughTunnel).
//! TunnelDown's AnimNotify_AddInvisibility (0.8119 s, 0.32 s) is the shared notify of `fighter::anim`.
//!
//! Comp_Pawn_Tunnel (MaxTunnels 2, ImmediatelyDestroyExtraTunnels, OnlyTeleportAllies, OnlyRotateCharactersIfHitstun,
//! FighterTeleportDuration 0.3; UseTeleportBuff and DestroyTunnelsAfterCharacterUse false; BuffDuration 6 is read by
//! AddPortalBuff alone, which nothing calls).
//! - AlliedTunnels is the owner's live tunnels in spawn order: the world's actors filtered ([`tunnels_of`]). The build
//!   appends after FinishSpawningActor @2287 and the tunnel's EndPlay removes itself @8643: the same list, nothing stored.
//! - SpawnObject(SpawnLocation, FacingLeft, _, SpawnDuration) @4776: Length + 1 > MaxTunnels -> the walk @877-1472
//!   (index i < Length + 1 - MaxTunnels: ImmediatelyDestroyExtraTunnels -> K2_DestroyActor of [i], whose EndPlay shortens
//!   the list the next test reads; else the break), then BeginDeferredActorSpawnFromClass at the location, yaw -90 facing
//!   left / +90 right, scale 1.25 @1794, Duration_0 = SpawnDuration, Instigator the pawn, FinishSpawningActor
//!   (BeginPlay), BPI_UpdateHitbox, the append, Init Facing.
//! - Handle Clear Tunnels (ClearTunnelsServer @4939) over a copy of the list: DestroySpawnedActor(true) on each, a
//!   K2_DestroyActor (no hit pause on a tunnel: SpawnedActorParent DestroySpawnedActor @0-280; CanBePooled false).
//! - TeleportTunnel(EnterTunnel, ActorToTeleport) @4614: Length > 1; OnlyTeleportAllies -> CheckIfAlly (the
//!   Comp_Actor_Destructible TeamIndex) @2965; BPI_IsAttacking false @3100; no Buff_Tunneling (tested twice, @3256 /
//!   @3493); Set Nearest Tunnel @3651 (FindClosestTunnel(EnterTunnel, ActorToTeleport)); BeamVisuals (empty); @38: the
//!   fighter cast; Get Teleport Destination (the closest + (0, 0, unscaled half height + 100)); TeleportVisuals @3922;
//!   RotateFighter; an ally -> TeleportFighter @260 and CurrentClosestTunnel.AddTeleportedActor @283 (the coop stat
//!   @400-756 is the player state's: not ported), an enemy SetMostRecentAttacker first @849 (unreached:
//!   OnlyTeleportAllies).
//! - FindClosestTunnel(Reference, TeleportingActorToCheck): the list in order, the reference skipped, VSize below the
//!   minimum (ties keep the first), and with a valid actor to check the candidate's IsTeleportingActor(ActorToTeleport,
//!   the member) false (the member, not the argument; the two are the same fighter on every path here).
//! - TeleportFighter: CurrentClosestTunnel.AddTeleportedActor(the fighter) and of each attached actor, then
//!   SpawnBuffActor(Buff_TunnelTeleport_C, f, the pawn, 0, 0.0, BreakIfBlocked true) and on the buff Activate(the
//!   destination, FighterTeleportDuration, EnterTunnel, CurrentClosestTunnel). So the exit tunnel adds the fighter
//!   twice.
//! - RotateFighter: OnlyRotateCharactersIfHitstun -> CheckHitstunActive; the closest's FacingLeft differs from
//!   (GetVelocity.Y > 0, UE: sim vx < 0) -> the CMC Velocity.Y negated.
//! - TeleportVisuals @3922 (a DoOnce, reopened by a Delay 0.3 @4559): sfx_cha_bugs_tunnel_warp_generic at the enter
//!   tunnel and sfx_cha_bugs_tunnel_warp_end at the actor, both posted for the pawn.
//!
//! BugsBunnyTunnelParent (SpawnedActorParent child; Die With Instigator: the world's teardown; Stop Delay 0.3,
//! TimePerActorTeleport 1, TunneledProjectileBufferTime 1, MaxProjectileSpeedThroughTunnel 2000,
//! ProjectieThroughTunnelTime 0.25). Shapes, at the actor's scale 1.25 and yaw +-90 (sim: hx along x, hy along y, depth
//! the UE X): Box (extent (250, 150, 50) x (0.35, 1.1, 1.2), Portal, overlaps Pawn) hx 109.375, hy 75, depth 206.25;
//! ProjectileOverlap (capsule half height 140, radius 120, pitch 90: lying along sim x; overlaps DamageDestructible,
//! ProjectileDestructible, ItemDestructible) a segment of half length 25 and radius 150.
//! - ReceiveBeginPlay @1203: the parent's; StartingTime; (authority, not a dedicated server) the ItemUiWidget's PickUp
//!   status @3124-3650 (the view's) and @1338: X = 0 (none in the sim), Box overlap events on, SphereTraceSingle(r 70,
//!   down 375, ProjectileGroundTrace) and K2_AttachToActor(its HitActor @1968): a static stage's floor has no actor; a
//!   DestructibleObjectBP hit has its ObjectDestroyed bound to Destroy @2249-2272: TransitionOut(null)
//!   @9940 -> @4584 TransitioningOut = true, K2_DestroyActor; the HitBoxManager's Instigator; TunnelTeamIndex; the 0.1 s
//!   looping CheckForGround timer @2531; the Instigator's OnExitedArena -> K2_DestroyActor @9925; Delay(Stop Delay)
//!   @2747 -> @59: BindOnHit? false (no HitBoxesOn) -> Delay(Duration_0 - 2) @258 -> @15 TransitionOut(the Instigator) ->
//!   @4584 TransitioningOut = true, K2_DestroyActor.
//! - BPI_UpdateHitbox (SpawnedActorParent @18336 -> @15092): HitBoxOn? true -> HitBoxesOn(HitBoxTags [HitBox]): no
//!   component of the class has the tag, so nothing is turned on (the hit packet is all zeros besides).
//! - ReceiveTick @9429 (authority): the elapsed time > 0.2 -> the Ak velocity RTPC (the view's) and the loop over
//!   Box.GetOverlappingActors(BP_FighterCharacter), queried again at each index @4817-8054: TunnelUI (empty); team equal
//!   (the PlayerData's), IsMovingOnGround, IsCrouching (the CMC's), not BPI_IsAttacking @7292; no attach parent @7558;
//!   ActorBeingTeleported; IsTeleportingActor false @8023; @623: Held (11) not in its Comp_State_Manager CurrentStates,
//!   IsTeleportingActor false again; the pawn's TeleportTunnel(self, it) @984, AddTeleportedActor(it).
//! - TeleportedActor (a map actor -> time): AddTeleportedActor removes and adds (now); IsTeleportingActor: absent false,
//!   elapsed >= TimePerActorTeleport -> removed and false, else true; RemoveTeleportedActor removes when the elapsed time
//!   is >= TimePerActorTeleport (@3711-3982).
//! - Box begin overlap @8128: a fighter of the tunnel's team -> UpdateOverlappingAllies(it, true) @10278 (a locally
//!   controlled one: AddUnique, the widget shown); end overlap @8491: present in TeleportedActor ->
//!   RemoveTeleportedActor, UpdateOverlappingAllies(it, false) (RemoveItem; the widget shown while any is left).
//! - ProjectileOverlap begin overlap @10766 (authority): a SpawnedActorParent whose root simulates or whose
//!   ProjectileMovement is active, not tagged IgnorePortal, of the tunnel's team @11206; TunneledProjectiles has no entry
//!   or one at least TunneledProjectileBufferTime old @11320-11585; FindClosestTunnel(self, null) valid; the entry written
//!   here and on the closest @11781 / @11944; MulticastDustVFX (the view's); the velocity: (Velocity of the
//!   ProjectileMovement when not exactly zero, else GetVelocity) with UE Y != 0 -> Normal((0, Y < 0 ? -0.2 : 0.2, 1)) x
//!   FMin(VSize, MaxProjectileSpeedThroughTunnel) @12324-12910, else (0, closest.Y > self.Y ? 0.05 : -0.05, 1) x the
//!   same FMin, not normalized @13502-14126; MulticastProjectileVelocity; SpawnBuffActor(
//!   Buff_ProjectileThroughTunnel_C, it, the Instigator, 0, 0.0) and StartMove(self - (0, 0, 50), closest - (0, 0, 50),
//!   (0, 0, 400), ProjectieThroughTunnelTime) @13367 (`actors::tunnel_carry`); Delay -> the closest's MulticastDustVFX
//!   (the view's).
//! - CheckForGround @9956: BoxTraceSingle at the actor (start = end, half (115, 115, 115), ProjectileGroundTrace); no hit
//!   -> K2_DestroyActor.
//! - ReceiveEndPlay @8643: a Bugs Instigator's AlliedTunnels.RemoveItem ([`tunnels_of`]), sfx_cha_bugs_tunnel_end at the
//!   actor @9022, the VFX components destroyed (the view's), TeleportedActor cleared.
//! - Not ported: the class's Comp_Pawn_PassiveBuffManager (Buff_Elemental_Fireproof_C on the tunnel itself: the tunnel
//!   takes no hit and no fire in the port); the KeepOnScreen tag (no reader); the dust and portal VFX, the widget, the
//!   RTPC (the view's).
//! - The spawn (BeginDeferredActorSpawnFromClass with collision handling 2, AdjustIfPossibleButAlwaysSpawn, @1897):
//!   FinishSpawningActor's FindTeleportSpot moves the root Capsule (no saved size: the shape's 22 / 44, at the scale
//!   1.25; Custom with no saved ObjectType: WorldDynamic, WorldStatic and Platform unlisted: Block) out of the stage
//!   block and the platforms (`root_body_blockers`): CastForGround's + 44 leaves it 11 into the floor, so it spawns 11
//!   higher.
//! - The parent's BeginPlay blast-box loop (`blast_box_loop`: DestroyOnExitedArena?, no ForceSpawnPositionInsideBlastBox),
//!   and its 0.1 s CheckBlastBoxTimer (CheckBlastBoxExit): outside, DestroySpawnedActor(true), a K2_DestroyActor.
//!
//! Model. A tunnel is a world actor; its step (after the projectiles' moves) is its tick: the owner's ring-out, the Box
//! overlaps' begin / end against the list of fighters inside the last step, ReceiveTick, then its latent Delays, then the
//! CheckForGround timer. Its ProjectileOverlap is met inside each projectile's move (`projectile_moved`), ahead of the
//! projectile's buff pass, where Buff_ProjectileThroughTunnel takes its first advance (`actors::tunnel_carry`).
// Deviation: (a) the owner's OnExitedArena is met at the tunnel's next step (the owner's `dead`), a frame after
// HandlePlayerKnockout's broadcast, so its end sound falls a frame late.
// Deviation: (l) ProjectileOverlap meets the actors with a DamageDestructible, ItemDestructible or ProjectileDestructible
// body (`capsule_x_overlap_actors`), inside the moves of the projectiles (the safe included); an item (its Sphere, out of
// a hand) is met at the tunnel's step, at the place of its last step: a frame after the move the build meets it in (the
// world has no item-move hook for world actors); a world actor host is not met. A projectile spawned inside it is met
// after its first move.
// Deviation: (b) the Box's overlaps are tested once a frame in the tunnel's step (begin / end by the list of the last
// step); the engine updates them inside each mover's move. GetOverlappingActors' order is the trace gather's
// (`box_overlap_actors`). Unverified: that a fighter's capsule overlaps the Portal channel and a projectile's HurtBox
// (its ProjectileDestructible body) is what ProjectileOverlap meets is taken from the profiles' names: the fighters' and
// projectiles' responses to those channels are not read.
// Deviation: (c) the second Delay (Duration_0 - 2) counts from the frame after the first fired; the CheckForGround timer
// is counted in the step (the timer pass comes after it in the frame: nothing it reads moves in between), a period of
// timerTicks(0.1) each, not the looping timer's float sum.
// Deviation: (d) the floor attach @1968 is a no-op on the static stages; a moving platform's tunnel stays where it was
// spawned.
// Deviation: (e) TeleportVisuals' DoOnce is the component's (per pawn); the port keeps it on the pawn's tunnels
// (`Tunnel::visuals_open`, sim state), a new tunnel taking the latest of the pawn's tunnels still in the world's actors
// (the ones destroyed in the same frame too). Lost only when every tunnel of his is gone for a whole frame and a new pair
// teleports within 0.3 s of the last warp: only the two warp sounds read it.
// Unverified: (m) the CheckBlastBoxTimer is taken to fire ahead of the CheckForGround timer of the same frame (it is set
// first, in the parent's BeginPlay); its expiries counted as Projectile.expiry counts them.
//
// Buff_Tunneling (group 13, the notify's BuffDuration 2):
// - ReceiveBeginPlay @965: StartTime; bCanWalkOffLedges false; AddActiveAttack; BPI_SetAllowGroundControlThruATKFlag
//   (true); EFighter 12 and authority @1481-1570: BPI_IsFacingLeft, a BoxTraceSingle (75, 75, 0) down 500 whose result
//   nothing reads @1942, ClearTunnelsServer @2226, CastForGround, SpawnObject(the hit, facing left, true, 13.5) @2356;
//   EnableInvulnerability(true); bCanWalkOffLedges false; HideVisibility (AddInvisibility @3729);
//   PS_BugsBunnyTunnel_Trail on the mesh (the view's); sfx_cha_bugs_tunnel_enter on the owner @2870; BPI_DisableAttack;
//   GroundEvadeAvailable? false @2998; SetMovementMode(Walking) @3031; the owner's JumpPressedDispatch bound @3116 ->
//   OnJumpPressed; Delay 1.5 @3157 -> @15.
// - @15: JumpPressedEndEarly -> nothing; else Expired = true; (authority) BPI_IsFacingLeft, SpawnObject(CastForGround,
//   facing left, false, 12.0) @270, OutofTunnelAttack (the pawn's BPI_TunnelPunch @3631), K2_DestroyActor.
// - ReceiveTick @3793 (authority): now - StartTime > 0.4 and not IsMovingOnGround -> K2_DestroyActor.
// - OnJumpPressed @4159: Expired -> nothing; else JumpPressedEndEarly = true, BPI_StartJump, BPI_IsFacingLeft,
//   sfx_cha_bugs_tunnel_enter on the owner, CastForGround, SpawnObject(.., false, 12.0) @4618, K2_DestroyActor.
// - ReceiveEndPlay @375: EnableInvulnerability(false); RemoveActiveAttack; the ground control flag false;
//   bCanWalkOffLedges true; GroundEvadeAvailable? true; BPI_EnableJump; BPI_EnableAttack; ShowVisibility
//   (RemoveInvisibility @3494); bCanWalkOffLedges true; the trail destroyed.
// - CastForGround: LineTraceSingle(the owner, down 500, ProjectileGroundTrace): its Location + (0, 0, 44); else
//   BoxTraceSingle((125, 125, 0)) on the same ends: (hit ? Location : TraceStart) + (0, 0, 44).
// Deviation: (g) the world parts of BeginPlay (ClearTunnels, CastForGround, SpawnObject, the bind) and of the Delay's end
// (SpawnObject, the punch, the destroy) run in the pawn's `world_tick` of the same frame (the buff's begin and tick have
// no use of the world's lists): behind the frame's anim pass for the Delay's end, ahead of the projectiles.
// Deviation: (h) the Delay counts from the buff's first tick, as the other buffs' BeginPlay Delays; the bind is not made
// for a non-Bugs wearer (a Disguise's copy: warned), whose OnJumpPressed would spawn nothing (no Comp_Pawn_Tunnel) but
// would end the buff early. Such a wearer's Delay end destroys the buff in its tick (the world parts do nothing for a
// pawn without Comp_Pawn_Tunnel and BPI_TunnelPunch).
// Deviation: (n) the OnJumpPressed listener is not an actor of the world's list bound to JumpPressedDispatch (a
// bare `SimActor` object, in no list): it is the flag `TunnelingExt::listener` (the owner it is bound to) and
// `jump_pressed` is the pawn's CharHooks entry, called by `jump_pressed_dispatch` behind the buffs' InputPreTriggered
// signal (a bound listener ran ahead of it). The record writes the listener's object from the flag.
// Unverified: (i) SetMovementMode(Walking) is taken as no change (the buff is applied on the ground).
// The ground-control flag write (@1318 true, @664 false) is the entry's `thru_atk.ground` (BuffEntry.thru_atk, read by
// the pawn's move input); AddActiveAttack is the count (`attack_is_active`), which `is_attacking` reads with no montage
// playing: underground the stick walks him (2150, his MaxWalkSpeed) with no dash and no turn (BaseFighter @113843,
// @115278 -> @5938-5976).
//
// Buff_TunnelTeleport (group 13; its timeline length 1, linear 0 -> 1):
// - ReceiveBeginPlay @5692: StartingLocation; SetCollisions(false) (SetActorEnableCollision on the owner and on its
//   attached actors with collision on, kept); HideVisibilityClient (AddInvisibility).
// - Activate(Destination, Time, Enter, Exit) @3092: kept; EnableInvulnerability(true); the timeline from the actor's
//   location to the destination at play rate 1 / Time, PlayFromStart; Delay(Time) -> @1967.
// - The timeline's update @7008: SetActorLocation(VLerp(start, destination, Alpha)) (no sweep); the trail VFX at
//   VLerp(Enter, Exit, Alpha) (the view's: ext ex0..ey1, alpha).
// - @1967: SetActorLocation(destination); LaunchCharacter((0, 0, 1), true, true); Collisions() (the owner's collision and
//   the kept actors' on again); Delay 0.05 -> K2_DestroyActor. Finished @15: the perks' (`perks::tunnel_finished`), ahead
//   of the Delay's end when both fall in one frame.
// - ReceiveEndPlay @5549: SetCollisions(true), the kept actors on, EnableInvulnerability(false), RemoveInvisibility, the
//   dirt burst at the location - (half height + 100) (the view's).
// Deviation: (j) SetActorEnableCollision(false) is `capsule_off` (the root capsule; the hurt boxes stay, the
// invulnerability covers them); the attached actors are not walked. The timeline's first advance is taken in Activate's
// frame (as Buff_ProjectileThroughTunnel's, `actors::tunnel_carry`), and the Delay counts that frame (the buff is spawned
// in it); the timeline advances ahead of the Delay in each later tick.

use crate::Real;
use crate::State;
use crate::actors::tunnel_carry::{can_tunnel, carry_team, carry_velocity, set_carry_velocity, through_tunnel};
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, HELD_GROUP, buff, buff_mut, buff_of, end_buff, spawn_buff};
use crate::combat::elem::instigator_charge;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hit::set_most_recent_attacker;
use crate::combat::holds::raw_half;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::common::{actor_data, buff_def, data_of, latent_frames, warn_once, warn_text};
use crate::fighter::control::enable_invulnerability;

use crate::fighter::moves::{
    Impulse, V2, attach_parent_of, disable_attack, enable_attack, enable_jump, max_jumps, recall_move,
    set_can_walk_off_ledges, try_jump,
};
use crate::fighter::remove_active_attack;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE, TRACE_PROJECTILE_GROUND,
    TraceBody, body_info, box_overlap_actors, box_trace_single, capsule_x_overlap_actors, sphere_trace_single,
};
use crate::fighter::{FighterId, actor_y, add_active_attack, facing, grounded, in_hitstun, is_attacking};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, HazardHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawning::{CapsuleShapeC, add_actor, capsule_shape, new_world_actor, root_body_blockers};
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};
use mvs_data::value::Props;

/// `TUNNEL`.
pub const TUNNEL: &str = "BugsBunnyTunnelParent_C";
/// `TUNNELING`.
pub const TUNNELING: &str = "Buff_Tunneling_C";
/// `TUNNEL_TELEPORT`.
pub const TUNNEL_TELEPORT: &str = "Buff_TunnelTeleport_C";
const COMP: &str = "Comp_Pawn_Tunnel_GEN_VARIABLE";
/// The actor's scale (see the head).
const SCALE: Real = 1.25;
const PO_TYPES: [Real; 3] =
    [OBJ_DAMAGE_DESTRUCTIBLE as Real, OBJ_ITEM_DESTRUCTIBLE as Real, OBJ_PROJECTILE_DESTRUCTIBLE as Real];

// The shapes at the actor's scale 1.25 (see the head).
fn box_hx() -> Real {
    250.0 * 0.35 * 1.25
}
fn box_hy() -> Real {
    50.0 * 1.2 * 1.25
}
fn box_depth() -> Real {
    150.0 * 1.1 * 1.25
}
fn po_half() -> Real {
    (140.0 - 120.0) * 1.25
}
fn po_r() -> Real {
    120.0 * 1.25
}

/// A fighter's entry of `TeleportedActor` (`{ a, t }`): the fighter and the time it was added.
#[derive(State, Copy, PartialEq, Debug)]
pub struct FighterStamp {
    pub a: FighterId,
    pub t: Real,
}

/// An actor's entry of `TunneledProjectiles` (`{ a, t }`).
#[derive(State, Copy, PartialEq, Debug)]
pub struct ActorStamp {
    pub a: ActorRef,
    pub t: Real,
}

/// `Tunnel` (BugsBunnyTunnelParent_C). The view's reads: `x`, `y` (the actor: Location, the Box's centre), `facing_left`
/// (Init Facing: the mesh's yaw -90 / +90), `transitioning_out`, `allies` (OverlappingAllies: every team mate in the
/// Box, the view keeps the locally controlled ones; the widget is shown while one is), the owner (its team: the Team
/// parameter of the VFX).
#[derive(State, Debug, Default)]
pub struct Tunnel {
    pub x: Real,
    pub y: Real,
    /// TunnelTeamIndex @2459 (the Comp_Actor_Destructible's).
    pub team: i32,
    /// StartingTime.
    pub start: Real,
    /// Duration_0.
    pub duration: Real,
    pub facing_left: bool,
    /// The Stop Delay's frames left (0: fired).
    pub stop_left: Real,
    /// The Duration_0 - 2 Delay's (0: not armed or fired).
    pub dur_left: Real,
    /// Frames to the CheckForGround timer's next call.
    pub ground_left: Real,
    pub transitioning_out: bool,
    /// TeleportedActor.
    pub teleported: Vec<FighterStamp>,
    /// TunneledProjectiles.
    pub tunneled: Vec<ActorStamp>,
    /// The fighters in the Box at the last step.
    pub inside: Vec<FighterId>,
    /// OverlappingAllies.
    pub allies: Vec<FighterId>,
    /// The projectiles in ProjectileOverlap after their last move, the items at the step.
    pub proj_inside: Vec<ActorRef>,
    /// The owner's TeleportVisuals DoOnce: the tick its Delay 0.3 opens it again (Deviation (e)).
    pub visuals_open: Real,
    /// BlastBoxBoundsMin / Max.
    pub blast_bounds: BlastBounds,
    /// Steps taken.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
}

/// Buff_Tunneling_C's extension (`TunnelingExt`): `start`, StartTime; `left`, the Delay 1.5's frames left; `expired`;
/// `early`, JumpPressedEndEarly; `pending`, the world part still to run in the pawn's `world_tick` (1 BeginPlay's, 2 the
/// Delay's end); `facing_left`, BPI_IsFacingLeft at BeginPlay; (`x`, `y`) the owner's location at BeginPlay
/// (CastForGround's start); `listener`, the owner the JumpPressedDispatch binding is made on (Deviation (n)).
#[derive(State, Debug, Default)]
pub struct TunnelingExt {
    pub start: Real,
    pub left: Real,
    pub expired: bool,
    pub early: bool,
    pub pending: i32,
    pub facing_left: bool,
    pub x: Real,
    pub y: Real,
    pub listener: Option<FighterId>,
}

/// Buff_TunnelTeleport_C's extension (`TeleportExt`): the trail's ends (`ex0`, `ey0` the enter tunnel, `ex1`, `ey1` the
/// exit) and `alpha` are what the view reads; (`sx`, `sy`) StartingLocation (BeginPlay); (`ax`, `ay`) the timeline's start
/// (the location at Activate) and (`dx`, `dy`) its destination; `rate` 0: not playing; `left` the Delay(Time)'s frames
/// left (0: not armed); `post` the Delay 0.05's frames left; `by` the Instigator.
#[derive(State, Debug, Default)]
pub struct TeleportExt {
    pub sx: Real,
    pub sy: Real,
    pub ax: Real,
    pub ay: Real,
    pub dx: Real,
    pub dy: Real,
    pub alpha: Real,
    pub rate: Real,
    pub left: Real,
    pub post: Real,
    pub ex0: Real,
    pub ey0: Real,
    pub ex1: Real,
    pub ey1: Real,
    pub by: Option<FighterId>,
}

/// The BugsBunnyTunnelParent_C's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &Tunnel) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

fn ts(w: &World, id: ActorId) -> Option<&Tunnel> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Tunnel(t) => Some(t),
        _ => None,
    }
}

fn ts_mut(w: &mut World, id: ActorId) -> Option<&mut Tunnel> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Tunnel(t) => Some(t),
        _ => None,
    }
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live actor").owner
}

/// The class data (`p`) of the tunnel `id` read as `num`.
fn pnum(w: &World, id: ActorId, key: &str) -> Real {
    if let Some(v) = num(actor_p(w, ActorRef::Actor(id)), key) {
        return v;
    }
    warn_text(w, &format!("{TUNNEL} data missing: {key}"));
    0.0
}

/// The pawn's Comp_Pawn_Tunnel values (Pawn_BugsBunny's template; BaseFighter has no such component).
fn comp(w: &World, f: FighterId) -> Option<&Props> {
    data_of(w, f).fighter.components.pawn.get(COMP).and_then(|c| c.p.as_obj())
}

fn cnum(w: &World, f: FighterId, key: &str) -> Real {
    if let Some(v) = comp(w, f).and_then(|c| num(c, key)) {
        return v;
    }
    warn_text(w, &format!("{COMP} data missing: {key}"));
    0.0
}

// ------------------------------------------------------------------ the tunnel

/// `elapsed`: `F(owner.time - t)`.
fn elapsed(w: &World, id: ActorId, t: Real) -> Real {
    w.fighter(owner_of(w, id)).clock.time - t
}

fn add_teleported(w: &mut World, id: ActorId, g: FighterId) {
    let now = w.fighter(owner_of(w, id)).clock.time;
    let Some(t) = ts_mut(w, id) else { return };
    t.teleported.retain(|s| s.a != g);
    t.teleported.push(FighterStamp { a: g, t: now });
}

fn is_teleporting(w: &mut World, id: ActorId, g: FighterId) -> bool {
    let Some(s) = ts(w, id).and_then(|t| t.teleported.iter().find(|e| e.a == g).copied()) else { return false };
    if elapsed(w, id, s.t) >= pnum(w, id, "TimePerActorTeleport") {
        if let Some(t) = ts_mut(w, id) {
            t.teleported.retain(|e| *e != s);
        }
        return false;
    }
    true
}

fn remove_teleported(w: &mut World, id: ActorId, g: FighterId) {
    let Some(s) = ts(w, id).and_then(|t| t.teleported.iter().find(|e| e.a == g).copied()) else { return };
    if elapsed(w, id, s.t) >= pnum(w, id, "TimePerActorTeleport")
        && let Some(t) = ts_mut(w, id)
    {
        t.teleported.retain(|e| *e != s);
    }
}

/// The fighters the Box overlaps now (GetOverlappingActors(BP_FighterCharacter)).
fn box_fighters(w: &mut World, id: ActorId) -> Vec<FighterId> {
    let Some(t) = ts(w, id) else { return Vec::new() };
    let (x, y) = (t.x, t.y);
    let found = box_overlap_actors(w, x, y, box_hx(), box_hy(), box_depth(), &[crate::real(OBJ_PAWN)], &[], -1.0);
    found
        .into_iter()
        .filter_map(|a| match a {
            ActorRef::Fighter(f) if w.fighter(f).life.dead == 0.0 => Some(f),
            _ => None,
        })
        .collect()
}

/// What ProjectileOverlap overlaps now: the actors with a DamageDestructible, ItemDestructible or ProjectileDestructible
/// body in it (a projectile's HurtBox sphere, a kind's own bodies such as BugsBunnySafe's DamageCollision, an item's
/// Sphere out of a hand).
fn overlapping(w: &mut World, id: ActorId) -> Vec<ActorRef> {
    let Some(t) = ts(w, id) else { return Vec::new() };
    let (x, y) = (t.x, t.y);
    capsule_x_overlap_actors(w, x, y, po_half(), po_r(), &PO_TYPES, &[])
}

/// AlliedTunnels of the pawn (see the head): the owner's live tunnels in spawn order.
pub fn tunnels_of(w: &World, f: FighterId) -> Vec<ActorId> {
    w.actors
        .actors
        .iter()
        .filter(|(_, a)| matches!(a.state, WorldActorState::Tunnel(_)) && a.owner == f && !a.dead)
        .map(|(id, _)| id)
        .collect()
}

/// `alliedTunnels`: GetComponentByClass(Comp_Pawn_Tunnel_C).AlliedTunnels of any pawn (BotDetectEnemy_BTS ubergraph
/// @8578-8719): `None` for a pawn without the component (IsValid false).
pub fn allied_tunnels(w: &World, f: FighterId) -> Option<Vec<ActorId>> {
    comp(w, f).map(|_| tunnels_of(w, f))
}

/// K2_DestroyActor of a tunnel: its EndPlay (the kind's `end`) at the frame's actor sweep; off AlliedTunnels at once.
fn destroy_tunnel(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// Comp_Pawn_Tunnel SpawnObject @4776.
fn spawn_object(w: &mut World, f: FighterId, at: V2, facing_left: bool, duration: Real) {
    let data = w.data.clone();
    let Some(c) = comp(w, f).cloned() else { return }; // GetComponentByClass(Comp_Pawn_Tunnel_C) on a pawn without one: None
    let cls = name!("BugsBunnyTunnelParent_C");
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), cls) else {
        warn_text(w, &format!("{TUNNEL}: no class defaults: no tunnel"));
        return;
    };
    let max_tunnels = num(&c, "MaxTunnels").unwrap_or_else(|| {
        warn_text(w, &format!("{COMP} data missing: MaxTunnels"));
        0.0
    });
    #[allow(clippy::cast_precision_loss, reason = "a short list")]
    let len = |w: &World| tunnels_of(w, f).len() as Real;
    if len(w) + 1.0 > max_tunnels {
        let mut brk = false;
        let mut i = 0usize;
        loop {
            let list = tunnels_of(w, f);
            if brk || i >= list.len() {
                break;
            }
            #[allow(clippy::cast_precision_loss, reason = "a short list")]
            let (ii, n) = (i as Real, list.len() as Real);
            if ii < n + 1.0 - max_tunnels {
                if flag(&c, "ImmediatelyDestroyExtraTunnels") {
                    destroy_tunnel(w, list[i]);
                }
            } else {
                brk = true;
            }
            i += 1;
        }
    }
    // FinishSpawningActor's FindTeleportSpot (see the head).
    let cap = obj(&def.components, "Capsule");
    let boxes = cap.and_then(|cap| root_body_blockers(w, obj(cap, "BodyInstance"), cls, name!("spawn push-out")));
    let to = match (&boxes, cap) {
        (Some(boxes), Some(cap)) => {
            let shape = capsule_shape(
                Some(CapsuleShapeC {
                    capsule_radius: num(cap, "CapsuleRadius"),
                    capsule_half_height: num(cap, "CapsuleHalfHeight"),
                }),
                SCALE,
                0.0,
                1.0,
            );
            crate::world::spawning::find_teleport_spot_shape(boxes.view(), at.x, at.y, &shape)
        }
        _ => at,
    };
    let p = &def.p;
    let stop = match num(p, "Stop Delay") {
        Some(v) => v,
        None => {
            warn_text(w, &format!("{TUNNEL} data missing: Stop Delay"));
            0.0
        }
    };
    let doe = flag(p, "DestroyOnExitedArena?");
    let force = flag(p, "ForceSpawnPositionInsideBlastBox");
    let mut t = Tunnel {
        x: to.x,
        y: to.y,
        team: w.fighter(f).team.team,
        start: w.fighter(f).clock.time,
        duration,
        facing_left,
        stop_left: latent_frames(stop),
        ground_left: timer_ticks(Some(0.1)) + 1.0,
        blast_bounds: new_blast_bounds(),
        blast_at: f64::from(0.1_f32),
        visuals_open: visuals_open(w, f),
        ..Tunnel::default()
    };
    // the parent's BeginPlay loop
    let sb = spawn_box(w);
    let (mut bx, mut by) = (t.x, t.y);
    let gone = blast_box_loop(&sb, &mut bx, &mut by, None, force, &mut t.blast_bounds, doe);
    (t.x, t.y) = (bx, by);
    let a = new_world_actor(w, cls, WorldActorClass::BugsBunnyTunnelParent, f, gone, WorldActorState::Tunnel(t));
    let id = w.actors.actors.insert(a);
    let charge = instigator_charge(w, f);
    add_actor(w, id, charge, true);
    // @1586-2272: the ground trace, its HitActor cast to DestructibleObjectBP has ObjectDestroyed += Destroy
    let (x, y) = ts_mut(w, id).map_or((0.0, 0.0), |t| (t.x, t.y));
    let me = ActorRef::Actor(id);
    let hit = sphere_trace_single(w, x, y, x, y - 375.0, 70.0, TRACE_PROJECTILE_GROUND, &[], Some(me));
    bind_ground_destroyed(w, hit.as_ref(), me, DESTROY);
}

/// Destroy @9940 (bound to the ground's ObjectDestroyed): TransitionOut(null) -> @4584.
const DESTROY: &str = "BugsBunnyTunnelParent_C.Destroy";

fn ground_destroy(w: &mut World, a: ActorRef, _wall: WallActor) {
    let ActorRef::Actor(id) = a else { return };
    if let Some(t) = ts_mut(w, id) {
        t.transitioning_out = true;
        destroy_tunnel(w, id);
    }
}

/// The parent's CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?).
fn left_arena(w: &mut World, id: ActorId) -> bool {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(t) = ts_mut(w, id) else { return false };
    blast_timer_exit(doe, &mut t.blast_at, &t.blast_bounds, crate::real(t.age) - 1.0, t.x, t.y)
}

/// Handle Clear Tunnels.
fn clear_tunnels(w: &mut World, f: FighterId) {
    for id in tunnels_of(w, f) {
        destroy_tunnel(w, id);
    }
}

/// FindClosestTunnel(Reference, TeleportingActorToCheck).
fn find_closest(w: &mut World, f: FighterId, reference: ActorId, check: Option<FighterId>) -> Option<ActorId> {
    let mut min_d = Real::INFINITY;
    let mut best = None;
    let (rx, ry) = ts(w, reference).map(|t| (t.x, t.y))?;
    for id in tunnels_of(w, f) {
        if id == reference {
            continue;
        }
        let Some(t) = ts(w, id) else { continue };
        let (dx, dy) = ((t.x - rx), (t.y - ry));
        let d = ((dx * dx) + (dy * dy)).sqrt();
        if d >= min_d || d.is_nan() {
            continue;
        }
        if let Some(g) = check
            && is_teleporting(w, id, g)
        {
            continue;
        }
        min_d = d;
        best = Some(id);
    }
    best
}

/// TeleportVisuals' DoOnce: per pawn, the tick at which its Delay 0.3 opens it again, kept on the pawn's tunnels in the
/// world's actors, dead ones of this frame included (Deviation (e)).
fn visuals_open(w: &World, f: FighterId) -> Real {
    let mut open: Real = 0.0;
    for (_, a) in w.actors.actors.iter() {
        if let WorldActorState::Tunnel(t) = &a.state
            && a.owner == f
            && t.visuals_open > open
        {
            open = t.visuals_open;
        }
    }
    open
}

fn teleport_visuals(w: &mut World, f: FighterId, enter: ActorId, g: FighterId) {
    if crate::real(w.fighter(f).clock.tick) < visuals_open(w, f) {
        return;
    }
    let open = crate::real(w.fighter(f).clock.tick) + latent_frames(0.3);
    let ids: Vec<ActorId> = w.actors.actors.ids().collect();
    for id in ids {
        if let Some(a) = w.actors.actors.get_mut(id)
            && a.owner == f
            && let WorldActorState::Tunnel(t) = &mut a.state
        {
            t.visuals_open = open;
        }
    }
    let Some((ex, ey)) = ts(w, enter).map(|t| (t.x, t.y)) else { return };
    let fid = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_bugs_tunnel_warp_generic"),
        x: Some(ex),
        y: Some(ey),
        hit: None,
        pooled: false,
        key: None,
    });
    let (gx, gy) = (w.fighter(g).body.x, actor_y(w, g));
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_bugs_tunnel_warp_end"),
        x: Some(gx),
        y: Some(gy),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// Comp_Pawn_Tunnel TeleportTunnel(EnterTunnel, ActorToTeleport) of the pawn `f`.
fn teleport_tunnel(w: &mut World, f: FighterId, enter: ActorId, g: FighterId) {
    if comp(w, f).is_none() || tunnels_of(w, f).len() <= 1 {
        return;
    }
    let ally = w.fighter(g).team.team == w.fighter(f).team.team; // CheckIfAlly
    let (only_allies, only_hitstun) = {
        let c = comp(w, f);
        (c.is_some_and(|c| flag(c, "OnlyTeleportAllies")), c.is_some_and(|c| flag(c, "OnlyRotateCharactersIfHitstun")))
    };
    if only_allies && !ally {
        return;
    }
    if is_attacking(w, g) || buff_of(w, g, name!("Buff_Tunneling_C")).is_some() {
        return;
    }
    let Some(closest) = find_closest(w, f, enter, Some(g)) else { return };
    let Some((cx, cy, cleft)) = ts(w, closest).map(|t| (t.x, t.y, t.facing_left)) else { return };
    // Get Teleport Destination: the closest + (0, 0, GetUnscaledCapsuleHalfHeight + 100).
    let (dx, dy) = (cx, (cy + (raw_half(w, g) + 100.0)));
    teleport_visuals(w, f, enter, g);
    // RotateFighter
    if (!only_hitstun || in_hitstun(w, g)) && cleft != (w.fighter(g).body.vx < 0.0) {
        let b = &mut w.fighter_mut(g).body;
        b.vx = -b.vx;
    }
    if !ally && !in_hitstun(w, g) {
        set_most_recent_attacker(w, g, f);
    }
    // TeleportFighter (the attached actors are not walked: Deviation (j))
    add_teleported(w, closest, g);
    let time = cnum(w, f, "FighterTeleportDuration");
    if let Some(b) = spawn_buff(w, g, name!("Buff_TunnelTeleport_C"), 0.0, Some(0), Some(f), true, None) {
        activate_teleport(w, g, b, dx, dy, time, enter, closest);
    }
    add_teleported(w, closest, g);
}

/// The tunnel's ReceiveTick loop @4817-8054.
fn crouch_teleports(w: &mut World, id: ActorId) {
    let mut i = 0;
    loop {
        let list = box_fighters(w, id);
        let Some(&g) = list.get(i) else { break };
        i += 1;
        let team = ts(w, id).map_or(-1, |t| t.team);
        let owner = owner_of(w, id);
        if w.fighter(g).team.team != w.fighter(owner).team.team
            || !grounded(w, g)
            || !w.fighter(g).capsule.cmc_crouched
            || is_attacking(w, g)
        {
            continue;
        }
        let _ = team;
        if attach_parent_of(w, g).is_some() || is_teleporting(w, id, g) {
            continue; // GetAttachParentActor @7487
        }
        if holds_held(w, g) {
            continue;
        }
        if is_teleporting(w, id, g) {
            continue;
        }
        teleport_tunnel(w, owner, id, g);
        add_teleported(w, id, g);
    }
}

/// Whether a buff of `f` with frames left is of the Held group.
fn holds_held(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|o| {
        o.frames > 0.0
            && buff_def(w, ActorRef::Fighter(f), o.cls).and_then(|d| obj(d, "StateInfo")).and_then(|s| text(s, "Group"))
                == Some(HELD_GROUP)
    })
}

fn tunnel_step(w: &mut World, id: ActorId) {
    let owner = owner_of(w, id);
    if let Some(t) = ts_mut(w, id) {
        t.age += 1;
    }
    // OnInstigatorExitedArena -> K2_DestroyActor (Deviation (a)).
    if w.fighter(owner).life.dead > 0.0 {
        destroy_tunnel(w, id);
        return;
    }
    // The Box's begin / end overlaps since the last step (Deviation (b)).
    let now = box_fighters(w, id);
    let (inside, team) = match ts(w, id) {
        Some(t) => (t.inside.clone(), t.team),
        None => return,
    };
    for &g in &inside {
        if now.contains(&g) {
            continue;
        }
        if ts(w, id).is_some_and(|t| t.teleported.iter().any(|s| s.a == g)) {
            remove_teleported(w, id, g);
        }
        if let Some(t) = ts_mut(w, id) {
            t.allies.retain(|x| *x != g);
        }
    }
    for &g in &now {
        if !inside.contains(&g)
            && w.fighter(g).team.team == team
            && let Some(t) = ts_mut(w, id)
            && !t.allies.contains(&g)
        {
            t.allies.push(g);
        }
    }
    if let Some(t) = ts_mut(w, id) {
        t.inside = now;
    }
    // ReceiveTick
    let start = ts(w, id).map_or(0.0, |t| t.start);
    if (w.fighter(owner).clock.time - start) > 0.2 {
        crouch_teleports(w, id);
    }
    // The Delays
    let Some(t) = ts_mut(w, id) else { return };
    if t.stop_left > 0.0 {
        t.stop_left -= 1.0;
        if t.stop_left == 0.0 {
            let d = t.duration;
            let l = latent_frames(d - 2.0);
            if let Some(t) = ts_mut(w, id) {
                t.dur_left = l;
            }
        }
    } else if t.dur_left > 0.0 {
        t.dur_left -= 1.0;
        if t.dur_left == 0.0 {
            t.transitioning_out = true; // TransitionOut @4584
            destroy_tunnel(w, id);
            return;
        }
    }
    // CheckBlastBoxExit (Unverified (m)), then CheckForGround (Deviation (c))
    if left_arena(w, id) {
        destroy_tunnel(w, id);
        return;
    }
    let Some(t) = ts_mut(w, id) else { return };
    t.ground_left -= 1.0;
    if t.ground_left <= 0.0 {
        t.ground_left = timer_ticks(Some(0.1));
        let (x, y) = (t.x, t.y);
        if box_trace_single(w, x, y, x, y, 115.0, 115.0, 115.0, TRACE_PROJECTILE_GROUND, &[], None).is_none() {
            destroy_tunnel(w, id);
        }
    }
    let alive: Vec<ActorRef> = ts(w, id)
        .map(|t| t.proj_inside.iter().copied().filter(|a| overlap_host_alive(w, *a)).collect())
        .unwrap_or_default();
    if let Some(t) = ts_mut(w, id) {
        t.proj_inside = alive;
    }
    // The items' begin overlaps with ProjectileOverlap (Deviation (l)): their Sphere at their last step's place.
    if w.actors.actors.get(id).is_some_and(|a| !a.dead) {
        let now = overlapping(w, id);
        let items: Vec<ActorRef> = w.actors.items.ids().map(ActorRef::Item).collect();
        for it in items {
            overlap_test(w, id, it, &now);
        }
    }
}

/// `!a.dead && w.items.includes(a)` / `w.projectiles.includes(a)`.
fn overlap_host_alive(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Item(i) => w.actors.items.get(i).is_some_and(|x| !x.dead),
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).is_some_and(|x| !x.dead),
        _ => false,
    }
}

/// ProjectileOverlap's begin overlap @10766 with the projectile's move.
fn projectile_moved(w: &mut World, id: ActorId, pr: crate::actors::ProjectileId) {
    let now = overlapping(w, id);
    overlap_test(w, id, ActorRef::Projectile(pr), &now);
}

/// Begin (enter) and end (leave) of a's overlap with ProjectileOverlap against the list of the last test.
fn overlap_test(w: &mut World, id: ActorId, a: ActorRef, now: &[ActorRef]) {
    let inside = overlap_host_alive(w, a) && now.contains(&a);
    let was = ts(w, id).is_some_and(|t| t.proj_inside.contains(&a));
    if !inside {
        if was && let Some(t) = ts_mut(w, id) {
            t.proj_inside.retain(|x| *x != a);
        }
        return;
    }
    if was {
        return;
    }
    if let Some(t) = ts_mut(w, id) {
        t.proj_inside.push(a);
    }
    overlap_began(w, id, a);
}

fn stamp(w: &mut World, id: ActorId, a: ActorRef, t: Real) {
    if let Some(u) = ts_mut(w, id) {
        u.tunneled.retain(|e| e.a != a);
        u.tunneled.push(ActorStamp { a, t });
    }
}

fn overlap_began(w: &mut World, id: ActorId, pr: ActorRef) {
    // Deviation: a stage item no one has picked up (Item.owner null) is not carried (its buff host needs an owner)
    if let ActorRef::Item(it) = pr
        && !crate::actors::item::is_owned(w, it)
    {
        return;
    }
    let owner = owner_of(w, id);
    let Some((tx, ty, tteam)) = ts(w, id).map(|t| (t.x, t.y, t.team)) else { return };
    if !can_tunnel(w, pr) || carry_team(w, pr) != tteam {
        return;
    }
    let s = ts(w, id).and_then(|t| t.tunneled.iter().find(|e| e.a == pr).copied());
    let now = w.fighter(owner).clock.time;
    if let Some(s) = s
        && (now - s.t) < pnum(w, id, "TunneledProjectileBufferTime")
    {
        return;
    }
    let Some(closest) = find_closest(w, owner, id, None) else { return };
    let Some((cx, cy)) = ts(w, closest).map(|t| (t.x, t.y)) else { return };
    stamp(w, id, pr, now);
    stamp(w, closest, pr, now);
    // MulticastDustVFX @12025 (PS_BugsBunnyDirtBurst at the actor @14716), and the closest's after the Delay
    // (ProjectieThroughTunnelTime) @13443 -> @586: one event, the Delay counted by the view.
    let fid = w.fighter(owner).id;
    w.events.push(Event::ProjectileDeath {
        cls: name!("BugsBunnyTunnelParent_C"),
        fighter: fid,
        vfx: Some(name!("PS_BugsBunnyDirtBurst")),
        x: tx,
        y: ty,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: Some([cx, cy]),
    });
    let v = carry_velocity(w, pr);
    let speed = min(((v.x * v.x) + (v.y * v.y)).sqrt(), pnum(w, id, "MaxProjectileSpeedThroughTunnel"));
    if v.x != 0.0 {
        // Normal((0, +-0.2, 1), 1e-4)
        let sx = if v.x > 0.0 { 0.2 } else { -0.2 };
        let [_, nx, ny] = crate::math::ue::normal([0.0, sx, 1.0], crate::math::ue::KINDA_SMALL_NUMBER);
        set_carry_velocity(w, pr, nx * speed, ny * speed);
    } else {
        let s = if cx < tx { -0.05 } else { 0.05 };
        set_carry_velocity(w, pr, s * speed, speed);
    }
    let time = pnum(w, id, "ProjectieThroughTunnelTime");
    through_tunnel(
        w,
        pr,
        owner,
        V2 { x: tx, y: (ty - 50.0) },
        V2 { x: cx, y: (cy - 50.0) },
        V2 { x: 0.0, y: 400.0 },
        time,
    );
}

/// The bodies a trace or overlap query meets: the Box (Portal) and ProjectileOverlap (HitBox) at the actor.
// Deviation: the root Capsule (bodyInfo: WorldDynamic, query on) is not entered. In the build a WorldDynamic overlap (a
// Morty grenade blast, the popcorn, the dynamite) finds it and calls pfgDoDamage on the tunnel; the port's tunnel takes
// no hits, and two tunnels' Capsules do not push each other out (FindTeleportSpot).
fn tunnel_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(t) = ts(w, id) else { return };
    let data = w.data.clone();
    let comps = actor_data(w, &data, ActorRef::Fighter(a.owner), a.cls).map(|d| &d.components);
    for (comp_name, key, is_box) in
        [(name!("Box"), "Box", true), (name!("ProjectileOverlap"), "ProjectileOverlap", false)]
    {
        let Some(info) = body_info(comps.and_then(|c| obj(c, key)), None) else { continue };
        if !info.on {
            continue;
        }
        out.push(TraceBody {
            ax: if is_box { t.x } else { t.x - po_half() },
            ay: t.y,
            bx: if is_box { t.x } else { t.x + po_half() },
            by: t.y,
            hx: if is_box { box_hx() } else { 0.0 },
            hy: if is_box { box_hy() } else { 0.0 },
            r: if is_box { 0.0 } else { po_r() },
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component: comp_name,
            actor: Some(ActorRef::Actor(id)),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

fn tunnel_end(w: &mut World, id: ActorId) {
    let Some(t) = ts(w, id) else { return };
    let (x, y) = (t.x, t.y);
    let fid = w.actors.actors.get(id).map_or(-1, |a| a.owner_id);
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_bugs_tunnel_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    if let Some(t) = ts_mut(w, id) {
        t.teleported.clear();
    }
}

/// BPI_EnvironmentalHazard (C019ScrubShield). What its Sphere overlaps: ProjectileOverlap (object type HitBox, Overlap to
/// ProjectileDestructible: the capsule along x of [`tunnel_bodies`]); the Box is a Portal (Ignore).
fn hazard_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let ActorRef::Actor(id) = a else { return false };
    let Some(t) = ts(w, id) else { return false };
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return false;
    }
    let dx = max((x - t.x).abs() - po_half(), 0.0);
    let dy = y - t.y;
    let s = po_r() + r;
    dx * dx + dy * dy <= s * s
}

/// BPI Clean @0-15: DestroySpawnedActor(true) (IgnoreHitPause: K2_DestroyActor, CanBePooled false), Success = true.
fn hazard_clean(w: &mut World, a: ActorRef) -> bool {
    if let ActorRef::Actor(id) = a {
        destroy_tunnel(w, id);
    }
    true
}

// ------------------------------------------------------------------ Buff_Tunneling

/// CastForGround from (x, y) (the owner's location at the call).
fn cast_for_ground(w: &mut World, x: Real, y: Real) -> V2 {
    let (ex, ey) = (x, (y - 500.0));
    // LineTraceSingle
    if let Some(h) = sphere_trace_single(w, x, y, ex, ey, 0.0, TRACE_PROJECTILE_GROUND, &[], None) {
        return V2 { x: h.x, y: (h.y + 44.0) };
    }
    match box_trace_single(w, x, y, ex, ey, 125.0, 0.0, 125.0, TRACE_PROJECTILE_GROUND, &[], None) {
        Some(b) => V2 { x: b.x, y: (b.y + 44.0) },
        None => V2 { x, y: (y + 44.0) },
    }
}

fn is_bugs(w: &World, f: FighterId) -> bool {
    w.names.str(w.fighter(f).ch) == super::KEY // BPI_GetFighterCharacter == 12
}

fn tun(w: &World, b: BuffRef) -> Option<&TunnelingExt> {
    match &buff(w, b)?.ext {
        BuffExt::Tunneling(e) => Some(e),
        _ => None,
    }
}

fn tun_mut(w: &mut World, b: BuffRef) -> Option<&mut TunnelingExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::Tunneling(e) => Some(e),
        _ => None,
    }
}

fn tunneling_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let e = TunnelingExt {
        start: w.fighter(f).clock.time,
        left: latent_frames(1.5),
        expired: false,
        early: false,
        pending: 1,
        facing_left: facing(w, f) < 0.0,
        x: w.fighter(f).body.x,
        y: actor_y(w, f),
        listener: None,
    };
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Tunneling(e);
    }
    set_can_walk_off_ledges(w, f, false);
    add_active_attack(w, f);
    enable_invulnerability(w, f, true, false);
    add_invisibility(w, f);
    let (fid, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_bugs_tunnel_enter"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    disable_attack(w, f);
    w.fighter_mut(f).evade.no_ground_evade = true;
    if !is_bugs(w, f) {
        warn_once(w, name!("Buff_Tunneling on a pawn other than Bugs Bunny: OnJumpPressed not bound"));
    }
}

fn tunneling_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = tun(w, b) else { return false };
    if (w.fighter(f).clock.time - e.start) > 0.4 && !grounded(w, f) {
        return true; // ReceiveTick
    }
    let Some(e) = tun_mut(w, b) else { return false };
    if e.left > 0.0 {
        e.left -= 1.0;
        if e.left == 0.0 && !e.early {
            e.expired = true;
            e.pending = 2;
            // A wearer other than Bugs (a Disguise copy) has no BPI_TunnelPunch and no Comp_Pawn_Tunnel: the SpawnObject
            // and BPI_TunnelPunch of the Delay's end do nothing, K2_DestroyActor remains (his world tick is not this
            // file's hook).
            if !is_bugs(w, f) {
                return true;
            }
        }
    }
    false
}

fn tunneling_end(w: &mut World, f: FighterId, _b: BuffRef) {
    enable_invulnerability(w, f, false, false);
    remove_active_attack(w, f);
    set_can_walk_off_ledges(w, f, true);
    w.fighter_mut(f).evade.no_ground_evade = false;
    enable_jump(w, f);
    enable_attack(w, f);
    remove_invisibility(w, f);
}

/// OnJumpPressed @4159: the binding's handler (Deviation (n): the pawn's CharHooks `jump_pressed`).
fn jump_pressed(w: &mut World, f: FighterId) {
    let Some(b) = w
        .fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|x| {
            x.frames > 0.0
                && x.class == BuffClass::BuffTunneling
                && matches!(&x.ext, BuffExt::Tunneling(e) if e.listener == Some(f))
        })
        .map(|x| BuffRef { fighter: f, serial: x.serial })
    else {
        return;
    };
    let Some(e) = tun_mut(w, b) else { return };
    if e.expired {
        return;
    }
    e.early = true;
    if crate::real(w.fighter(f).jump.jumps) < max_jumps(w, f) {
        try_jump(w, f); // BPI_StartJump
    }
    let left = facing(w, f) < 0.0;
    let (fid, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_bugs_tunnel_enter"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    let at = cast_for_ground(w, x, y);
    spawn_object(w, f, at, left, 12.0);
    end_buff(w, f, b);
}

/// The pawn's `world_tick`: Buff_Tunneling's world parts (Deviation (g)).
fn world_tick(w: &mut World, f: FighterId) {
    let Some(b) = buff_of(w, f, name!("Buff_Tunneling_C")) else { return };
    let Some(e) = tun(w, b) else { return };
    let (pending, fl, ex, ey) = (e.pending, e.facing_left, e.x, e.y);
    if pending == 1 {
        if let Some(e) = tun_mut(w, b) {
            e.pending = 0;
        }
        clear_tunnels(w, f);
        let at = cast_for_ground(w, ex, ey);
        spawn_object(w, f, at, fl, 13.5);
        if let Some(e) = tun_mut(w, b) {
            e.listener = Some(f);
        }
    } else if pending == 2 {
        if let Some(e) = tun_mut(w, b) {
            e.pending = 0;
        }
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        let at = cast_for_ground(w, x, y);
        let left = facing(w, f) < 0.0;
        spawn_object(w, f, at, left, 12.0);
        super::pawn::tunnel_punch(w, f);
        end_buff(w, f, b);
    }
}

/// The hooks this file joins in Bugs Bunny's CharHooks (`world_tick`, `jump_pressed`).
pub fn hooks(h: &mut CharHooks) {
    h.world_tick = Some(world_tick);
    h.jump_pressed = Some(jump_pressed);
}

// ------------------------------------------------------------------ Buff_TunnelTeleport

fn tel(w: &World, b: BuffRef) -> Option<&TeleportExt> {
    match &buff(w, b)?.ext {
        BuffExt::TunnelTeleport(e) => Some(e),
        _ => None,
    }
}

fn tel_mut(w: &mut World, b: BuffRef) -> Option<&mut TeleportExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::TunnelTeleport(e) => Some(e),
        _ => None,
    }
}

fn teleport_update(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = tel(w, b) else { return };
    let (x, y) = ((e.ax + (e.alpha * (e.dx - e.ax))), (e.ay + (e.alpha * (e.dy - e.ay))));
    recall_move(w, f, x, y);
}

fn teleport_advance(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = tel_mut(w, b) else { return };
    if e.rate <= 0.0 {
        return;
    }
    e.alpha += DELTA_SECONDS * e.rate;
    let done = e.alpha >= 1.0;
    if done {
        e.alpha = 1.0;
        e.rate = 0.0;
    }
    let (by, dx, dy) = (e.by, e.dx, e.dy);
    teleport_update(w, f, b);
    if done {
        super::perks::tunnel_finished(w, f, by, dx, dy); // Finished @15, behind the timeline's last update
    }
}

/// Activate(Destination, Time, Enter, Exit) @3092.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn activate_teleport(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    dx: Real,
    dy: Real,
    time: Real,
    enter: ActorId,
    exit: ActorId,
) {
    let (e0, e1) = (ts(w, enter).map(|t| (t.x, t.y)), ts(w, exit).map(|t| (t.x, t.y)));
    let (ax, ay) = (w.fighter(f).body.x, actor_y(w, f));
    let Some(e) = tel_mut(w, b) else { return };
    if let Some((x, y)) = e0 {
        e.ex0 = x;
        e.ey0 = y;
    }
    if let Some((x, y)) = e1 {
        e.ex1 = x;
        e.ey1 = y;
    }
    e.dx = dx;
    e.dy = dy;
    enable_invulnerability(w, f, true, false);
    let Some(e) = tel_mut(w, b) else { return };
    e.ax = ax;
    e.ay = ay;
    e.alpha = 0.0;
    e.rate = 1.0 / time;
    teleport_update(w, f, b); // PlayFromStart's update at 0
    teleport_advance(w, f, b); // the first advance in this frame (Deviation (j))
    if let Some(e) = tel_mut(w, b) {
        e.left = latent_frames(time) - 1.0; // the Delay counts this frame
    }
}

fn teleport_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    // by: the actor's Instigator (the tunnels' owner), read at Timeline Finished (`perks::tunnel_finished`). Kept in
    // the extension, not in `src` (the passive buffs' field, the respawn's passive_from).
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let e = TeleportExt { sx: x, sy: y, ax: x, ay: y, dx: x, dy: y, by: from, ..TeleportExt::default() };
    if let Some(s) = buff_mut(w, b) {
        s.ext = BuffExt::TunnelTeleport(e);
    }
    w.fighter_mut(f).capsule.capsule_off = true; // SetCollisions(false)
    add_invisibility(w, f); // HideVisibilityClient
}

fn teleport_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    teleport_advance(w, f, b);
    let Some(e) = tel_mut(w, b) else { return false };
    if e.left > 0.0 {
        e.left -= 1.0;
        if e.left == 0.0 {
            let (dx, dy) = (e.dx, e.dy);
            recall_move(w, f, dx, dy);
            // LaunchCharacter((0, 0, 1), true, true)
            w.fighter_mut(f).moves.impulse = Some(Impulse { vx: 0.0, vy: 1.0, xy: true, kind: 0, z: true, d: None });
            w.fighter_mut(f).capsule.capsule_off = false; // Collisions()
            if let Some(e) = tel_mut(w, b) {
                e.post = latent_frames(0.05);
            }
            return false;
        }
    }
    let Some(e) = tel_mut(w, b) else { return false };
    if e.post > 0.0 {
        e.post -= 1.0;
        return e.post == 0.0;
    }
    false
}

fn teleport_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).capsule.capsule_off = false;
    enable_invulnerability(w, f, false, false);
    remove_invisibility(w, f);
}

/// This file's registry entries (BugsBunnyTunnelParent_C, Buff_Tunneling_C, Buff_TunnelTeleport_C; the OnJumpPressed
/// listener is the pawn's `jump_pressed`, Deviation (n)), after Buff_ProjectileThroughTunnel_C (`tunnel_carry.rs` is
/// imported ahead of this file's own entries).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    crate::actors::tunnel_carry::register(r)?;
    let hooks = crate::registry::entries::WorldActorHooks {
        bodies: Some(tunnel_bodies),
        step: Some(tunnel_step),
        end: Some(tunnel_end),
        projectile_moved: Some(projectile_moved),
        ..crate::registry::entries::WorldActorHooks::default()
    };
    let mut e = ActorEntry::new(ActorClass::World(WorldActorClass::BugsBunnyTunnelParent), ActorHooks::World(hooks));
    e.hazard = Some(HazardHooks { overlaps: hazard_overlaps, clean: hazard_clean });
    r.actors.enter(TUNNEL, e)?;
    wall_delegate_fns(r, &[(DESTROY, ground_destroy)])?;
    let mut t = BuffEntry::new(
        BuffClass::BuffTunneling,
        BuffHooks {
            begin: Some(tunneling_begin),
            tick: Some(tunneling_tick),
            end: Some(tunneling_end),
            ..BuffHooks::default()
        },
    );
    t.thru_atk = Some([true, false, false]); // BeginPlay @1318, EndPlay @664
    t.active_attack = true; // BeginPlay @1181, EndPlay @527
    r.buffs.enter(TUNNELING, t)?;
    r.buffs.enter(
        TUNNEL_TELEPORT,
        BuffEntry::new(
            BuffClass::BuffTunnelTeleport,
            BuffHooks {
                begin: Some(teleport_begin),
                tick: Some(teleport_tick),
                end: Some(teleport_end),
                ..BuffHooks::default()
            },
        ),
    )
}
