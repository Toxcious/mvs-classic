//! Finn's high five and kick: FinnHighFiveListener and FinnKickListener
//! ([`FinnListener`]), Finn_HighFive_Shockwave ([`HighFiveShockwave`], a DealerActor),
//! AnimNotify_FinnHighFive_Hitframe; joined to hitAnything, hitAlly and hitEnemy.
//!
//! The joined hooks: a listener's binds come at its BeginPlay, after Pawn_Finn's own, so the joined hooks run behind
//! pawn.rs's; each passes the call to the fighter's live listeners (the build's delegate holds only a live
//! listener's bind: a destroyed one's is gone). The listeners' state is their own (no FinnHighFiveState field); the
//! reads are FinnState.momentum (high_five, high_five_max, high_five_pct).
//! MoveCommand OnlyPerformOnGround on HighFive_Air is the shared AnimNotify_MoveCommand's Received_NotifyBegin gate
//! (IsMovingOnGround).
//!
//! A listener is a SpawnedActorParent child with no hit box of its own (Capsule only; the high five's Sphere: below),
//! attached to the owner (the Spawner's AttachToActor) and ended by the world at its notify's end
//! (DestroyAllActorsOnNotifyEnd), Die With Instigator, or the reset. Its graph acts through the binds alone; nothing
//! reads its place.
//!
//! FinnKickListener (Finn_F_Attack_Kick_Montage's Spawner, t 0.023 to 0.321, BranchingPoint).
//! - ReceiveBeginPlay @769: the owner a BP_FighterCharacter -> its Comp_Pawn_HitBoxHandler's HitEnemyPlayer bound to
//!   LaunchSelf (@968 / @991).
//! - LaunchSelf @1037 -> @581 (authority): TriggerAbilityCheck(0) @686 -> LaunchSelfClient @750 -> @1042 -> @294:
//!   TriggerAbilityActual(0) @365, BPI_IsFacingLeft @514, LaunchCharacter((LaunchDirection.X, IsFacingLeft ? Y : -Y,
//!   Z), true, true) @242: (0, 1800, 1400) is 1800 forward and 1400 up. Every enemy hit of Finn's own handler while
//!   it lives goes there (the kick's later hits find the ability on cooldown).
//!
//! FinnHighFiveListener (the high fives' Spawners: HighFive_Montage t 0.213, HighFive_Ground t 0.192,
//! HighFive_Ground_Run t 0.250, HighFive_Air t 0.250; Queued, attached, DestroyAllActorsOnNotifyEnd).
//! - ReceiveBeginPlay @3763: the owner a BP_FighterCharacter -> its handler's HitAnything bound to Hit (@3887 / @3981)
//!   and HitAlly to HitAllyEvent (@4022 / @4116); HighFiveCharge = the component's HighFivePercent @4228 (read again
//!   @805).
//! - Hit @4278 -> @3537 (authority): a HitActor that implements BPI_BaseFighter (@3567), of another TeamIndex than
//!   the listener's Comp_Actor_Destructible (@3666: the owner's team) -> @2759, the self launch.
//! - HitAllyEvent @5206: a BPI_BaseFighter ally -> OverlapAlly = it, @704 (the high five).
//! - @704 (authority): HighFiveCharge = HighFivePercent @805; the DoOnce @859; then, by the pushed flows: every
//!   active hit box's ComponentTags into CurrentTags and HitBoxesOff(CurrentTags) @986-3417; with HighFiveCharge >=
//!   0.95 @1286 the owner's Comp_Spawner.CompSpawnActor(Finn_HighFive_Shockwave_C, no attach, world transform at the
//!   middle of Finn and the ally + (0, 0, 120), rot 0) @1791; PopTextMulticast("High Five!!", 0.5 s, attached to
//!   Finn) @2399 (its body @5278: UI_Functions PopText and PF_Post_Event(sfx_cha_finn_highfive_hit) on Finn);
//!   SpawnBuffActor(Buff_Perk_SpeedUp_Medium_C, Finn, Finn, 0 stacks, 5.0) @2519 and the same on the ally's
//!   Comp_State_Manager with Finn as Instigator @2674; then @2759.
//! - @2759: BPI_IsFacingLeft, the owner's CharacterMovement IsMovingOnGround -> nothing; else LaunchCharacter(
//!   (LaunchDirection.X, IsFacingLeft ? Y : -Y, Z), true, true) @3282: (0, -400, 950) is 400 back and 950 up.
//! - The class's Sphere (radius 100) with BeginOverlap @5438: its template has NoCollision and no component tag, so
//!   SpawnedActorParent's UpdateHitbox (tagged components) never turns it on and the event never fires. Not ported.
// Deviation: the active hit boxes' ComponentTags are taken as the active Hitframes' HitBoxTags (the port keeps no box
// component; the RightHand box of the high five's Hitframe is the one on in every placement).
//
// Finn_HighFive_Shockwave: a SpawnedActorParent child, made by the listener's CompSpawnActor (no notify spawns it):
// rot 0, so its forward is UE X and a facing hit's X takes ComingFromLeft false; its HitBoxDamage (12 / 1650 / 14 /
// (-1, 0.5), HitReaction 2, KnockBackMirror, HitEachActorOnlyOnce) mirrors X for a fighter on the actor's left: the
// hit throws away from the middle of the pair.
// - Its Shockwave sphere (radius 450, ComponentTags HitBox; HitBoxOn? true): on from the spawn, tested at once
//   against the enemies as the cannon burst's.
// - ReceiveBeginPlay @584 -> @266: the parent's, then @30 PF_Post_Event(sfx_cha_finn_highfive_shockwave) on Finn,
//   Delay(0.2) @123 -> @178 Shockwave.K2_DestroyComponent (no more hits), Delay(1.0) @211 -> @15 K2_DestroyActor:
//   dead 1.2 s after the spawn. ApplyHitPauseToOwner: Finn takes the hit pause.
// Deviation: the sphere against the fighters' hurt shapes as a circle in the play plane; the Delays count from the
// spawn in the actor's step.
//
// AnimNotify_FinnHighFive_Hitframe: a notify state of its own (not an AnimNotify_Hitframe child); the packet goes
// through the shared Hitframe entry under the parent's property names. Received_NotifyBegin: with the owner's
// Comp_FinnMomentumCharge (@191) ratio = HighFiveCharge / HighFiveMaxCharge (no clamp), FinalDamage = Lerp(Damage,
// Damage x HighFiveChargeDamageMult, ratio) @434, FinalBaseKnockBack = Lerp(BaseKnockBack, BaseKnockBack x
// HighFiveChargeKB_Mult, ratio) @745; without it both stay 0 (@23 / @46). Then RecentAttackID, TotalChargedTime and
// HitBoxesOn(HitBoxTags, the packet, HitBoxScale, ElementalBuffs, no OnHitBuffs) @2917: Damage / BaseKnockBack the
// final ones, HitPauseTime = TotalChargedTime x ChargeHitPauseMult + HitPauseTime, AttackDecayType 0 / None,
// AdditiveKBMode = the notify's 'Additive KBMode', KBScalingStartingDmg 0, HitEachActorOnlyOnce,
// AccountForVelocityWithVacuumKnockback true, IgnoreKBI false, KBI 1, VacuumInfo the struct's defaults (no vacuum);
// the rest the notify's own. Received_NotifyEnd: with the component HighFiveCharge = 0 @229 and ResetHighFive @367,
// then HitBoxesOff(HitBoxTags) @632.

