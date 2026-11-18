//! Finn's gem and apple: FinnDashThrownTarget ([`Gem`], an item that flies by
//! `actors::item_bounce`), CoinThrow, the conditions FinnMoneyForGemCondition / FinnNoMoneyForGemCondition /
//! TeleportProjectileFinnCondition.
//!
//! FinnDashThrownTarget (an ItemActorParent child: Thrown, PickUp, RestrictedToTeam / CanBeHitWhileIdle /
//! DestroyWhenSpent from its CDO, ReceiveAnyDamage, ActivateObject, the gravity writes @30 / @76 / @3076 / @6620, the
//! bounce event, the fade timers @2658 / @5884, the Delays @884 -> 793, @2743 -> 15, @2882 -> 752, @3665 -> 249,
//! BeingWarpedTo @6231, the GemBoost branches), CoinThrow / CoinThrow_Weak (Comp_Spawner notifies with
//! AddMomentumAlpha 0.5: the item's `make` below), Item_Use's ItemUse (AnimNotify_ItemUse: the kind's activate).
//! Ability slot 1 (the apple: MaxCount 2, StartCount 2, CountCooldown 10) is the data's: the abilities take it as it
//! is; the apple's BeginPlay spends it (TriggerAbilityServer(1) @2569), CoinThrow_Weak's branch row checks it.
//!
//! The gem has no record on the pawn: Pawn_Finn holds no variable for it, and the build finds it through Comp_Spawner's
//! SpawnedActors (TeleportProjectileFinnCondition @156: FilterArray FinnDashThrownTarget_C, the first one). So does the
//! port: [`find_dash_target`] reads the fighter's spawned actors. Pawn_Finn.DashTarget (the aimer's lock) is another
//! variable: aimer.rs's.
//!
//! Not reached in play, ported as the graph has it: ActivateObject (CanUseItem false: the hold's neutral press is a
//! throw), TargetedServer's callers Targeted / Untargeted (the aimer calls Targeted on FinnTreasure alone:
//! [`target_gem`] / [`untarget_gem`] have no caller), the apple's OnBounce destroy.
//! UI (AimWidget, UiChecker, ToggleUIOwner, MulticastUIAvailability, SetTargetVisibilityLocal, UpdateTargetVisuals) and
//! the visuals (SpawnVisuals' team materials, SetAppleVisualsMulticast's mesh) are the view's: `weak_throw` says which
//! mesh.
// Unverified: the Delay of BeingWarpedTo is taken as first counted in the call's frame (the caller's tick ahead of the
// item's).

use super::coins::wallet_stacks;
use super::perks::spawn_gem_boost_blast;
use crate::Real;
use crate::State;
use crate::actors::classes::ItemClass;
use crate::actors::item::{BLINK_HIDDEN, Item, ItemBox, ItemDef, ItemExt, item_activate_parent};
use crate::actors::item_bounce::Bounce;
use crate::actors::{ActorClass, ActorRef, ItemId};
use crate::combat::buffs::{buff_of, remove_stack};
use crate::combat::hit::knockback::timer_ticks;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_def, def_is_projectile, latent_frames, warn_text};
use crate::fighter::control::trigger_ability;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::projectile_speed_buffed;
use crate::fighter::{FighterId, actor_y, facing, percent_charged};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, size2d};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ItemHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Condition, Registry, RegistryError};
use crate::world::World;
use crate::world::items::{add_item, destroy_item, drop_held_item, item_def, resolve_def};
use mvs_data::value::{Props, Value};
use std::sync::Arc;

/// The gem's class.
pub const DASH_TARGET: &str = "FinnDashThrownTarget_C";

const FADE_LEN: Real = 3.0; // DeathBlinkWarning_0's length; its Blink track has the parent DeathBlinkWarning's keys
const PITCH_END: Real = 1.0038; // its Pitch track: linear from (0, 0) to (3, 1.0038)
const BOOST_RATE: Real = 1.75; // GemBoostTimer @1144, looping

