//! Finn's coins: Pawn_Finn's SpawnTreasure @2381-7455 with Calculate Treasure
//! Knockback, BPI_Finn_DropCoins @8786-8824, AnimNotify_FinnDropCoins, AnimNotify_FinnBag_Hitframe (the stack-scaled
//! damage), FinnTreasure_C ([`FinnTreasure`], a projectile: PickUpDelay @708, FadeDelay @1155, ProjectileGravityDelay
//! @1263, the pickup overlap and AfterPickupGate, Delay 0.066 @1839 -> 15, Targeted / DashPickup, the death blink
//! timeline) and FinnTreasureBuff_C (the wallet: AddStack from the coin @121 / @929, the pop text @148-973).
//!
//! A SpawnedActorParent child with a ProjectileMovement, spawned by Comp_Spawner.CompSpawnActor, so
//! `comp_spawn_projectile` gives it the launch, the bounce of its movement, the blast box and its place in Finn's
//! SpawnedActors. Sim space: x = -UE Y, y = UE Z; a direction handed to CompSpawnActor is in the spawner's facing
//! frame (Y forward: HandleActorPostSpawn mirrors it with the owner's facing, @5786 / @5890).
// Deviation (pooling): DestroySpawnedActor on this CanBePooled class is ReleaseToPool (OnReleasedToPool @221: gravity
// 1.5, LifeTimer cleared, IsTargeted false; ticks off), and a later spawn may take the pooled actor back (InitFromPool
// @2712: the timers set again, ResetTreasureLogic reopens the DoOnce). The port ends the coin and makes a new one. Left
// out with it: a PickUpDelay or ProjectileGravityDelay timer, or a DashPickup Delay, still pending on a released coin
// (the release clears neither: a pending DashPickup on a pooled coin would award again, as would AfterPickupGate's
// CheckOverlap at the pooled coin's place), and the blink timeline a release leaves where it was.

use super::state::{finn, finn_of, finn_w};
use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, set_gravity_scale};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::buffs::{BuffRef, remove_stack, spawn_buff};
use crate::combat::decay::DecayHit;
use crate::combat::{DataVec, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef, PopKind};
use crate::fighter::FighterId;
use crate::fighter::anim::montages::slot_name;
use crate::fighter::anim::{MontageSlot, active_instance, conds_ok, instance_section, notify_data, sample_boxes};
use crate::fighter::common::{latent_frames, warn_text};
use crate::fighter::control::pawn_prop;
use crate::fighter::traces::{OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_WORLD_DYNAMIC, sphere_overlap_actors};
use crate::fighter::{actor_y, facing, grounded, mesh_wx, mesh_wy, weight};
use crate::math::ue::{safe_normal2d, size2d};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::notifies::{hitframe_begin_with, hitframe_end};
use crate::registry::props::num;
use crate::registry::spawner_notifies::cond_list;
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;

/// The coin's class.
pub const TREASURE: &str = "FinnTreasure_C";

/// The wallet buff's class.
pub const TREASURE_BUFF: &str = "FinnTreasureBuff_C";

// The class's constants (FinnTreasure CDO; DeathBlinkWarning's length; the Sphere's radius, DetectPawn).
const SPHERE_R: Real = 50.85687; // Sphere.SphereRadius (CheckOverlap @2137 passes the same number)
const BLINK_LENGTH: Real = 3.0; // DeathBlinkWarning: TimelineLength 3 s, not looped
const BACKPACK: &str = "fml_un_C_BackPack_b"; // Mesh.GetSocketLocation @6781
const SWING: Name = name!("Finn_BackpackSwing_Montage");
const SWING_AIR: Name = name!("Finn_BackpackSwing_Air_Montage");
const FINAL_HIT: Name = name!("FinalHit");
const COIN_SOUND: Name = name!("sfx_cha_finn_backpack_hit_02_treasure");