use indexmap::IndexSet;

use super::pawn::reset_high_five;
use super::state::{finn_mut, finn_of};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_signal, spawn_buff};
use crate::combat::elem::{charge_pct, instigator_charge};
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::combat::spawned_hits::class_rows;
use crate::events::{Event, NotifyRef, PopKind};
use crate::fighter::anim::montages::{hit_boxes_off_tags, notify_data};
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::control::{attack_dodged, dodge_over, trigger_ability, trigger_ability_check};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::{Impulse, V2};
use crate::fighter::{FighterId, actor_y, facing, grounded, invulnerable};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{arr, num, num_in, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use crate::world::spawning::{comp_spawn_actor, new_world_actor};
use mvs_data::value::{Props, Value};

const HIGH_FIVE_LISTENER: &str = "FinnHighFiveListener_C";
const HIGH_FIVE_SHOCKWAVE: &str = "Finn_HighFive_Shockwave_C";
const KICK_LISTENER: &str = "FinnKickListener_C";

/// `Listener` (FinnHighFiveListener_C, FinnKickListener_C).
#[derive(State, Debug, Default)]
pub struct FinnListener {
    /// The high five's DoOnce (@859-889: Temp_bool_Has_Been_Initd / IsClosed, one per listener actor).
    pub done: bool,
}

/// `HighFiveShockwave` (Finn_HighFive_Shockwave_C).
#[derive(State, Debug)]
pub struct HighFiveShockwave {
    pub hit: HitPacket,
    pub facing: Real,
    pub attack_id: i32,
    /// At scale 1 (the root's scale is taken at the read).
    pub r: Real,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    pub hit_on: bool,
    /// Frames to the first Delay's resume.
    pub left: Real,
    /// Frames to the second's.
    pub end: Real,
    pub x: Real,
    pub y: Real,
}

impl Default for HighFiveShockwave {
    /// The field initial values.
    fn default() -> Self {
        HighFiveShockwave {
            hit: HitPacket::default(),
            facing: -1.0,
            attack_id: 0,
            r: 0.0,
            hit_set: IndexSet::new(),
            hit_on: true,
            left: 0.0,
            end: 0.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

/// The Finn_HighFive_Shockwave_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &HighFiveShockwave) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The Finn_HighFive_Shockwave_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &HighFiveShockwave) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The Finn_HighFive_Shockwave_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &HighFiveShockwave) -> &HitPacket {
    &s.hit
}

fn sw_of(w: &World, id: ActorId) -> Option<&HighFiveShockwave> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::HighFiveShockwave(s) => Some(s),
        _ => None,
    }
}

fn sw_mut(w: &mut World, id: ActorId) -> Option<&mut HighFiveShockwave> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::HighFiveShockwave(s) => Some(s),
        _ => None,
    }
}