const GEM_BOOST: Name = name!("Buff_Finn_GemBoost_C");
const GEM_ON_CHARGE: Name = name!("Buff_Finn_GemOnCharge_C");
const TREASURE_BUFF: Name = name!("FinnTreasureBuff_C");
const BOOST_PROJECTILE: Name = name!("FinnGemBoostUseProjectile_C");

/// `Gem extends BouncingItem` (FinnDashThrownTarget_C): what the class adds.
#[derive(State, Debug)]
pub struct Gem {
    /// The bouncing flight's state (`BouncingItem`).
    pub bounce: Bounce,
    /// IsTargeted (CDO false, @3125): an aimer's lock on it.
    pub is_targeted: bool,
    /// WeakThrow (@3497, SetAppleVisualsMulticast): the apple when true, else the gem.
    pub weak_throw: bool,
    /// The fade: FadeTimer's fire time in World ticks (@2715), -1 when none is set.
    pub fade_at: i32,
    /// World.tick of the spawn (BeginPlay): the frame the Delays of BeginPlay first count in.
    pub born: i32,
    /// DeathBlinkWarning_0: its position (-1 before it first plays), playing or stopped (Stop keeps the position), the
    /// actor hidden by its Update, and the DoOnce of the blink sound (@964, reset @1068).
    pub fade: Real,
    pub fading: bool,
    pub fade_hidden: bool,
    pub blink_closed: bool,
    /// K2_PauseTimerHandle(FadeTimer) @3202: the timer passes it had left, -1 when not paused.
    pub fade_paused: i32,
    /// GemBoostPerkActive (@1525, @2456).
    pub boost: bool,
    /// GemBoostTimer: the World.tick it was set at (-1: none) and its next expiry in counted seconds.
    pub boost_set: i32,
    pub boost_next: Real,
    /// InstigatingFinn (@2818).
    pub instigator: Option<FighterId>,
    /// The Capsule's Platform response: Ignore from the construction script, Block from @752.
    pub plat_on: bool,
    /// CanBePickedUp = false (@6555).
    pub no_pick: bool,
    /// BeingWarpedTo's Delay(0.2) @884: the World.tick of its ReleaseToPool, -1 none.
    pub warp_at: i32,
    /// OnProjectileBounce's sfx_cha_finn_treasure_bounce of this frame's flight, posted by the tick.
    pub bounce_sounds: i32,
    /// The apple's OnBounce (@3720 K2_DestroyActor) in this frame's flight.
    pub bounce_kill: bool,
    /// GemOnCharge's NextGemFree (@1722 true, @1804 the spawn, @10 false): true only inside [`spawn_gem`], which the
    /// gem's BeginPlay reads at once (@1891). A module-level variable, not sim state: not recorded.
    pub next_free: bool,
}

impl Default for Gem {
    /// The field initial values.
    fn default() -> Self {
        Gem {
            bounce: Bounce::default(),
            is_targeted: false,
            weak_throw: false,
            fade_at: -1,
            born: 0,
            fade: -1.0,
            fading: false,
            fade_hidden: false,
            blink_closed: false,
            fade_paused: -1,
            boost: false,
            boost_set: -1,
            boost_next: 0.0,
            instigator: None,
            plat_on: false,
            no_pick: false,
            warp_at: -1,
            bounce_sounds: 0,
            bounce_kill: false,
            next_free: false,
        }
    }
}

fn gem(w: &World, it: ItemId) -> &Gem {
    match &w.actors.items.get(it).expect("a live item").ext {
        ItemExt::Gem(g) => g,
        _ => panic!("item {it:?} is not a gem"),
    }
}

fn gem_w(w: &mut World, it: ItemId) -> &mut Gem {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::Gem(g) => g,
        _ => panic!("item {it:?} is not a gem"),
    }
}

/// `Gem.pickupOn`: CanBePickedUp gates the PickupArea's begin overlap (Comp_ItemManager @3083).
// Unverified: taken as the area's collision, which also ends a reach that began before (the gem is gone 0.2 s later).
pub fn pickup_on(g: &Gem) -> bool {
    !g.no_pick
}

