//! The tether: StevenTetherStunProjectile_C ([`StevenTether`], a projectile with
//! an `attachedTo`), Buff_BubbleCount ([`BubbleCount`]) and Buff_StevenBubbleEnemy ([`BubbleEnemy`]: the enemy in the
//! bubble). No pawn hook: nothing of the three is reached through the pawn.
//!
//! ---- StevenTetherStunProjectile_C ----
//! Creature's TetherProjectile (`chars/creature/tether.rs`) without its homing, its pull and its cylinder damage; the
//! shapes are shared (`actors::tether_shape`). By entry:
//! - ReceiveBeginPlay @3966 (SpawnedActorParent's first; ParentProjectilePhysics has none): the EndPoint TargetPoint
//!   @4004, then (push_flow order) OriginalRotation @4247, AttachBeam(self) @4274, BeamVFX @11749 (PS_StevenBeam_Point
//!   on the owner's mesh at Gem, sfx_cha_steven_tether_cast on the owner @12049, sfx_cha_steven_tether_loop on self
//!   @12122 kept as AK RayLoop, the team colour), Delay(1.0) @4359 -> @187, Delay(0.3) @4304 -> @15, the owner's
//!   HitStunStart / HitStunEnded bound to HitStunEffectOn / Off @4509 / @4595 (the beam's Team parameter: the
//!   render's), ChargeSFX @13625 (sfx_cha_steven_tether_charge on self).
//! - @15 (0.3 s): a DoOnce, SetVelocityInLocalSpace(0) @61 and, unless Attached, SpawnAttachPoint, AttachedToAlly =
//!   false, DestroyOnBlocked = false. Blocked (@13134) enters at @61, behind the DoOnce.
//! - @187 (1 s): AttachedToAlly or not, Delay(1.0) @201 -> @256: DoKnockUp, DoKnockUpEnd, EndVFX, Delay(0.5) @298 ->
//!   K2_DestroyActor @353. The burst comes 2 s after the spawn whether the tether found an ally or not.
//! - The Sphere's (PlayerAttachCollision, 40 x scale 8 = 320) begin overlap @9224 -> @368: !AttachedToAlly &&
//!   IsOverlappingActor(other), else Delay(0.05) @1632 -> @368 again; other a BaseFighter, the owner one, the same
//!   TeamIndex, other != owner (no NotPlayerCharacter test, unlike Creature's); unless Attached SpawnAttachPoint
//!   first (@1613); DestroyOnBlocked = false, AttachedToAlly = true, K2_AttachToActor(other, SnapToTarget x 3) @938,
//!   AttachedAlly = the attach parent, collision off on PlayerAttachCollision / Capsule / StaticMesh / DamageBox /
//!   HitBox, PF_Post_VO_Event(owner, ability_tether, 0.3) @1254, AttachFXMulticast @12785
//!   (sfx_cha_steven_tether_attach on its Ak component, PS_StevenBeam_AllyPoint on the root, PS_TetherProjectile
//!   destroyed: the render's), the ally's OnExitedArena bound to the actor's end @1413.
//! - ReceiveTick @4932 (every frame: no TickInterval; it does not call the parent's: own tick): the beam's source
//!   points at the owner's Gem; unless Attached nothing more @5315. Then LineTraceSingleByProfile(Gem -> self,
//!   'Tether') @5525, whose result nothing reads (not run); the EndPoint at self; the Cylinder's transform
//!   @5762-6599 (as Creature's); TetherSlope / TetherB @7013 / @7267; OverlappedActors =
//!   Cylinder.GetOverlappingActors(BP_FighterCharacter) less the owner and the attach parent @7305-7482; each one's
//!   IsVulnerableTo(the owner's TeamIndex) @7960, whose answer nothing reads (no damage: this class's tick hurts
//!   nobody). Then HitVFXEnabled && VFXTriggered -> both false, K2_SetTimerDelegate(ResetVFXEnabled, HitVFXWindow)
//!   @8252 (-> HitVFXEnabled = true @13257).
//! - DoKnockUp @13317 -> @9229: for each of OverlappedActors (the last tick's) with a Comp_Actor_Destructible: 'Hit Box
//!   Damage'.ForceKnockback = another team @3398, the actor's own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, it, 'Hit
//!   Box Damage', 'On Hit Buffs') @3485, KnockupVFX @3589, BPI_IncrementCoopActions @3720 (statistics: left out).
//! - DoKnockUpEnd @13489: SphereOverlapActors(the actor's location, 360, [PhysicsBody, Pawn, DamageDestructible,
//!   ProjectileDestructible, ItemDestructible], BP_FighterCharacter) @13549 -> @9280: each valid one with a
//!   Comp_Actor_Destructible (the owner and the ally among them), the same ForceKnockback write @9885, pfgDoDamage
//!   @9972, KnockupVFX @10076; then SphereOverlapActors(the Instigator's Gem, 360, the same types) @10381 -> each
//!   valid fighter of another team than the Instigator that is not IsInvulnerable? @10944-11280, ForceKnockback @11469
//!   (false: @11290 reads the tether's own Comp_Actor_Destructible, @11387 compares its team with itself), pfgDoDamage
//!   @11556, KnockupVFX @11660.
//! - KnockupVFX @13484 -> @8480 reads the ubergraph's locals of the tick's loop: IsValid(the owner cast @7885, set only
//!   inside that loop) and then the last fighter that loop went over (@7790), not the burst's victim:
//!   sfx_cha_reindog_tether_hit (Reindog's event) at that fighter's location @8569 and, with HitVFXEnabled,
//!   VFXTriggered = true and PS_HitImpact_TEMPLATE at (its X, its Y, TetherSlope x its Y + TetherB) + (55, 0, 0),
//!   scale 0.625, its damage colour @8665-9165. A tether whose Cylinder never held a fighter plays nothing there.
//! - EndVFX @13327: PS_TetherProjectile and TetherSourceVFX destroyed, the VFXCleanup list emptied @3001, then
//!   PS_StevenNANAIR_BubbleBurst on the Capsule @2095 and at the Instigator's Gem @2394 (Team),
//!   sfx_cha_steven_tether_end at the actor's location @2914. BeamParticleSystem is not destroyed: the tick keeps it
//!   until the actor goes.
//! - ReceiveDestroyed @4697: ExecuteActionOnPlayingID(Stop, AK RayLoop, 500 ms, curve 4 = Linear), the EndPoint,
//!   TetherSourceVFX and ConnectedVFX destroyed.
//! - Dormant: the DamageBox's ComponentHit @8320 (DamageBox NoCollision in the data: its SeismicReactor_C exclusion
//!   @8349 and K2_AttachToComponent @8426 are never reached), ExitBlastBoxWarningMulticast (no caller), SwapPosition /
//!   DisableProjectile (no caller), the TetherBreakWarning test @8309 (nothing writes it).
//!
//! Model: a Projectile of the world's list, as Creature's: no hit box (HitBox untagged, HitBox2D and HurtBox
//! NoCollision), its move swept against the stage block (`receive_hit`; the Capsule ignores Platform), on an ally
//! riding that fighter's actor location ([`ride`], called by the shared step). Its Delays and its timer count on the
//! actor's age in its step, as Creature's (one made in BeginPlay or an overlap event counts that frame, one made in a
//! Delay's link the next).
// Deviation (as Creature's): the begin overlaps are tested once a frame behind the move against the root capsules; the
// Cylinder is the rectangle of half width 25 (the Cylinder mesh's radius 50 x scale 0.5) from the Gem to the actor;
// the ally's OnExitedArena is read at the tether's next step (`dead`); the Delays are taken in the projectile pass.
// Unverified: in a frame the latent pass follows ReceiveTick (AActor::Tick's order), so the burst reads that frame's
// OverlappedActors.
// Unverified: the spheres' order is the gather's (`sphere_overlap_actors`: fighters in sim order).
// pfgDoDamage from the actor's own handler, as `chars/batman/perks_grapple.rs` after_grapple_travel: a dead or
// unhittable one is left; HitEachActorOnlyOnce with AttackTag None (CheckIfActorHasBeenHit of the tether's handler)
// leaves one the burst already took, across the three lists (`knocked`); the projectile pops
// (`hits::projectile_blocks`); an invulnerable enemy takes the dodge path (BPI_CanMarkFighter: the class default), an
// invulnerable team mate nothing; a team mate is tracked and gets NotifyAllyOfHit (`hits::ally_hit`: nothing for the
// Instigator himself); an enemy the hit (`apply_projectile_hit` with the 'On Hit Buffs' rows, HitLocation its
// capsule) and HitEnemyPlayer. ForceKnockback is written per call as the graph does; the port's spawned-actor hit
// path reads it nowhere.
//
// ---- Buff_BubbleCount_C, the tether's on-hit row ----
// The class data the shared code takes as it is: MaxStacks 10, BuffDuration 3, CanStack, PreventStacksWhenReapplied,
// StackDecayRate 0.5, StackDecayBuffer 7, IsDebuff, BlockerBuffs [Buff_StevenBubbleEnemy_C] (no count on a bubbled
// fighter), BlockerGroups [CC Immune].
// - BPI On Fighter Owner Knocked Back -> @1340 -> AddStack(0) @1094 (BuffParentActor's AddStack takes 0 as 1:
//   `combat::buffs::stack_buff`). The knockback of the hit that brings the buff precedes the row's spawn.
// - BPI On Max Stacks @914: the owner's SpawnBuffActor(Buff_StevenBubbleEnemy, the owner, the Instigator, 1 stack,
//   5.0) @1005, Bubble Visuals @1118 (PS_Explosion_Satchel on the owner's mesh at VFX_Root @1217), K2_DestroyActor
//   @24.
// - BPI After Status Update Multicast @43: removed or stacks <= 0 -> RemoveStateForPlayer(BubbleCount) @388, else
//   AddStateForPlayer(BubbleCount: type 1, display 0, the stacks of MaxStacks, timer -1, MI_State_Rose, active,
//   AutoRemoveTime -1) @524-855. As Harley's count (`chars/harley/actors.rs`): AddStack's own status call behind the
//   hook runs on the destroyed actor with the cap's count, so the state EndPlay removed is added again, 10 of 10,
//   until the next count buff on this fighter ends (`stack_buff` makes that call).
// Deviation (as Harley's): PS_Explosion_Satchel is sent as an emitter at the owner's place (no attach to VFX_Root).
//
// ---- Buff_StevenBubbleEnemy_C ----
// The class data the shared code takes as it is: BuffDuration 5 (the count's spawn passes 5.0), HasBuffDurationDecay
// (5 s, levels [5, 3, 1, 0.1]), DealtDamageMods.TurnOffHitBoxes (the bubbled fighter's hit boxes deal nothing),
// SpeedMod 0.5 Multiply, Stun 0.5 (no struggle, no suspension), Group Stunned, BreakBuffs [Buff_BubbleCount_C].
// - ReceiveBeginPlay @2182 (after BuffParentActor's): the owner a BaseFighter with a DamageManager: BPI_DisableJump /
//   DisableEvade / DisableAttack @2397-2469, LaunchCharacter((0, 0, 50), true, true) @2505, FMC.OverrideGravity(0.0)
//   @2653, Comp_State_Manager.UpdatePlayerStatus(null) @2690, SpawnVFX @2853 (PS_StevenDebuffBubble_Activate and
//   PS_StevenDebuff_BubbleRose on the active mesh's Root, scaled by the mesh's VFX scale x BubbleVFXScaling 0.85, at
//   the capsule's half height: the render's; sfx_cha_steven_bubble_cast at the capsule @3628,
//   sfx_cha_steven_bubble_loop on self @3714; the PopText @4298-4807 ([`bubbled_pop`]); the owner's
//   OnActiveMeshChanged bound), Delay(1.0) @2798 -> @15: the owner's Comp_Pawn_HitBoxHandler.HitByAlly ->
//   BreakandDestroy (K2_DestroyActor) @219 and Comp_Pawn_DamageManager.KnockedBack -> KnockbackByEnemy @464.
// - ReceiveTick @6464 (authority): unless stopTick, LaunchCharacterMulticast @7948 -> @7121: any stick (MoveXAxisValue
//   or MoveYAxisValue not 0) -> LaunchCharacter((0, MoveX > 0 ? -650 : 650, MoveY > 0 ? 350 : 200), true, true)
//   @7900, else LaunchCharacter((0, 0, 250), true, true) @6874. UE Y -650 is the sim's +650: the stick's side.
// - KnockbackByEnemy @8019 -> @6926: Delay(0.0) @7066 -> @1232: CheckHasBuffGroup(Held) -> nothing; else a DoOnce
//   @1365: MoveToLocationMulticast @7953 (stopTick = true, Delay(0.1) @7964 -> @1517: SpawnBuffActor(Buff_MoveToPosition,
//   the owner, the owner, 0 stacks, 0.1 s) @1588, its MulticastBeginMove(owner + (0, 0, 250), false, false, 0.1) @1867,
//   K2_TeleportTo of this buff actor (nothing of the owner) @2089, Delay(0.1) @2127 -> stopTick = false @1505), then
//   Delay(0.25) @1425 -> the DoOnce open again @1209.
// - ReceiveEndPlay @4960 (after the parent's): EndVFX @5371 (OnActiveMeshChanged unbound, the emitters, a last
//   PS_StevenDebuffBubble_Activate: the render's; sfx_cha_steven_bubble_end at the actor @6287 and
//   mix_stop_all_fade_eighthofsecond on self @6373: the loop's stop), ClearGravityOverride @5230, BPI_EnableAttack /
//   EnableEvade / EnableJump @5262-5334.
// Buff_MoveToPosition is the force move of Arya's knife and Comp_Pawn_TeleportToAlly (`fighter::match_side`):
// `Fighter.team.ally.move_id`, ended by its own count or by the knockback (DestroyOnKnockedBack). Deviation
// (Buff_MoveToPosition the Arya way): no buff entry, so its StateInfo (Group 13, BreakGroups 12 and 13) breaks
// nothing and CheckHasBuff(Buff_MoveToPosition) finds nothing.
// OverrideGravity(0) / ClearGravityOverride: `override_gravity` (GravityScale 1.0 offline whatever the scale, from the
// second move; the clear gives the BeginPlay scale 5.25 at once).
// Deviation: the buff's ReceiveTick and its latent actions are the port's tick-buffs pass (`BuffHooks::tick`), the
// tick ahead of the Delays (AActor::Tick); a Delay made in BeginPlay counts the spawn frame (the leftover latent
// pass), one made in a Delay's link the next frame, the KnockedBack's Delay(0.0) the frame after the hit.
// The launches are direct (CancelJump's kind: `hit_launch`), taken in a hitpause and in the knockback's frames too.
// The sounds at "the actor" are put at the owner (a buff actor stays where it was spawned; the K2_TeleportTo above
// moves it 250 up per knockback: not followed).
// Unverified: SpawnVFX is called behind IsServer @2749-2784 (a plain function, no multicast), so an online client of
// the build may not see it; the port's peers each run the sim and draw it, as for its two sounds.
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "NaN-keeping negated tests")]

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, stop_simulating};
use crate::actors::tether_shape::{attach_radius, gem_of, rect_hits, sphere_hits};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, HELD_GROUP, buff, buff_mut, spawn_buff, stack_buff, update_states};
use crate::combat::elem::charge_pct;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{self, Dealer};
use crate::combat::packet::HitPacket;
use crate::combat::phases::no_hit;
use crate::combat::pop_text::{GraphPopText, pop_text_on};
use crate::events::{Event, SoundKey, UiStateOp, UiStateParams};
use crate::fighter::common::{buff_def, latent_frames, peers};
use crate::fighter::control::attack_dodged;
use crate::fighter::match_side::{TutorialEvent, end_move_buff, post_vo};
use crate::fighter::moves::{Impulse, V2, clear_gravity_override, force_move_to_point, override_gravity};
use crate::fighter::moves::{disable_attack, disable_evade, disable_jump, enable_attack, enable_evade, enable_jump};
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE,
    sphere_overlap_actors,
};
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks, ProjectileReceiveHit};
use crate::registry::props::{flag, num, num_or, obj, text};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::tutorial_event;
use indexmap::IndexSet;
use mvs_data::value::Value;