fn listener_mut(w: &mut World, id: ActorId) -> Option<&mut FinnListener> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::FinnListener(s) => Some(s),
        _ => None,
    }
}

/// The live listeners of class `cls` of `f`, in their spawn order.
fn live(w: &World, f: FighterId, cls: Name) -> Vec<ActorId> {
    w.fighter(f)
        .items
        .spawned_actors
        .iter()
        .filter_map(|a| match a {
            ActorRef::Actor(id) => Some(*id),
            _ => None,
        })
        .filter(|&id| {
            w.actors
                .actors
                .get(id)
                .is_some_and(|x| !x.dead && x.cls == cls && matches!(x.state, WorldActorState::FinnListener(_)))
        })
        .collect()
}

/// LaunchCharacter(v, true, true) on the owner: v's (X, SelectFloat(Y, -Y, IsFacingLeft), Z) (UE Y = sim -x), the
/// listener's `LaunchDirection`.
fn launch_self(w: &mut World, f: FighterId, l: ActorId) {
    let d = obj(actor_p(w, ActorRef::Actor(l)), "LaunchDirection");
    let (dy, dz) = (d.and_then(|d| num(d, "Y")).unwrap_or(0.0), d.and_then(|d| num(d, "Z")).unwrap_or(0.0));
    let y = if facing(w, f) < 0.0 { dy } else { -dy };
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse { vx: -y, vy: dz, xy: true, z: true, kind: 0, d: None });
    m.hit_launch = true; // a direct LaunchCharacter, as CancelJump's
}

// ---------------------------------------------------------------- the listeners

