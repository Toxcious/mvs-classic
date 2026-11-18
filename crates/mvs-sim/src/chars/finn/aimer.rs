//! Finn's side special: the target pick by GetDotProductTo >= -0.1 / >= 0.4
//! @1390-1901 and @3486-3633, LastHitActor @2551, the lock timeline, the coin and gem branch @5696-6313, the teleport
//! @6526-7122, OnTargetDestroyed, EndPlay of Finn_Dash_AimerExperiment ([`FinnAimer`], a world actor),
//! AnimNotify_MoveCommand_FinnAim, Pawn_Finn's BPI_Finn_Dash_Aim ([`dash_aim`]), FinnAimDashCondition with its Reset,
//! ButtonHoldSectionJump's FailedConditionJumpToSection (WeakAttack) on the two F_Special_01 montages (the shared
//! montage code runs the condition's Reset, then its Check: nothing of its own here). The gem's reads go to gem.rs
//! (`find_dash_target`, `gem_being_warped_to`: its gem link).
//!
//! The aimer. Spawned by AnimNotify_Spawner (t 0.04, 0.41 s, AttachToActor, DestroyAllActorsOnNotifyEnd) of both
//! F_Special_01 montages at socket None with the translation (-100, 0, 0): Finn's location - (0, 0, 100) and UE X -100
//! (Comp_Spawner CompSpawnActor's transform); attached to Finn, so it follows him (its place is set from his each time
//! it is read). Components (class data): AimOverlap sphere 425 and ThrownTargetAimOverlap sphere 2000, both at relative
//! X +100 (along the facing), AimWidget (UI_Finn_DashTarget, hidden), Arrow; TargetLockTimeline 0.2 s (Scale 2 -> 1,
//! linear). CDO: ClosestDistance 1e7, TeleportActorLocation (0, 0, 0).
//!
//! BeginPlay @7127: LastHitActor / LastHitTime from Pawn_Finn, then @7328 the gather: SphereOverlapActors(Finn's
//! location, ThrownTargetAimOverlap 2000, [PhysicsBody, Pawn, WorldDynamic], ignore Finn and itself) as AimableActors,
//! with each valid FinnDashThrownTarget of Finn's Comp_Spawner SpawnedActors within 2000 of him added (AddUnique);
//! an empty overlap goes to the LastHit test @2320 (the gems are not looked at). Else the gem loop @4791 over
//! AimableActors: a FinnDashThrownTarget of the aimer's team, not WeakThrow; CapsuleOverlapActors(gem, its Capsule
//! radius - 5 both ways, [WorldStatic, DamageDestructible], ignore itself and Finn) hit: the gem is destroyed; else
//! BeingWarpedTo, FoundCoin, TeleportActor = it, TeleportEffectRot = FindLookAtRotation(Finn, gem). FoundCoin @6526:
//! TeleportVisuals, BPI_SetAllowRotationChangeThruATKFlag(true), and at a gem location of (0, 0, 0) (tolerance 1e-4)
//! Delay 0.1 -> @15; else TeleportActorLocation = the gem's, Finn K2_SetActorLocation((0, Y, Z), no sweep, teleport),
//! then @15: the gem destroyed, Delay 0.075 -> @106 BPI_SetAllowRotationChangeThruATKFlag(false), then the search
//! @243. No gem: the search @243 at once.
//! The search @243: SphereOverlapActors(Finn's location, FoundCoin ? 750 : AimOverlap's 425, the same types, ignore
//! Finn, itself and TeleportActor); FoundCoin = false; each FinnTreasure of its team or fighter (allies too) with the
//! aimer's GetDotProductTo >= -0.1: at a dot >= 0.4 with !InFront it is taken; else a fighter is taken over a coin,
//! and over a fighter when ClosestDistance >= its distance; a coin is taken over a coin (or none) when
//! ClosestDistance >= its distance, never over a fighter. Taken: ClosestDistance, InFront = dot >= 0.4, ClosestActor.
//! Then @2320 (authority): LastHitActor within 750 of TeleportActorLocation (the origin without a gem) or of the
//! aimer, at a dot >= -0.1, hit at most 0.65 s ago: ClosestActor = it. SetAimActorMulticast(ClosestActor) @4356.
// Deviation: (a) the aimer's place is Finn's - (0, 0, 100) at UE X -100 (the spawn's translation); its
// GetDistanceTo / GetDotProductTo take that depth (the targets at X 0), its forward is Finn's facing (attached: it
// turns with him). The overlaps are at Finn's location (depth 0), the AimOverlap begin test at the aimer + 100 along
// the facing with the sphere's cut through the play plane (sqrt(425^2 - 100^2)).
// Deviation: (b) the overlaps' WorldDynamic bodies are not gathered by the trace code: the coins (both their bodies
// are WorldDynamic: the Capsule's Custom profile keeps the shape's OverlapAllDynamic type, the Sphere is DetectPawn)
// are added by their Capsule (54, a sphere: half height = radius) after the traced actors, in projectile arena order;
// the gems are added by their UiChecker (2000, DetectPawn: QueryOnly WorldDynamic, never turned off by a graph or by
// gem.rs, which toggles the PickupArea only) after the coins, in item arena order; the other WorldDynamic bodies
// (BaseFighter's own Capsule, a spawned actor's Capsule) are not met. The overlap's non-empty test at the gather
// therefore counts fighters, PhysicsBody bodies, coins and gems only.
// Unverified: (c) the order of SphereOverlapActors' result (PhysX's) is the traces' order, coins last.
// Unverified: (d) ClosestActor, ClosestDistance and InFront carry over between the passes (as the graph has them);
// OnTargetDestroyed re-runs the gather @7328 with the stale ClosestActor, which SetAimActorMulticast may write back
// into DashTarget (a destroyed actor: != null for FinnAimDashCondition, not IsValid for BPI_Finn_Dash_Aim).
// Deviation: (e) the bound actors' OnEndPlay is a non-fighter HitActor's `dead` (an Item, a Projectile, a world
// actor), seen in the aimer's step (after the death, in the aimer's tick of that frame or the next); a FinnTreasure is
// pooled (ReleaseToPool: no EndPlay), so its death does not count; a fighter is never destroyed in a match.
// (f) BPI_SetAllowRotationChangeThruATKFlag is BaseFighter's counter (@97859-98055, `rot_thru_atk_count`): the stick
// turns Finn through the attack from the raise to the lower 0.075 s on. The Delay is not retriggerable and is dropped
// with the aimer, so a raise while it is pending, or an aimer ended inside it, leaves the count up, as in the build.
// Deviation: (g) the AimOverlap begin events are taken in the aimer's step (after the fighters' moves) against the
// fighters' root capsules (Pawn), the first ones right after BeginPlay inside the spawn; the build's come with each
// move of either body. Coins never begin to overlap it (their bodies ignore WorldDynamic).
// Unverified: (h) a dead LastHitActor (destroyed) is taken as null (K2_GetActorLocation (0, 0, 0), GetDotProductTo
// -2: never taken).
// Unverified: (i) GetDotProductTo(null) is -2 and GetDistanceTo(null) 0 (stock AActor; not read in the exe).