const STEVEN_TETHER: Name = name!("StevenTetherStunProjectile_C");
const BUBBLE_COUNT: Name = name!("Buff_BubbleCount_C");
const BUBBLE_ENEMY: Name = name!("Buff_StevenBubbleEnemy_C");
const LOOP: Name = name!("sfx_cha_steven_tether_loop");
/// The loop sound's key (`'stevenTether' + id`).
const LOOP_TAG: Name = name!("stevenTether");
/// The bubble's loop key (`'stevenBubble' + f.id`).
const BUBBLE_TAG: Name = name!("stevenBubble");
/// The Cylinder mesh's radius 50 x RelativeScale3D 0.5.
const CYL_HALF_WIDTH: Real = 25.0;
/// Both SphereOverlapActors of DoKnockUpEnd.
const KNOCK_RADIUS: Real = 360.0;

/// `StevenTether extends Projectile` (StevenTetherStunProjectile_C): what the class adds.
#[derive(State, Debug)]
pub struct StevenTether {
    /// Attached (SpawnAttachPoint: stopped, the tether points shown, the beam to the EndPoint).
    pub pinned: bool,
    /// AttachedAlly; AttachedToAlly with it.
    pub ally: Option<FighterId>,
    /// The 0.3 link's DoOnce.
    pub stop_once: bool,
    /// EndVFX ran: the source and projectile emitters gone, the bursts shown (the render's).
    pub end_vfx: bool,
    /// The ages its Delays fire at (-1: none): 0.3 -> @15, 1 -> @187, 1 -> @256, 0.5 -> @353, the overlap retry 0.05.
    pub at03: Real,
    pub at1: Real,
    pub knock_at: Real,
    pub destroy_at: Real,
    pub retry_at: Real,
    pub retry_who: Option<FighterId>,
    /// The fighters (ids) the PlayerAttachCollision overlaps.
    pub over: IndexSet<i32>,
    /// OverlappedActors, the last tick's.
    pub overlapped: Vec<FighterId>,
    /// The tick loop's last fighter (@7790), what KnockupVFX reads.
    pub last_cyl: Option<FighterId>,
    /// TetherSlope in the play plane (dy / dx of the Gem -> actor line; Divide_FloatFloat by 0 is 0).
    pub slope: Real,
    /// The Gem of the last tick (TetherB's point).
    pub gem_x: Real,
    pub gem_y: Real,
    /// The tether handler's ActorsHitByAttacks[None]: the fighters the burst took (ids).
    pub knocked: Vec<i32>,
    /// HitVFXEnabled.
    pub vfx_on: bool,
    /// VFXTriggered.
    pub vfx_triggered: bool,
    /// ResetVFXEnabled's timer, as the age it is seen at.
    pub vfx_at: Real,
}