fn listener_spawn(_w: &mut World, _f: FighterId, _r: &SpawnRequest) -> (WorldActorState, bool) {
    (WorldActorState::FinnListener(FinnListener::default()), false)
}

/// The kick's LaunchSelf on every enemy hit of Finn's own handler while a kick listener lives.
fn kick_hit_enemy(w: &mut World, f: FighterId, _vic: FighterId) {
    for l in live(w, f, name!("FinnKickListener_C")) {
        if !trigger_ability_check(w, f, 0) {
            continue;
        }
        trigger_ability(w, f, 0);
        launch_self(w, f, l);
    }
}

/// high_five.rs's part of the joined hitAnything (after pawn.rs's): Hit @4278 -> @2759 for a fighter of another team.
pub fn hit_anything(w: &mut World, f: FighterId, a: ActorRef, hit: &HitPacket, boxed: bool) {
    let _ = (hit, boxed);
    let ActorRef::Fighter(victim) = a else { return };
    for l in live(w, f, name!("FinnHighFiveListener_C")) {
        if w.fighter(victim).team.team == w.fighter(f).team.team {
            continue;
        }
        self_launch(w, f, l);
    }
}

fn high_five_hit_ally(w: &mut World, f: FighterId, ally: FighterId) {
    for l in live(w, f, name!("FinnHighFiveListener_C")) {
        high_five(w, f, l, ally);
    }
}

/// @2759.
fn self_launch(w: &mut World, f: FighterId, l: ActorId) {
    if grounded(w, f) {
        return;
    }
    launch_self(w, f, l);
}

/// Comp_Pawn_HitBoxHandler.HitBoxesOff(tags) of the listener's call: the HitBoxesDeactivated broadcast @7889 ahead of
/// the deactivation, HasHitAFighter false @7250, the boxes of the tags off.
fn handler_hit_boxes_off(w: &mut World, f: FighterId, tags: &[String]) {
    if !w.fighter(f).buffs.buffs.is_empty() {
        buff_signal(w, f, BuffSignal::HitBoxesDeactivated, BuffSignalArg::default());
    }
    w.fighter_mut(f).decay.has_hit_a_fighter = false;
    let t: Vec<&str> = tags.iter().map(String::as_str).collect();
    hit_boxes_off_tags(w, f, &t, None, true);
}

/// @704.
fn high_five(w: &mut World, f: FighterId, l: ActorId, ally: FighterId) {
    let charge = finn_of(w, f).map_or(0.0, |s| s.momentum.high_five_pct); // HighFiveCharge @805
    let Some(s) = listener_mut(w, l) else { return };
    if s.done {
        return;
    }
    s.done = true;
    let mut tags: Vec<String> = Vec::new();
    let hits: Vec<Props> =
        w.fighter(f).montages.active_hits.values().map(|h| crate::combat::hit::hit_props(w, f, h)).collect();
    for p in &hits {
        for t in arr(p, "HitBoxTags").iter().filter_map(Value::as_str) {
            if !tags.iter().any(|x| x == t) {
                tags.push(t.to_owned());
            }
        }
    }
    handler_hit_boxes_off(w, f, &tags);
    if charge >= 0.95 {
        let (fx, ax) = (w.fighter(f).body.x, w.fighter(ally).body.x);
        let (fy, ay) = (actor_y(w, f), actor_y(w, ally));
        spawn_shockwave(w, f, (fx + ax) / 2.0, ((fy + ay) / 2.0) + 120.0);
    }
    let fid = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: fid,
        kind: PopKind::FinnHighFive,
        stacks: 0.0,
        gain: None,
        params: None,
    });
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_finn_highfive_hit"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
    let speed_up = name!("Buff_Perk_SpeedUp_Medium_C");
    spawn_buff(w, f, speed_up, 5.0, Some(0), Some(f), true, Some(f));
    spawn_buff(w, ally, speed_up, 5.0, Some(0), Some(f), true, Some(ally));
    self_launch(w, f, l);
}

// ---------------------------------------------------------------- Finn_HighFive_Shockwave