/// `FinnTreasure extends Projectile` (FinnTreasure_C): what the class adds.
#[derive(State, Debug)]
pub struct FinnTreasure {
    /// IsTargeted: an aimer's lock on it (`targeted`).
    pub is_targeted: bool,
    /// DeathBlinkWarning's position (s) while it plays, -1 before and once PauseFade stopped it.
    pub fade: Real,
    /// The tick of its PlayFromStart (the timeline ticks from the next frame).
    pub fade_at: i32,
    /// AfterPickupGate ran: Sphere.SetGenerateOverlapEvents(true) @3674.
    pub pickup_open: bool,
    /// The DoOnce of the pickup @832 (Temp_bool_IsClosed).
    pub closed: bool,
    /// LifeTimer (FadeDelay -> StartFade): the tick it fires at, -1 for none (cleared or fired).
    pub life_at: i32,
    /// K2_PauseTimerHandle @1724 (`life_left`: the ticks it had left). K2_IsTimerActiveHandle is false for a paused one.
    pub life_paused: bool,
    pub life_left: i32,
    /// ProjectileGravityDelay's timer -> AfterProjectileGravityDelay @1946, -1 once fired.
    pub grav_at: i32,
    /// PickUpDelay's timer -> AfterPickupGate @3674, -1 once fired.
    pub pick_at: i32,
    /// The tick DashPickup's Delay(0.066) resumes at, -1 for none pending.
    pub dash_at: i32,
}

impl Default for FinnTreasure {
    /// The field initial values.
    fn default() -> Self {
        FinnTreasure {
            is_targeted: false,
            fade: -1.0,
            fade_at: -1,
            pickup_open: false,
            closed: false,
            life_at: -1,
            life_paused: false,
            life_left: 0,
            grav_at: -1,
            pick_at: -1,
            dash_at: -1,
        }
    }
}

/// `isTreasure`: the actor is a coin.
pub fn is_treasure(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => {
            w.actors.projectiles.get(id).is_some_and(|p| matches!(p.ext, ProjectileExt::FinnTreasure(_)))
        }
        _ => false,
    }
}