impl Default for StevenTether {
    /// The field initial values.
    fn default() -> Self {
        StevenTether {
            pinned: false,
            ally: None,
            stop_once: false,
            end_vfx: false,
            at03: -1.0,
            at1: -1.0,
            knock_at: -1.0,
            destroy_at: -1.0,
            retry_at: -1.0,
            retry_who: None,
            over: IndexSet::new(),
            overlapped: Vec::new(),
            last_cyl: None,
            slope: 0.0,
            gem_x: 0.0,
            gem_y: 0.0,
            knocked: Vec::new(),
            vfx_on: true,
            vfx_triggered: false,
            vfx_at: -1.0,
        }
    }
}

/// Buff_BubbleCount_C's extension (`CountExt`: `{ from }`, the Instigator).
#[derive(State, Debug, Default)]
pub struct BubbleCount {
    /// The extension is set in the buff's begin: absent (written null) before it.
    pub begun: bool,
    pub from: Option<FighterId>,
}

/// Buff_StevenBubbleEnemy_C's extension (`BubbleExt`).
#[derive(State, Debug, Default)]
pub struct BubbleEnemy {
    /// The extension is set in the buff's begin: absent (written null) before it.
    pub begun: bool,
    /// The actor's Instigator.
    pub from: Option<FighterId>,
    /// Tick-buffs passes since the spawn.
    pub age: Real,
    /// Delay(1.0) @2798: the two delegates bound.
    pub bind_at: Real,
    pub bound: bool,
    pub stop_tick: bool,
    /// KnockbackByEnemy's Delay(0.0) -> @1232 (0: none).
    pub kb_at: Real,
    /// The DoOnce @1365.
    pub closed: bool,
    /// Delay(0.25) -> @1209.
    pub open_at: Real,
    /// Delay(0.1) -> @1517.
    pub move_at: Real,
    /// Delay(0.1) -> @1505.
    pub stop_at: Real,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn t(w: &World, id: ProjectileId) -> &StevenTether {
    match &pr(w, id).ext {
        ProjectileExt::StevenTether(t) => t,
        e => panic!("StevenTetherStunProjectile_C without its state: {e:?}"),
    }
}

fn t_mut(w: &mut World, id: ProjectileId) -> &mut StevenTether {
    match &mut pm(w, id).ext {
        ProjectileExt::StevenTether(t) => t,
        e => panic!("StevenTetherStunProjectile_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a tether has an owner")
}

fn team(w: &World, f: FighterId) -> i32 {
    w.fighter(f).team.team
}

fn sound(w: &mut World, id: ProjectileId, ev: Name, at: Option<(Real, Real)>) {
    let e = Event::Sound {
        fighter: pr(w, id).owner_id,
        ev,
        x: at.map(|a| a.0),
        y: at.map(|a| a.1),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
}

fn loop_key(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: LOOP_TAG, n: pr(w, id).seq }
}

/// The tether's `attachedTo` getter (`actors::world_actor::attached_to`).
pub fn attached_to(_p: &Projectile, t: &StevenTether) -> Option<FighterId> {
    t.ally
}

/// `StevenTether.step` while on an ally and not dead: the movement is stopped and the actor rides its parent's
/// location (SnapToTarget). True: it rode (the shared step returns); false: the parent's step runs.
pub fn ride(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let Some(a) = t(w, id).ally else { return false };
    if p.dead {
        return false;
    }
    let (ax, ay) = (w.fighter(a).body.x, actor_y(w, a));
    let p = pm(w, id);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.age += 1;
    p.x = ax;
    p.y = ay;
    true
}

/// SpawnAttachPoint.
fn spawn_attach_point(w: &mut World, id: ProjectileId) {
    stop_simulating(pm(w, id));
    t_mut(w, id).pinned = true;
}

/// The begin overlap's flow from @368 with `o` the event's OtherActor; `retry`: the age a Delay(0.05) made here fires
/// at.
fn attach_test(w: &mut World, id: ProjectileId, o: FighterId, retry: Real) {
    t_mut(w, id).retry_who = Some(o);
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let r = attach_radius(w, pr(w, id));
    if t(w, id).ally.is_some() || w.fighter(o).life.dead > 0.0 || !sphere_hits(w, x, y, r, o) {
        let s = t_mut(w, id);
        if s.retry_at < 0.0 {
            s.retry_at = retry; // a Delay node already counting ignores the call
        }
        return;
    }
    let f = owner(w, id);
    if o == f || team(w, o) != team(w, f) {
        return;
    }
    if !t(w, id).pinned {
        spawn_attach_point(w, id);
    }
    let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
    let p = pm(w, id);
    p.x = ox;
    p.y = oy;
    let s = t_mut(w, id);
    s.ally = Some(o);
    s.over.clear();
    post_vo(w, f, name!("ability_tether"), None, 0.3); // @1254
    sound(w, id, name!("sfx_cha_steven_tether_attach"), Some((ox, oy)));
    tutorial_event(w, TutorialEvent::UniqueCharacterMechanic0); // @1573
}

/// The overlap's begin edges since the last test. `base`: the age a Delay made here counts from.
fn attach_overlaps(w: &mut World, id: ProjectileId, base: Real) {
    if t(w, id).ally.is_some() {
        return;
    }
    let r = attach_radius(w, pr(w, id));
    for i in 0..peers(w).len() {
        let v = peers(w)[i];
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        let on = w.fighter(v).life.dead <= 0.0 && sphere_hits(w, x, y, r, v);
        let vid = w.fighter(v).id;
        if !on {
            t_mut(w, id).over.shift_remove(&vid);
            continue;
        }
        if !t_mut(w, id).over.insert(vid) {
            continue;
        }
        attach_test(w, id, v, base + latent_frames(0.05));
        if t(w, id).ally.is_some() {
            return;
        }
    }
}

/// ReceiveTick from @5315 on (the actor is Attached): the Cylinder's overlaps and the line.
fn cylinder_tick(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let (gx, gy) = gem_of(w, f, STEVEN_TETHER);
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let (vx, vy) = (px - gx, py - gy);
    let len = (vx * vx + vy * vy).sqrt();
    // The axis: Gem -> tether; level when the two share a UE Y (SignOfFloat(yaw) 0, no roll), as Creature's.
    let level = vx == 0.0 || len == 0.0;
    let (ux, uy) = if level { (1.0, 0.0) } else { (vx / len, vy / len) };
    let (sx, sy) = if level { ((gx + px) / 2.0 - len / 2.0, (gy + py) / 2.0) } else { (gx, gy) };
    let ally = t(w, id).ally;
    {
        let s = t_mut(w, id);
        s.slope = if vx == 0.0 { 0.0 } else { vy / vx };
        s.gem_x = gx;
        s.gem_y = gy;
        s.overlapped.clear();
    }
    for i in 0..peers(w).len() {
        let v = peers(w)[i];
        if v == f || Some(v) == ally || w.fighter(v).life.dead > 0.0 {
            continue;
        }
        if !rect_hits(w, sx, sy, ux, uy, len, CYL_HALF_WIDTH, v) {
            continue;
        }
        let s = t_mut(w, id);
        s.overlapped.push(v);
        s.last_cyl = Some(v); // the loop's cast @7790 (its IsVulnerableTo @7960 is not read)
    }
    if t(w, id).vfx_on && t(w, id).vfx_triggered {
        let window = num(class_p(w, pr(w, id)), "HitVFXWindow");
        let at = crate::real(pr(w, id).age) + timer_ticks(window);
        let s = t_mut(w, id);
        s.vfx_on = false;
        s.vfx_triggered = false;
        s.vfx_at = at;
    }
}

/// KnockupVFX @8480 (see the header: the tick loop's last fighter, not the victim).
fn knockup_vfx(w: &mut World, id: ProjectileId) {
    let Some(v) = t(w, id).last_cyl else { return };
    let (vx, vy) = (w.fighter(v).body.x, actor_y(w, v));
    sound(w, id, name!("sfx_cha_reindog_tether_hit"), Some((vx, vy)));
    if !t(w, id).vfx_on {
        return;
    }
    let s = t_mut(w, id);
    s.vfx_triggered = true;
    let (slope, gx, gy) = (s.slope, s.gem_x, s.gem_y);
    let e = Event::ProjectileDeath {
        cls: pr(w, id).cls,
        fighter: pr(w, id).owner_id,
        vfx: Some(name!("PS_HitImpact_TEMPLATE")),
        x: vx,
        y: gy + slope * (vx - gx),
        ox: 55.0,
        rot: [0.0, 0.0, 0.0],
        scale: 0.625,
        tint: Some(crate::real(w.fighter(v).id)),
        to: None,
    };
    w.events.push(e);
}

/// One pfgDoDamage of the tether's handler on `vic` with ForceKnockback `force`, then KnockupVFX.
fn knock(w: &mut World, id: ProjectileId, vic: FighterId, force: bool) {
    let f = owner(w, id);
    // the class's own variable (not HitBoxDamage)
    let mut p = obj(class_p(w, pr(w, id)), "Hit Box Damage").cloned().unwrap_or_default();
    p.insert("ForceKnockback".to_owned(), Value::Bool(force));
    deal(w, id, f, vic, &p);
    knockup_vfx(w, id);
}

/// The damage part of [`knock`] (its `deal:` block).
fn deal(w: &mut World, id: ProjectileId, f: FighterId, vic: FighterId, p: &mvs_data::value::Props) {
    let once = flag(p, "HitEachActorOnlyOnce");
    let vid = w.fighter(vic).id;
    if w.fighter(vic).life.dead > 0.0 || no_hit(w, vic) || (once && t(w, id).knocked.contains(&vid)) {
        return;
    }
    let d: Dealer = hits::dealer(w, ActorRef::Projectile(id));
    if hits::projectile_blocks(w, &d, vic) {
        return;
    }
    if invulnerable(w, vic) {
        if team(w, vic) != team(w, f) {
            let aid = pr(w, id).attack_id;
            let tick = w.flow.tick;
            let bkb = num_or(p, "BaseKnockBack", 0.0);
            attack_dodged(w, ActorRef::Projectile(id), vic, aid, bkb, tick, None, false);
        }
        return;
    }
    if once {
        t_mut(w, id).knocked.push(vid);
    }
    let hp = HitPacket::from_props(&w.names, p, None);
    if team(w, vic) == team(w, f) {
        hits::ally_hit(w, &d, vic, &hp);
        return;
    }
    let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
    let pct = charge_pct(w, ActorRef::Projectile(id));
    let rows = d.buffs(w);
    if hits::apply_projectile_hit(w, &d, vic, &hp, &rows, at, None, &[], pct) {
        hits::actor_hit_enemy(w, &d, vic);
    }
}

/// The fighters of a SphereOverlapActors over the burst's types, the BP_FighterCharacter class filter.
fn knock_sphere(w: &mut World, x: Real, y: Real) -> Vec<FighterId> {
    let types = [
        crate::real(OBJ_PHYSICS_BODY),
        crate::real(OBJ_PAWN),
        crate::real(OBJ_DAMAGE_DESTRUCTIBLE),
        crate::real(OBJ_PROJECTILE_DESTRUCTIBLE),
        crate::real(OBJ_ITEM_DESTRUCTIBLE),
    ];
    sphere_overlap_actors(w, x, y, KNOCK_RADIUS, &types, &[], -1.0)
        .into_iter()
        .filter_map(|a| match a {
            ActorRef::Fighter(o) if peers(w).contains(&o) => Some(o),
            _ => None,
        })
        .collect()
}

/// @256: DoKnockUp, DoKnockUpEnd, EndVFX.
fn burst(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    // DoKnockUp: the last tick's OverlappedActors.
    let last = t(w, id).overlapped.clone();
    for v in last {
        let force = team(w, v) != team(w, f);
        knock(w, id, v, force);
    }
    // DoKnockUpEnd: the sphere at the actor, every BP_FighterCharacter (the class filter: the players and the minions)
    // with its team test.
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    for v in knock_sphere(w, px, py) {
        let force = team(w, v) != team(w, f);
        knock(w, id, v, force);
    }
    // The sphere at the Instigator's Gem: enemies that are not IsInvulnerable? alone.
    let (gx, gy) = gem_of(w, f, STEVEN_TETHER);
    for v in knock_sphere(w, gx, gy) {
        if team(w, v) == team(w, f) || invulnerable(w, v) {
            continue;
        }
        knock(w, id, v, false);
    }
    // EndVFX.
    t_mut(w, id).end_vfx = true;
    let (cls, fid) = (pr(w, id).cls, pr(w, id).owner_id);
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let burst_at = |x: Real, y: Real| Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_StevenNANAIR_BubbleBurst")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    };
    w.events.push(burst_at(px, py));
    let (g2x, g2y) = gem_of(w, f, STEVEN_TETHER);
    w.events.push(burst_at(g2x, g2y));
    sound(w, id, name!("sfx_cha_steven_tether_end"), Some((px, py)));
}

/// The constructor's tail (`new StevenTether`): no hurt shape (HurtBox NoCollision).
fn construct(_w: &World, p: &mut Projectile) {
    p.hurt_radius = 0.0;
    p.ext = ProjectileExt::StevenTether(StevenTether::default());
}

fn begin(w: &mut World, id: ProjectileId) {
    // BeamVFX: PS_StevenBeam_Point on the owner's Gem and the beam (the render's, while the tether lives).
    sound(w, id, name!("sfx_cha_steven_tether_cast"), None); // posted on the owner
    let e = Event::SoundStart { fighter: pr(w, id).owner_id, ev: LOOP, key: loop_key(w, id) };
    w.events.push(e);
    let s = t_mut(w, id);
    s.at1 = latent_frames(1.0);
    s.at03 = latent_frames(0.3);
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    sound(w, id, name!("sfx_cha_steven_tether_charge"), Some((x, y))); // ChargeSFX, on self
    attach_overlaps(w, id, 0.0); // the overlaps at the spawn place
}

/// `receive_hit.boxes`: none besides the stage block.
fn hit_boxes(_w: &mut World, _id: ProjectileId, out: &mut crate::world::stage_query::StageShapes) {
    out.boxes.clear();
    out.polys.clear();
}

/// The Capsule's blocking hit: no ReceiveHit of its own; HandleImpact stops the movement.
fn receive_hit(_w: &mut World, _id: ProjectileId, _nx: Real, _ny: Real, _t: Real, _pen: bool) -> bool {
    false
}

/// `step`: the class's tick part and its latent actions. True: K2_DestroyActor.
fn step(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    // Attached Target On Exited Arena @13284.
    // Unverified: a minion ally (Watermelon Steven) destroyed without a defeat (`dead` = Infinity) ends the tether here
    // too. In the build its K2_DestroyActor with the tether welded to it is a native rule (the attached children
    // detached or destroyed) and its OnExitedArena does not fire for a plain destroy.
    if let Some(al) = t(w, id).ally
        && w.fighter(al).life.dead > 0.0
    {
        return true;
    }
    // ResetVFXEnabled (the timer pass of an earlier frame).
    let s = t_mut(w, id);
    if s.vfx_at >= 0.0 && a >= s.vfx_at {
        s.vfx_on = true;
        s.vfx_at = -1.0;
    }
    attach_overlaps(w, id, a - 1.0); // the overlaps behind the move (an event ahead of this frame's latent pass)
    if t(w, id).pinned && !pr(w, id).dead {
        cylinder_tick(w, id); // ReceiveTick
    }
    // The latent actions.
    if a == t(w, id).at03 {
        let s = t_mut(w, id);
        s.at03 = -1.0;
        if !s.stop_once {
            s.stop_once = true;
            let p = pm(w, id);
            p.vx = 0.0; // SetVelocityInLocalSpace(0)
            p.vy = 0.0;
            if !t(w, id).pinned {
                spawn_attach_point(w, id); // AttachedToAlly false (it is), DestroyOnBlocked false (`blocked`)
            }
        }
    }
    if a == t(w, id).at1 {
        let s = t_mut(w, id);
        s.at1 = -1.0;
        s.knock_at = a + latent_frames(1.0); // both branches of @187
    }
    if a == t(w, id).retry_at {
        t_mut(w, id).retry_at = -1.0;
        if let Some(o) = t(w, id).retry_who {
            attach_test(w, id, o, a + latent_frames(0.05));
        }
    }
    if a == t(w, id).knock_at {
        t_mut(w, id).knock_at = -1.0;
        burst(w, id);
        t_mut(w, id).destroy_at = a + latent_frames(0.5);
    }
    a == t(w, id).destroy_at
}

/// Blocked -> @61 (behind the 0.3 link's DoOnce): SetVelocityInLocalSpace(0) and, unless Attached, SpawnAttachPoint.
fn blocked(w: &mut World, id: ProjectileId, _by: ActorRef) {
    let p = pm(w, id);
    p.vx = 0.0;
    p.vy = 0.0;
    if !t(w, id).pinned {
        spawn_attach_point(w, id);
    }
}

fn destroyed(w: &mut World, id: ProjectileId) {
    let e = Event::SoundStop {
        fighter: pr(w, id).owner_id,
        key: loop_key(w, id),
        fade: Some(0.5),
        ev: None,
        curve: Some(name!("Linear")),
        cut: false,
    };
    w.events.push(e);
}

// ---- Buff_BubbleCount_C ----

fn count_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(e) = buff_mut(w, b) {
        e.ext = BuffExt::BubbleCount(BubbleCount { begun: true, from });
    }
}

fn count_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s == BuffSignal::KnockedBack {
        stack_buff(w, f, b, 0.0); // AddStack(0)
    }
    false
}