use super::coins::{self, is_treasure};
use super::gem::{find_dash_target, gem_being_warped_to};
use super::state::{finn_mut, finn_of};
use crate::Real;
use crate::State;
use crate::actors::item::ItemExt;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_dead, actor_place};
use crate::actors::{ActorClass, ActorId, ActorRef, ItemId, ProjectileId, WorldActorClass};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::notify_data;
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::{MoveCtl, MoveEnd, V2, add_move_cmd, evade_play_rate, teleport_move};
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_WORLD_DYNAMIC, OBJ_WORLD_STATIC, sphere_overlap_actors,
    sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::ue::{KINDA_SMALL_NUMBER, SMALL_NUMBER, normal, rot_from_x, safe_normal, size, size2d};
use crate::math::{min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::notifies::move_command_end;
use crate::registry::props::{flag, flag_or, num, num_or, obj};
use crate::registry::{Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::spawn_spot::spawn_place;
use mvs_data::value::Props;
use std::f32::consts::FRAC_1_SQRT_2;

/// The aimer's class.
pub const AIMER: &str = "Finn_Dash_AimerExperiment_C";
/// |UE X| of the aimer (the spawn's translation X -100); the targets' X is 0.
const DEPTH: Real = 100.0;
/// TargetLockTimeline's length.
const LOCK_LENGTH: Real = 0.2;

/// `FinnAimer` (Finn_Dash_AimerExperiment_C). What the view reads: `x`, `y` (the place), `target` (ClosestActor: the
/// locked actor), `lock` (the lock timeline's position in s: -1 before its first PlayFromStart, its length once
/// finished; the AimWidget's scale is 2 - 5 * lock) and `widget` (the AimWidget's visibility).
#[derive(State, Debug)]
pub struct FinnAimer {
    pub x: Real,
    pub y: Real,
    /// Its place relative to Finn's location (sim x along his facing, y).
    pub dx: Real,
    pub dy: Real,
    /// AimOverlap.GetScaledSphereRadius / ThrownTargetAimOverlap's, at scale 1.
    pub aim_r: Real,
    pub thrown_r: Real,
    pub target: Option<ActorRef>,
    pub lock: Real,
    pub widget: bool,
    pub closest_distance: Real,
    pub in_front: bool,
    pub found_coin: bool,
    pub teleport_actor: Option<ItemId>,
    pub teleport_x: Real,
    pub teleport_y: Real,
    pub last_hit: Option<ActorRef>,
    pub last_hit_time: Real,
    /// AllTargetedFinnTreasure.
    pub targeted: Vec<ProjectileId>,
    /// The actors whose OnEndPlay has OnTargetDestroyed bound (AddUnique).
    pub bound: Vec<ActorRef>,
    /// AimOverlap's overlaps of the last test.
    pub overlapping: Vec<FighterId>,
    /// Delay(0.1) @6811 -> @15: frames left, 0: not running.
    pub wait01: i32,
    /// Delay(0.075) @51 -> @106.
    pub wait075: i32,
}

/// The Finn_Dash_AimerExperiment_C's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &FinnAimer) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

fn st(w: &World, id: ActorId) -> &FinnAimer {
    match &w.actors.actors.get(id).expect("a live aimer").state {
        WorldActorState::FinnAimer(s) => s,
        _ => panic!("a Finn aimer"),
    }
}

fn st_mut(w: &mut World, id: ActorId) -> &mut FinnAimer {
    match &mut w.actors.actors.get_mut(id).expect("a live aimer").state {
        WorldActorState::FinnAimer(s) => s,
        _ => panic!("a Finn aimer"),
    }
}

fn owner(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live aimer").owner
}

/// The root's scale (`sizeScale ?? 1`).
fn scale(w: &World, id: ActorId) -> Real {
    w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0)
}

fn is_fighter(a: Option<ActorRef>) -> bool {
    matches!(a, Some(ActorRef::Fighter(_)))
}

/// IsValid of a HitActor: not destroyed (a fighter is never destroyed in a match).
fn alive(w: &World, a: Option<ActorRef>) -> bool {
    match a {
        None => false,
        Some(ActorRef::Fighter(_)) => true,
        Some(a) => !actor_dead(w, a),
    }
}

/// The actor's place (`locX`, `locY`).
fn loc(w: &World, a: ActorRef) -> (Real, Real) {
    match a {
        ActorRef::Fighter(f) => (w.fighter(f).body.x, actor_y(w, f)),
        a => actor_place(w, a).map_or((0.0, 0.0), |(x, y, _)| (x, y)),
    }
}

/// Attached to Finn: its place follows his.
fn follow(w: &mut World, id: ActorId) {
    let f = owner(w, id);
    let (fx, ay, face) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    let m = st_mut(w, id);
    m.x = fx + m.dx * face;
    m.y = ay + m.dy;
}

/// AActor::GetDistanceTo of the aimer (the head's (a), (i)).
fn dist_to(w: &World, id: ActorId, a: Option<ActorRef>) -> Real {
    let Some(a) = a else { return 0.0 };
    let m = st(w, id);
    let (ax, ay) = loc(w, a);
    let (dx, dy) = (ax - m.x, ay - m.y);
    size([DEPTH, dx, dy])
}

/// AActor::GetDotProductTo of the aimer.
fn dot_to(w: &World, id: ActorId, a: Option<ActorRef>) -> Real {
    let Some(a) = a else { return -2.0 };
    let m = st(w, id);
    let (ax, ay) = loc(w, a);
    let (dx, dy) = (ax - m.x, ay - m.y);
    // (Other - this).GetSafeNormal() | the forward (UE -Y x facing): the normal's UE Y
    facing(w, owner(w, id)) * safe_normal([DEPTH, dx, dy], SMALL_NUMBER)[1]
}

/// SphereOverlapActors(Finn's location, r, [PhysicsBody, Pawn, WorldDynamic], Class Actor, ignore) (the head's (b),
/// (c)).
fn overlap(w: &mut World, id: ActorId, r: Real, ignore: &[Option<ActorRef>]) -> Vec<ActorRef> {
    let f = owner(w, id);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN)];
    let mut out = sphere_overlap_actors(w, fx, fy, r, &types, ignore, -1.0);
    let data = w.data.clone();
    let coins: Vec<ProjectileId> = w.actors.projectiles.iter().map(|(p, _)| p).collect();
    for c in coins {
        let a = ActorRef::Projectile(c);
        let p = w.actors.projectiles.get(c).expect("listed");
        if p.dead || !is_treasure(w, a) || ignore.contains(&Some(a)) || out.contains(&a) {
            continue;
        }
        let (px, py, own) = (p.x, p.y, p.owner);
        let cr = own
            .and_then(|o| actor_data(w, &data, ActorRef::Fighter(o), name!("FinnTreasure_C")))
            .and_then(|d| obj(&d.components, "Capsule"))
            .and_then(|c| num(c, "CapsuleRadius"));
        let Some(cr) = cr else {
            warn_text(w, "FinnTreasure: Capsule radius missing");
            continue;
        };
        if size2d([px - fx, py - fy]) <= r + cr {
            out.push(a);
        }
    }
    out
}

