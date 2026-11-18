//! Rick's portals: PortalParent_C and its children AutoTeleportPortal_C,
//! VisualOnlyPortal_C and ExitPortalParent_C ([`PortalActor`], a DealerActor; offsets of PortalParent's ubergraph
//! unless a function is named).
//!
//! One class for the four: AutoTeleportPortal's graph is the parent's (AutoTeleport true in its data),
//! ExitPortalParent has no graph of its own, VisualOnlyPortal's BeginPlay is the parent's then (authority)
//! PortalProjectileOnlyVisuals (@68 -> @29 -> @10). Their data differ (MaxFightersToTeleport, AllowOnePortalTeleports,
//! AutoTeleport, HitBoxStartDelay, SpawnVFX, VisualOnly's Overlap1 responses). The component side (Comp_Pawn_Portal:
//! SpawnObject, TeleportServer, the AlliedPortals array, the buffs) is portal_comp.rs / teleport.rs. Sim space:
//! x = -UE Y, y = UE Z.
//!
//! Spawn: Comp_Pawn_Portal SpawnObject @2506 (portal_comp.rs spawn_object) calls CompSpawnActor with a world transform
//! (location + (0, 0, the owner's half height - 100), X 0, rotation the constant (0, -90, 0), scale 1).
//! ReceiveBeginPlay @12934 (authority, the port's only case):
//! - The parent's first (SpawnedActorParent @9233-10573): the blast-box bounds (BlastBoxBoundsMin / Max), and with
//!   ForceSpawnPositionInsideBlastBox (true) a place outside the box clamped into it, unswept. The MaxLifetime timer
//!   (8.25 s: SpawnedActorParent @413 / UpdateMaxLifetimeWithAuthority @54) starts.
//! - @12989: X 0, Y = FClamp(Y, Min.Y + BlastBoxBuffer.Y, Max.Y - BlastBoxBuffer.Y), Z likewise with .Z (BlastBoxBuffer
//!   (0, 1000, 1250)); MulticastLocation @13408 -> @15 (unswept, teleport).
//! - @12034: PortalOverlap1.SetGenerateOverlapEvents(true); UpdatePortalCount @12067; Init Facing(yaw < 0) @12189: with
//!   the spawn yaw -90 FacingLeft is true; then (not a dedicated server, PortalVFX not valid) @4814:
//!   PF_Post_Event_At_Location sfx_cha_C020_portal_gun_open @4942 and _loop @5076 (the loop's playing id kept),
//!   SpawnVFX attached (the view's), Delay(0.5) @5575 -> @451 PortalMesh visible and its colour (the view's: meshAt).
//! - Buff_C020PortalIgniteProjectiles on the instigator (perk, no data gives it) -> Buff_Elemental_Fire on self: dormant.
//! - HitBoxManager.Instigator; PortalTeamIndex = the instigator's team @12732; Delay(0.0) @12781 -> @82: AutoTeleport
//!   -> AutoTeleportInstigator @16761: Delay(0.15) -> @946: the instigator in hitstun -> nothing, else
//!   TeleportActor(him) @1207.
//! - ShouldHitOnSpawn (false in every child's data): the SpawnHitbox branch @13527 never runs (HitEnemyPlayer @18562
//!   is unreachable; not ported).
//! - Delay(0.21) @13462 -> @1341 SetActorEnableCollision(true) (the construction script turned it off): no overlap and
//!   no body before that.
//!
//! Overlaps: PortalOverlap1 (Box's BeginOverlap @12910) and PortalOverlap2 (FighterOverlap's @19110) both call
//! OverlapActor @10375; either's EndOverlap calls EndOverlapActor @19134. ShouldTeleport (function), TeleportActor
//! @7571 / @1353, ReceiveTick @7853 (the QueuedFighters loop), IncrementFightersTeleported @17575,
//! MulticastFighterTeleportedEffects @18018, KnockbackFighterOnExit @18410, Transition Out @6967, PortalOutro @3476,
//! OnPortalCountUpdated @16559 / @15852, UpdatePortalCount (function), PortalInputMulticast @13586, ReceiveEndPlay
//! @17182 -> @15663 -> @15582 -> @15534 -> @15412 -> @15488 -> @5630: below by function. QueuedFighters' values written
//! by Map_Add @1793, @9163 and @11080 are Temp_bool_Variable_2, _ and _3, which nothing assigns: false (only
//! @10288's Temp_bool_Variable_1 is set, true).
//!
//! Tick order: PrimaryActorTick TG_PostPhysics: the actor's step in World.actors (after the pawns and the
//! projectiles' moves); AActor::Tick runs ReceiveTick, then the latent actions: a Delay armed in BeginPlay or ahead of
//! the pass counts this frame, one armed inside a latent link from the next.
//! What the view reads: x, y (the actor; the boxes at y + 30), facing_left, owner (the Team colour), spawn_tick,
//! mesh_at, transitioning_out and outro_tick, only_visuals_tick, through_tick / through_in, fighters_teleported; at
//! the end (dead) PS_C020_Portal00_Outro at VFXAttach.
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a < b)` is true for NaN")]
// Deviation (portal a): the boxes' overlaps are tested once a frame in the step (begin / end against the last step's
// list; the ends first), for a projectile after each of its moves (WorldActorHooks.projectile_moved), at the place of
// its move; the engine updates them inside each mover's move. A projectile already inside when the collision turns on
// begins at its next move. The types: what the box's ResponseArray overlaps and the traces gather (WorldDynamic,
// HitBox, Destructible are not: Overlap1 of PortalParent overlaps HitBox alone, so only Overlap2 meets anything;
// PolymorphGrenade's bodies (WorldDynamic, PhysicsBody) are never met: Overlap2 overlaps WorldDynamic, which is not
// gathered); the other side's response to the box's type is read for a spawned actor's body (traces `responder`), not
// for a fighter's or the level's.
// Unverified (portal b): CurrentStates (TeleportActor @1551, group 11) and CheckHasBuffGroup(12) are read off the
// fighter's live buffs' StateInfo.Group. GetAttachParentActor @7601 is a fighter's (attach_parent_of: an item's carry,
// a hold, an attach buff); no TeleportWhenAttached tag is in the build's data, so an attached fighter is not
// teleported. A teleported actor that is no fighter is taken as attached to nothing.
// Deviation (portal c): QueuedFighters and TeleportedActor are kept in insertion order (an insertion-ordered map); a
// TMap reuses the
// slot of a removed pair, so Map_Keys' order after a removal and a re-add is not the port's. Only the order of two
// queued fighters' teleports in one tick reads it.
// Unverified (portal d): the latent actions of one portal resume in the order they were armed
// (FLatentActionManager's per-object map order not read); the MaxLifetime timer fires in Rick's timer pass
// (timer_ticks from the next frame, restarted by OnPortalCountUpdated's UpdateMaxLifetimeWithAuthority).
// Deviation (portal e): the clamps use SpawnedActorParent's stored jump-out bounds (blast::stored_blast_bounds, with
// its note on the blast box's location); SpawnedActorParent's CheckBlastBoxTimer is not run (the clamp keeps a portal
// 1000 / 1250 inside).
// Unverified (portal f): GetVelocity of a non-fighter (TeleportActor @3277): a Projectile's (vx, vy), an Item's
// through the shared item hooks (tunnel_carry carry_hooks), a World.actors entry's through its kind's carry hooks, else
// 0 (a class with no hooks: its velocity is not read).
// Unverified (portal h): UpdatePortalCount's loop @919 only casts each AlliedPortals entry; the port skips the invalid
// ones (is_valid: pending kill), and takes IsValid as true for the portal inside its own EndPlay (`in_end_play`): the
// native rule for IsValid during the actor's own EndPlay is not read.
// Unverified (portal g): the portal's Comp_Actor_Destructible TeamIndex is its owner's team (SpawnedActorParent's copy
// from the Instigator, as portal_comp.rs (Deviation portalcomp f) takes a spawned actor's).
// Not ported: BPI_IncrementCoopActions (a statistic) @2458-3239; the perks' branches (Ignite, Buff_C020_Portal's
// alternate cooldown 1); Comp_Pawn_PassiveBuffManager (Iceproof / Fireproof on self: the portal takes no hit); the
// WallBP push of a portal overlapping a wall box; BotDetectEnemy_BTS's PortalParent branch (the Portal key).
// The actor is listed and numbered first (`comp_spawn_actor`), then its BeginPlay hook runs.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::item::is_owned;
use crate::actors::tunnel_carry::carry_hooks;
use crate::actors::world_actor::{actor_entry, actor_p, spawned_dead};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass, WorldActorState};
use crate::combat::HitPacket;
use crate::combat::buffs::{HELD_GROUP, buff_of};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer};
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_data, latent_frames, warn_parts};
use crate::fighter::control::{ability_row, attack_dodged, begin_alternate_cooldown, dodge_over, trigger_ability};
use crate::fighter::moves::V2;
use crate::fighter::moves::attach_parent_of;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_HURT_BOX, OBJ_ITEM, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PLATFORM,
    OBJ_PORTAL, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_STATIC, TraceBody, body_info, box_overlap_actors,
    box_overlaps_actor,
};
use crate::fighter::{FighterId, actor_y, in_hitstun, invulnerable, is_attacking, is_evading};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{arr, flag, num, obj, text};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, new_blast_bounds, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::spawning::new_world_actor;

use super::portal_comp::{add_allied_portal, get_allied_portals, has_comp, remove_allied_portal, teleport_server};
use super::teleport::is_valid;

pub const PORTAL_PARENT: &str = "PortalParent_C";
pub const AUTO_PORTAL: &str = "AutoTeleportPortal_C";
pub const VISUAL_ONLY_PORTAL: &str = "VisualOnlyPortal_C";
pub const EXIT_PORTAL: &str = "ExitPortalParent_C";
/// EStateGroups 12 Move (CheckHasBuffGroup(12) @9606 / @11335).
const MOVE_GROUP: &str = "StateGroups::NewEnumerator13";
const VISUAL_ONLY: Name = name!("VisualOnlyPortal_C");
const PERK_PORTAL: Name = name!("Buff_C020_Portal_C");
const BLOCK_PORTAL: Name = name!("Buff_BlockPortal_C");
/// The boxes (PortalOverlap1 / 2, SpawnHitbox): BoxExtent (600, 250, 250) x RelativeScale3D (0.15, 1, 0.65) at
/// (0, 0, 30) under Scene under the root; the actor's yaw -90 turns the local X (90) across the screen and Y (250)
/// into depth.
const BOX_DEPTH: Real = 250.0;
const BOX_DY: Real = 30.0;

fn box_hx() -> Real {
    600.0 * 0.15
}

fn box_hy() -> Real {
    250.0 * 0.65
}

/// A latent action of the portal (Delay nodes by their node; a pending node ignores a retrigger).
const L_BEGIN0: Name = name!("begin0");
const L_AUTO: Name = name!("auto");
const L_COLLISION: Name = name!("collision");
const L_MESH: Name = name!("mesh");
const L_ACTOR0: Name = name!("actor0");
const L_OUTRO: Name = name!("outro");

/// A pending Delay: its node and the ticks left.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct Pending {
    pub k: Name,
    pub left: Real,
}

/// `PortalActor` (AutoTeleportPortal_C, VisualOnlyPortal_C, ExitPortalParent_C).
#[derive(State, Debug, Default)]
pub struct PortalActor {
    /// Comp_Actor_Destructible.TeamIndex / PortalTeamIndex (Unverified portal g).
    pub team: i32,
    /// K2_GetActorLocation.
    pub x: Real,
    pub y: Real,
    /// Its BlastBoxBoundsMin / Max (SpawnedActorParent @9720-10573).
    pub blast_bounds: BlastBounds,
    /// FacingLeft.
    pub facing_left: bool,
    /// TransitioningOut.
    pub transitioning_out: bool,
    /// Inside its own EndPlay (IsValid still true there); `None` until the end has run.
    pub in_end_play: Option<bool>,
    /// ReceiveEndPlay has run (destroyActor's call, or the world's sweep).
    pub ended: bool,
    /// PortalCount.
    pub portal_count: i32,
    /// AllowOnePortalTeleports.
    pub allow_one: bool,
    /// FightersTeleported.
    pub fighters_teleported: i32,
    /// SetActorEnableCollision.
    pub collision: bool,
    /// QueuedFighters.
    pub queued: IndexMap<FighterId, bool>,
    /// TeleportedActor: GetGameTimeInSeconds at the add.
    pub teleported: IndexMap<ActorRef, Real>,
    /// TeleportedHitstunnedFighters.
    pub hitstunned: IndexMap<FighterId, Real>,
    /// InputLabels: the attach pawns' ids, one per widget (PortalInputMulticast).
    pub label_for: Vec<i32>,
    /// The actors either box overlapped at the last test.
    pub inside: Vec<ActorRef>,
    /// ActorBeingTeleported.
    pub being_teleported: Option<ActorRef>,
    pub lat: Vec<Pending>,
    /// The MaxLifetime timer's tick.
    pub life_at: Real,
    /// PortalProjectileOnlyVisuals' DoOnce.
    pub only_visuals: bool,
    /// OutroPortalVFX valid.
    pub outro_vfx: bool,
    /// Init Facing's sounds, posted at the first step (the id is the world's after the spawn).
    pub visuals_due: bool,
    // the view's (see the head)
    pub spawn_tick: Real,
    pub mesh_at: Real,
    pub outro_tick: Real,
    pub only_visuals_tick: Real,
    pub through_tick: Real,
    pub through_in: bool,
    /// DealerActor (KnockbackFighterOnExit's pfgDoDamage).
    pub attack_id: i32,
    pub hit: HitPacket,
}

// ---------------------------------------------------------------- access

fn pa(w: &World, id: ActorId) -> &PortalActor {
    match &w.actors.actors.get(id).expect("a listed actor").state {
        WorldActorState::PortalActor(p) => p,
        s => panic!("a portal class without its state: {s:?}"),
    }
}

fn pa_mut(w: &mut World, id: ActorId) -> &mut PortalActor {
    match &mut w.actors.actors.get_mut(id).expect("a listed actor").state {
        WorldActorState::PortalActor(p) => p,
        s => panic!("a portal class without its state: {s:?}"),
    }
}

/// The portal behind a reference (`None` for another family, or an actor no longer in its arena).
pub fn portal_id(w: &World, p: ActorRef) -> Option<ActorId> {
    let ActorRef::Actor(id) = p else { return None };
    let a = w.actors.actors.get(id)?;
    matches!(a.state, WorldActorState::PortalActor(_)).then_some(id)
}

/// Whether the actor is a portal still in its arena (the dead flag).
pub fn portal_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

/// IsValid of a portal: a live actor, or one inside its own EndPlay (IsValid is true there).
pub fn portal_valid(w: &World, id: ActorId) -> bool {
    let Some(a) = w.actors.actors.get(id) else { return false };
    let WorldActorState::PortalActor(p) = &a.state else { return false };
    !a.dead || p.in_end_play == Some(true)
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a listed actor").owner
}

fn now(w: &World, id: ActorId) -> Real {
    w.fighter(owner_of(w, id)).clock.time
}

fn tick_of(w: &World) -> Real {
    crate::real(w.flow.tick)
}

/// A number of the class data (unserialized: the class default, 0).
fn pnum(w: &World, id: ActorId, k: &str) -> Real {
    num(actor_p(w, ActorRef::Actor(id)), k).unwrap_or(0.0)
}

fn pflag(w: &World, id: ActorId, k: &str) -> bool {
    flag(actor_p(w, ActorRef::Actor(id)), k)
}

fn delay(w: &mut World, id: ActorId, k: Name, s: Real) {
    let p = pa_mut(w, id);
    if p.lat.iter().any(|e| e.k == k) {
        return; // a pending Delay ignores the retrigger
    }
    p.lat.push(Pending { k, left: latent_frames(s) });
}

/// SelectInt(1, 0, BPI_CheckIfHasBuff(Buff_C020_Portal_C)) on the Instigator (PortalParent @4443 / @6543).
fn perk_alt(w: &World, owner: FighterId) -> i32 {
    i32::from(buff_of(w, owner, PERK_PORTAL).is_some())
}

/// A live buff of the state group (Unverified portal b).
fn has_group(w: &World, g: FighterId, group: &str) -> bool {
    w.fighter(g).buffs.buffs.iter().any(|b| {
        b.frames > 0.0
            && crate::fighter::common::buff_def(w, ActorRef::Fighter(g), b.cls)
                .and_then(|d| obj(d, "StateInfo"))
                .and_then(|s| text(s, "Group"))
                == Some(group)
    })
}

/// ActorHasTag(IgnorePortal): the class default's Tags (a fighter's pawn data has none).
fn ignores_portal(w: &World, a: ActorRef) -> bool {
    if matches!(a, ActorRef::Fighter(_)) {
        return false;
    }
    arr(actor_p(w, a), "Tags").iter().any(|t| t.as_str() == Some("IgnorePortal"))
}

/// Cast<SpawnedActorParent>: a Projectile, an Item or a World.actors entry that is a SpawnedActorParent_C child
/// (portal_comp.rs `is_spawned_actor`'s rule).
pub fn is_spawned_actor_parent(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(_) | ActorRef::Item(_) => true,
        ActorRef::Actor(id) => w.actors.actors.contains(id) && !actor_entry(w, a).is_some_and(|e| e.keep_at_lab_reset),
        _ => false,
    }
}

/// GetVelocity's size (Unverified portal f).
fn speed_of(w: &World, a: ActorRef) -> Real {
    let v = match a {
        ActorRef::Projectile(id) => {
            w.actors.projectiles.get(id).map_or(V2 { x: 0.0, y: 0.0 }, |p| V2 { x: p.vx, y: p.vy })
        }
        _ => {
            let k = actor_entry(w, a).and_then(|e| e.carry).or_else(|| match a {
                ActorRef::Item(id) if is_owned(w, id) => carry_hooks(w, a),
                _ => None,
            });
            k.map_or(V2 { x: 0.0, y: 0.0 }, |k| (k.velocity)(w, a))
        }
    };
    ((v.x * v.x) + (v.y * v.y)).sqrt()
}

/// The team of a spawned actor's owner (`(a as SimActor).owner?.team ?? -1`).
fn owner_team(w: &World, a: ActorRef) -> i32 {
    let owner = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| p.owner),
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|p| p.owner),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|p| p.owner),
        _ => None,
    };
    owner.map_or(-1, |f| w.fighter(f).team.team)
}

// ---------------------------------------------------------------- BeginPlay

/// ReceiveBeginPlay @12934, behind the parent's (`add_actor`).
fn begin_play(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let sb = spawn_box(w);
    let force = pflag(w, id, "ForceSpawnPositionInsideBlastBox");
    let doe = pflag(w, id, "DestroyOnExitedArena?");
    // SpawnedActorParent's blast-box loop (Use Jump Buffer For Blast Box Checks true, no C020 override): the bounds
    // stored; outside the jump-out area (@10738), ForceSpawnPositionInsideBlastBox clamps into the stored bounds'
    // corner and its mirror (@14347-14989). Every portal class sets it: never the destroy.
    let (mut x, mut y, mut bounds) = {
        let p = pa(w, id);
        (p.x, p.y, p.blast_bounds)
    };
    blast_box_loop(&sb, &mut x, &mut y, None, force, &mut bounds, doe);
    let max_life = pnum(w, id, "MaxLifetime");
    let (bx, bz) = {
        let buf = obj(actor_p(w, me), "BlastBoxBuffer");
        (buf.and_then(|b| num(b, "Y")).unwrap_or(0.0), buf.and_then(|b| num(b, "Z")).unwrap_or(0.0))
    };
    let tick = tick_of(w);
    {
        let p = pa_mut(w, id);
        p.blast_bounds = bounds;
        p.life_at = tick + crate::combat::hit::knockback::timer_ticks(Some(max_life));
        // @12989-13408: FClamp(v, BlastBoxBoundsMin + buffer, BlastBoxBoundsMax - buffer) (Y mirrored: x = -Y; Max.Y =
        // -l, Min.Y = -r).
        p.x = -min(max(-x, -bounds.right + bx), -bounds.left - bx);
        p.y = min(max(y, bounds.bottom + bz), bounds.top - bz);
    }
    update_portal_count(w, id); // @12034
    {
        let p = pa_mut(w, id);
        p.facing_left = true; // Init Facing(yaw -90 < 0)
        p.spawn_tick = tick;
    }
    delay(w, id, L_MESH, 0.5); // @5575
    delay(w, id, L_BEGIN0, 0.0); // @12781
    delay(w, id, L_COLLISION, 0.21); // @13462
    if w.actors.actors.get(id).is_some_and(|a| a.cls == VISUAL_ONLY) {
        projectile_only_visuals(w, id); // VisualOnlyPortal ReceiveBeginPlay @68
    }
}

// ---------------------------------------------------------------- the count

/// UpdatePortalCount (function, authority).
fn update_portal_count(w: &mut World, id: ActorId) {
    let owner = owner_of(w, id);
    if !has_comp(w, owner) {
        return;
    }
    let me = ActorRef::Actor(id);
    add_allied_portal(w, owner, me); // @239
    let list = get_allied_portals(w, owner); // @347
    let n = i32::try_from(list.len()).expect("a few portals");
    on_portal_count_updated(w, id, n); // @478
    for q in list {
        if q != me
            && let Some(qid) = portal_id(w, q)
            && is_valid(w, Some(q))
        {
            on_portal_count_updated(w, qid, n); // @919
        }
    }
}

/// OnPortalCountUpdated(Count) @16559, then @15852.
fn on_portal_count_updated(w: &mut World, id: ActorId, count: i32) {
    let max_life = pnum(w, id, "MaxLifetime");
    let tick = tick_of(w);
    if count > pa(w, id).portal_count {
        let p = pa_mut(w, id);
        p.life_at = tick + crate::combat::hit::knockback::timer_ticks(Some(max_life)); // UpdateMaxLifetimeWithAuthority @16612
        if p.transitioning_out {
            p.transitioning_out = false;
            if p.outro_vfx {
                p.outro_vfx = false; // OutroPortalVFX.K2_DestroyComponent @16695
                p.outro_tick = -1.0;
            }
        }
    }
    pa_mut(w, id).portal_count = count;
    if count > 1 {
        pa_mut(w, id).allow_one = false; // @15926
        let owner = owner_of(w, id);
        let slot = to_int32(pnum(w, id, "TriggerAbilityIndex"));
        if ability_row(w, owner, slot).is_some() {
            trigger_ability(w, owner, slot); // @16118
        }
    }
}

/// EnoughtPortalsToTeleport(actor) (function).
fn enough(w: &World, id: ActorId, a: ActorRef) -> bool {
    let p = pa(w, id);
    p.portal_count > 1 || (p.allow_one && a != ActorRef::Fighter(owner_of(w, id)))
}

// ---------------------------------------------------------------- the teleported actors

/// AddTeleportedActor @7011: Map_Remove then Map_Add (the pair goes to the end).
pub fn add_teleported_actor(w: &mut World, p: ActorRef, a: ActorRef) {
    let Some(id) = portal_id(w, p) else { return };
    let t = now(w, id);
    let m = &mut pa_mut(w, id).teleported;
    m.shift_remove(&a);
    m.insert(a, t);
}

/// RemoveTeleportedActor @7160.
fn remove_teleported_actor(w: &mut World, id: ActorId, a: ActorRef) {
    let Some(&t) = pa(w, id).teleported.get(&a) else { return };
    if matches!(a, ActorRef::Fighter(_)) && !((now(w, id) - t) >= pnum(w, id, "TimePerFighterTeleport")) {
        return;
    }
    pa_mut(w, id).teleported.shift_remove(&a);
}

/// Is Teleporting Actor (function).
pub fn is_teleporting_actor(w: &mut World, p: ActorRef, a: ActorRef) -> bool {
    let Some(id) = portal_id(w, p) else { return false };
    teleporting(w, id, a)
}

fn teleporting(w: &mut World, id: ActorId, a: ActorRef) -> bool {
    let Some(&t) = pa(w, id).teleported.get(&a) else { return false };
    let per = if matches!(a, ActorRef::Fighter(_)) {
        pnum(w, id, "TimePerFighterTeleport")
    } else {
        pnum(w, id, "TimePerProjectileTeleport")
    };
    if (now(w, id) - t) >= per {
        remove_teleported_actor(w, id, a);
        return false;
    }
    true
}

/// IncrementFightersTeleported @17575.
pub fn increment_fighters_teleported(w: &mut World, p: ActorRef) {
    let Some(id) = portal_id(w, p) else { return };
    pa_mut(w, id).fighters_teleported += 1;
    if crate::real(pa(w, id).fighters_teleported) >= pnum(w, id, "MaxFightersToTeleport") {
        clear_queued_fighters(w, id); // @17724
        if pflag(w, id, "EndAfterMaxFightersTeleported") {
            transition_out_id(w, id);
        } else {
            projectile_only_visuals(w, id); // @17757
        }
    }
    if pflag(w, id, "TriggerAbilityOnTeleportFighter") {
        let owner = owner_of(w, id);
        trigger_ability(w, owner, to_int32(pnum(w, id, "TriggerAbilityIndex"))); // @17963
    }
}

/// PortalProjectileOnlyVisuals @18886 -> @16187: DoOnce, VisualsScaleTimeline at play rate 1 / VisualsScaleDownDuration.
fn projectile_only_visuals(w: &mut World, id: ActorId) {
    let tick = tick_of(w);
    let p = pa_mut(w, id);
    if p.only_visuals {
        return;
    }
    p.only_visuals = true;
    p.only_visuals_tick = tick;
}

/// MulticastFighterTeleportedEffects(Entering) @18018 (the view's: the emitter and the WPO timeline).
pub fn multicast_fighter_teleported_effects(w: &mut World, p: ActorRef, entered: bool) {
    let Some(id) = portal_id(w, p) else { return };
    let tick = tick_of(w);
    let s = pa_mut(w, id);
    s.through_tick = tick;
    s.through_in = entered;
}

/// KnockbackFighterOnExit(Fighter) @18410: the portal's Comp_Pawn_HitBoxHandler.pfgDoDamage(None, fighter,
/// FighterExitKnockback, ...): the dodge branch for an invulnerable fighter, else the hit; HitLocation of a null
/// HitCollision: the victim's capsule. The velocity argument is not read by the graph. Buff_Teleporting3 calls it @253
/// ahead of its K2_DestroyActor @307, with the fighter's actor collision still off: no overlap is involved, so
/// no_hit's actor_collision_off is not a gate here, DamageDisabled (CanBeHitBy) is.
pub fn knockback_fighter_on_exit(w: &mut World, p: ActorRef, g: FighterId, _vx: Real, _vy: Real) {
    let Some(id) = portal_id(w, p) else { return };
    let me = ActorRef::Actor(id);
    if invulnerable(w, g) {
        let h = &pa(w, id).hit;
        let (aid, kb) = (h.attack_id.unwrap_or(0), h.base_knock_back.unwrap_or(0.0));
        let tick = w.flow.tick;
        attack_dodged(w, me, g, aid, kb, tick, None, false); // AttackingActor: the portal (its handler's owner)
    } else if w.fighter(g).life.dead <= 0.0 && !w.fighter(g).invuln.damage_disabled {
        let pkt = pa(w, id).hit.clone();
        let at = V2 { x: w.fighter(g).body.x, y: actor_y(w, g) };
        let dl = dealer(w, me);
        let charge = crate::combat::elem::charge_pct(w, me);
        if apply_projectile_hit(w, &dl, g, &pkt, &[], at, None, &[], charge) {
            let dl = dealer(w, me);
            actor_hit_enemy(w, &dl, g);
        }
    }
    let _ = dodge_over;
}

// ---------------------------------------------------------------- the queue and the label

/// PortalInputMulticast(Base_Fighter, UI_On) @13586: the label is made / exited only for a pawn with Enough, and only
/// while RequireEvadeInput or TeleportIfEvading; IsLocallyControlled and PopInputLabel's own bot test are the HUD's.
/// Off exits the first label attached to the pawn (@14549-15304).
fn portal_input(w: &mut World, id: ActorId, g: FighterId, on: bool) {
    if !(pflag(w, id, "RequireEvadeInput") || pflag(w, id, "TeleportIfEvading")) || !enough(w, id, ActorRef::Fighter(g))
    {
        return;
    }
    let fid = w.fighter(g).id;
    let labels = &mut pa_mut(w, id).label_for;
    if on {
        labels.push(fid);
    } else if let Some(i) = labels.iter().position(|&l| l == fid) {
        labels.remove(i);
    }
}

/// ClearQueuedFightersServer @18820: PortalInputMulticast(false) on each key, then Map_Clear.
fn clear_queued_fighters(w: &mut World, id: ActorId) {
    let keys: Vec<FighterId> = pa(w, id).queued.keys().copied().collect();
    for g in keys {
        portal_input(w, id, g, false);
    }
    pa_mut(w, id).queued.clear();
}

/// ShouldTeleport(Fighter) (function) -> [ShouldTeleport, ShouldShowQueueUI].
fn should_teleport(w: &mut World, id: ActorId, g: FighterId) -> (bool, bool) {
    let owner = owner_of(w, id);
    let ally = w.fighter(g).team.team == pa(w, id).team;
    let opt_in = (ally || !pflag(w, id, "OnlyAlliesCanOptToTeleport"))
        && (pflag(w, id, "AllowInstigatorToDodgeIntoPortal") || pa(w, id).portal_count > 1 || g != owner);
    if opt_in {
        if pflag(w, id, "RequireEvadeInput") {
            let cls = w.names.str(w.actors.actors.get(id).expect("a listed actor").cls);
            warn_parts(w, &[cls, ": RequireEvadeInput (BPI_IsButtonPressed(3)) not ported"]);
        }
        if pflag(w, id, "RequireEvadeInput") || pflag(w, id, "TeleportIfEvading") {
            if is_evading(w, g) {
                return (true, false); // @2254
            }
        } else {
            return (true, false); // @2357
        }
    }
    // @918
    if in_hitstun(w, g) && (ally || buff_of(w, g, BLOCK_PORTAL).is_none()) {
        let t = pa(w, id).hitstunned.get(&g).copied();
        let tm = now(w, id);
        if t.is_none_or(|t| (tm - t) >= pnum(w, id, "HitstunTeleportBuffer")) {
            pa_mut(w, id).hitstunned.insert(g, tm); // @1444
            return (true, false);
        }
    }
    (false, opt_in) // @2126
}

/// TeleportActor(ActorBeingTeleported) @7571.
fn teleport_actor(w: &mut World, id: ActorId, a: ActorRef) {
    let me = ActorRef::Actor(id);
    if let ActorRef::Fighter(g) = a
        && attach_parent_of(w, g).is_some()
    {
        return; // attached, no TeleportWhenAttached tag @7686 (Unverified portal b)
    }
    if a == me {
        return;
    }
    pa_mut(w, id).being_teleported = Some(a); // @7829
    if let ActorRef::Fighter(g) = a {
        // @1353
        if !(crate::real(pa(w, id).fighters_teleported) < pnum(w, id, "MaxFightersToTeleport")) {
            return;
        }
        if has_group(w, g, HELD_GROUP) {
            return; // CurrentStates contains 11 @1551
        }
        if teleporting(w, id, a) || !enough(w, id, a) {
            pa_mut(w, id).queued.insert(g, false); // @1793
            return;
        }
        teleport_server(w, me, a); // @1924
        if pa(w, id).queued.contains_key(&g) {
            pa_mut(w, id).queued.shift_remove(&g);
            portal_input(w, id, g, false); // @2104
        }
        increment_fighters_teleported(w, me); // @2128
        multicast_fighter_teleported_effects(w, me, true); // @2142
        return;
    }
    // @3277
    if speed_of(w, a) != 0.0 {
        teleport_non_fighter(w, id);
    } else {
        delay(w, id, L_ACTOR0, 0.0); // @3421
    }
}

/// @107: the member ActorBeingTeleported as it is now.
fn teleport_non_fighter(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = pa(w, id).being_teleported else { return };
    if spawned_dead(w, a).is_none_or(|dead| dead) || !is_spawned_actor_parent(w, a) {
        return;
    }
    // BPI_OnTeleport @234: SpawnedActorParent's is empty; the overrides (C018_Platform, C019_Ship) are of classes
    // tagged IgnorePortal or not ported.
    if teleporting(w, id, a) {
        return; // @271
    }
    teleport_server(w, me, a); // @389
    multicast_fighter_teleported_effects(w, me, true); // @435
}

/// OverlapActor(Actor) @10375.
fn overlap_actor(w: &mut World, id: ActorId, a: ActorRef) {
    if ignores_portal(w, a) {
        return;
    }
    if let ActorRef::Fighter(g) = a {
        if pa(w, id).queued.contains_key(&g) {
            return;
        }
        if !(crate::real(pa(w, id).fighters_teleported) < pnum(w, id, "MaxFightersToTeleport")) {
            return;
        }
        if w.fighter(g).life.dead > 0.0 || is_attacking(w, g) || has_group(w, g, MOVE_GROUP) {
            // @10864
            let (tp, ui) = should_teleport(w, id, g);
            if (tp || ui) && !pa(w, id).queued.contains_key(&g) {
                pa_mut(w, id).queued.insert(g, false); // @11080
            }
            return;
        }
        let (tp, ui) = should_teleport(w, id, g); // @11401
        if tp {
            return teleport_actor(w, id, a); // @11456
        }
        if pa(w, id).queued.contains_key(&g) {
            return;
        }
        let e = enough(w, id, a);
        pa_mut(w, id).queued.insert(g, e); // @11611
        if e {
            portal_input(w, id, g, ui); // @11712
        }
        return;
    }
    // @11777
    if !is_spawned_actor_parent(w, a) || owner_team(w, a) != pa(w, id).team {
        return;
    }
    if pa(w, id).portal_count > 1 {
        teleport_actor(w, id, a); // @12010
    }
}

/// EndOverlapActor(Actor) @19134: only when neither box overlaps it any more.
fn end_overlap(w: &mut World, id: ActorId, a: ActorRef) {
    if pa(w, id).inside.contains(&a) {
        return;
    }
    if let ActorRef::Fighter(g) = a
        && pa(w, id).queued.contains_key(&g)
    {
        pa_mut(w, id).queued.shift_remove(&g);
        portal_input(w, id, g, false); // @19532
    }
    remove_teleported_actor(w, id, a); // @19556
}

/// ReceiveTick @7853 (the parent's ReceiveTick first: nothing of the portal's), over a copy of QueuedFighters' keys.
fn tick(w: &mut World, id: ActorId) {
    let keys: Vec<FighterId> = pa(w, id).queued.keys().copied().collect();
    for g in keys {
        if portal_dead(w, id) {
            return;
        }
        if !pa(w, id).inside.contains(&ActorRef::Fighter(g)) {
            // @8664
            portal_input(w, id, g, false);
            pa_mut(w, id).queued.shift_remove(&g);
            continue;
        }
        if !enough(w, id, ActorRef::Fighter(g)) {
            // @8883
            if pa(w, id).queued.get(&g).copied().unwrap_or(false) {
                portal_input(w, id, g, false);
                pa_mut(w, id).queued.insert(g, false); // @9163
            }
            continue;
        }
        if w.fighter(g).life.dead > 0.0 {
            portal_input(w, id, g, false);
            pa_mut(w, id).queued.shift_remove(&g);
            continue;
        }
        if is_attacking(w, g) || has_group(w, g, MOVE_GROUP) {
            continue; // @9495, @9667
        }
        let (tp, ui) = should_teleport(w, id, g);
        if tp {
            teleport_actor(w, id, ActorRef::Fighter(g)); // @9841
            continue;
        }
        if pa(w, id).queued.get(&g).copied().unwrap_or(false) {
            continue; // @10022 (Map_Find of a removed key: false)
        }
        if teleporting(w, id, ActorRef::Fighter(g)) {
            continue; // @10123
        }
        portal_input(w, id, g, ui); // @10197
        pa_mut(w, id).queued.insert(g, true); // @10299
    }
}

// ---------------------------------------------------------------- the overlaps (Deviation portal a)

/// The object index of a ResponseArray channel name.
fn obj_by_name(c: &str) -> Option<i32> {
    Some(match c {
        "WorldStatic" => 0,
        "WorldDynamic" => 1,
        "Pawn" => 2,
        "PhysicsBody" => 3,
        "Vehicle" => 4,
        "Destructible" => 5,
        "PlatformDetection" => 6,
        "HitBox" => 7,
        "DamageDestructible" => 8,
        "KnockBack" => 9,
        "Platform" => 10,
        "ProjectileDestructible" => 11,
        "Cleanse" => 12,
        "FadeCheck" => 13,
        "Item" => 14,
        "Portal" => 15,
        "ItemDestructible" => 16,
        "WorldStaticDetection" => 17,
        "HurtBox" => 18,
        "BlockTrace" => 19,
        _ => return None,
    })
}

/// The object types the traces gather.
const GATHERED_TYPES: [i32; 10] = [
    OBJ_WORLD_STATIC,
    OBJ_PAWN,
    OBJ_PHYSICS_BODY,
    OBJ_DAMAGE_DESTRUCTIBLE,
    OBJ_PLATFORM,
    OBJ_PROJECTILE_DESTRUCTIBLE,
    OBJ_ITEM,
    OBJ_ITEM_DESTRUCTIBLE,
    OBJ_HURT_BOX,
    OBJ_PORTAL,
];

/// The object types a box's ResponseArray overlaps among those the traces gather (Deviation portal a).
fn overlap_types(comp: Option<&Props>) -> Vec<Real> {
    let Some(comp) = comp else { return Vec::new() };
    let rows = comp
        .get("BodyInstance")
        .and_then(Value::as_obj)
        .and_then(|b| b.get("CollisionResponses"))
        .and_then(Value::as_obj)
        .map_or(&[][..], |c| arr(c, "ResponseArray"));
    let mut t = Vec::new();
    for r in rows {
        let Some(r) = r.as_obj() else { continue };
        let ty = text(r, "Channel").and_then(obj_by_name);
        if text(r, "Response") == Some("ECR_Overlap")
            && let Some(ty) = ty
            && GATHERED_TYPES.contains(&ty)
        {
            t.push(crate::real(ty));
        }
    }
    t
}

/// What the two boxes overlap now (none with the collision off), as (types, the box's own type) for each box.
fn boxes_of(w: &World, id: ActorId) -> Vec<(Vec<Real>, Real)> {
    let data = Arc::clone(&w.data);
    let a = w.actors.actors.get(id).expect("a listed actor");
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(a.owner), a.cls) else { return Vec::new() };
    let mut out = Vec::new();
    for name in ["PortalOverlap1", "PortalOverlap2"] {
        let comp = obj(&d.components, name);
        let types = overlap_types(comp);
        if types.is_empty() {
            continue;
        }
        let own = body_info(comp, None).map_or(-1, |b| b.kind); // the other body's response to the box's type
        out.push((types, crate::real(own)));
    }
    out
}

fn box_at(w: &World, id: ActorId) -> (Real, Real, Real, Real, Real, Real) {
    let k = w.actors.actors.get(id).expect("a listed actor").size_scale.unwrap_or(1.0);
    let p = pa(w, id);
    (p.x, (p.y + BOX_DY * k), box_hx() * k, box_hy() * k, BOX_DEPTH * k, k)
}

/// What the two boxes overlap now (none with the collision off).
fn overlapping(w: &mut World, id: ActorId) -> Vec<ActorRef> {
    if !pa(w, id).collision || portal_dead(w, id) {
        return Vec::new();
    }
    let me = ActorRef::Actor(id);
    let (x, y, hx, hy, depth, _) = box_at(w, id);
    let mut out: Vec<ActorRef> = Vec::new();
    for (types, own) in boxes_of(w, id) {
        for a in box_overlap_actors(w, x, y, hx, hy, depth, &types, &[Some(me)], own) {
            if !out.contains(&a) {
                out.push(a);
            }
        }
    }
    out
}

/// Whether `overlapping` lists this projectile, with only its bodies gathered.
fn overlaps_one(w: &mut World, id: ActorId, pr: ActorRef) -> bool {
    if !pa(w, id).collision || portal_dead(w, id) {
        return false;
    }
    let me = ActorRef::Actor(id);
    let (x, y, hx, hy, depth, _) = box_at(w, id);
    for (types, own) in boxes_of(w, id) {
        if box_overlaps_actor(w, x, y, hx, hy, depth, &types, &[Some(me)], own, pr) {
            return true;
        }
    }
    false
}

/// The begin and end overlaps of everything but the projectiles since the last test.
fn overlaps(w: &mut World, id: ActorId) {
    let now = overlapping(w, id);
    let was = std::mem::take(&mut pa_mut(w, id).inside);
    let mut inside = Vec::new();
    for &a in &was {
        let keep = match a {
            ActorRef::Projectile(p) => w.actors.projectiles.get(p).is_some_and(|p| !p.dead),
            _ => now.contains(&a),
        };
        if keep {
            inside.push(a);
        }
    }
    pa_mut(w, id).inside = inside;
    for &a in &was {
        if !pa(w, id).inside.contains(&a) {
            end_overlap(w, id, a);
        }
    }
    for a in now {
        if matches!(a, ActorRef::Projectile(_)) || pa(w, id).inside.contains(&a) {
            continue;
        }
        if portal_dead(w, id) {
            return;
        }
        pa_mut(w, id).inside.push(a);
        overlap_actor(w, id, a);
    }
}

/// A projectile's begin / end with the boxes, after its move.
fn projectile_moved(w: &mut World, id: ActorId, p: crate::actors::ProjectileId) {
    let pr = ActorRef::Projectile(p);
    let dead = w.actors.projectiles.get(p).is_none_or(|x| x.dead);
    let now = !dead && overlaps_one(w, id, pr);
    let was = pa(w, id).inside.contains(&pr);
    if now == was {
        return;
    }
    if !now {
        pa_mut(w, id).inside.retain(|&a| a != pr);
        return end_overlap(w, id, pr);
    }
    pa_mut(w, id).inside.push(pr);
    overlap_actor(w, id, pr);
}

// ---------------------------------------------------------------- the step

fn akey(w: &World, id: ActorId) -> SoundKey {
    SoundKey { tag: name!("C020Portal"), n: w.actors.actors.get(id).expect("a listed actor").seq }
}

fn step(w: &mut World, id: ActorId) {
    if pa(w, id).visuals_due {
        pa_mut(w, id).visuals_due = false;
        // Init Facing's visuals @4942 / @5076 (see the head).
        let (fighter, x, y) = (w.actors.actors.get(id).expect("a listed actor").owner_id, pa(w, id).x, pa(w, id).y);
        w.events.push(Event::Sound {
            fighter,
            ev: name!("sfx_cha_C020_portal_gun_open"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
        let key = akey(w, id);
        w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_C020_portal_gun_loop"), key });
    }
    overlaps(w, id);
    if portal_dead(w, id) {
        return;
    }
    // A spawned actor gone without an end overlap here (died in the box, or teleported to this exit and gone before
    // its first test) leaves the map: the build's key is a dead pointer nothing reads; the port drops it.
    if !pa(w, id).teleported.is_empty() {
        let gone: Vec<ActorRef> = pa(w, id)
            .teleported
            .keys()
            .copied()
            .filter(|&a| !matches!(a, ActorRef::Fighter(_)) && spawned_dead(w, a).is_some_and(|dead| dead))
            .collect();
        for a in gone {
            pa_mut(w, id).teleported.shift_remove(&a);
        }
    }
    tick(w, id);
    // The latent actions armed ahead of this pass (Unverified portal d).
    let due: Vec<Name> = pa(w, id).lat.iter().map(|e| e.k).collect();
    for k in due {
        if portal_dead(w, id) {
            return;
        }
        let p = pa_mut(w, id);
        let Some(i) = p.lat.iter().position(|e| e.k == k) else { continue };
        p.lat[i].left -= 1.0;
        if p.lat[i].left > 0.0 {
            continue;
        }
        p.lat.remove(i);
        resume(w, id, k);
    }
}

fn resume(w: &mut World, id: ActorId, k: Name) {
    if k == L_MESH {
        // @451
        let t = tick_of(w);
        pa_mut(w, id).mesh_at = t;
    } else if k == L_BEGIN0 {
        // @82
        if pflag(w, id, "AutoTeleport") {
            delay(w, id, L_AUTO, 0.15); // AutoTeleportInstigator @16761
        }
    } else if k == L_AUTO {
        // @946
        let owner = owner_of(w, id);
        if !in_hitstun(w, owner) {
            teleport_actor(w, id, ActorRef::Fighter(owner)); // @1207
        }
    } else if k == L_COLLISION {
        pa_mut(w, id).collision = true; // @1341
    } else if k == L_ACTOR0 {
        teleport_non_fighter(w, id); // @3421 -> @107
    } else if k == L_OUTRO && pa(w, id).transitioning_out {
        destroy_actor_id(w, id); // DestroySpawnedActor(false) @66
    }
}

// ---------------------------------------------------------------- the end

/// Transition Out @6967 -> PortalOutro @3476; TriggerAbilityOnTransitionOut (false in every child's data) @4224.
pub fn transition_out(w: &mut World, p: ActorRef) {
    if let Some(id) = portal_id(w, p) {
        transition_out_id(w, id);
    }
}

fn transition_out_id(w: &mut World, id: ActorId) {
    if pa(w, id).transitioning_out {
        return;
    }
    pa_mut(w, id).transitioning_out = true;
    let (fighter, x, y) = (w.actors.actors.get(id).expect("a listed actor").owner_id, pa(w, id).x, pa(w, id).y);
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C020_portal_gun_outro"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    }); // @3600
    if !pa(w, id).outro_vfx {
        let t = tick_of(w);
        let s = pa_mut(w, id);
        s.outro_vfx = true; // PS_C020_PortalPreOutro @3788
        s.outro_tick = t;
    }
    let dur = pnum(w, id, "TransitionOutDurationDuration");
    delay(w, id, L_OUTRO, dur); // @3729
    // @4443-4708: BeginAlternateCooldownOnAuthority(TriggerAbilityIndex, SelectInt(1, 0, the Instigator has
    // Buff_C020_Portal), true): the portal perk's 25 s of AlternateCooldowns [15, 25].
    if pflag(w, id, "TriggerAbilityOnTransitionOut") {
        let owner = owner_of(w, id);
        let alt = perk_alt(w, owner);
        begin_alternate_cooldown(w, owner, to_int32(pnum(w, id, "TriggerAbilityIndex")), alt, true);
    }
}

/// K2_DestroyActor: ReceiveEndPlay inside the call.
pub fn destroy_actor(w: &mut World, p: ActorRef) {
    if let Some(id) = portal_id(w, p) {
        destroy_actor_id(w, id);
    }
}

fn destroy_actor_id(w: &mut World, id: ActorId) {
    if portal_dead(w, id) {
        return;
    }
    w.actors.actors.get_mut(id).expect("a listed actor").dead = true;
    end_play(w, id);
}

/// ReceiveEndPlay @17182 (IsValid still true inside: `in_end_play`).
fn end_play(w: &mut World, id: ActorId) {
    if pa(w, id).ended {
        return;
    }
    {
        let p = pa_mut(w, id);
        p.ended = true;
        p.in_end_play = Some(true);
        p.label_for.clear(); // each InputLabels entry's Exit @17449
    }
    let (fighter, x, y) = (w.actors.actors.get(id).expect("a listed actor").owner_id, pa(w, id).x, pa(w, id).y);
    if !pa(w, id).visuals_due {
        let key = akey(w, id);
        w.events.push(Event::SoundStop {
            fighter,
            key,
            fade: Some(0.25),
            ev: None,
            curve: Some(name!("Linear")),
            cut: false,
        }); // @15663: Stop, 250 ms, curve 4
    }
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C020_portal_gun_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    }); // @15761
    {
        let p = pa_mut(w, id);
        p.outro_vfx = false;
        p.teleported.clear(); // @15488
    }
    // The Lab reset's pass: the build's calls land on the old pawn; the new one keeps no portal, no ability trigger and
    // no cooldown of this end.
    if !reset_pass(w) {
        let owner = owner_of(w, id);
        if has_comp(w, owner) {
            remove_allied_portal(w, owner, ActorRef::Actor(id)); // @5815
        }
        update_portal_count(w, id); // @5852 (adds this one back: IsValid holds here, Unverified portal h)
        // @6316: TriggerAbilityOnEnd -> BeginAlternateCooldownOnAuthority(TriggerAbilityIndex, the perk's 1 else 0,
        // true) (@6543-6808, as @4708).
        if pflag(w, id, "TriggerAbilityOnEnd") {
            let alt = perk_alt(w, owner);
            begin_alternate_cooldown(w, owner, to_int32(pnum(w, id, "TriggerAbilityIndex")), alt, true);
        }
    }
    clear_queued_fighters(w, id); // @6863
    pa_mut(w, id).in_end_play = Some(false);
}

// ---------------------------------------------------------------- the kind

/// The portal's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &PortalActor) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The portal as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &PortalActor) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, if s.facing_left { -1.0 } else { 1.0 }, s.attack_id)
}

/// The portal's FighterExitKnockback packet (`hit`).
pub fn hit(s: &PortalActor) -> &HitPacket {
    &s.hit
}

/// `new PortalActor(cls, owner, data, attackId)` at the transform `spawn_object` gives: the state of a portal at
/// (x, y), ahead of the World's listing (portal_comp.rs).
pub fn new_portal(w: &World, owner: FighterId, cls: Name, x: Real, y: Real) -> Option<WorldActorState> {
    let data = Arc::clone(&w.data);
    let d = actor_data(w, &data, ActorRef::Fighter(owner), cls)?;
    let none = Props::new();
    let allow_one = flag(&d.p, "AllowOnePortalTeleports");
    let hit = HitPacket::from_props(&w.names, obj(&d.p, "FighterExitKnockback").unwrap_or(&none), None);
    Some(WorldActorState::PortalActor(PortalActor {
        team: w.fighter(owner).team.team,
        x,
        y,
        blast_bounds: new_blast_bounds(),
        allow_one,
        life_at: Real::INFINITY,
        visuals_due: true,
        spawn_tick: -1.0,
        mesh_at: -1.0,
        outro_tick: -1.0,
        only_visuals_tick: -1.0,
        through_tick: -1.0,
        attack_id: w.fighter(owner).decay.attack_count,
        hit,
        ..PortalActor::default()
    }))
}

/// The actor of a portal class around its state, ahead of its listing.
pub fn portal_actor(w: &World, owner: FighterId, cls: Name, state: WorldActorState) -> Option<WorldActor> {
    let class = if cls == name!("AutoTeleportPortal_C") {
        WorldActorClass::AutoTeleportPortal
    } else if cls == VISUAL_ONLY {
        WorldActorClass::VisualOnlyPortal
    } else if cls == name!("ExitPortalParent_C") {
        WorldActorClass::ExitPortalParent
    } else {
        return None;
    };
    Some(new_world_actor(w, cls, class, owner, false, state))
}

/// The bodies a trace or overlap query meets while the collision is on: PortalOverlap1 (ProjectileDestructible) and
/// PortalOverlap2 (HitBox) at the boxes. None is of type Portal: the BPI_GetSafePortalLocation sweeps (WorldStatic,
/// DamageDestructible, Portal) do not meet a portal. SpawnHitbox is NoCollision; the root Capsule (Custom, no
/// ObjectType: a shape's WorldDynamic) is of no gathered type.
fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    if !pa(w, id).collision {
        return;
    }
    let data = Arc::clone(&w.data);
    let a = w.actors.actors.get(id).expect("a listed actor");
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(a.owner), a.cls) else { return };
    let k = a.size_scale.unwrap_or(1.0);
    let p = pa(w, id);
    for (comp, cname) in [("PortalOverlap1", name!("PortalOverlap1")), ("PortalOverlap2", name!("PortalOverlap2"))] {
        let Some(info) = body_info(obj(&d.components, comp), None) else { continue };
        if !info.on {
            continue;
        }
        let y = p.y + BOX_DY * k;
        out.push(TraceBody {
            ax: p.x,
            ay: y,
            bx: p.x,
            by: y,
            hx: box_hx() * k,
            hy: box_hy() * k,
            r: 0.0,
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component: cname,
            actor: Some(ActorRef::Actor(id)),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

/// Die With Instigator, the Lab reset: ended by the world (dead set), EndPlay at its sweep.
fn kind_end(w: &mut World, id: ActorId) {
    end_play(w, id);
}

/// Another actor's K2_DestroyActor (C021_ForceField's Destroy On Enter): EndPlay inside the call.
fn kind_destroy(w: &mut World, id: ActorId) {
    destroy_actor_id(w, id);
}

/// The MaxLifetime timer (SpawnedActorParent's, Unverified portal d) -> OnMaxLifetimeExpired @18951 -> Transition Out.
fn timer_tick(w: &mut World, f: FighterId) {
    let mut i = 0;
    while let Some(&a) = w.fighter(f).items.spawned_actors.get(i) {
        i += 1;
        let Some(id) = portal_id(w, a) else { continue };
        if portal_dead(w, id) || tick_of(w) < pa(w, id).life_at {
            continue;
        }
        pa_mut(w, id).life_at = Real::INFINITY;
        transition_out_id(w, id);
    }
}

/// The hooks this file joins in Rick's CharHooks (`timer_tick`).
pub fn hooks(h: &mut CharHooks) {
    h.timer_tick = Some(timer_tick);
}

/// This file's registry entries (the portal classes), in registration order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        begin_play: Some(begin_play),
        // The boxes read the actor's scale; the view too.
        size: Some(SizeHooks::default()),
        bodies: Some(bodies),
        step: Some(step),
        projectile_moved: Some(projectile_moved),
        end: Some(kind_end),
        destroy: Some(kind_destroy),
        ..WorldActorHooks::default()
    };
    for (name, class) in [
        (AUTO_PORTAL, WorldActorClass::AutoTeleportPortal),
        (VISUAL_ONLY_PORTAL, WorldActorClass::VisualOnlyPortal),
        (EXIT_PORTAL, WorldActorClass::ExitPortalParent),
    ] {
        r.actors.enter(
            name,
            ActorEntry {
                parents: &[PORTAL_PARENT],
                ..ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
            },
        )?;
    }
    Ok(())
}