/// BPI On Max Stacks @914. True: K2_DestroyActor.
fn count_max_stacks(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let from = match buff(w, b).map(|e| &e.ext) {
        Some(BuffExt::BubbleCount(e)) => e.from,
        _ => None,
    };
    spawn_buff(w, f, BUBBLE_ENEMY, 5.0, Some(1), from, true, None);
    let (fid, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    let e = Event::ProjectileDeath {
        cls: BUBBLE_COUNT,
        fighter: fid,
        vfx: Some(name!("PS_Explosion_Satchel")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    };
    w.fighter_mut(f).events.push(e);
    true
}

fn count_status(w: &mut World, f: FighterId, class: BuffClass, stacks: Real, _applied: bool, removed: bool) {
    let id = name!("BubbleCount");
    let op = if removed || stacks <= 0.0 {
        UiStateOp::Remove(id)
    } else {
        let cls = w.lit(class.registered_name().expect("a registered class"));
        let max = buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| num(d, "MaxStacks")).unwrap_or(1.0);
        UiStateOp::Add(UiStateParams {
            id,
            state_type: 1,
            display_type: 0,
            current_value: stacks,
            max_value: max,
            timer: -1.0,
            icon_material: name!("MI_State_Rose"),
            active: true,
            pop_text: Name::NONE,
            auto_remove_time: -1.0,
            subicon: None,
        })
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

// ---- Buff_StevenBubbleEnemy_C ----

/// SpawnVFX's UI_Functions PopText @4298-4807: "Bubbled!", lifetime 1.5, text colour (1, 0.18552, 0.155671, 1), team
/// colour 0, location (0, 0, 250), space 3, scale 1, centred, no delay, no icons or audio, attached to the owner.
pub fn bubbled_pop() -> GraphPopText {
    GraphPopText {
        text_key: name!("4894864A4D2BF5B41DC832B836243CC0"),
        source_string: None,
        localized_string: None,
        lifetime: 1.5,
        color: [1.0, 0.18552, 0.155671, 1.0],
        location: [0.0, 0.0, 250.0],
        space: 3,
        scale: 1.0,
        justification: 1,
        delay: 0.0,
        audio: None,
        queued: false,
        local_only: false,
        args: Vec::new(),
    }
}

/// The bubble's extension of `b`.
fn be(w: &World, b: BuffRef) -> &BubbleEnemy {
    match buff(w, b).map(|e| &e.ext) {
        Some(BuffExt::StevenBubbleEnemy(e)) => e,
        _ => panic!("Buff_StevenBubbleEnemy_C without its extension"),
    }
}

fn be_mut(w: &mut World, b: BuffRef) -> &mut BubbleEnemy {
    match buff_mut(w, b).map(|e| &mut e.ext) {
        Some(BuffExt::StevenBubbleEnemy(e)) => e,
        _ => panic!("Buff_StevenBubbleEnemy_C without its extension"),
    }
}

/// LaunchCharacter(v, true, true) on the bubbled fighter.
fn launch(w: &mut World, f: FighterId, vx: Real, vy: Real) {
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse { vx, vy, xy: true, kind: 0, z: true, d: None });
    m.hit_launch = true;
}

fn bubble_key(w: &World, f: FighterId) -> SoundKey {
    SoundKey { tag: BUBBLE_TAG, n: w.fighter(f).id }
}

fn bubble_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    *be_ext_slot(w, b) = BubbleEnemy {
        begun: true,
        from,
        age: 0.0,
        bind_at: max(latent_frames(1.0) - 1.0, 1.0),
        bound: false,
        stop_tick: false,
        kb_at: 0.0,
        closed: false,
        open_at: 0.0,
        move_at: 0.0,
        stop_at: 0.0,
    };
    disable_jump(w, f);
    disable_evade(w, f);
    disable_attack(w, f);
    launch(w, f, 0.0, 50.0);
    override_gravity(w, f); // OverrideGravity(0.0): GravityScale 1.0 from the second move
    update_states(w, f); // UpdatePlayerStatus(null) @2690
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let src = from.map_or(w.fighter(f).id, |o| w.fighter(o).id);
    let cast = Event::Sound {
        fighter: src,
        ev: name!("sfx_cha_steven_bubble_cast"),
        x: Some(fx),
        y: Some(fy),
        hit: None,
        pooled: false,
        key: None,
    };
    let fid = w.fighter(f).id;
    let loop_start = Event::SoundStart { fighter: fid, ev: name!("sfx_cha_steven_bubble_loop"), key: bubble_key(w, f) };
    let fi = w.fighter_mut(f);
    fi.events.push(cast);
    fi.events.push(loop_start);
    pop_text_on(w, f, &bubbled_pop()); // @4807
}

/// The extension slot of `b`, for `bubble_begin` to write whole.
fn be_ext_slot(w: &mut World, b: BuffRef) -> &mut BubbleEnemy {
    let e = buff_mut(w, b).expect("the buff just begun");
    if !matches!(e.ext, BuffExt::StevenBubbleEnemy(_)) {
        e.ext = BuffExt::StevenBubbleEnemy(BubbleEnemy::default());
    }
    match &mut e.ext {
        BuffExt::StevenBubbleEnemy(x) => x,
        _ => unreachable!(),
    }
}

/// Whether a buff of `f` with frames left is of the Held group (`CheckHasBuffGroup`).
fn holds_held(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|o| {
        o.frames > 0.0
            && buff_def(w, ActorRef::Fighter(f), o.cls).and_then(|d| obj(d, "StateInfo")).and_then(|s| text(s, "Group"))
                == Some(HELD_GROUP)
    })
}