fn shockwave_step(w: &mut World, id: ActorId) {
    let Some(s) = sw_mut(w, id) else { return };
    if s.left > 0.0 {
        s.left -= 1.0;
        if s.left == 0.0 {
            s.hit_on = false; // @178
        }
        return;
    }
    s.end -= 1.0;
    if s.end <= 0.0
        && let Some(a) = w.actors.actors.get_mut(id)
    {
        a.dead = true; // @15
    }
}

/// ReceiveBeginPlay @584 -> @266 behind the parent's: @30 PF_Post_Event(sfx_cha_finn_highfive_shockwave) on Finn.
fn shockwave_begin_play(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let fid = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_finn_highfive_shockwave"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
}

fn shockwave_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, scale) = (a.owner, a.size_scale.unwrap_or(1.0));
    let Some(s) = sw_of(w, id) else { return };
    if !s.hit_on {
        return;
    }
    let (attack_id, hit, sx, sy, r) = (s.attack_id, s.hit.clone(), s.x, s.y, s.r);
    let shapes = [WorldSphere { x: sx, y: sy, r: r * scale, ..WorldSphere::default() }];
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = sw_of(w, id) else { return };
        if w.fighter(vic).team.team == team || !alive(w, vic) || no_hit(w, vic) || s.hit_set.contains(&vid) {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = hurt_hit(w, vic, &shapes, &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = sw_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: sx, y: sy }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// CompSpawnActor(Finn_HighFive_Shockwave_C) through Finn's Comp_Spawner at (x, y): the actor joins his SpawnedActors
/// (the charge copy and the granted buffs), its BeginPlay (the sound), then AfterSpawnedActor (the owner's
/// Buff_Enlarge binding and the buffs' bindings), then the hit box's first test at the spawn place. The call's
/// AttackID argument is 0: the spawn takes the owner's attack count.
fn spawn_shockwave(w: &mut World, f: FighterId, x: Real, y: Real) {
    let data = w.data.clone();
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(f), name!("Finn_HighFive_Shockwave_C")) else { return };
    let none = Props::new();
    let sphere = obj(&a.components, "Shockwave");
    let r = sphere.and_then(|s| num(s, "SphereRadius")).unwrap_or(32.0)
        * sphere.and_then(|s| num_in(s, "RelativeScale3D", "X")).unwrap_or(1.0)
        * num_in(&a.p, "HitBoxScale", "X").unwrap_or(1.0);
    let state = HighFiveShockwave {
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&none), None),
        attack_id: w.fighter(f).decay.attack_count,
        r,
        left: latent_frames(0.2),
        end: latent_frames(1.0),
        x,
        y,
        ..HighFiveShockwave::default()
    };
    let actor = new_world_actor(
        w,
        name!("Finn_HighFive_Shockwave_C"),
        WorldActorClass::FinnHighFiveShockwave,
        f,
        false,
        WorldActorState::HighFiveShockwave(state),
    );
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    comp_spawn_actor(w, id, charge, f);
    shockwave_hits(w, id);
}

// ---------------------------------------------------------------- AnimNotify_FinnHighFive_Hitframe

fn no_vacuum() -> Value {
    let mut v = Props::new();
    v.insert("UseVacuumKnockBack".into(), Value::Bool(false));
    v.insert("VacuumStrengthAlpha".into(), Value::Num(0.0));
    v.insert("VacuumTowardsInstigatingFighter".into(), Value::Bool(false));
    v.insert("VacuumTargetSocket".into(), Value::Str("None".into()));
    let mut o = Props::new();
    o.insert("X".into(), Value::Num(0.0));
    o.insert("Y".into(), Value::Num(0.0));
    v.insert("VacuumSocketOffset".into(), Value::Obj(o));
    v.insert("VacuumDuration".into(), Value::Num(0.0));
    v.insert("VacuumPause".into(), Value::Num(0.0));
    v.insert("ForceSmallHitEffects".into(), Value::Bool(false));
    v.insert("AccountForAttackerVelocity".into(), Value::Bool(true));
    Value::Obj(v)
}