/// SetAimActorMulticast @4356: DashTarget (Pawn_Finn), the OnEndPlay binding, TargetLockVisuals(true), a coin's
/// AllTargetedFinnTreasure AddUnique and Targeted().
fn set_aim_actor(w: &mut World, id: ActorId, a: Option<ActorRef>) {
    let f = owner(w, id);
    let Some(fs) = finn_mut(w, f) else { return };
    fs.dash.target = a;
    let Some(a) = a else { return };
    bind(w, id, a);
    lock_visuals(w, id);
    if let ActorRef::Projectile(c) = a
        && is_treasure(w, a)
    {
        let m = st_mut(w, id);
        if !m.targeted.contains(&c) {
            m.targeted.push(c);
        }
        coins::targeted(w, c);
    }
}

/// Bind OnTargetDestroyed to the actor's OnEndPlay (AddUnique); an actor already destroyed never broadcasts it.
fn bind(w: &mut World, id: ActorId, a: ActorRef) {
    if alive(w, Some(a)) && !st(w, id).bound.contains(&a) {
        st_mut(w, id).bound.push(a);
    }
}

/// TargetLockVisuals(true) @11108: (IsLocallyControlled: every fighter of the port) AimWidget shown, its relative
/// transform by Finn's facing (the view's), TargetLockTimeline.PlayFromStart.
fn lock_visuals(w: &mut World, id: ActorId) {
    let m = st_mut(w, id);
    m.widget = true;
    m.lock = 0.0;
}