fn bubble_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let a = {
        let e = be_mut(w, b);
        e.age += 1.0;
        e.age
    };
    // ReceiveTick.
    if !be(w, b).stop_tick {
        let (x, y) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
        if x != 0.0 || y != 0.0 {
            launch(w, f, if x > 0.0 { 650.0 } else { -650.0 }, if y > 0.0 { 350.0 } else { 200.0 });
        } else {
            launch(w, f, 0.0, 250.0);
        }
    }
    // The latent actions.
    if a == be(w, b).bind_at {
        be_mut(w, b).bound = true;
        if let Some(e) = buff_mut(w, b) {
            e.ally_hit = true; // HitByAlly bound @219
        }
    }
    let kb = be(w, b).kb_at;
    if kb > 0.0 && a >= kb {
        be_mut(w, b).kb_at = 0.0;
        let held = holds_held(w, f);
        if !held && !be(w, b).closed {
            let e = be_mut(w, b);
            e.closed = true;
            e.stop_tick = true; // MoveToLocationMulticast
            e.move_at = a + latent_frames(0.1);
            e.open_at = a + latent_frames(0.25);
        }
    }
    if a == be(w, b).move_at {
        be_mut(w, b).move_at = 0.0;
        // Buff_MoveToPosition the Arya way (see the header's Deviation).
        end_move_buff(w, f); // CanOverride: an older one goes first
        let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
        let mid = force_move_to_point(w, f, fx, fy + 250.0, 0.1, 2.0, 0.0, 0.0, 0.0, false);
        let ally = &mut w.fighter_mut(f).team.ally;
        ally.move_id = to_int32(mid);
        ally.buff = latent_frames(0.1);
        be_mut(w, b).stop_at = a + latent_frames(0.1);
    }
    if a == be(w, b).stop_at {
        let e = be_mut(w, b);
        e.stop_at = 0.0;
        e.stop_tick = false;
    }
    if a == be(w, b).open_at {
        let e = be_mut(w, b);
        e.open_at = 0.0;
        e.closed = false;
    }
    false
}