/// `Gem.hidden`'s own part: SetActorHiddenInGame from DeathBlinkWarning_0's Update (@5527); the parent's DeathWarning
/// is the caller's.
pub fn fade_hidden(g: &Gem) -> bool {
    g.fade_hidden
}

/// The gem's OnProjectileBounce handlers (`bounced`): its flight is BouncingItem's (Bounciness 0.35, Friction 2). With
/// HitBoxOn? (the apple, a throw) a blocking hit is the parent's ReceiveHit (@4579): its tail deactivates the
/// ProjectileMovement (@5148) inside the move's dispatch, HandleBlockingHit aborts and no OnProjectileBounce fires.
/// Without it the hit bounces: the bounce event @5625 (bound first): |ImpactVelocity.Z| > 200 ->
/// sfx_cha_finn_treasure_bounce on the gem. The apple's OnBounce (@2376, bound behind it): K2_DestroyActor (@3720).
pub fn bounced(it: &mut Item, ivy: Real) {
    let ItemExt::Gem(g) = &mut it.ext else { return };
    if ivy.abs() > 200.0 {
        g.bounce_sounds += 1;
    }
    if g.weak_throw {
        g.bounce_kill = true;
    }
}

/// A handler destroyed the gem in this frame's flight (`bounceStop`).
pub fn bounce_stop(it: &Item) -> bool {
    matches!(&it.ext, ItemExt::Gem(g) if g.bounce_kill)
}

/// The Capsule's Platform response (`platIgnored`): Ignore until the Delay(0.2) of BeginPlay (@752) and while IsThrown.
pub fn plat_ignored(it: &Item, p: &ItemBox) -> bool {
    let plat_on = matches!(&it.ext, ItemExt::Gem(g) if g.plat_on);
    (it.thrown || !plat_on) && !p.is_static
}

// ---------------------------------------------------------------------------------------------- the graph's events