/// The gather @7328 (BeginPlay, OnTargetDestroyed).
fn gather(w: &mut World, id: ActorId) {
    let f = owner(w, id);
    follow(w, id);
    let thrown_r = st(w, id).thrown_r * scale(w, id);
    let me = ActorRef::Actor(id);
    let mut aimable = overlap(w, id, thrown_r, &[Some(ActorRef::Fighter(f)), Some(me)]);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    // Every gem's UiChecker (DetectPawn: QueryOnly, WorldDynamic; an object-type query ignores the target's responses,
    // and no graph turns it off) is met by the 2000 sphere: a live gem of any owner within ThrownTargetAimOverlap +
    // UiChecker of Finn (both at UE X 0), after the traced actors and the coins, in item arena order (the head's (b),
    // (c)). The PickupArea (250, DetectPawn too) lies inside the UiChecker and adds nothing.
    let data = w.data.clone();
    let gd = actor_data(w, &data, ActorRef::Fighter(f), name!("FinnDashThrownTarget_C"));
    let ui_r = gd.and_then(|d| obj(&d.components, "UiChecker")).and_then(|c| num(c, "SphereRadius"));
    let gem_cap_r = gd.and_then(|d| obj(&d.components, "Capsule")).and_then(|c| num(c, "CapsuleRadius"));
    if ui_r.is_none() || gem_cap_r.is_none() {
        warn_text(w, "FinnDashThrownTarget: UiChecker / Capsule size missing");
    }
    let gems: Vec<ItemId> = w
        .actors
        .items
        .iter()
        .filter(|(_, it)| !it.dead && it.cls == name!("FinnDashThrownTarget_C"))
        .map(|(i, _)| i)
        .collect();
    for g in gems {
        let a = ActorRef::Item(g);
        let it = w.actors.items.get(g).expect("listed");
        if aimable.contains(&a) {
            continue;
        }
        if size2d([it.x - fx, it.y - fy]) <= thrown_r + ui_r.unwrap_or(0.0) {
            aimable.push(a);
        }
    }
    let found = !aimable.is_empty();
    // Comp_Spawner SpawnedActors @7733-8835: each valid FinnDashThrownTarget within ThrownTargetAimOverlap's radius of
    // Finn, AddUnique (one the UiChecker reach above has not already added: none at the same place).
    // Deviation: find_dash_target gives Finn's one gem (the build loops every entry).
    if let Some(g) = find_dash_target(w, f) {
        let it = w.actors.items.get(g).expect("found");
        if !it.dead && size2d([it.x - fx, it.y - fy]) <= thrown_r && !aimable.contains(&ActorRef::Item(g)) {
            aimable.push(ActorRef::Item(g));
        }
    }
    if !found {
        return last_hit_test(w, id); // @8840
    }
    // The gem loop @4791.
    let mut pitch = 0.0; // TeleportEffectRot's pitch (deg)
    let team = w.fighter(f).team.team;
    for a in aimable {
        let ActorRef::Item(g) = a else { continue };
        let Some(it) = w.actors.items.get(g) else { continue };
        let ItemExt::Gem(gm) = &it.ext else { continue };
        if it.dead || it.destructible_team(w) != team {
            continue; // @5040: Comp_Actor_Destructible.TeamIndex, the aimer's is Finn's
        }
        if gm.weak_throw {
            continue; // @5256
        }
        let (gx, gy) = (it.x, it.y);
        let r = gem_cap_r.unwrap_or(0.0) - 5.0; // Capsule.CapsuleRadius @5467: the property, unscaled
        let mut hits = Vec::new();
        let types = [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_DAMAGE_DESTRUCTIBLE)];
        sphere_trace_multi_for_objects(
            w,
            gx,
            gy,
            gx,
            gy,
            r,
            &types,
            &[Some(me), Some(ActorRef::Fighter(f))],
            None,
            &mut hits,
        );
        if !hits.is_empty() {
            crate::world::items::destroy_item(w, g); // @5769
            continue;
        }
        gem_being_warped_to(w, g, Some(f)); // @5930
        let m = st_mut(w, id);
        m.found_coin = true;
        m.teleport_actor = Some(g);
        // (the gem's place after the warp: a held gem is dropped first)
        let (gx, gy) = w.actors.items.get(g).map_or((gx, gy), |i| (i.x, i.y));
        // FindLookAtRotation(Finn, gem) @6240 (142f6cad0): MakeFromX(gem - Finn), then FMatrix::Rotator.
        pitch = rot_from_x([0.0, gx - fx, gy - actor_y(w, f)])[0];
    }
    if !st(w, id).found_coin {
        return search(w, id); // @6526
    }
    // TeleportVisuals @10011 (the Instigator is a BaseFighter): PS_Finn_DashTrail on his Mesh at (0, pitch, 0), the
    // Team parameter (the view's); sfx_cha_wonderwoman_shieldbuff_dash at his location.
    let tick = w.flow.tick;
    if let Some(fs) = finn_mut(w, f) {
        fs.dash.trail_at = tick;
        fs.dash.trail_pitch = pitch;
    }
    let (fid, ay) = (w.fighter(f).id, actor_y(w, f));
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_wonderwoman_shieldbuff_dash"),
        x: Some(fx),
        y: Some(ay),
        hit: None,
        pooled: false,
        key: None,
    });
    // BPI_SetAllowRotationChangeThruATKFlag(true) @6662: the head's (f).
    let c = &mut w.fighter_mut(f).controls.rot_thru_atk_count;
    *c = Some(c.unwrap_or(0) + 1);
    let g = st(w, id).teleport_actor.and_then(|g| w.actors.items.get(g).filter(|i| !i.dead).map(|i| (i.x, i.y)));
    let Some((gx, gy)) = g else {
        return goto15(w, id); // K2_GetActorLocation of none: (0, 0, 0)
    };
    if gx.abs() <= 1e-4 && gy.abs() <= 1e-4 {
        let m = st_mut(w, id);
        if m.wait01 == 0 {
            m.wait01 = to_int32(latent_frames(0.1)); // @6811 -> @15
        }
        return;
    }
    let m = st_mut(w, id);
    m.teleport_x = gx;
    m.teleport_y = gy;
    // K2_SetActorLocation(Finn, (0, Y, Z), no sweep, teleport) @7060: the CMC's next move resolves the overlap
    // (teleport_move).
    teleport_move(w, f, gx, gy);
    follow(w, id);
    goto15(w, id);
}