fn bubble_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if !be(w, b).bound {
        return false;
    }
    if s == BuffSignal::HitByAlly {
        return true; // BreakandDestroy
    }
    if s == BuffSignal::KnockedBack && be(w, b).kb_at == 0.0 {
        let e = be_mut(w, b);
        e.kb_at = e.age + 1.0; // Delay(0.0): a node already counting ignores the call
    }
    false
}

fn bubble_end(w: &mut World, f: FighterId, b: BuffRef) {
    let from = be(w, b).from;
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let src = from.map_or(w.fighter(f).id, |o| w.fighter(o).id);
    let fid = w.fighter(f).id;
    let end = Event::Sound {
        fighter: src,
        ev: name!("sfx_cha_steven_bubble_end"),
        x: Some(fx),
        y: Some(fy),
        hit: None,
        pooled: false,
        key: None,
    };
    let stop = Event::SoundStop {
        fighter: fid,
        key: bubble_key(w, f),
        fade: None,
        ev: Some(name!("mix_stop_all_fade_eighthofsecond")),
        curve: None,
        cut: false,
    };
    let fi = w.fighter_mut(f);
    fi.events.push(end);
    fi.events.push(stop);
    clear_gravity_override(w, f);
    enable_attack(w, f);
    enable_evade(w, f);
    enable_jump(w, f);
}