fn high_five_packet(w: &World, f: FighterId, p: &Props) -> Props {
    let (mut dmg, mut kb) = (0.0, 0.0);
    if let Some(s) = finn_of(w, f) {
        let m = &s.momentum;
        // Divide_FloatFloat (0 by 0)
        let ratio = if m.high_five_max != 0.0 { m.high_five / m.high_five_max } else { 0.0 };
        let lerp = |a: Real, b: Real| a + (ratio * (b - a));
        let d = num(p, "Damage").unwrap_or(0.0);
        let k = num(p, "BaseKnockBack").unwrap_or(0.0);
        dmg = lerp(d, d * num(p, "HighFiveChargeDamageMult").unwrap_or(0.0));
        kb = lerp(k, k * num(p, "HighFiveChargeKB_Mult").unwrap_or(0.0));
    }
    let mut out = p.clone();
    out.insert("Damage".into(), Value::Num(f64::from(dmg)));
    out.insert("BaseKnockBack".into(), Value::Num(f64::from(kb)));
    out.insert("AttackDecayType".into(), Value::Num(0.0));
    out.insert("AttackDecayName".into(), Value::Str("None".into()));
    // `p['Additive KBMode'] ?? 0`: the notify's value as it stands (an enum name in the data, not a number).
    out.insert("AdditiveKBMode".into(), p.get("Additive KBMode").cloned().unwrap_or(Value::Num(0.0)));
    out.insert("KBScalingStartingDmg".into(), Value::Num(0.0));
    out.insert("HitEachActorOnlyOnce".into(), Value::Bool(true));
    out.insert("Account for Velocity with Vacuum Knockback".into(), Value::Bool(true));
    out.insert("IgnoreKBI".into(), Value::Bool(false));
    out.insert("KBI_Multiplier".into(), Value::Num(1.0));
    out.insert("VacuumInfo".into(), no_vacuum());
    out.shift_remove("OnHitBuffs");
    out
}

fn high_five_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let p = high_five_packet(w, f, &notify_data(w, &data, f, n).p);
    crate::registry::notifies::hitframe_begin_with(w, f, n, idx, Some(p));
}

fn high_five_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if let Some(s) = finn_mut(w, f) {
        s.momentum.high_five = 0.0; // @229
        reset_high_five(w, f); // @367
    }
    crate::registry::notifies::hitframe_end(w, f, n, idx);
}

/// The hooks this file joins in Finn's CharHooks (`hit_ally`, `hit_enemy`); `hit_anything` is composed in mod.rs.
pub fn hooks(h: &mut CharHooks) {
    h.hit_ally = Some(high_five_hit_ally);
    h.hit_enemy = Some(kick_hit_enemy);
}

/// This file's registry entries (the two listeners, Finn_HighFive_Shockwave_C, the notify), in registration order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // listeners: no shape of their own and nothing drawn
    let listener = |class: WorldActorClass| {
        ActorEntry::new(
            ActorClass::World(class),
            ActorHooks::World(WorldActorHooks {
                spawn: Some(listener_spawn),
                size: Some(SizeHooks::default()),
                ..WorldActorHooks::default()
            }),
        )
    };
    r.actors.enter(KICK_LISTENER, listener(WorldActorClass::FinnKickListener))?;
    r.actors.enter(HIGH_FIVE_LISTENER, listener(WorldActorClass::FinnHighFiveListener))?;
    r.actors.enter(
        HIGH_FIVE_SHOCKWAVE,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::FinnHighFiveShockwave),
            ActorHooks::World(WorldActorHooks {
                begin_play: Some(shockwave_begin_play),
                step: Some(shockwave_step),
                hits: Some(shockwave_hits),
                size: Some(SizeHooks::default()),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_FinnHighFive_Hitframe_C",
        NotifyHandler { begin: Some(high_five_begin), end: Some(high_five_end), ..NotifyHandler::default() },
    )
}