/// @15: TeleportActor.K2_DestroyActor, Delay 0.075 -> @106.
fn goto15(w: &mut World, id: ActorId) {
    if let Some(g) = st(w, id).teleport_actor
        && w.actors.items.get(g).is_some_and(|i| !i.dead)
    {
        crate::world::items::destroy_item(w, g);
    }
    let m = st_mut(w, id);
    if m.wait075 == 0 {
        m.wait075 = to_int32(latent_frames(0.075));
    }
}

/// The search @243.
fn search(w: &mut World, id: ActorId) {
    follow(w, id);
    let f = owner(w, id);
    let m = st(w, id);
    let r = if m.found_coin { 750.0 } else { m.aim_r * scale(w, id) };
    let ignore = [Some(ActorRef::Fighter(f)), Some(ActorRef::Actor(id)), m.teleport_actor.map(ActorRef::Item)];
    let found = overlap(w, id, r, &ignore);
    st_mut(w, id).found_coin = false;
    for a in found {
        consider(w, id, a);
    }
    last_hit_test(w, id);
}

#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the graph's NOT of a float compare: a NaN takes the branch")]
fn consider(w: &mut World, id: ActorId, a: ActorRef) {
    let coin = is_treasure(w, a);
    if !coin && !is_fighter(Some(a)) {
        return; // @981
    }
    let f = owner(w, id);
    if coin {
        // @1239 (the coin's team: its spawner's)
        let ActorRef::Projectile(c) = a else { return };
        let team = w.actors.projectiles.get(c).and_then(|p| p.owner).map(|o| w.fighter(o).team.team);
        if team != Some(w.fighter(f).team.team) {
            return;
        }
    }
    let dot = dot_to(w, id, Some(a));
    if !(dot >= -0.1) {
        return; // @1461
    }
    let m = st(w, id);
    if !(dot >= 0.4 && !m.in_front) {
        // @2976
        let closest_fighter = is_fighter(m.target);
        if coin && closest_fighter {
            return; // @3340
        }
        if (coin || closest_fighter) && !(m.closest_distance >= dist_to(w, id, Some(a))) {
            return; // @3139
        }
    }
    // @1682
    let d = dist_to(w, id, Some(a));
    let m = st_mut(w, id);
    m.closest_distance = d;
    m.in_front = dot >= 0.4;
    m.target = Some(a);
}

/// @2320 (authority) and SetAimActorMulticast(ClosestActor).
fn last_hit_test(w: &mut World, id: ActorId) {
    let f = owner(w, id);
    let m = st(w, id);
    let lh = if alive(w, m.last_hit) { m.last_hit } else { None }; // the head's (h)
    let (lx, ly) = lh.map_or((0.0, 0.0), |a| loc(w, a));
    let near = dist_to(w, id, lh) <= 750.0 || size2d([lx - m.teleport_x, ly - m.teleport_y]) <= 750.0;
    if near && dot_to(w, id, lh) >= -0.1 && (w.fighter(f).clock.time - m.last_hit_time) <= 0.65 {
        st_mut(w, id).target = lh;
    }
    let t = st(w, id).target;
    set_aim_actor(w, id, t);
}