/// This file's registry entries (StevenTetherStunProjectile_C, Buff_BubbleCount_C, Buff_StevenBubbleEnemy_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        // ReceiveTick @4932 does not call ParentProjectilePhysics'.
        own_tick: true,
        construct: Some(construct),
        begin: Some(begin),
        receive_hit: Some(ProjectileReceiveHit { boxes: hit_boxes, hit: receive_hit }),
        step: Some(step),
        blocked: Some(blocked),
        destroyed: Some(destroyed),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "StevenTetherStunProjectile_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::StevenTetherStunProjectile),
            ActorHooks::Projectile(kind),
        ),
    )?;
    r.buffs.enter(
        "Buff_BubbleCount_C",
        BuffEntry::new(
            BuffClass::BuffBubbleCount,
            BuffHooks {
                begin: Some(count_begin),
                on: Some(count_on),
                max_stacks: Some(count_max_stacks),
                status: Some(count_status),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        "Buff_StevenBubbleEnemy_C",
        BuffEntry::new(
            BuffClass::BuffStevenBubbleEnemy,
            BuffHooks {
                begin: Some(bubble_begin),
                tick: Some(bubble_tick),
                on: Some(bubble_on),
                end: Some(bubble_end),
                ..BuffHooks::default()
            },
        ),
    )
}