fn sound(w: &mut World, it: ItemId, ev: Name) {
    let i = w.actors.items.get(it).expect("a live item");
    let (x, y, owner) = (i.x, i.y, i.owner);
    let fighter = owner.map_or(-1, |f| w.fighter(f).id);
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// The FadeTimer (K2_SetTimerDelegate(FadeDelay) @2658 / @5884 -> StartFade), set outside the timer pass: it fires in
/// the timer pass timerTicks(FadeDelay) frames on.
fn set_fade_timer(w: &mut World, it: ItemId) {
    let delay = item_def(w, it).and_then(|p| num(p, "FadeDelay")).unwrap_or(3.0);
    let at = to_int32(crate::real(w.flow.tick) + timer_ticks(Some(delay)));
    let g = gem_w(w, it);
    g.fade_at = at;
    g.fade_paused = -1;
}

/// StartFade @3390: FadeTimer cleared, DeathBlinkWarning_0.Play() (from where it stood: Stop keeps the position). The
/// timeline first advances in the next frame's actor tick.
fn start_fade(g: &mut Gem) {
    g.fade_at = -1;
    g.fade_paused = -1;
    if g.fade < 0.0 {
        g.fade = 0.0;
    }
    g.fading = true;
}

/// PauseFade @3443: DeathBlinkWarning_0.Stop(), SetActorHiddenInGame(false).
fn pause_fade(g: &mut Gem) {
    g.fading = false;
    g.fade_hidden = false;
}

/// DeathBlinkWarning_0's tick (with the actor, TG_PrePhysics): position + delta (float32), past the length clamped,
/// Update (@5493), then Finished (@8643 -> @2987 K2_DestroyActor). True: destroyed.
fn fade_tick(w: &mut World, it: ItemId) -> bool {
    let g = gem_w(w, it);
    g.fade += DELTA_SECONDS;
    let end = g.fade > FADE_LEN;
    if end {
        g.fade = FADE_LEN;
    }
    let t = g.fade;
    let hide = BLINK_HIDDEN.iter().any(|&[a, b]| t >= a && t < b);
    g.fade_hidden = hide; // @5527
    // @5550 SetRTPCValue(cha_finn_treasure_pitch, Pitch, 50 ms, no actor: the global value)
    w.events.push(Event::Rtpc {
        fighter: -1,
        name: name!("cha_finn_treasure_pitch"),
        value: (t / FADE_LEN) * PITCH_END,
        interp_ms: 50.0,
    });
    // @5440: Blink < 0.5 resets the DoOnce (@1068), else the DoOnce posts sfx_cha_finn_treasure_blink on the gem (@990).
    let g = gem_w(w, it);
    if hide {
        g.blink_closed = false;
    } else if !g.blink_closed {
        g.blink_closed = true;
        sound(w, it, name!("sfx_cha_finn_treasure_blink"));
    }
    end
}

/// TargetedServer(IsTargeted, Gravity) -> TargetedMulticast @3338: with IsTargeted the velocity 0 (@3023), then the
/// gravity scale and IsTargeted; IsTargeted pauses an active FadeTimer and PauseFade; else an active FadeTimer is
/// unpaused (a no-op) or, with none active (a paused one is not), StartFade. The branch has no caller in the build.
fn targeted_multicast(w: &mut World, it: ItemId, on: bool, gravity: Real) {
    let tick = w.flow.tick;
    let i = w.actors.items.get_mut(it).expect("a live item");
    if on {
        i.vx = 0.0;
        i.vy = 0.0;
    }
    i.grav_scale = gravity;
    let ItemExt::Gem(g) = &mut i.ext else { return };
    g.is_targeted = on;
    if on {
        if g.fade_at >= 0 {
            g.fade_paused = to_int32(max(crate::real(g.fade_at - tick), 1.0));
            g.fade_at = -1;
        }
        pause_fade(g);
    } else if g.fade_at < 0 {
        start_fade(g);
    }
}

/// Targeted @3002: TargetedServer(true, 0.5). No caller in the build (the aimer's Targeted call is FinnTreasure's).
pub fn target_gem(w: &mut World, it: ItemId) {
    targeted_multicast(w, it, true, 0.5);
}

/// Untargeted @3317: TargetedServer(true, 4.0): the build's own call passes true, so the gem stays targeted (and its
/// fade paused) after it.
pub fn untarget_gem(w: &mut World, it: ItemId) {
    targeted_multicast(w, it, true, 4.0);
}

/// BeingWarpedTo @6231 (Finn_Dash_AimerExperiment @5930, on the dash's target; the event takes no argument: `on`, the
/// warping fighter, is unused). In a hand (GetAttachParentActor with a Comp_ItemManager): ThrowItem((0, 0, 0), not
/// thrown) and CanBePickedUp = false. Then with authority the DoOnce Delay(0.2) -> ReleaseToPool (@884 -> @793), and
/// the velocity 0, gravity scale 0, physics off (@6567-6665): the gem hangs where it is until then (the port keeps it
/// in its flight with nothing to move it).
pub fn gem_being_warped_to(w: &mut World, it: ItemId, _on: Option<FighterId>) {
    if w.actors.items.get(it).is_none_or(|i| i.dead) {
        return;
    }
    let holder = w.actors.items.get(it).expect("a live item").holder;
    if holder >= 0
        && let Some(h) = w.by_id(holder)
    {
        drop_held_item(w, h);
        gem_w(w, it).no_pick = true;
    }
    let at = w.flow.tick + to_int32(latent_frames(0.2)) - 1;
    let g = gem_w(w, it);
    if g.warp_at < 0 {
        g.warp_at = at;
    }
    let i = w.actors.items.get_mut(it).expect("a live item");
    i.vx = 0.0;
    i.vy = 0.0;
    i.grav_scale = 0.0;
    i.flying = true;
}

/// The first FinnDashThrownTarget of f's Comp_Spawner SpawnedActors (TeleportProjectileFinnCondition @156: FilterArray,
/// index 0), `None` for none. The build's list drops an actor at its EndPlay; the port's keeps it, marked dead, until
/// the frame's end.
pub fn find_dash_target(w: &World, f: FighterId) -> Option<ItemId> {
    w.fighter(f).items.spawned_actors.iter().find_map(|a| match *a {
        ActorRef::Item(id)
            if w.actors.items.get(id).is_some_and(|i| !i.dead && i.cls == name!("FinnDashThrownTarget_C")) =>
        {
            Some(id)
        }
        _ => None,
    })
}

// ---------------------------------------------------------------------------------------------- the spawn

/// Comp_Spawner HandleActorPostSpawn with EnableProjectileMovement (Projectile's constructor): the speed
/// (ApplyProjectileSpeedBuffsToInputs for a projectile class alone: the gem's Comp_Actor_Destructible says not), the
/// charge Lerp with ChargeAffectsInitialSpeed (not set by CoinThrow), AddMomentumAlpha (the owner's velocity in its
/// facing frame x the alpha, added to direction x InitialSpeed before VSize / Normal), MaxSpeed max(launch, the
/// notify's). No impulse, no half of the fighter's velocity.
#[allow(clippy::many_single_char_names, reason = "names kept as ported")]
fn launch_gem(w: &mut World, f: FighterId, n: &Props, x: Real, y: Real, attack_id: i32) -> Option<ItemId> {
    let cls = name!("FinnDashThrownTarget_C");
    let Some(src) = resolve_def(w, Some(f), DASH_TARGET) else {
        warn_text(w, "spawned actor not implemented (FinnDashThrownTarget_C)");
        return None;
    };
    let buffed = actor_def(w, ActorRef::Fighter(f), cls).is_some_and(def_is_projectile);
    let initial0 = num(n, "InitialSpeed").unwrap_or(0.0);
    let initial = if buffed { projectile_speed_buffed(w, f, initial0) } else { initial0 };
    let mult = num(n, "ChargeSpeedMultiplier").unwrap_or(1.0);
    let alpha = if flag(n, "ChargeAffectsInitialSpeed") { min(max(percent_charged(w, f), 0.0), 1.0) } else { 0.0 };
    let speed = initial + (initial * mult - initial) * alpha;
    let (dy, dz) = obj(n, "InitialMovementDirection")
        .map_or((1.0, 0.0), |d| (num(d, "Y").unwrap_or(0.0), num(d, "Z").unwrap_or(0.0)));
    let face = facing(w, f);
    let mut launch = speed * size2d([dy, dz]); // VSize
    let mut vx = dy * speed * face;
    let mut vy = dz * speed;
    let m = num(n, "AddMomentumAlpha").unwrap_or(0.0);
    if m != 0.0 {
        let (bvx, bvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
        let my = dy * initial + bvx * face * m;
        let mz = dz * initial + bvy * m;
        let size = size2d([my, mz]); // VSize
        launch = size + (size * mult - size) * alpha;
        let n = normal([0.0, my, mz], KINDA_SMALL_NUMBER);
        vx = n[1] * launch * face;
        vy = n[2] * launch;
    }
    let data = Arc::clone(&w.data);
    let sb = crate::world::blast::spawn_box(w);
    let mut it = Item::make(
        w,
        cls,
        ItemClass::FinnDashThrownTarget,
        Some(f),
        src,
        ItemDef::at(&data, src),
        x,
        y,
        0.0,
        w.fighter(f).body.facing_dir,
        vx,
        vy,
        &sb,
        false,
    );
    // EnableProjectileMovement: the ProjectileMovement flies from the spawn (HitBoxOn? false in the CDO).
    it.flying = true;
    it.attack_id = attack_id;
    let mx0 = num(n, "MaxSpeed").unwrap_or(0.0);
    let mx = if buffed { projectile_speed_buffed(w, f, mx0) } else { mx0 };
    it.max_speed = max(launch, mx + (mx * mult - mx) * alpha);
    it.ext = ItemExt::Gem(Gem { bounce: Bounce::new(ItemDef::at(&data, src)), ..Gem::default() });
    Some(w.actors.items.insert(it))
}

/// Buff_Finn_GemOnCharge's CompSpawnActor(FinnDashThrownTarget_C, ..., GemTransform, LaunchDirection, LaunchSpeed, 4000)
/// @1804 (perks.rs): the gem at (x, y) with NextGemFree set around the spawn, then as the notify's spawn goes on (the
/// world's item path: numbered, listed, BeginPlay, the instigator's buffs, AfterSpawnedActor). `dir`: UE (Y, Z),
/// mirrored by the facing as compSpawnProjectile's. The gem, or `None` when it is gone at once.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
pub fn spawn_gem(
    w: &mut World,
    f: FighterId,
    x: Real,
    y: Real,
    dir: (Real, Real),
    speed: Real,
    max_speed: Real,
    free: bool,
) -> Option<ItemId> {
    let mut d = Props::new();
    d.insert("X".into(), Value::Num(0.0));
    d.insert("Y".into(), Value::Num(f64::from(dir.0)));
    d.insert("Z".into(), Value::Num(f64::from(dir.1)));
    let mut n = Props::new();
    n.insert("InitialMovementDirection".into(), Value::Obj(d));
    n.insert("InitialSpeed".into(), Value::Num(f64::from(speed)));
    n.insert("MaxSpeed".into(), Value::Num(f64::from(max_speed)));
    let attack = w.fighter(f).decay.attack_count;
    let id = launch_gem(w, f, &n, x, y, attack)?;
    gem_w(w, id).next_free = free;
    let charge = crate::combat::elem::instigator_charge(w, f);
    add_item(w, id, Some(f), charge, true);
    // (the flag lives only inside the spawn)
    if let Some(g) =
        w.actors.items.get_mut(id).and_then(|i| if let ItemExt::Gem(g) = &mut i.ext { Some(g) } else { None })
    {
        g.next_free = false;
    }
    if w.actors.items.get(id).is_none_or(|i| i.dead) {
        return None;
    }
    crate::world::spawning::item_spawned(w, Some(id), f);
    Some(id)
}

/// SpawnGemBoostBlast @6978: the owner's Comp_Spawner CompSpawnActor(FinnGemBoostBlast_C) at the gem's (0, Y, Z), a
/// transform with no rotation @7284 (perks.rs).
// Unverified: the zero rotation is taken as facing 1 (as spawn_stage_item); no read of the blast's hit takes it (its
// HitBoxDamage is DirectionalKnockback).
fn spawn_boost_blast(w: &mut World, it: ItemId) {
    let i = w.actors.items.get(it).expect("a live item");
    let (f, x, y) = (i.owner, i.x, i.y);
    if let Some(f) = f {
        spawn_gem_boost_blast(w, f, x, y, 1.0);
    }
}

/// CoinThrow / CoinThrow_Weak (Comp_Spawner, Translation (0, 50, 100), direction (0, 1, 1), 1250 / 800, MaxSpeed 4000,
/// AddMomentumAlpha 0.5) with the AttackID of the move.
fn make(w: &mut World, f: FighterId, r: &SpawnRequest, x: Real, y: Real) -> ItemId {
    let np = crate::world::spawning::request_props(w, f, r);
    launch_gem(w, f, &np, x, y, r.attack_id).expect("FinnDashThrownTarget_C in its spawner's data")
}

fn sub() -> ItemExt {
    ItemExt::Gem(Gem::default())
}

/// ReceiveBeginPlay @1225 after the parent's, in the order of its push_flow stack: the GemBoost check (@1265), the
/// visuals (@2620, the view's), the payment (@1541), FadeTimer (@2635), Delay(1.0) (@2743), InstigatingFinn (@2798),
/// Delay(0.2) (@2882).
fn begin(w: &mut World, it: ItemId) -> bool {
    let tick = w.flow.tick;
    let f = w.actors.items.get(it).expect("a live item").owner;
    gem_w(w, it).born = tick;
    // @1364: the owner's BPI_CheckIfHasBuff(Buff_Finn_GemBoost) -> GemBoostPerkActive and the looping 1.75 s
    // SpawnGemBoostBlast timer (@1121-1197).
    if let Some(f) = f {
        if buff_of(w, f, GEM_BOOST).is_some() {
            let g = gem_w(w, it);
            g.boost = true;
            g.boost_set = tick;
            g.boost_next = BOOST_RATE;
        }
        // @1642: GemOnCharge with NextGemFree pays nothing (Instructions: nothing outside the view).
        let free = gem(w, it).next_free && buff_of(w, f, GEM_ON_CHARGE).is_some();
        if !free {
            // @2013: the wallet's HasBuff and stacks >= ThrowCost -> RemoveStack(GemBoostPerkActive ? BoostThrowCost :
            // ThrowCost).
            let p = item_def(w, it);
            let cost = p.and_then(|p| num(p, "ThrowCost")).unwrap_or(2.0);
            let boost_cost = p.and_then(|p| num(p, "BoostThrowCost")).unwrap_or(2.0);
            let wallet = buff_of(w, f, TREASURE_BUFF);
            match wallet {
                Some(b) if wallet_stacks(w, f) >= cost => {
                    let n = if gem(w, it).boost { boost_cost } else { cost };
                    remove_stack(w, f, b, to_int32(n));
                }
                _ => {
                    // @2353: the apple. OnBounce bound, HitBoxOn? with UpdateHitbox, SetAppleVisualsMulticast (WeakThrow,
                    // its Delay(0.1) -> @249 the materials and sfx_cha_finn_treasure_apple_throw), GemBoostPerkActive
                    // false, CanUseItem false (the CDO's already), GemBoostTimer cleared, the instigator's
                    // TriggerAbilityServer(1).
                    w.actors.items.get_mut(it).expect("a live item").hit_on = true;
                    let g = gem_w(w, it);
                    g.weak_throw = true;
                    g.boost = false;
                    g.boost_set = -1;
                    trigger_ability(w, f, 1);
                }
            }
        }
    }
    set_fade_timer(w, it);
    gem_w(w, it).instigator = f;
    false
}

/// The actor's latent actions and its timeline, in a hand or not (ahead of its move).
fn step(w: &mut World, it: ItemId) -> bool {
    let tick = w.flow.tick;
    let n = crate::real(tick - gem(w, it).born);
    if n == latent_frames(1.0) - 1.0 && !gem(w, it).is_targeted {
        w.actors.items.get_mut(it).expect("a live item").grav_scale = 4.0; // @2743 -> @15
    }
    if n == latent_frames(0.2) - 1.0 {
        gem_w(w, it).plat_on = true; // @2882 -> @752 SetCollisionResponseToChannel(20, Block)
    }
    if gem(w, it).weak_throw && n == latent_frames(0.1) - 1.0 {
        sound(w, it, name!("sfx_cha_finn_treasure_apple_throw")); // @3665 -> @249
    }
    let g = gem(w, it);
    if g.warp_at >= 0 && tick >= g.warp_at {
        return true; // @884 -> @793 ReleaseToPool
    }
    g.fading && fade_tick(w, it)
}

/// After the move: the flight's bounce handlers' sounds, then the apple's OnBounce destroy.
fn tick(w: &mut World, it: ItemId) -> bool {
    while gem(w, it).bounce_sounds > 0 {
        gem_w(w, it).bounce_sounds -= 1;
        sound(w, it, name!("sfx_cha_finn_treasure_bounce"));
    }
    gem(w, it).bounce_kill
}

fn timer(w: &mut World, it: ItemId) -> bool {
    let tick = w.flow.tick;
    let g = gem_w(w, it);
    if g.fade_at >= 0 && tick >= g.fade_at {
        start_fade(g); // FadeTimer -> StartFade
    }
    if g.boost_set >= 0 && crate::consts::clock(crate::real(tick - g.boost_set)) > f64::from(g.boost_next) {
        g.boost_next += BOOST_RATE;
        spawn_boost_blast(w, it); // GemBoostTimer -> SpawnGemBoostBlast (authority)
    }
    false
}

/// PickUp @5818 after the parent's: gravity scale 3 (@76), FadeTimer cleared, PauseFade; ToggleUIOwner(false) when the
/// grabber is InstigatingFinn (the view's).
fn grabbed(w: &mut World, it: ItemId, _f: FighterId) {
    w.actors.items.get_mut(it).expect("a live item").grav_scale = 3.0;
    let g = gem_w(w, it);
    g.fade_at = -1;
    g.fade_paused = -1;
    pause_fade(g);
}

/// Thrown @5842 after the parent's: FadeTimer set again (@5884), ToggleUIOwner(true) (the view's). The parent's release
/// turns the capsule to the velocity; the gem's ProjectileMovement does not follow it and the release's transform has
/// rotation 0: upright.
fn thrown(w: &mut World, it: ItemId) {
    w.actors.items.get_mut(it).expect("a live item").pitch = 0.0;
    set_fade_timer(w, it);
}

/// ActivateObject @7448 (unreachable: CanUseItem false): the parent's (a held item is dropped), then with the owner's
/// Comp_Spawner two FinnGemBoostUseProjectile at the owner's location, directions (0, 1, 0) and (0, -1, 0), 2000 /
/// 2000, then ReleaseToPool.
fn activate(w: &mut World, it: ItemId, _user: FighterId) {
    if item_activate_parent(w, it) || w.actors.items.get(it).is_none_or(|i| i.dead) {
        return;
    }
    if let Some(f) = w.actors.items.get(it).expect("a live item").owner {
        let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
        for dy in [1.0, -1.0] {
            crate::world::spawning::comp_spawn_projectile(
                w,
                f,
                BOOST_PROJECTILE,
                fx,
                fy,
                [Some(0.0), Some(dy), Some(0.0)],
                2000.0,
                2000.0,
                None,
            );
        }
    }
    if !w.actors.items.get(it).is_none_or(|i| i.dead) {
        destroy_item(w, it);
    }
}

/// ReceiveEndPlay (the parent's, then ThrowItem on an attach parent: none left after ClearHeldItem) and
/// ReceiveDestroyed @4713: sfx_cha_finn_treasure_apple_destroy (the apple) or sfx_cha_finn_treasure_destroy at the
/// capsule.
fn end(w: &mut World, it: ItemId) {
    let ev = if gem(w, it).weak_throw {
        name!("sfx_cha_finn_treasure_apple_destroy")
    } else {
        name!("sfx_cha_finn_treasure_destroy")
    };
    sound(w, it, ev);
}

// ---------------------------------------------------------------------------------------------- the conditions

/// FinnMoneyForGemCondition.Check: with Buff_Finn_GemBoost, the wallet's HasBuff and stacks > 1; without it, HasBuff
/// alone (@0-488).
fn money_for_gem(w: &mut World, f: FighterId) -> bool {
    let has = buff_of(w, f, TREASURE_BUFF).is_some();
    if buff_of(w, f, GEM_BOOST).is_some() { has && wallet_stacks(w, f) > 1.0 } else { has }
}

/// FinnNoMoneyForGemCondition.Check: the parent's Check, negated.
fn no_money_for_gem(w: &mut World, f: FighterId) -> bool {
    !money_for_gem(w, f)
}

/// TeleportProjectileFinnCondition.Check @0-687: the first FinnDashThrownTarget of the spawner's SpawnedActors
/// implements BPI_SpawnedActor and is not the apple.
fn teleport_projectile(w: &mut World, f: FighterId) -> bool {
    find_dash_target(w, f).is_some_and(|g| !gem(w, g).weak_throw)
}

/// This file's registry entries (FinnDashThrownTarget_C, the three conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        make: Some(make),
        sub: Some(sub),
        begin: Some(begin),
        step: Some(step),
        tick: Some(tick),
        timer: Some(timer),
        grabbed: Some(grabbed),
        thrown: Some(thrown),
        activate: Some(activate),
        end: Some(end),
        ..ItemHooks::default()
    };
    r.actors.enter(
        ItemClass::FinnDashThrownTarget.name(),
        ActorEntry::new(ActorClass::Item(ItemClass::FinnDashThrownTarget), ActorHooks::Item(hooks)),
    )?;
    r.conditions.enter("FinnMoneyForGemCondition_C", Condition::Fn(money_for_gem))?;
    r.conditions.enter("FinnNoMoneyForGemCondition_C", Condition::Fn(no_money_for_gem))?;
    r.conditions.enter("TeleportProjectileFinnCondition_C", Condition::Fn(teleport_projectile))
}