/// The AimOverlap begin @10721 -> @3341 (the head's (g)): ClosestActor null, not Finn, !FoundCoin; a dot >= -0.1 takes
/// it ((dot >= 0.4 and !InFront) @3705 or ClosestActor null @4199: both go to @3719; InFront is not written).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the graph's NOT of a float compare: a NaN takes the branch")]
fn overlap_begin(w: &mut World, id: ActorId, a: FighterId) {
    let f = owner(w, id);
    let m = st(w, id);
    if m.target.is_some() || a == f || m.found_coin {
        return;
    }
    let who = Some(ActorRef::Fighter(a));
    if !(dot_to(w, id, who) >= -0.1) {
        return; // @3557
    }
    let d = dist_to(w, id, who);
    let m = st_mut(w, id);
    m.closest_distance = d;
    m.target = who;
    let Some(fs) = finn_mut(w, f) else { return }; // @3887
    fs.dash.target = who;
    bind(w, id, ActorRef::Fighter(a));
    lock_visuals(w, id);
}

fn overlap_test(w: &mut World, id: ActorId) {
    let f = owner(w, id);
    let g = scale(w, id);
    let m = st(w, id);
    let (cx, cy) = (m.x + 100.0 * g * facing(w, f), m.y); // AimOverlap: relative X +100
    // Deviation: the sphere is cut through the play plane and the fighter's capsule radius added to the cut circle;
    // the build's sphere-capsule test in 3D reaches about 1.4 u further at the cut's edge.
    let ar = m.aim_r * g;
    let rr = if ar > DEPTH { (ar * ar - DEPTH * DEPTH).sqrt() } else { 0.0 };
    let now: Vec<FighterId> = sphere_overlap_actors(
        w,
        cx,
        cy,
        rr,
        &[crate::real(OBJ_PAWN)],
        &[Some(ActorRef::Actor(id))],
        crate::real(OBJ_WORLD_DYNAMIC),
    )
    .into_iter()
    .filter_map(|a| if let ActorRef::Fighter(x) = a { Some(x) } else { None })
    .collect();
    let was = std::mem::replace(&mut st_mut(w, id).overlapping, now.clone());
    for a in now {
        if !was.contains(&a) {
            overlap_begin(w, id, a);
        }
    }
}

/// OnTargetDestroyed @9780: unbound; (Pawn_Finn) DashTarget = null, AimWidget hidden, the gather @7328 again (no
/// LastHit refresh).
fn target_destroyed(w: &mut World, id: ActorId, a: ActorRef) {
    let m = st_mut(w, id);
    if let Some(i) = m.bound.iter().position(|b| *b == a) {
        m.bound.remove(i);
    }
    let f = owner(w, id);
    let Some(fs) = finn_mut(w, f) else { return };
    fs.dash.target = None;
    st_mut(w, id).widget = false;
    gather(w, id);
}

/// The kind's `spawn` (`new FinnAimer`): the actor data's overlap sizes and ClosestDistance, the place of the
/// spawn.
fn aimer_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{AIMER}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = crate::world::spawning::request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let r1 = obj(&a.components, "AimOverlap").and_then(|c| num(c, "SphereRadius"));
    let r2 = obj(&a.components, "ThrownTargetAimOverlap").and_then(|c| num(c, "SphereRadius"));
    if r1.is_none() || r2.is_none() {
        warn_text(w, "Finn_Dash_AimerExperiment: overlap sphere size missing");
    }
    let cd = num(&a.p, "ClosestDistance");
    if cd.is_none() {
        warn_text(w, "Finn_Dash_AimerExperiment data missing: ClosestDistance");
    }
    let (face, fx, fy) = (facing(w, f), w.fighter(f).body.x, actor_y(w, f));
    let s = FinnAimer {
        x: at.x,
        y: at.y,
        dx: (at.x - fx) * face,
        dy: at.y - fy,
        aim_r: r1.unwrap_or(0.0),
        thrown_r: r2.unwrap_or(0.0),
        target: None,
        lock: -1.0,
        widget: false,
        closest_distance: cd.unwrap_or(0.0),
        in_front: false,
        found_coin: false,
        teleport_actor: None,
        teleport_x: 0.0,
        teleport_y: 0.0,
        last_hit: None,
        last_hit_time: 0.0,
        targeted: Vec::new(),
        bound: Vec::new(),
        overlapping: Vec::new(),
        wait01: 0,
        wait075: 0,
    };
    (WorldActorState::FinnAimer(s), false)
}

/// BeginPlay @7127 and the begin overlaps right after it (the rest of the kind's `spawn`, with the actor listed).
fn aimer_spawned(w: &mut World, id: ActorId) {
    let f = owner(w, id);
    if let Some(fs) = finn_mut(w, f) {
        let (lh, lt) = (fs.last_hit_actor, fs.last_hit_time);
        fs.aimer = Some(ActorRef::Actor(id));
        let m = st_mut(w, id);
        m.last_hit = lh; // @7237
        m.last_hit_time = lt;
    }
    gather(w, id);
    overlap_test(w, id);
}