fn pr_ref(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pr_mut(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn coin(w: &World, id: ProjectileId) -> &FinnTreasure {
    match &pr_ref(w, id).ext {
        ProjectileExt::FinnTreasure(s) => s,
        _ => panic!("a Finn coin"),
    }
}

fn coin_mut(w: &mut World, id: ProjectileId) -> &mut FinnTreasure {
    match &mut pr_mut(w, id).ext {
        ProjectileExt::FinnTreasure(s) => s,
        _ => panic!("a Finn coin"),
    }
}

/// `dashDelay`: DashPickup's Delay(0.066) @1839 -> @15, in frames.
fn dash_delay() -> i32 {
    to_int32(latent_frames(0.066))
}

/// K2_IsTimerActiveHandle(LifeTimer).
fn life_active(c: &FinnTreasure) -> bool {
    c.life_at >= 0 && !c.life_paused
}

/// StartFade @1961: K2_ClearAndInvalidateTimerHandle(LifeTimer), DeathBlinkWarning.PlayFromStart @799.
fn start_fade(w: &mut World, id: ProjectileId) {
    let t = w.flow.tick;
    let c = coin_mut(w, id);
    c.life_at = -1;
    c.life_paused = false;
    c.fade = 0.0;
    c.fade_at = t;
}

/// TargetedMulticast(IsTargeted, Gravity) @1894 (TargetedServer @1913 calls it on the authority).
fn targeted_multicast(w: &mut World, id: ProjectileId, is_targeted: bool, gravity: Real) {
    let t = w.flow.tick;
    if is_targeted {
        let p = pr_mut(w, id);
        p.vx = 0.0; // ProjectileMovement.Velocity = 0 @1470
        p.vy = 0.0;
    }
    set_gravity_scale(pr_mut(w, id), gravity); // @1523
    coin_mut(w, id).is_targeted = is_targeted; // @1572
    // SkelMesh_FinnCoin.SetPlayRate(0.1) / SetPosition(0) @1591-1628: the coin's spin (the view's).
    if is_targeted {
        let c = coin_mut(w, id);
        if life_active(c) {
            c.life_left = c.life_at - t; // K2_PauseTimerHandle @1724
            c.life_paused = true;
        }
        // PauseFade @1744 -> @1986: DeathBlinkWarning.Stop, Alpha 1, the sparkle shown. LifeTimer is left as it is.
        c.fade = -1.0;
        return;
    }
    // @1759-1824: the untargeted branch. Neither caller reaches it (Targeted and Untargeted both pass true, @1391 /
    // @1412): kept as the graph has it. K2_UnPauseTimerHandle @1803 runs only on an active timer (a no-op).
    if life_active(coin(w, id)) {
        return;
    }
    start_fade(w, id);
}

/// Targeted @1391 (Finn_Dash_AimerExperiment's lock on the coin, aimer.rs): TargetedServer(true, 0.5).
pub fn targeted(w: &mut World, c: ProjectileId) {
    targeted_multicast(w, c, true, 0.5);
}

/// Untargeted @1412: TargetedServer(true, 4.0) (true, as the build has it: the coin stays targeted, so its overlap
/// pickup and its fade never come back), then SkelMesh_FinnCoin.SetPlayRate(1.0) @1432 (the view's).
pub fn untargeted(w: &mut World, c: ProjectileId) {
    targeted_multicast(w, c, true, 4.0);
}

/// DashPickup @1839: Delay(0.066) (a pending Delay ignores the call), then @15: unless the coin is being destroyed,
/// the owner's SpawnBuffActor(FinnTreasureBuff, 1 stack) and DestroySpawnedActor(false). No team, target or gate test.
/// A Delay made in frame k counts that frame (in the coin's tick or the leftover pass): it resumes in the coin's tick
/// of frame k + DASH_DELAY - 1.
pub fn dash_pickup(w: &mut World, c: ProjectileId) {
    let t = w.flow.tick;
    if pr_ref(w, c).dead || coin(w, c).dash_at >= t {
        return;
    }
    coin_mut(w, c).dash_at = t + dash_delay() - 1;
}

/// The award of @15-205 and @847-1013: the coin's owner's Comp_State_Manager SpawnBuffActor(FinnTreasureBuff_C,
/// Instigator owner, Owner owner, 1 stack, duration 0, BreakIfBlocked false); the caller then releases the coin.
fn award(w: &mut World, id: ProjectileId) {
    let Some(o) = pr_ref(w, id).owner else { return };
    spawn_buff(w, o, name!("FinnTreasureBuff_C"), 0.0, Some(1), Some(o), false, Some(o));
}

/// OnOverlappedActor @3097: a fighter (a Destructible, BPI_BaseFighter, not being destroyed) of the coin's team (the
/// coin's Destructible: its instigator's), the coin not targeted and not being destroyed -> the DoOnce @832 -> the
/// award. The stack goes to the coin's owner whoever of his team touched it. True: the coin is released.
fn overlapped(w: &mut World, id: ProjectileId, o: FighterId) -> bool {
    let owner_team = pr_ref(w, id).owner.map(|f| w.fighter(f).team.team);
    let c = coin(w, id);
    if pr_ref(w, id).dead || c.is_targeted || owner_team != Some(w.fighter(o).team.team) || c.closed {
        return false;
    }
    coin_mut(w, id).closed = true;
    award(w, id);
    true
}

/// The fighters a query of the coin's sphere meets, by the overlap's object types.
fn overlap_fighters(w: &mut World, id: ProjectileId, types: &[i32], responder: Real) -> Vec<FighterId> {
    let (x, y) = (pr_ref(w, id).x, pr_ref(w, id).y);
    let types: Vec<Real> = types.iter().map(|&t| crate::real(t)).collect();
    sphere_overlap_actors(w, x, y, SPHERE_R, &types, &[], responder)
        .into_iter()
        .filter_map(|a| if let ActorRef::Fighter(f) = a { Some(f) } else { None })
        .collect()
}

fn construct(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::FinnTreasure(FinnTreasure::default());
}

/// ReceiveBeginPlay @1107: the parent's (NumberOfActorsAllowed 8 @5864-6515: past the limit the oldest coin of the
/// owner's SpawnedActors goes), then on the authority the three timers: PickUpDelay (0.5) -> AfterPickupGate @708,
/// FadeDelay (3) -> StartFade as LifeTimer @1155, ProjectileGravityDelay (1) -> AfterProjectileGravityDelay @1263.
fn begin(w: &mut World, id: ProjectileId) {
    let t = w.flow.tick;
    let cp = class_p(w, pr_ref(w, id));
    let d = |k: &str| crate::combat::hit::knockback::timer_count(num(cp, k).unwrap_or(0.0));
    let (pick, life, grav) = (d("PickUpDelay"), d("FadeDelay"), d("ProjectileGravityDelay"));
    let c = coin_mut(w, id);
    c.pick_at = t + pick;
    c.life_at = t + life;
    c.grav_at = t + grav;
}

/// The coin's tick, behind its movement's. Deviation: its timers fire here in the frame the world's timer pass fires
/// them (a timer set in frame k: frame k + timerCount), ahead of that pass within the frame. Its DashPickup Delay
/// resumes here, its blink timeline advances here, and the Sphere's begin overlaps with a fighter's root capsule are
/// tested here (a fighter moving into the sphere raises it in his own move, earlier in the frame; the overlap is tested
/// as it stands at the coin's tick, every tick, which the DoOnce and the IsTargeted gate make the same as a begin
/// overlap). True: K2_DestroyActor (DeathBlinkWarning__FinishedFunc @3895, the award's release).
fn step(w: &mut World, id: ProjectileId) -> bool {
    let t = w.flow.tick;
    // DeathBlinkWarning's tick (from the frame after PlayFromStart).
    let c = coin_mut(w, id);
    if c.fade >= 0.0 && t > c.fade_at {
        c.fade += DELTA_SECONDS;
        if c.fade >= BLINK_LENGTH {
            c.fade = BLINK_LENGTH;
            return true;
        }
    }
    if t == c.pick_at {
        c.pick_at = -1;
        c.pickup_open = true;
        // CheckOverlap @2081: SphereOverlapActors(the location, 50.85687, [PhysicsBody, Pawn, WorldDynamic]) and
        // OnOverlappedActor for each (SetGenerateOverlapEvents' own begin overlaps meet the closed DoOnce).
        // Deviation: WorldDynamic left out (the traces gather no body of that type; of a fighter's bodies the root
        // capsule, Pawn, is the one the port has), so the query meets the same fighters.
        for o in overlap_fighters(w, id, &[OBJ_PHYSICS_BODY, OBJ_PAWN], -1.0) {
            if overlapped(w, id, o) {
                return true;
            }
        }
    }
    let c = coin_mut(w, id);
    if t == c.grav_at {
        c.grav_at = -1;
        if !c.is_targeted {
            set_gravity_scale(pr_mut(w, id), 4.0); // @1946 -> @1345
        }
    }
    let c = coin(w, id);
    if t == c.life_at && !c.life_paused {
        start_fade(w, id);
    }
    if t == coin(w, id).dash_at {
        coin_mut(w, id).dash_at = -1;
        award(w, id);
        return true;
    }
    if coin(w, id).pickup_open {
        // the Sphere's begin overlaps: DetectPawn (WorldDynamic) overlaps Pawn alone
        for o in overlap_fighters(w, id, &[OBJ_PAWN], crate::real(OBJ_WORLD_DYNAMIC)) {
            if overlapped(w, id, o) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------- SpawnTreasure

/// Calculate Treasure Knockback(Target, InputPin) (Pawn_Finn's function, @0-806): the knockback formula with the
/// target's Comp_Actor_Damage CurrentDamage and Weight and Finn's own KnockBackExpCurveMult.
fn treasure_knockback(w: &World, f: FighterId, vic: FighterId, hit: &HitPacket) -> Real {
    let dmg = hit.damage.unwrap_or(0.0);
    let cur = w.fighter(vic).stats.damage;
    let exp = pawn_prop(w, f, "KnockBackExpCurveMult").unwrap_or(0.0);
    let a = dmg / 10.0;
    let b = (dmg * cur) / 20.0;
    let e = ((cur * cur) * exp) + 1.0;
    let r = 200.0 / (weight(w, vic) + 100.0);
    let k = ((((a + b) * r) * 1.6) + 18.0) * hit.knock_back_scaling.unwrap_or(0.0);
    e * (k + hit.base_knock_back.unwrap_or(0.0))
}

/// UKismetMathLibrary::Normal2D (142f7acd0): zero unless the squared size is above 1e-8, else times InvSqrt.
fn normal_2d(x: Real, y: Real) -> (Real, Real) {
    let [x, y] = safe_normal2d([x, y]);
    (x, y)
}

/// Mesh.GetSocketLocation(BACKPACK) in the current pose, the actor's location when the socket is not sampled.
fn backpack(w: &World, f: FighterId) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let index = w
        .names
        .get(&format!("socket:{BACKPACK}"))
        .and_then(|n| w.fixed.samplers.get(&ch).and_then(|b| b.by_name.get(&n).map(|&i| (b, i))));
    let mut pose: Vec<f32> = Vec::new();
    if let Some((b, i)) = index
        && sample_boxes(w, f, &mut pose)
        && let Some(&o) = b.offsets.get(i as usize)
    {
        let o = o as usize;
        return (mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), mesh_wy(w, f) + crate::real(pose[o + 1]));
    }
    warn_text(w, "FinnTreasure_C: socket fml_un_C_BackPack_b not sampled (the actor location taken)");
    (w.fighter(f).body.x, actor_y(w, f))
}

/// GetHitStunDecay(self, HitBoxDamage, ComboResistanceEnabled) on Finn's own Comp_Pawn_DamageManager @4885: its
/// AttackDecayStacks.
// Deviation: the DecayHit takes Finn's current attack (AttackID as his attack count, the decay and combo names as the
// active montage's), the HitBoxDamage's own AttackID and AttackAnimation not being handed to the hook. The fighter's
// lastDecayStacks (the HUD's, of his last hit taken) is put back.
fn decay_stacks(w: &mut World, f: FighterId, hit: &HitPacket) -> Real {
    let dt = hit.attack_decay_type.map_or(0, |d| match d {
        crate::combat::DataEnum::Value(v) => v,
        crate::combat::DataEnum::Name(n) => w.names.str(n).parse().unwrap_or(0),
    });
    let slot = active_instance(w, f);
    let active = slot.and_then(|s| instance_section(w, f, s)).map(|(m, _)| m);
    let nm = active.unwrap_or_else(|| slot_name(w, f, MontageSlot::Main));
    let decay_name = if [1, 2, 3, 6].contains(&dt) { hit.attack_decay_name.unwrap_or(name!("None")) } else { nm };
    let h = DecayHit {
        attack_id: w.fighter(f).decay.attack_count,
        decay_type: dt,
        decay_name,
        combo_name: nm,
        base_knock_back: hit.base_knock_back.unwrap_or(0.0),
        launch_knock_back: None,
        additive_kb_mode: None,
        knock_back_scaling: None,
        hit_stun_mult: None,
    };
    let keep = w.fighter(f).attackers.last_decay_stacks;
    let enabled = w.flow.same_attack_penalty;
    crate::combat::hit::hit_stun_decay(w, f, Some(f), &h, enabled);
    let n = w.fighter(f).attackers.last_decay_stacks;
    w.fighter_mut(f).attackers.last_decay_stacks = keep;
    n
}

/// The single coin @1613-1806: CompSpawnActor(FinnTreasure_C, at TreasureSpawnLocation, direction (0, the direction
/// vector), InitialSpeed 'Treasure Knock Back' x 0.5, MaxSpeed 4000, AttackID 0: Finn's attack count at the spawn,
/// Comp_Spawner HandleActorPostSpawn @2444-2747; the same at @7202).
fn spawn_one(w: &mut World, f: FighterId) {
    let s = &finn(w, f).coins;
    let (x, y, dx, dy, kb) = (s.spawn_x, s.spawn_y, s.dir_x, s.dir_y, s.knock_back);
    crate::world::spawning::comp_spawn_projectile(
        w,
        f,
        name!("FinnTreasure_C"),
        x,
        y,
        [Some(0.0), Some(dx), Some(dy)],
        kb * 0.5,
        4000.0,
        None,
    );
}

/// The wallet's last live FinnTreasureBuff (`f.buffs.filter(...).pop()`): its ref and stacks.
fn live_wallet(w: &World, f: FighterId) -> Option<(BuffRef, Real)> {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .rev()
        .find(|x| x.class == crate::combat::BuffClass::FinnTreasureBuff && x.frames > 0.0)
        .map(|b| (BuffRef { fighter: f, serial: b.serial }, b.stacks))
}

/// The wallet's part @4606-7455 (SpawnTreasure's and BPI_Finn_DropCoins'): CheckHasBuff(FinnTreasureBuff_C).
fn wallet(w: &mut World, f: FighterId) {
    let over = finn(w, f).drop_coin_override;
    let Some((b, stacks)) = live_wallet(w, f) else {
        if !over {
            spawn_one(w, f); // @5047
        }
        return;
    };
    if stacks == 1.0 && !over {
        spawn_one(w, f); // @4866 -> @1613: the stack stays
        return;
    }
    // @5062-5679: TreasureLoss = clamp(DropCoinOverride ? min(DropCoinAmount - 1, stacks - 1) : stacks / 2, 0, 4),
    // RemoveStack(TreasureLoss + 1); then TreasureLoss + 1 coins (@5724-7455; the stacks read are CheckHasBuff's, from
    // before the RemoveStack).
    let amount = finn(w, f).drop_coin_amount;
    let lost = if over { min(amount - 1.0, stacks - 1.0) } else { (stacks / 2.0).trunc() };
    let n = min(4.0, max(0.0, lost));
    let n_i = to_int32(n);
    remove_stack(w, f, b, n_i + 1);
    let (sx, sy, kb, dir_y) = {
        let s = &finn(w, f).coins;
        (s.spawn_x, s.spawn_y, s.knock_back, s.dir_y)
    };
    let step = 2.0 / (n + 1.0);
    let id = w.fighter(f).id;
    for i in 0..=n_i {
        // @6158: RandomFloatInRange(-100, 250) (one draw of the world's stream: Min + (Max - Min) x FRand).
        let rnd = -100.0 + (350.0 * w.random_float());
        let speed = min(1500.0, max(if over { 850.0 } else { 0.0 }, kb * 0.5)) + rnd;
        let at = if over { backpack(w, f) } else { (sx, sy) };
        // (0, 1 - i x 2 / (TreasureLoss + 1), DropCoinOverride ? 1 : the vector's Y)
        let dir_z = 1.0 - (crate::real(i) * step);
        crate::world::spawning::comp_spawn_projectile(
            w,
            f,
            name!("FinnTreasure_C"),
            at.0,
            at.1,
            [Some(0.0), Some(dir_z), Some(if over { 1.0 } else { dir_y })],
            speed,
            4000.0,
            None,
        );
        // PF_Post_Event(sfx_cha_finn_backpack_hit_02_treasure, self) @7320, per coin.
        w.fighter_mut(f).events.push(Event::Sound {
            fighter: id,
            ev: COIN_SOUND,
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
    }
}

/// SpawnTreasure(HitBoxInfo, HitActor) @2381-7455 (pawn.rs hit_anything, the HitAnything bind).
pub fn spawn_treasure(w: &mut World, f: FighterId, victim: ActorRef, hit: &HitPacket, hit_box: bool) {
    // @2411-2592: a BPI_BaseFighter of another team.
    let ActorRef::Fighter(vic) = victim else { return };
    if w.fighter(vic).team.team == w.fighter(f).team.team {
        return;
    }
    let (vx, vy, vtime) = (w.fighter(vic).body.x, actor_y(w, vic), w.fighter(f).clock.time);
    let st = finn_w(w, f);
    st.last_hit_actor = Some(victim); // @2602
    st.last_hit_time = vtime; // @2650: GetGameTimeInSeconds (seconds of world time, float32)
    st.coins.spawn_x = vx; // @2824: the victim's actor location, UE X 0
    st.coins.spawn_y = vy;
    if !hit_box {
        return; // @2851-2980: a valid HitBox whose owner is Finn (the hook's hitBox)
    }
    // @2990-3285: 'Treasure Knock Back' = min(max(clamp(|velocity|, 0, 800) + 500, K), 3000).
    let k = treasure_knockback(w, f, vic, hit);
    let (bvx, bvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    let vel = min(800.0, max(0.0, size2d([bvx, bvy])));
    finn_w(w, f).coins.knock_back = min(max(vel + 500.0, k), 3000.0);
    // @3312-4211: the direction, from a second Calculate Treasure Knockback x 0.5 (not the clamped value): Normal2D(|X|,
    // |Y| + 0.2) x it, plus (min(|velocity Y|, grounded ? 0 : 1000), max(velocity Z, 0)), Normal2D.
    let half = treasure_knockback(w, f, vic, hit) * 0.5;
    let d = hit.knock_back_direction.or(hit.knock_back_direction_vector).unwrap_or(DataVec {
        x: Some(1.0),
        y: Some(0.0),
        z: None,
    });
    let (nx, ny) = normal_2d(d.x.unwrap_or(0.0).abs(), d.y.unwrap_or(0.0).abs() + 0.2);
    let (hx, hy) = ((nx * half), (ny * half));
    let (dx, dy) = normal_2d(hx + min(bvx.abs(), if grounded(w, f) { 0.0 } else { 1000.0 }), hy + max(bvy, 0.0));
    let s = &mut finn_w(w, f).coins;
    s.dir_x = dx;
    s.dir_y = dy;
    // @4238-4595: in the backpack swing (either montage) only its FinalHit section goes on, clearing DropCoinOverride,
    // to the wallet; any other attack: GetHitStunDecay's AttackDecayStacks >= 4 stops, else the single coin.
    let cur = active_instance(w, f).and_then(|s| instance_section(w, f, s));
    if let Some((m, sec)) = cur
        && (m == SWING || m == SWING_AIR)
    {
        if sec != FINAL_HIT {
            return;
        }
        finn_w(w, f).drop_coin_override = false;
        wallet(w, f);
        return;
    }
    if decay_stacks(w, f, hit) >= 4.0 {
        return;
    }
    spawn_one(w, f);
}

/// BPI_Finn_DropCoins(CoinAmount) @8786-8824 (pawn.rs's forward): DropCoinAmount, DropCoinOverride = true, the wallet.
/// The override holds until a FinalHit of the swing clears it (@4595).
pub fn drop_coins(w: &mut World, f: FighterId, amount: Real) {
    let st = finn_w(w, f);
    st.drop_coin_amount = amount;
    st.drop_coin_override = true;
    wallet(w, f);
}

/// The wallet: CheckHasBuff(FinnTreasureBuff) and its CurrentStackAmount (SpawnTreasure @4606, Finn_Shop's prices
/// @6888, FinnMoneyForGemCondition): the stacks of his last FinnTreasureBuff, 0 without one. What gem.rs and shop.rs
/// read.
pub fn wallet_stacks(w: &World, f: FighterId) -> Real {
    crate::combat::buffs::buff_stacks(w, f, TREASURE_BUFF)
}

// ---------------------------------------------------------------- entries

/// FinnTreasureBuff (CanStack, MaxStacks 100, no duration): BPI After Status Update Multicast @10 (not when
/// BuffRemoved): the pop text on the owner (stacks x 100, '000' at 0; gold for a gain over LastStackCount, else red;
/// the coin icon UI_Coin_03 over 10, UI_Coin_02 over 5, else UI_Coin_01; the collect sound, the silence at 0), then
/// LastStackCount = the new count @1028.
fn buff_status(
    w: &mut World,
    f: FighterId,
    _class: crate::combat::BuffClass,
    stacks: Real,
    _applied: bool,
    removed: bool,
) {
    if removed {
        return;
    }
    let last = finn_of(w, f).map_or(0.0, |s| s.coins.last_stack_count);
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: id,
        kind: PopKind::FinnTreasure,
        stacks,
        gain: Some(stacks > last),
        params: None,
    });
    if let Some(s) = crate::chars::finn::state::finn_mut(w, f) {
        s.coins.last_stack_count = stacks;
    }
}

/// A new buff actor starts at the class's LastStackCount 0.
fn buff_end(w: &mut World, f: FighterId, _b: BuffRef) {
    if let Some(s) = crate::chars::finn::state::finn_mut(w, f) {
        s.coins.last_stack_count = 0.0;
    }
}

/// AnimNotify_FinnBag_Hitframe (a notify state of its own, not a child of AnimNotify_Hitframe): Received_NotifyBegin
/// @0-2956: with a FinnTreasureBuff on the owner, n = clamp(stacks / 2, 0, 4): Damage + n x TeasureDamageAddPerStack,
/// BaseKnockBack + n x TeasureKBAddPerStack (@457-846; without the buff the plain values @3247), then HitBoxesOn with
/// the packet the Hitframe entry makes of the rest (RecentAttackID, the charge's hit pause, the display name as
/// AttackTag, AttackDecayType 0, HitEachActorOnlyOnce, KBI 1, no vacuum info) and Set Hit Attack Context.
fn bag_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let mut p = notify_data(w, &data, f, n).p.clone();
    if let Some((_, stacks)) = live_wallet(w, f) {
        let k = min(4.0, max(0.0, (stacks / 2.0).trunc()));
        let dmg = num(&p, "Damage").unwrap_or(0.0) + (k * num(&p, "TeasureDamageAddPerStack").unwrap_or(0.0));
        let kb = num(&p, "BaseKnockBack").unwrap_or(0.0) + (k * num(&p, "TeasureKBAddPerStack").unwrap_or(0.0));
        p.insert("Damage".into(), mvs_data::value::Value::Num(f64::from(dmg)));
        p.insert("BaseKnockBack".into(), mvs_data::value::Value::Num(f64::from(kb)));
    }
    hitframe_begin_with(w, f, n, idx, Some(p));
}

/// AnimNotify_FinnDropCoins (Received_NotifyBegin only): on the authority, with Conditions all true when it has any
/// (BPI_CheckConditions), the owner's BPI_Finn_DropCoins(NumberOfCoins) (a failed cast to BPI_Finn: nothing).
fn drop_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let conds = cond_list(w, p, "Conditions").unwrap_or_default();
    if !conds.is_empty() && !conds_ok(w, f, Some(&conds), false) {
        return;
    }
    if finn_of(w, f).is_none() {
        return;
    }
    drop_coins(w, f, num(p, "NumberOfCoins").unwrap_or(1.0));
}

/// This file's registry entries (FinnTreasure_C, FinnTreasureBuff_C, the two notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        TREASURE,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::FinnTreasure), ActorHooks::Projectile(kind)),
    )?;
    r.buffs.enter(
        TREASURE_BUFF,
        BuffEntry::new(
            crate::combat::BuffClass::FinnTreasureBuff,
            BuffHooks { status: Some(buff_status), end: Some(buff_end), ..BuffHooks::default() },
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_FinnBag_Hitframe_C",
        NotifyHandler { begin: Some(bag_begin), end: Some(hitframe_end), ..NotifyHandler::default() },
    )?;
    r.notifies
        .enter("AnimNotify_FinnDropCoins_C", NotifyHandler { begin: Some(drop_begin), ..NotifyHandler::default() })
}