/// A listed aimer holds item `id` as its TeleportActor: the item stays in `Actors::gone_items` after its removal, as
/// the aimer keeps the object (the record writes it whole).
pub fn names_item(w: &World, id: ItemId) -> bool {
    w.actors
        .actors
        .iter()
        .any(|(_, a)| matches!(&a.state, WorldActorState::FinnAimer(m) if m.teleport_actor == Some(id)))
}

fn aimer_step(w: &mut World, id: ActorId) {
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return;
    }
    follow(w, id);
    // OnEndPlay of a bound actor (the head's (e)).
    let mut i = 0;
    while let Some(&a) = st(w, id).bound.get(i) {
        if is_fighter(Some(a)) || is_treasure(w, a) || !actor_dead(w, a) {
            i += 1;
            continue;
        }
        target_destroyed(w, id, a);
        i = 0; // the list changed: from the start
    }
    overlap_test(w, id);
    // ReceiveTick @9340: the Arrow at ClosestActor (the view's). TargetLockTimeline's tick.
    let m = st_mut(w, id);
    if m.lock >= 0.0 && m.lock < LOCK_LENGTH {
        m.lock = min(m.lock + DELTA_SECONDS, LOCK_LENGTH);
    }
    // Its latent actions, after the tick (the BeginPlay Delay counts the spawn frame). The 0.075 Delay first: one the
    // 0.1 Delay's link @15 creates below starts counting next frame; one created in the tick or the spawn counts this
    // frame.
    // @106: BPI_SetAllowRotationChangeThruATKFlag(false) (the head's (f)), then the search @243.
    if m.wait075 > 0 {
        m.wait075 -= 1;
        if m.wait075 == 0 {
            let f = owner(w, id);
            let c = &mut w.fighter_mut(f).controls.rot_thru_atk_count;
            *c = Some((c.unwrap_or(0) - 1).max(0));
            search(w, id);
        }
    }
    let m = st_mut(w, id);
    if m.wait01 > 0 {
        m.wait01 -= 1;
        if m.wait01 == 0 {
            goto15(w, id);
        }
    }
}

/// ReceiveEndPlay @9671: ClosestActor's binding removed (the others die with the aimer); each valid FinnTreasure of
/// AllTargetedFinnTreasure Untargeted() @9229. The pending Delays are dropped. DashTarget stays.
fn aimer_end(w: &mut World, id: ActorId) {
    let f = owner(w, id);
    let targeted = {
        let m = st_mut(w, id);
        m.bound.clear();
        m.targeted.clone()
    };
    for c in targeted {
        if w.actors.projectiles.get(c).is_some_and(|p| !p.dead) {
            coins::untargeted(w, c);
        }
    }
    if let Some(fs) = finn_mut(w, f)
        && fs.aimer == Some(ActorRef::Actor(id))
    {
        fs.aimer = None;
    }
}

/// BPI_Finn_Dash_Aim(DashTarget, FacingLeft; out HasValidTarget, DashAimVector) @0-1782 (Pawn_Finn's function). The
/// DashAimVector's Y (along the facing: the move vector's X) and Z, `None` for no valid target. FacingLeft picks which
/// of the two opposite unit vectors is read; both components are taken absolute, so it does not change the result.
// Deviation: a dead FinnTreasure (pooled in the build: still IsValid there, at its pool place) is taken as not valid.
pub fn dash_aim(w: &mut World, f: FighterId, target: Option<ActorRef>, _facing_left: bool) -> Option<(Real, Real)> {
    if !alive(w, target) {
        return None; // @29
    }
    let t = target?;
    if is_treasure(w, t) {
        if let ActorRef::Projectile(c) = t {
            coins::dash_pickup(w, c); // @122
        }
        if actor_dead(w, t) {
            return None; // IsActorBeingDestroyed @158
        }
    }
    let (tx, ty) = loc(w, t);
    let (dx, dy) = (tx - w.fighter(f).body.x, ty - actor_y(w, f));
    let n = safe_normal([0.0, dx, dy], SMALL_NUMBER);
    let dot = facing(w, f) * n[1]; // GetDotProductTo (both at X 0)
    let in_r = |v: Real, lo: Real, hi: Real| v >= lo && v <= hi;
    let horizontal = if matches!(t, ActorRef::Fighter(_)) {
        in_r(dot, 0.3, 1.0) || in_r(dot, -1.0, -0.3)
    } else {
        in_r(dot, 0.5, 1.0) || in_r(dot, -1.0, -0.5)
    };
    let above = ty > actor_y(w, f); // @965
    if horizontal {
        // GetDirectionUnitVector (zero under 1e-8 squared): (0, |Y|, (above ? |Z| : -|Z|) * 0.5).
        let (ux, uy) = (n[1].abs(), n[2].abs());
        Some((ux, ((if above { uy } else { -uy }) * 0.5)))
    } else {
        let k = FRAC_1_SQRT_2; // Normal((0, 1, +-1))
        Some((k, if above { k } else { -k }))
    }
}

fn v2(p: &Props, k: &str) -> V2 {
    match obj(p, k) {
        None => V2 { x: 0.0, y: 0.0 },
        Some(o) => V2 { x: num(o, "X").unwrap_or(Real::NAN), y: num(o, "Y").unwrap_or(Real::NAN) },
    }
}

/// AnimNotify_MoveCommand_FinnAim (parent AnimNotifyState, not AnimNotify_MoveCommand: no OnlyPerformOnGround,
/// AdjustStickInfluence or move buffs). Begin: BPI_GetEvadeData (IsEvade: TotalDuration / EvadeAnimPlayRate),
/// BPI_Finn_Dash_Aim (an owner without BPI_Finn: false, (0, 0, 0)), then AddMoveCommandNotify(valid ?
/// Normal(DashAimVector).(Y, Z) * VSize2D(AimVector) : AimVector, XYOverride, ZOverride, the control values, StopOnEnd
/// (CDO true), BreakAttach, WallSkate, ControlSpeedFriction, DisablePredictiveHitboxes, ClampSpeedOnEnd false, (0, 0),
/// (0, 0)). End: CompleteMoveCommand. Tick: the corner check under CornerCheck, which no instance sets (CDO false):
/// nothing.
fn aim_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    let av = v2(p, "AimVector");
    let target = finn_of(w, f).and_then(|s| s.dash.target);
    let have = finn_of(w, f).is_some();
    let facing_left = facing(w, f) < 0.0;
    let aim = if have { dash_aim(w, f, target, facing_left) } else { None };
    let mut mv = av;
    if let Some((ax, ay)) = aim {
        let n = normal([0.0, ax, ay], KINDA_SMALL_NUMBER);
        let s = size2d([av.x, av.y]);
        mv = V2 { x: n[1] * s, y: n[2] * s };
    }
    let er = if flag(p, "IsEvade") { evade_play_rate(w, f) } else { 1.0 };
    let total = if er == 1.0 { nt.dur } else { f64::from(crate::math::narrow(nt.dur / f64::from(er))) };
    let end = MoveEnd {
        stop: flag_or(p, "StopOnEnd", true),
        clamp: false,
        min: V2 { x: 0.0, y: 0.0 },
        max: V2 { x: 0.0, y: 0.0 },
    };
    let ctl = MoveCtl {
        speed: num_or(p, "ControlSpeed", 0.0),
        max: num_or(p, "ControlAngleMax", 0.0),
        min: num_or(p, "ControlAngleMin", 0.0),
        friction: num_or(p, "ControlSpeedFriction", 0.0),
        decay: num_or(p, "ControlSpeedDecay", 0.0),
    };
    let (xy, z) = (flag_or(p, "XYOverride", true), flag_or(p, "ZOverride", true));
    add_move_cmd(w, f, idx, crate::math::narrow(total), mv, xy, z, &end, false, flag_or(p, "WallSkate", false), &ctl);
}

/// FinnAimDashCondition_C's check (`combat::conditions`): the actor cast to Pawn_Finn, DashTarget != null (not
/// IsValid); another pawn: false.
pub fn aim_dash_check(w: &mut World, f: FighterId, _name: Name) -> bool {
    finn_of(w, f).is_some_and(|s| s.dash.target.is_some())
}

/// FinnAimDashCondition_C's reset (`combat::conditions`): the parent's (empty), HasHit and HitCheck false (kept
/// nowhere: SetHit unbound from the HitBoxHandler's HitEnemyPlayerLight is bound nowhere, SetHit is never called).
pub fn aim_dash_reset(_w: &mut World, _f: FighterId, _name: Name) {}

/// This file's registry entries (AnimNotify_MoveCommand_FinnAim_C, Finn_Dash_AimerExperiment_C,
/// FinnAimDashCondition_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        AIMER,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::FinnDashAimerExperiment),
            ActorHooks::World(WorldActorHooks {
                spawn: Some(aimer_spawn),
                spawned: Some(aimer_spawned),
                step: Some(aimer_step),
                end: Some(aimer_end),
                // the two overlap spheres read the scale (gather, search); its widget is the HUD's
                size: Some(SizeHooks::default()),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_MoveCommand_FinnAim_C",
        NotifyHandler { begin: Some(aim_begin), end: Some(move_command_end), ..NotifyHandler::default() },
    )?;
    r.conditions.enter("FinnAimDashCondition_C", Condition::Class(crate::combat::conditions::CondClass::FinnAimDash))
}

impl Default for FinnAimer {
    /// The field initial values (the constructor then reads the data's sizes and ClosestDistance).
    fn default() -> Self {
        FinnAimer {
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
            aim_r: 0.0,
            thrown_r: 0.0,
            target: None,
            lock: -1.0,
            widget: false,
            closest_distance: 0.0,
            in_front: false,
            found_coin: false,
            teleport_actor: None,
            teleport_x: 0.0,
            teleport_y: 0.0,
            last_hit: None,
            last_hit_time: 0.0,
            targeted: Vec::new(),
            bound: Vec::new(),
            overlapping: Vec::new(),
            wait01: 0,
            wait075: 0,
        }
    }
}
