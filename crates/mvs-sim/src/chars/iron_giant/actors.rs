//! Iron Giant's actors: the eye laser, the bolts and Buff_C017_AllyBurst,
//! C017_Bomb, the missiles and the spit bolt, the cannon ball, the turret aim, the stomp wave, the down thruster, the
//! thrown scrap ([`ThrownScrap`]), the C017_ScrapCondition, the items Item_C017_Scrap / Item_C017_Art and his
//! pickups. The classes' state is actor_state.rs.

use super::actor_state::{
    AntenaBolts, Bomb, CannonBall, DownThruster, Expansion, Missile, StompHit, StompWave, TurretAim, bomb_expand,
    bomb_explode, socket_index, spawn_point, translation,
};
use crate::Real;
use crate::State;
use crate::actors::projectile::{MakeOpts, ProjectileExt, class_p, make_projectile, set_gravity_scale, set_hitbox};
use crate::actors::world_actor::{WorldActorState, actor_cls, actor_dead};
use crate::actors::{ActorClass, ActorId, ActorRef, ItemClass, ItemId, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::BuffRef;
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{BuffExt, buff_mut, buff_of, has_buff, spawn_buff, tick_buffs};
use crate::combat::defense::add_temp_health_pack;
use crate::combat::elem::{charge_pct, elem_tick, instigator_charge};
use crate::combat::hit::hurt::{box_hurt_hit, hurt_hit};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::holds::HoldHit;
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef, SoundKey};
use crate::fighter::anim::sample_boxes;
use crate::fighter::common::{actor_data, latent_frames, peers};
use crate::fighter::control::{attack_dodged, dodge_over, trigger_ability, trigger_ability_check, update_ability_ammo};
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::fighter::moves::{Impulse, V2};
use crate::fighter::{FighterId, actor_y, facing, grounded, in_hitstun, invulnerable, no_hit, percent_charged};
use crate::math::ue::{SMALL_NUMBER, safe_normal};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, ElementalParts, ItemHooks, PickupEntry, ProjectileHooks, SizeHooks,
    WorldActorHooks,
};
use crate::registry::props::{flag, name_of, num, obj, text};
use crate::registry::{Condition, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::spawn_spot;
use crate::world::spawning::{SpawnSpotA, SpawnSpotR};
use crate::world::spawning::{add_projectile, comp_spawn_item, request_props, spawned_moved};
use mvs_data::value::{Props, Value};
use std::sync::Arc;

use super::giant;

/// Buff_C017_AllyBurst's own variables (`AllyBurst`, the buff entry's extension).
#[derive(State, Debug, Default)]
pub struct AllyBurst {
    /// Set by the buff's begin: its extension is absent (null) before it.
    pub begun: bool,
    /// The actor's Instigator (SpawnBuffActor's argument: the Giant).
    pub by: Option<FighterId>,
    /// Latent ticks left of Delay(BurstDelay) @2057.
    pub wait: Real,
    /// StartDamage @1966.
    pub damage: Real,
    /// 'Start Base Knock Back' @2002.
    pub base_kb: Real,
}

/// `ThrownScrap extends Projectile` (C017_ThrownScrap_C).
#[derive(State, Debug, Default)]
pub struct ThrownScrap {
    /// SetMesh's index.
    pub mesh: i32,
    /// Either DoOnce.
    pub spent: bool,
}

const ALLY_BURST: &str = "Buff_C017_AllyBurst_C";

fn sound(w: &mut World, f: FighterId, ev: Name, at: Option<(Real, Real)>) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter,
        ev,
        x: at.map(|a| a.0),
        y: at.map(|a| a.1),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// `emitter`: the emitter's world rotation (UE Pitch, Yaw, Roll in degrees), where it is not zero.
#[allow(clippy::too_many_arguments, reason = "the helper's parameters")]
fn emitter(w: &mut World, cls: Name, f: FighterId, vfx: Name, x: Real, y: Real, scale: Real, rot: [Real; 3]) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot,
        scale,
        tint: None,
        to: None,
    });
}

/// The enemy fighters a hit of the actor can reach (CanBeHitBy: another team; a fighter out of play has no boxes).
fn enemies(w: &World, owner: FighterId) -> Vec<FighterId> {
    let team = w.fighter(owner).team.team;
    match_fighters(w)
        .into_iter()
        .filter(|&v| w.fighter(v).team.team != team && w.fighter(v).life.dead == 0.0 && !no_hit(w, v))
        .collect()
}

// ---- C017_AntenaBolts (the neutral special's bolts) ----

/// Pawn_C017 EyeLaser (@15603-16387), the Special press with no direction (@16845 -> @16432, behind
/// !Polymorphed && IntroOver && !isDead @16475 and !(IsMaxRage && IsWalking)): nothing with Buff_C017_Ally_Pilot or
/// at max rage on the ground out of hitstun; else TriggerAbilityCheck(0), TriggerAbilityActual(0) and the owner's
/// CompSpawnActor(C017_AntenaBolts_C, attached, socket fml_un_vr_spine3, identity transform). No weapon mode gate.
/// For the pawn's press handler (pawn.rs) to call; true when the bolts were asked for.
pub fn eye_laser(w: &mut World, f: FighterId) -> bool {
    if has_buff(w, f, name!("Buff_C017_Ally_Pilot_C"))
        || (giant(w, f).is_max_rage && grounded(w, f) && !in_hitstun(w, f))
    {
        return false;
    }
    if !trigger_ability_check(w, f, 0) {
        return false;
    }
    trigger_ability(w, f, 0);
    let mut p = Props::new();
    p.insert("AttachToActor".to_owned(), Value::Bool(true));
    p.insert("Socket".to_owned(), Value::Str("fml_un_vr_spine3".to_owned()));
    let r = SpawnRequest {
        cls: name!("C017_AntenaBolts_C"),
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: instigator_charge(w, f),
        aim: None,
        props: Some(Arc::new(p)),
        instigator: None,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
    true
}

fn bolts_loop(w: &World, f: FighterId) -> SoundKey {
    SoundKey { tag: name!(":screws"), n: w.fighter(f).id }
}

/// ReceiveBeginPlay (see [`AntenaBolts`]). After TriggerAbilityActual (@7665), @7715 -> @8711: GetAllActorsOfClass(
/// BP_FighterCharacter) and, for each one that is not the owner and has the owner's TeamIndex (@5751-6031), the
/// owner's Comp_State_Manager.SpawnBuffActor(Buff_C017_AllyBurst_C, that fighter, the owner as Instigator,
/// StartingBoltNumber + 1 stacks, Duration 0.0) @6213, whatever N and wherever the fighter is.
// Deviation: UpdateAbilityAmmo's own ammo cooldown start (@4380) is left to the TriggerAbilityActual that
// follows it; the fighters are taken in the sim's order and one waiting for its respawn is left out.
fn bolts_begin(w: &mut World, f: FighterId, ab: &mut AntenaBolts) {
    let n = ab.start;
    if n > 0.0 {
        sound(w, f, name!("sfx_cha_C017_special_neutral_screws_leadin"), None);
        let fighter = w.fighter(f).id;
        let ev = name!("sfx_cha_C017_special_neutral_screws_loop");
        let key = bolts_loop(w, f);
        w.events.push(Event::SoundStart { fighter, ev, key });
    }
    if let Some(s) = w.fighter_mut(f).abilities.abilities.get_mut(0) {
        // UpdateAbilityAmmo(0, -N, true) @7543: ui_ability_ammo (@4141) and AbilityAmmoUpdated (@4334), whatever N.
        s.ammo = max(s.ammo - n, 0.0);
        let ammo = s.ammo;
        crate::fighter::control::emit(w, f, |fighter| Event::Sound {
            fighter,
            ev: name!("ui_ability_ammo"),
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
        crate::fighter::control::emit(w, f, |fighter| Event::AbilityAmmo { fighter, slot: 0, ammo, increase: false });
    }
    trigger_ability(w, f, 0);
    let team = w.fighter(f).team.team;
    for o in match_fighters(w) {
        if o != f && w.fighter(o).life.dead == 0.0 && w.fighter(o).team.team == team {
            spawn_buff(w, o, name!("Buff_C017_AllyBurst_C"), 0.0, Some(to_int32(n + 1.0)), Some(f), true, None);
        }
    }
    if n <= 0.0 {
        return;
    }
    ab.bolts = n;
    ab.ring = true; // ElectricRing.SetVisibility(true) @8660
    ab.bound = true;
    ab.kb_seq = w.fighter(f).attackers.knocked_back_count;
    ab.buff = spawn_buff(w, f, name!("Buff_C017_Passive_C"), 0.0, Some(0), Some(f), true, None);
}

fn bolts_unbind(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let owner = a.owner;
    let WorldActorState::AntenaBolts(ab) = &mut a.state else { return };
    ab.bound = false;
    let Some(b) = ab.buff.take() else { return };
    if let Some(x) = buff_mut(w, b) {
        x.frames = 0.0; // K2_DestroyActor on ProtectiveBuff
    }
    tick_buffs(w, owner, true);
}

fn ab_of(w: &World, id: ActorId) -> Option<&AntenaBolts> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::AntenaBolts(s) => Some(s),
        _ => None,
    }
}

fn ab_mut(w: &mut World, id: ActorId) -> Option<&mut AntenaBolts> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::AntenaBolts(s) => Some(s),
        _ => None,
    }
}

/// The burst (@258): PS_C017_Burst (scale 1.75) at the actor; SphereOverlapActors(the actor's location, 500, Pawn,
/// BaseFighter, the owner ignored): each fighter of another team takes PushHitBox with Damage = N + 1 and
/// KnockBackDirectionVector = (SelectFloat(-1, 1, the actor's Y >= the target's Y), 1), IgnoreFacing: away from
/// the actor in the play plane. The HitEnemyPlayer delegate posts the hit sound.
fn bolts_burst(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let Some(ab) = ab_of(w, id) else { return };
    let (x, y, attack_id) = (ab.x, ab.y, ab.attack_id);
    let hit = ab.hit.clone();
    emitter(w, cls, f, name!("PS_C017_Burst"), x, y, 1.75, [0.0; 3]);
    let team = w.fighter(f).team.team;
    for vic in w.sphere_trace_multi(x, y, 500.0) {
        if w.fighter(vic).team.team == team || no_hit(w, vic) {
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
        // HitEnemyPlayer's sound: none on a failed hit (Light Projectile Armor).
        let vx = w.fighter(vic).body.x;
        let d = dealer(w, me);
        let charge = charge_pct(w, me);
        if !apply_projectile_hit(
            w,
            &d,
            vic,
            &hit,
            &[],
            V2 { x, y },
            Some(V2 { x: if x <= vx { 1.0 } else { -1.0 }, y: 1.0 }),
            &[],
            charge,
        ) {
            continue;
        }
        actor_hit_enemy(w, &d, vic);
        sound(w, f, name!("sfx_cha_C017_special_neutral_screws_hit"), Some((x, y)));
    }
}

fn bolts_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls, sc) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    // follow
    let Some(ab) = ab_of(w, id) else { return };
    let at = spawn_point(w, f, ab.socket, ab.rel_y, ab.rel_z);
    let (burst, death, end) = {
        let p = actor_data(w, &w.data.clone(), ActorRef::Fighter(f), cls).map(|d| d.p.clone()).unwrap_or_default();
        (AntenaBolts::burst_tick(&p), AntenaBolts::death_tick(), AntenaBolts::end_tick())
    };
    let ab = ab_mut(w, id).expect("the bolts");
    ab.age += 1;
    (ab.x, ab.y) = at;
    // RotatingMovement and Timeline_0 (see AntenaBolts.spin / alpha), for the renderer.
    let age = crate::real(ab.age);
    ab.spin = (if ab.facing < 0.0 { -180.0 } else { 180.0 }) * age * DELTA_SECONDS;
    ab.alpha = if age >= death {
        max(1.0 - ((age - death) * DELTA_SECONDS) / 0.25, 0.0)
    } else {
        min((age * DELTA_SECONDS) / 0.25, 1.0)
    };
    let (start, x, y, radius) = (ab.start, ab.x, ab.y, ab.radius);
    // SetBoltNumberMulticast's Delay(0.5) (@2195): the cast sounds, the tier by the bolt count.
    if start > 0.0 && age == latent_frames(0.5) {
        sound(w, f, name!("sfx_cha_C017_special_neutral_screws_cast_secondary_burst"), None);
        let tier = match to_int32(min(max(start, 1.0), 5.0)) {
            1 => name!("sfx_cha_C017_special_neutral_screws_cast_tier01"),
            2 => name!("sfx_cha_C017_special_neutral_screws_cast_tier02"),
            3 => name!("sfx_cha_C017_special_neutral_screws_cast_tier03"),
            4 => name!("sfx_cha_C017_special_neutral_screws_cast_tier04"),
            _ => name!("sfx_cha_C017_special_neutral_screws_cast_tier05"),
        };
        sound(w, f, tier, None);
    }
    if age == burst {
        bolts_burst(w, id);
    }
    // HitEvent (@11611), once per KnockedBack of the owner: SetBoltVisualsMulticast (the bolt's mesh hidden,
    // PS_TomJerryPoof00) and BoltNumber - 1; at 0 the delegate is unbound and ProtectiveBuff destroyed.
    loop {
        let kb = w.fighter(f).attackers.knocked_back_count;
        let ab = ab_mut(w, id).expect("the bolts");
        if !(ab.bound && ab.kb_seq != kb) {
            break;
        }
        ab.kb_seq += 1;
        if ab.bolts > 0.0 {
            // SetBoltVisualsMulticast(BoltNumber - 1) (@12271): the poof at that bolt's component, scale 0.5
            // (@12484-12534); ElectricRing hidden when the bolt's index - 1 < 0 (@9530-9608).
            let at = ab.bolt_at(ab.bolts - 1.0, sc);
            ab.bolts -= 1.0;
            if ab.bolts == 0.0 {
                ab.ring = false;
            }
            emitter(w, cls, f, name!("PS_TomJerryPoof00"), at.0, at.1, 0.5, [0.0; 3]);
        }
        if ab_of(w, id).expect("the bolts").bolts <= 0.0 {
            bolts_unbind(w, id);
        }
    }
    // AllyTempHealthSphere BeginOverlap (@8827): the pack IGBoltTempHealth, CanOverride, HealthAmount 3, Time 3
    // (@9360-9484). (The sphere at the root's scale.)
    let team = w.fighter(f).team.team;
    for o in w.sphere_trace_multi(x, y, radius * sc) {
        let oid = w.fighter(o).id;
        if o == f || w.fighter(o).team.team != team || ab_of(w, id).expect("the bolts").allies.contains(&oid) {
            continue;
        }
        ab_mut(w, id).expect("the bolts").allies.push(oid);
        add_temp_health_pack(w, o, name!("IGBoltTempHealth"), 3.0, true, 3.0);
    }
    if age == death {
        ab_mut(w, id).expect("the bolts").dying = true;
        sound(w, f, name!("sfx_cha_C017_special_neutral_screws_retract"), None);
        let fighter = w.fighter(f).id;
        let key = bolts_loop(w, f);
        w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
    }
    if age >= end {
        w.actors.actors.get_mut(id).expect("the bolts").dead = true;
    }
}

/// ReceiveEndPlay (@10345): ProtectiveBuff destroyed; the loop sound's stop.
fn bolts_end(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if !crate::world::flow::reset_pass(w) {
        // (not in the Lab reset's pass: an old pawn there)
        bolts_unbind(w, id);
    }
    let Some(ab) = ab_of(w, id) else { return };
    if !ab.dying && ab.start > 0.0 {
        let fighter = w.fighter(f).id;
        let key = bolts_loop(w, f);
        w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
    }
}

fn bolts_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let np = request_props(w, f, r);
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::AntenaBolts(AntenaBolts::default()), true);
    };
    let ammo = w.fighter(f).abilities.abilities.first().map_or(0.0, |s| s.ammo);
    let mut ab = AntenaBolts::new(w, f, r.attack_id, &def.p, &def.components, &np, ammo);
    bolts_begin(w, f, &mut ab);
    (WorldActorState::AntenaBolts(ab), false)
}

// ---- Buff_C017_AllyBurst (the buff the bolts put on the team's other fighters) ----

/// Buff_C017_AllyBurst: a child of BuffParentActor with BuffDuration 0 (no life timer), CanStack,
/// PreventStacksWhenReapplied (a second spawn on a fighter that has one adds nothing) and MaxStacks 6 (N + 1 for the
/// 5 bolts at most); its stacks never decay (no StackDecayRate).
/// ReceiveBeginPlay @1956, after the parent's: StartDamage = 'Hit Box Damage'.Damage (2) @1966, 'Start Base Knock
/// Back' = its BaseKnockBack (850) @2002, AddVisuals @2038, Delay(BurstDelay 1.0) @2057, and in the same call @2116:
/// with CurrentStackAmount > 1 the owner's Comp_Armor.AddTempHealthPack(C017_AllyBurst, CanOverride, HealthAmount =
/// (stacks - 1) + BaseTempHealthAdded (0: the class default leaves it out), Time 3.0) @2484.
/// After the delay @15: SphereOverlapActors(the owner's location, 400, object types [2, 8], class Actor, ignoring
/// the owner and the Instigator) @140; each actor whose Comp_Actor_Destructible.TeamIndex is not the Instigator's
/// (@561) takes the Instigator's Comp_Pawn_HitBoxHandler.pfgDoDamage(null, the actor, 'Hit Box Damage') @1440 with
/// Damage = StartDamage + stacks @1130, BaseKnockBack = stacks x 50 + 'Start Base Knock Back' @1176 and
/// KnockBackDirectionVector = (SelectFloat(1, -1, the actor's Y >= the owner's Y), 1) @924-1009; the rest is the
/// class's packet. Then K2_DestroyActor @1564, hit or not.
/// The hit is a fighter's own pfgDoDamage with a packet (holds.hold_hits): the Giant is the dealer. Its gates ahead
/// of DamageAndLaunch: an invulnerable target dodges (@4608 -> @7381-8035, BaseKnockBack > 0), and nothing is dealt
/// while the Giant is in hitstun (@5107-5257).
/// ReceiveEndPlay @3409, after the parent's: RemoveBuffVisuals @3265: PS_C017_Burst at the owner's location (scale
/// 1), however the buff ended.
// Unverified: the Delay is taken with the owner's buff tick, the first one counting the spawn frame; the hits are
// dealt after the frame's ticks and are lost when the Giant is waiting for his respawn; the overlap is the sphere
// against the fighters' root capsules in the play plane (object type 8 is not resolved; other actors with a
// Comp_Actor_Destructible in the sphere are not hit); the order of the overlapped actors is the sim's.
fn burst_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let cls = name!("Buff_C017_AllyBurst_C");
    let d = crate::fighter::hold_def(w, f, cls);
    let hit = obj(d, "Hit Box Damage");
    let wait = max(latent_frames(num(d, "BurstDelay").unwrap_or(0.0)) - 1.0, 1.0);
    let damage = hit.and_then(|h| num(h, "Damage")).unwrap_or(0.0);
    let base_kb = hit.and_then(|h| num(h, "BaseKnockBack")).unwrap_or(0.0);
    let extra = num(d, "BaseTempHealthAdded").unwrap_or(0.0);
    let tick = crate::real(w.flow.tick);
    let Some(x) = buff_mut(w, b) else { return };
    x.ext = BuffExt::AllyBurst(AllyBurst { begun: true, by: from, wait, damage, base_kb });
    x.born = Some(tick);
    let stacks = x.stacks;
    if stacks > 1.0 {
        add_temp_health_pack(w, f, name!("C017_AllyBurst"), stacks - 1.0 + extra, true, 3.0);
    }
}

fn burst_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let stacks = x.stacks;
    let BuffExt::AllyBurst(e) = &mut x.ext else { return false };
    e.wait -= 1.0;
    if e.wait > 0.0 {
        return false;
    }
    let (by, damage, base_kb) = (e.by, e.damage, e.base_kb);
    let Some(by) = by else { return true };
    let cls = name!("Buff_C017_AllyBurst_C");
    // the class defaults (Fighter.holdDef: any buff's)
    let packet = obj(crate::fighter::hold_def(w, f, cls), "Hit Box Damage").cloned().unwrap_or_default();
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let all = peers(w).to_vec();
    let by_team = w.fighter(by).team.team;
    for vic in crate::world::capsules_in_sphere(w, &all, fx, fy, 400.0) {
        if vic == f || vic == by || w.fighter(vic).team.team == by_team || no_hit(w, vic) {
            continue;
        }
        let bkb = stacks * 50.0 + base_kb;
        // The Instigator's handler (its DodgingFighters), AttackID 0; Fighter.tick is the world's frame count.
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, ActorRef::Fighter(by), vic, 0, bkb, tick, Some(by), false);
            continue;
        }
        dodge_over(w, ActorRef::Fighter(by), vic);
        if in_hitstun(w, by) {
            continue;
        }
        // sim x = -UE Y: the target's Y >= the owner's Y is vic.x <= f.x
        let mut p = packet.clone();
        p.insert("Damage".to_owned(), Value::Num(f64::from(damage + stacks)));
        p.insert("BaseKnockBack".to_owned(), Value::Num(f64::from(bkb)));
        let mut d = Props::new();
        d.insert("X".to_owned(), Value::Num(if w.fighter(vic).body.x <= fx { 1.0 } else { -1.0 }));
        d.insert("Y".to_owned(), Value::Num(1.0));
        p.insert("KnockBackDirectionVector".to_owned(), Value::Obj(d));
        w.fighter_mut(by).holds.hold_hits.push(HoldHit { vic, p });
    }
    true
}

fn burst_end(w: &mut World, f: FighterId, _b: BuffRef) {
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let (cls, vfx) = (name!("Buff_C017_AllyBurst_C"), Some(name!("PS_C017_Burst")));
    crate::fighter::control::emit(w, f, |fighter| Event::ProjectileDeath {
        cls,
        fighter,
        vfx,
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
}

// ---- C017_Bomb ----

fn bomb_overlaps(w: &mut World, id: ProjectileId) -> Vec<FighterId> {
    let Some(b) = w.actors.projectiles.get(id) else { return Vec::new() };
    let (owner, x, y, r, s3) = (b.owner, b.x, b.y, b.hit_radius, b.sphere_3d);
    let Some(owner) = owner else { return Vec::new() };
    let mut out = Vec::new();
    for vic in enemies(w, owner) {
        let mut vp = Vec::new();
        if !sample_boxes(w, vic, &mut vp) {
            continue;
        }
        let sphere = WorldSphere { x, y, z: if s3 { Some(0.0) } else { None }, r, ..WorldSphere::default() };
        if hurt_hit(w, vic, &[sphere], &vp).is_some() {
            out.push(vic);
        }
    }
    out
}

fn bomb_data(w: &World, id: ProjectileId) -> (Real, Real, Real) {
    let b = w.actors.projectiles.get(id).expect("a live bomb");
    let p = class_p(w, b);
    (
        num(p, "ExpansionTime").unwrap_or(0.0),
        num(p, "RepeatedDamageTime").unwrap_or(0.0),
        num(p, "HitStartScale").unwrap_or(1.0),
    )
}

fn bomb_start(w: &mut World, id: ProjectileId) {
    let (_, repeated, start) = bomb_data(w, id);
    let p = w.actors.projectiles.get_mut(id).expect("a live bomb");
    if bomb_explode(p, repeated, start) {
        let (owner, x, y) = (p.owner, p.x, p.y);
        if let Some(o) = owner {
            sound(w, o, name!("sfx_cha_C017_weaponmode_bomb_impact_expanding"), Some((x, y)));
        }
    }
}

/// TickDamage (@2563): TickHitDamage with KnockBackDirectionVector = (Y, Z) of GetDirectionUnitVector(the target's
/// location, the bomb's), IgnoreFacing: toward the bomb. EndExpantion (@2568): LastHitDamage on every overlapping
/// enemy, then K2_DestroyActor. Both pass no buffs.
// Unverified: the HitBox sphere is tested against the hurtboxes, as a projectile's hit.
fn bomb_step(w: &mut World, id: ProjectileId) -> bool {
    let me = ActorRef::Projectile(id);
    let (total, repeated, _) = bomb_data(w, id);
    let Some(p) = w.actors.projectiles.get_mut(id) else { return false };
    if !matches!(&p.ext, ProjectileExt::Bomb(b) if b.exploding) {
        return false;
    }
    let Some(what) = bomb_expand(p, total, repeated) else { return false };
    let (attack_id, owner) = (p.attack_id, p.owner);
    let Some(owner) = owner else { return false };
    let _ = owner;
    let packet_p = {
        let b = w.actors.projectiles.get(id).expect("a live bomb");
        let key = if what == Expansion::End { "LastHitDamage" } else { "TickHitDamage" };
        obj(class_p(w, b), key).cloned().unwrap_or_default()
    };
    let packet = HitPacket::from_props(&w.names, &packet_p, None);
    for vic in bomb_overlaps(w, id) {
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        let d = dealer(w, me);
        let charge = charge_pct(w, me);
        if what == Expansion::End {
            if apply_projectile_hit(w, &d, vic, &packet, &[], V2 { x: d.x, y: d.y }, None, &[], charge) {
                actor_hit_enemy(w, &d, vic);
            }
            continue;
        }
        let (dx, dy) = (d.x - w.fighter(vic).body.x, d.y - actor_y(w, vic));
        let n = safe_normal([0.0, dx, dy], SMALL_NUMBER); // GetDirectionUnitVector
        let dir = V2 { x: n[1], y: n[2] };
        if apply_projectile_hit(w, &d, vic, &packet, &[], V2 { x: d.x, y: d.y }, Some(dir), &[], charge) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    what == Expansion::End
}

fn bomb_construct(w: &World, p: &mut crate::actors::Projectile) {
    let comps = crate::actors::projectile::class_def(w, p).map(|d| d.components.clone()).unwrap_or_default();
    let b = Bomb::new(p, &comps);
    p.hit_radius = b.sphere_r * p.scale;
    p.ext = ProjectileExt::Bomb(b);
}

fn bomb_begin(w: &mut World, id: ProjectileId) {
    let p = w.actors.projectiles.get(id).expect("a live bomb");
    let (owner, x, y) = (p.owner, p.x, p.y);
    if let Some(o) = owner {
        sound(w, o, name!("sfx_cha_C017_weaponmode_bomb_cast"), Some((x, y)));
    }
}

/// Bounce (@3623, bound to ProjectileMovement.OnProjectileBounce in BeginPlay @3493-3516): StartExplosionMulticast,
/// inside the move. Its MaxSpeed 0 and Velocity 0 are the member HandleImpact then tests: under the stop threshold
/// the component stops simulating where the bomb hit, and the time of the tick left after the hit is not moved.
fn bomb_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    bomb_start(w, id);
    false
}

/// The HitEnemyPlayer override (@2558) and the bound delegate's hit sound (@3628).
fn bomb_hit_enemy(w: &mut World, id: ProjectileId, _vic: FighterId) {
    let p = w.actors.projectiles.get(id).expect("a live bomb");
    let (owner, x, y) = (p.owner, p.x, p.y);
    if let Some(o) = owner {
        sound(w, o, name!("sfx_cha_C017_weaponmode_bomb_hit"), Some((x, y)));
    }
    bomb_start(w, id);
}

// ---- WeaponModeMissile ----

/// WeaponModeMissile_C (ParentProjectilePhysics with its own graph; HitBoxOn? false at the spawn). ReceiveBeginPlay
/// (@606): FaceAtStart = the owner faces left, the Rotation timeline (the mesh), Delay(DurationToGravity 0.75) ->
/// (@384) MultiCastProjectileVelocity (@2510): HitBoxOn? = true and UpdateHitbox, ProjectileGravityScale = 0,
/// Velocity = (0, FaceAtStart ? 3500 : -3500, -2000) (forward and down; the movement's MaxSpeed limits it), and
/// after Delay(0.033) the launch sound. HitEnemyPlayer (@860): the parent's (DestroyOnHitFighter), the
/// ability_sandwich_hit line (probability 0.2), the hit sound.
fn missile_begin(w: &mut World, id: ProjectileId) {
    let Some(owner) = w.actors.projectiles.get(id).and_then(|p| p.owner) else { return };
    let left = facing(w, owner) < 0.0;
    if let Some(ProjectileExt::Missile(m)) = w.actors.projectiles.get_mut(id).map(|p| &mut p.ext) {
        m.face_at_start = left; // @715-761
    }
}

fn missile_step(w: &mut World, id: ProjectileId) -> bool {
    let p = w.actors.projectiles.get(id).expect("a live missile");
    let turn = latent_frames(num(class_p(w, p), "DurationToGravity").unwrap_or(0.0));
    let (age, owner, x, y) = (crate::real(p.age), p.owner, p.x, p.y);
    if age == turn {
        let p = w.actors.projectiles.get_mut(id).expect("a live missile");
        set_hitbox(p, true);
        set_gravity_scale(p, 0.0);
        let left = matches!(&p.ext, ProjectileExt::Missile(m) if m.face_at_start);
        p.vx = if left { -3500.0 } else { 3500.0 }; // @384: UE Y 3500 facing left (sim x = -UE Y)
        p.vy = -2000.0;
    }
    if age == turn + latent_frames(0.033)
        && let Some(o) = owner
    {
        sound(w, o, name!("sfx_cha_C017_weaponsmode_rockets_secondary_launch"), Some((x, y)));
    }
    false
}

fn missile_hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    let p = w.actors.projectiles.get(id).expect("a live missile");
    let (owner, x, y) = (p.owner, p.x, p.y);
    let Some(o) = owner else { return };
    crate::fighter::match_side::post_vo(w, o, name!("ability_sandwich_hit"), Some(vic), 0.2);
    sound(w, o, name!("sfx_cha_C017_weaponsmode_rockets_hit"), Some((x, y)));
}

// ---- C017_CannonBall ----

fn ball_of(w: &World, id: ActorId) -> Option<&CannonBall> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::CannonBall(s) => Some(s),
        _ => None,
    }
}

fn class_props(w: &World, f: FighterId, cls: Name) -> Props {
    actor_data(w, &w.data.clone(), ActorRef::Fighter(f), cls).map(|d| d.p.clone()).unwrap_or_default()
}

fn ball_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let p = class_props(w, f, r.cls);
    (WorldActorState::CannonBall(CannonBall::new(w, f, &p, &np)), false)
}

fn ball_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let p = class_props(w, f, cls);
    let hitstun = in_hitstun(w, f);
    let mx = w.fighter(f).controls.inp.x;
    let a = w.actors.actors.get_mut(id).expect("the ball");
    let WorldActorState::CannonBall(cb) = &mut a.state else { return };
    if hitstun {
        cb.age += 1;
        a.dead = true;
        return;
    }
    cb.step(mx, &p);
}

fn ball_activate(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let p = class_props(w, f, cls);
    let Some(cb) = ball_of(w, id) else { return };
    let (vx, vy) = cb.launch(w, a, &p);
    w.fighter_mut(f).moves.impulse = Some(Impulse { vx, vy, xy: true, kind: 0, z: true, d: None });
    w.actors.actors.get_mut(id).expect("the ball").dead = true;
}

// ---- C017_CannonAim ----

fn aim_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    if let Some(ga) = super::giant_mut(w, f).and_then(|g| g.giant_anim.as_mut()) {
        ga.aim_alpha2 = 0.0;
    }
    (WorldActorState::TurretAim(TurretAim { attack_id: r.attack_id, ..TurretAim::default() }), false)
}

fn aim_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let p = class_props(w, f, cls);
    let a = w.actors.actors.get_mut(id).expect("the aim");
    let WorldActorState::TurretAim(ta) = &mut a.state else { return };
    if let Some(o) = ta.step(&p)
        && let Some(ga) = super::giant_mut(w, f).and_then(|g| g.giant_anim.as_mut())
    {
        ga.aim_alpha2 = o;
    }
}

/// ActivateObject (see [`TurretAim`]): the direction is handed over in the owner's facing frame, not normalised, so
/// the launch speed is 5000 x its size (Comp_Spawner HandleActorPostSpawn, as every CompSpawnActor).
fn turret_fire(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if in_hitstun(w, f) {
        return;
    }
    let cls = name!("C017_CannonShot_C");
    let WorldActorState::TurretAim(ta) = &a.state else { return };
    let aim = ta.aim(&class_props(w, f, a.cls));
    w.actors.actors.get_mut(id).expect("the aim").dead = true;
    if actor_data(w, &w.data.clone(), ActorRef::Fighter(f), cls).is_none() {
        return;
    }
    // BPI_CheckTurretFacing (Pawn_C017: FacingLeft == TurretAimStartFacingLeft): Y is negated once the owner has
    // turned since the turret aim began.
    let y = if (facing(w, f) < 0.0) == giant(w, f).turret_aim_start_left { aim.0 } else { -aim.0 };
    let mut np = Props::new();
    np.insert("Socket".to_owned(), Value::Str("fml_un_R_wrist".to_owned()));
    let mut dir = Props::new();
    dir.insert("X".to_owned(), Value::Num(0.0));
    dir.insert("Y".to_owned(), Value::Num(f64::from(y)));
    dir.insert("Z".to_owned(), Value::Num(f64::from(aim.1)));
    np.insert("InitialMovementDirection".to_owned(), Value::Obj(dir));
    np.insert("InitialSpeed".to_owned(), Value::Num(5000.0));
    np.insert("MaxSpeed".to_owned(), Value::Num(5000.0));
    let attack = w.fighter(f).decay.attack_count;
    let pr = make_projectile(w, cls, ProjectileClass::C017CannonShot, Some(f), attack, &np, None, MakeOpts::default());
    let pid = w.actors.projectiles.insert(pr);
    add_projectile(w, pid, None);
}

// ---- C017_ShockwaveActor ----

fn sw_of(w: &World, id: ActorId) -> Option<&StompWave> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::StompWave(s) => Some(s),
        _ => None,
    }
}

fn stomp_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let np = request_props(w, f, r);
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::StompWave(StompWave::default()), true);
    };
    let (rel_y, rel_z) = translation(&np);
    let socket = socket_index(w, f, text(&np, "Socket").unwrap_or(""));
    let (x, y) = spawn_point(w, f, socket, rel_y, rel_z);
    let ground = w.stage.ground;
    let mut sw =
        StompWave::new(r.attack_id, facing(w, f), percent_charged(w, f), x, y, &def.p, &def.components, ground);
    // PostActorConstruction's FindTeleportSpot (spawn_spot), ahead of BeginPlay
    let at = spawn_spot(
        w,
        &SpawnSpotR { cls: r.cls, p: np.clone() },
        &SpawnSpotA { components: def.components.clone() },
        sw.x,
        sw.y,
    );
    sw.x = at.x;
    sw.y = at.y;
    (WorldActorState::StompWave(sw), false)
}

/// shockwave_hit on every actor overlapping the wave's sphere (collision_sphere.GetOverlappingActors(null) @1203) but
/// the Instigator (@125), whatever its team, then Spawn Shockwave Visuals (the wave's ParticleSystem component at the
/// ImpactPoint) and Shockwave Audio (AK_Rock_Explosion). The actors that are no fighter: as ShaggyShockwaveActor's
/// ([`crate::chars::shaggy::shockwave::wave_actor_hits`]).
/// No fighter is among them. The Shockwave<n> spheres are object type HitBox (ECC_GameTraceChannel3) with a Custom
/// profile whose ResponseArray leaves HurtBox out (the channel's default Ignore, DefaultEngine.ini), so no HurtBox
/// capsule; BaseFighter's construction script (@655 -> @1639) sets the root capsule to Ignore HitBox on a pawn with
/// HurtBox capsules, as every pawn of the roster has; the mesh has its overlap events off. So the stomp's damage on a
/// fighter is its own Hitframe's.
/// First the overlap update of the move that came before (BeginPlay's K2_AddActorWorldOffset @2128, spawn_shockwave's
/// K2_SetWorldLocation @890): an enemy ProjectileBarrier destroys the wave ahead of the GetOverlappingActors @1203.
// Unverified: read from the data alone, not seen in the live game; no later hit from a wave destroyed that way.
fn stomp_hits(w: &mut World, id: ActorId, hits: &[StompHit]) {
    let me = ActorRef::Actor(id);
    spawned_moved(w, me);
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let (f, cls, rot) = (a.owner, a.cls, sw_of(w, id).map_or([0.0; 3], |s| s.rot));
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), cls) else { return };
    for h in hits {
        crate::chars::shaggy::shockwave::wave_actor_hits(
            w,
            me,
            &format!("Shockwave{}", h.n),
            h.x,
            h.y,
            h.r,
            &h.hit,
            &[],
        );
        let comp = format!("ParticleSystem{}", if h.n == 3 { 4 } else { h.n });
        // Spawn Shockwave Visuals moves the wave's own ParticleSystem component: it keeps the actor's rotation.
        if let Some(ps) = obj(&def.components, &comp).and_then(|c| text(c, "Template")).and_then(|t| w.names.get(t)) {
            emitter(w, cls, f, ps, h.x, h.y, 1.0, rot);
        }
        if flag(&def.p, "AK_Rock_Explosion")
            && let Some(ev) = name_of(w, &def.p, "AK_Rock_Explosion")
        {
            let fighter = w.fighter(f).id;
            w.events.push(Event::Sound {
                fighter,
                ev,
                x: Some(h.x),
                y: Some(h.y),
                hit: None,
                pooled: false,
                key: None,
            });
        }
    }
}

fn stomp_advance(w: &mut World, id: ActorId, first: bool) -> Vec<StompHit> {
    let boxes = w.blocker_shapes(crate::world::spawned_platform::BlockKind::Trace);
    let Some(a) = w.actors.actors.get(id) else { return Vec::new() };
    let (f, cls) = (a.owner, a.cls);
    let p = class_props(w, f, cls);
    let a = w.actors.actors.get_mut(id).expect("the wave");
    let k = a.size_scale.unwrap_or(1.0);
    let WorldActorState::StompWave(sw) = &mut a.state else { return Vec::new() };
    if first { sw.begin(boxes.view(), k, &p) } else { sw.step(&mut a.dead, boxes.view(), k, &p) }
}

fn stomp_begin_play(w: &mut World, id: ActorId) {
    let hits = stomp_advance(w, id, true);
    stomp_hits(w, id, &hits);
}

fn stomp_step(w: &mut World, id: ActorId) {
    let hits = stomp_advance(w, id, false);
    stomp_hits(w, id, &hits);
}

fn stomp_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    let Some(a) = w.actors.actors.get(id) else { return false };
    let Some(sw) = sw_of(w, id) else { return false };
    if a.dead {
        return false;
    }
    sw.hit_shapes(a.size_scale.unwrap_or(1.0), out);
    true
}

// ---- C017_DownThrusterAttack ----

fn th_of(w: &World, id: ActorId) -> Option<&DownThruster> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::DownThruster(s) => Some(s),
        _ => None,
    }
}

fn th_mut(w: &mut World, id: ActorId) -> Option<&mut DownThruster> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::DownThruster(s) => Some(s),
        _ => None,
    }
}

fn thruster_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let np = request_props(w, f, r);
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::DownThruster(DownThruster::default()), true);
    };
    let hit = HitPacket::from_props(&w.names, obj(&def.p, "HitBoxDamage").unwrap_or(&Props::new()), None);
    (
        WorldActorState::DownThruster(DownThruster::new(r.attack_id, facing(w, f), hit, &np, &def.p, &def.components)),
        false,
    )
}

/// The class's answers to BPI_Elemental_System and its pooling, for its passive Buff_Elemental_Fire (2 stacks):
/// BPI_ShouldActorOverlap true, BPI_GetBoxCollision = LeftFlame (C017_DownThrusterAttack), CanBePooled from the class
/// defaults (true). In play (combat/elem.rs): each hit that goes through brings the victim's Buff_Fire to 2 stacks
/// (ElementalHitEnemy @8429-9432), and every 0.5 s from the spawn each enemy fighter in the flames takes one more
/// stack (ApplyFireOverTime @1647).
// Deviation: the fire timer's fighters are those in the flame boxes (`inFlame`'s test), not
// GetOverlappingActors(BaseFighter) of the whole actor: the enemies, and the fighters of the Giant's team (himself
// among them), who only lose 20 stacks of Buff_Ice there (@3443; looked for only while one has it). A pooled
// thruster's Buff_Elemental_Fire and its looping timer outlive ReleaseToPool (SpawnedActorParent @6423,
// DestroySpawnedActor @261; ReinitPassiveBuffsFromPool @1459 sets the stacks again), so from the second use on the
// 0.5 s is counted from the first spawn's BeginPlay in the build; here each spawn starts its own. A HitTimer hit and
// a fire tick that fall in one tick are taken in this order (the timer manager's is by expiry time).
///
/// DamageActor (@5529). pfgDoDamage's HitEnemyPlayer broadcast (@22770, once the hit went through: nothing on an
/// invulnerable victim, @4608) reaches the elemental's ElementalHitEnemy.
fn thruster_damage(w: &mut World, id: ActorId, vic: FighterId) {
    let me = ActorRef::Actor(id);
    if projectile_blocks(w, &dealer(w, me), vic) {
        return;
    }
    let Some(th) = th_of(w, id) else { return };
    let (hit, attack_id) = (th.hit.clone(), th.attack_id);
    if invulnerable(w, vic) {
        let tick = w.flow.tick;
        attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
        return;
    }
    dodge_over(w, me, vic);
    let d = dealer(w, me);
    let charge = charge_pct(w, me);
    if apply_projectile_hit(w, &d, vic, &hit, &[], V2 { x: d.x, y: d.y }, None, &[], charge) {
        actor_hit_enemy(w, &d, vic); // HitEnemyPlayer only on a hit: its elemental and its granted buffs
    }
}

fn in_boxes(w: &mut World, vic: FighterId, boxes: &[super::actor_state::FlameBox]) -> bool {
    let mut vp = Vec::new();
    sample_boxes(w, vic, &mut vp) && boxes.iter().any(|b| box_hurt_hit(w, vic, b.x, b.y, b.hx, b.hy, &vp))
}

fn thruster_step(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls, k) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    let p = class_props(w, f, cls);
    let grounds = crate::world::stage_query::static_shapes(
        w,
        crate::consts::SOLID_NOPLATFORM,
        name!("C017_DownThrusterAttack TraceFlame"),
    );
    let owner = (w.fighter(f).body.x, actor_y(w, f));
    let alive = th_mut(w, id).expect("the thruster").step(owner, grounds.view(), k, &p);
    if !alive {
        w.actors.actors.get_mut(id).expect("the thruster").dead = true;
        return;
    }
    // TraceFlame, the left foot's call (@2289) then the right's (@3887): SetRTPCValue(cha_C017_attack_thruster_length)
    // on the owner, FlameLength - the trace's distance over 50 ms on a hit (@1515), 0 at once on a miss (@4559).
    // Each foot's hit then broadcasts ThrusterHitGround(ImpactPoint, IsLeft) (@3828 left, @5484 right; TraceFlame
    // @3621: the floor's top under the foot), whose one binder is Buff_C017_ScorchedEarth (perks.rs).
    let flame = th_of(w, id).expect("the thruster").flame.clone();
    let len = num(&p, "FlameLength").unwrap_or(0.0);
    for (i, &r) in flame.iter().enumerate() {
        let fighter = w.fighter(f).id;
        w.events.push(Event::Rtpc {
            fighter,
            name: name!("cha_C017_attack_thruster_length"),
            value: if r < 0.0 { 0.0 } else { r },
            interp_ms: if r < 0.0 { 0.0 } else { 50.0 },
        });
        if r >= 0.0 {
            let th = th_of(w, id).expect("the thruster");
            let fx = th.foot_x(i == 0, k, &p);
            let top = th.foot_top(grounds.view(), fx, len).expect("a foot's floor");
            super::perks::thruster_hit_ground(w, id, fx, top, i == 0);
        }
    }
    let boxes = th_of(w, id).expect("the thruster").boxes.clone();
    let mut now = Vec::new();
    for vic in enemies(w, f) {
        if in_boxes(w, vic, &boxes) {
            now.push(vic);
        }
    }
    let due = th_mut(w, id).expect("the thruster").timer_due(&p);
    if now.is_empty() {
        th_mut(w, id).expect("the thruster").timer_on = false;
    } else if due {
        for &vic in &now {
            thruster_damage(w, id, vic);
        }
    }
    for &vic in &now {
        let vid = w.fighter(vic).id;
        let th = th_mut(w, id).expect("the thruster");
        if th.in_flame.contains(&vid) {
            continue;
        }
        if !th.timer_on {
            th.start_timer(&p);
        }
        thruster_damage(w, id, vic);
    }
    let ids: Vec<i32> = now.iter().map(|&v| w.fighter(v).id).collect();
    th_mut(w, id).expect("the thruster").in_flame = ids;
    // Buff_Elemental_Fire's looping ApplyFireOverTime
    let mut fire = now;
    let team = w.fighter(f).team.team;
    for o in match_fighters(w) {
        if w.fighter(o).team.team != team
            || w.fighter(o).life.dead != 0.0
            || buff_of(w, o, name!("Buff_Ice_C")).is_none()
        {
            continue;
        }
        if in_boxes(w, o, &boxes) {
            fire.push(o);
        }
    }
    elem_tick(w, me, &fire, &[]);
}

fn thruster_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    let Some(a) = w.actors.actors.get(id) else { return false };
    let Some(th) = th_of(w, id) else { return false };
    if a.dead {
        return false;
    }
    th.hit_shapes(out);
    true
}

// ---- the scrap and the art: C017_ThrownScrap, Item_C017_Scrap, Item_C017_Art, C017_ScrapCondition ----

/// `isGiant`: DoesImplementInterface(BPI_C017).
fn is_giant(w: &World, f: FighterId) -> bool {
    w.names.str(w.fighter(f).ch) == super::KEY
}

/// C017_ScrapCondition Check (@57-637): the owner's Comp_Spawner.SpawnedActors is walked to its end and the class of
/// the last element read: true unless it is Item_C017_Scrap. With an empty list Array_Get(0) leaves a null actor,
/// whose class is not the scrap's: true.
// Unverified: the empty list's null is KismetArrayLibrary::GenericArray_Get of the 4.26 source, and World lists only
// the projectiles, registered actors and items in Fighter.spawnedActors.
pub fn scrap_condition(w: &mut World, f: FighterId) -> bool {
    let mut last = None;
    for &s in &w.fighter(f).items.spawned_actors {
        if !actor_dead(w, s) {
            last = Some(s);
        }
    }
    last.and_then(|s| actor_cls(w, s)) != Some(name!("Item_C017_Scrap_C"))
}

/// C017_ThrownScrap (ParentProjectilePhysics; ProjectileMovement bShouldBounce, gravity scale 5).
/// ReceiveBeginPlay (@1596 -> @165, authority): SetMesh(RandomIntegerInRange(0, 1)) on the Sphere mesh component,
/// 0 C017_Scrap_Base, 1 C017_Scrap_Switch (@1616).
/// OnBounce (@227, bound to OnProjectileBounce; a DoOnce): the owner's CompSpawnActor(Item_C017_Scrap) at
/// ImpactResult.Location (world transform) with direction Normal(ImpactVelocity x (1, -1, 1)) and InitialSpeed =
/// MaxSpeed = 0.25 x VSize(ImpactVelocity) (@771), then K2_DestroyActor (@893). The spawner mirrors the direction
/// by its owner's facing as for every spawn, so the scrap keeps the shot's side with the Giant facing right and
/// takes the other with him facing left.
/// HitEnemyPlayer (@933, its own DoOnce): the same spawn at the event's Actor's location (@1034) with direction
/// SelectVector((0, -0.5, 1), (0, 0.5, 1), that Actor's Y >= the scrap's Y) (@1338, @1376) and speed 250 (@1439),
/// then K2_DestroyActor (@1553; the class has DestroyOnHitFighter as well) and the parent's HitEnemyPlayer. The
/// Actor is the scrap itself (Comp_Pawn_HitBoxHandler @22770 broadcasts HitEnemyPlayer(HitActor, GetOwner())), so
/// the spawn is at the scrap and the test compares its Y with itself: always (0, -0.5, 1).
/// SFX_Multicast (@1794, SFX_Bounce) has no caller in the build; the bounce event is heard as 'Ak Death Location'.
// Deviation: the mesh is the actor number's parity instead of the build's unseeded random.
fn scrap_begin(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get_mut(id) else { return };
    let seq = p.seq;
    if let ProjectileExt::ThrownScrap(s) = &mut p.ext {
        s.mesh = seq & 1;
    }
}

fn scrap_spend(w: &mut World, id: ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get_mut(id) else { return false };
    let ProjectileExt::ThrownScrap(s) = &mut p.ext else { return false };
    if s.spent {
        return false;
    }
    s.spent = true;
    true
}

/// OnBounce (@1611, bound to ProjectileMovement.OnProjectileBounce in BeginPlay @71-94), inside the move: the spawn at
/// ImpactResult.Location (the capsule at the hit) and K2_DestroyActor @893, which ends the move there.
fn scrap_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, ivx: Real, ivy: Real) -> bool {
    if !scrap_spend(w, id) {
        return false;
    }
    let p = w.actors.projectiles.get(id).expect("a live scrap");
    let (Some(owner), x, y) = (p.owner, p.x, p.y) else { return false };
    // sim x = -UE Y: Normal(V x (1, -1, 1)).Y = sim vx / |V|, then x the owner's facing (Comp_Spawner 5890).
    let face = facing(w, owner);
    let charge = instigator_charge(w, owner);
    comp_spawn_item(w, name!("Item_C017_Scrap_C"), owner, x, y, 0.25 * ivx * face, 0.25 * ivy, false, charge);
    true
}

fn scrap_hit_enemy(w: &mut World, id: ProjectileId, _vic: FighterId) {
    if !scrap_spend(w, id) {
        return;
    }
    let p = w.actors.projectiles.get(id).expect("a live scrap");
    let (Some(owner), x, y) = (p.owner, p.x, p.y) else { return };
    let face = facing(w, owner);
    let charge = instigator_charge(w, owner);
    comp_spawn_item(w, name!("Item_C017_Scrap_C"), owner, x, y, -0.5 * 250.0 * face, 250.0, false, charge);
}

/// Item_C017_Scrap (ItemActorParent; UseOnPickup, RestrictedToTeam, ItemIdleLife 8, NumberOfActorsAllowed 3).
/// ReceiveBeginPlay (@71) only turns ScrapMesh; MagnetTo_C017 (@86) is empty.
/// ActivateOnPickup (@115): nothing for an Instigator with BPI_C017 (@187: the Giant's own ItemPickUpEvent eats
/// it). Else (@192) the grabber's Comp_Armor pack of 'Temp Health Added'.TempHealthType (IG_Scrap):
/// FMin(MaxTempHelthFromScrap - its HealthAmount, the pack's HealthAmount) <= 0 -> the pack with CanOverride and
/// HealthAmount 15 (@502-625), else the pack with that amount (@773-990); then the Creator's
/// UpdateAbilityAmmo(0, 1, true) (@670) and K2_DestroyActor (@96).
/// The pack keeps its Time (3 s in the class defaults) on both paths.
fn scrap_picked_up(w: &mut World, id: ItemId, f: FighterId) -> bool {
    if is_giant(w, f) {
        return false;
    }
    let Some(p) = crate::world::items::item_def(w, id) else { return false };
    let max = num(p, "MaxTempHelthFromScrap").unwrap_or(0.0);
    let pack = obj(p, "Temp Health Added").cloned().unwrap_or_default();
    let kind = name_of(w, &pack, "TempHealthType").unwrap_or(Name::NONE);
    let have = w.fighter(f).stats.temp_health.iter().find(|pk| pk.kind == kind).map_or(0.0, |pk| pk.amount);
    let room = min(max - have, num(&pack, "HealthAmount").unwrap_or(0.0));
    let time = num(&pack, "Time").unwrap_or(0.0);
    if room <= 0.0 {
        add_temp_health_pack(w, f, kind, 15.0, true, time);
    } else {
        add_temp_health_pack(w, f, kind, room, flag(&pack, "CanOverride"), time);
    }
    if let Some(sp) = w.actors.items.get(id).and_then(|it| it.spawner) {
        update_ability_ammo(w, sp, 0, 1.0, true);
    }
    true
}

/// Item_C017_Art (ItemActorParent; UseOnPickup, RestrictedToTeam, ItemIdleLife 0, NumberOfActorsAllowed 1).
/// ReceiveBeginPlay (@254): UsableTimer (TimeToUpdate, UpdateTempHealth: an empty function); Delay(0.5) ->
/// Capsule.SetLinearDamping(1) (@15; the template's 100 holds the plant in the air until then); LineTraceSingle
/// from the actor 300 down on channel 3 (ProjectileGroundTrace), the owner ignored: no hit -> K2_DestroyActor
/// (@418-687). A polygon solid in the way is met by its own line trace.
/// ActivateOnPickup (@1087): an Instigator with BPI_C017 that is a Pawn_C017 -> Delay(0.4) -> its
/// UpdateAbilityAmmo(0, 1, false) and K2_DestroyActor (@57); any other with a Comp_Armor (@856) ->
/// AddTempHealthPack('Temp HealthOnUse'), the Creator's UpdateAbilityAmmo(0, 1, true) (@141), K2_DestroyActor.
// Unverified: a trace that starts inside a box counts as a hit.
fn art_begin(w: &mut World, id: ItemId) -> bool {
    let it = w.actors.items.get(id).expect("a live item");
    let (x, y) = (it.x, it.y);
    let under = |b: &mvs_collision::Ground| {
        x >= crate::math::narrow(b.left)
            && x <= crate::math::narrow(b.right)
            && y >= crate::math::narrow(b.bottom)
            && y - 300.0 <= crate::math::narrow(b.top)
    };
    let ground = w.stage.ground;
    let s = w.blocker_shapes(crate::world::spawned_platform::BlockKind::Trace);
    !(under(&ground) || s.boxes.iter().any(under) || s.polys.iter().any(|q| q.ray(x, y, x, y - 300.0).is_some()))
}

fn art_step(w: &mut World, id: ItemId) -> bool {
    let tick = w.flow.tick;
    let it = w.actors.items.get_mut(id).expect("a live item");
    if crate::real(it.age) == latent_frames(0.5) {
        it.lin_damp = 1.0;
    }
    if it.act_at < 0 || tick < it.act_at {
        return false;
    }
    if let Some(o) = it.owner {
        update_ability_ammo(w, o, 0, 1.0, false);
    }
    true
}

fn art_picked_up(w: &mut World, id: ItemId, f: FighterId) -> bool {
    if is_giant(w, f) {
        let tick = w.flow.tick;
        w.actors.items.get_mut(id).expect("a live item").act_at = tick + to_int32(latent_frames(0.4));
        return false;
    }
    let Some(p) = crate::world::items::item_def(w, id) else { return false };
    let pack = obj(p, "Temp HealthOnUse").cloned().unwrap_or_default();
    let kind = name_of(w, &pack, "TempHealthType").unwrap_or(Name::NONE);
    add_temp_health_pack(
        w,
        f,
        kind,
        num(&pack, "HealthAmount").unwrap_or(0.0),
        flag(&pack, "CanOverride"),
        num(&pack, "Time").unwrap_or(0.0),
    );
    if let Some(sp) = w.actors.items.get(id).and_then(|it| it.spawner) {
        update_ability_ammo(w, sp, 0, 1.0, true);
    }
    true
}

/// Pawn_C017 ItemPickUpEvent (@25071, bound to Comp_ItemManager.OnItemPickedUp; any ItemActorParent): with a valid
/// CurrentEatItem (@25214) that one and the new item are destroyed and UpdateAbilityAmmo(0, 1, true) runs at once
/// (@1959); the new item becomes CurrentEatItem and RetriggerableDelay(0.5) (@25016) ends in: CurrentEatItem, if
/// still valid, destroyed and UpdateAbilityAmmo(0, 1, true) (@1873). CurrentEatItem is the live item with pawnOf.
// Unverified: the delay's end is taken latentFrames(0.5) ticks after the pickup, before that tick's pickups; the
// eaten item stays where it was picked up (the hold socket is not followed without a hold pack).
fn eat(w: &mut World, it: ItemId) {
    crate::world::items::destroy_item(w, it);
    if let Some(i) = w.actors.items.get_mut(it) {
        i.pawn_of = None;
    }
}

fn items_of(w: &World) -> Vec<ItemId> {
    w.actors.items.iter().map(|(id, _)| id).collect()
}

fn pickup_picked_up(w: &mut World, f: FighterId, it: ItemId) {
    if w.actors.items.get(it).is_none_or(|i| i.dead) {
        return;
    }
    let cur = items_of(w)
        .into_iter()
        .find(|&o| o != it && w.actors.items.get(o).is_some_and(|i| i.pawn_of == Some(f) && !i.dead));
    if let Some(cur) = cur {
        eat(w, cur);
        crate::world::items::destroy_item(w, it);
        update_ability_ammo(w, f, 0, 1.0, true);
        return;
    }
    let tick = w.flow.tick;
    let i = w.actors.items.get_mut(it).expect("a live item");
    i.pawn_of = Some(f);
    i.pawn_at = tick + to_int32(latent_frames(0.5));
}

fn pickup_tick(w: &mut World, f: FighterId) {
    for it in items_of(w) {
        let tick = w.flow.tick;
        let Some(i) = w.actors.items.get(it) else { continue };
        if i.pawn_of != Some(f) || i.dead || tick < i.pawn_at {
            continue;
        }
        eat(w, it);
        update_ability_ammo(w, f, 0, 1.0, true);
    }
}

/// This file's registry entries (the actor kinds of actors.rs, Buff_C017_AllyBurst_C, C017_ScrapCondition, the two
/// items and the pickups), in module order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let world = |class: WorldActorClass, hooks: WorldActorHooks| {
        ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
    };
    r.actors.enter(
        "C017_AntenaBolts_C",
        world(
            WorldActorClass::C017AntenaBolts,
            WorldActorHooks {
                size: Some(SizeHooks::default()),
                spawn: Some(bolts_spawn),
                step: Some(bolts_step),
                end: Some(bolts_end),
                ..WorldActorHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        ALLY_BURST,
        BuffEntry::new(
            crate::combat::BuffClass::BuffC017AllyBurst,
            BuffHooks {
                begin: Some(burst_begin),
                tick: Some(burst_tick),
                end: Some(burst_end),
                ..BuffHooks::default()
            },
        ),
    )?;
    let proj = |class: ProjectileClass, hooks: ProjectileHooks| {
        ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(hooks))
    };
    r.actors.enter(
        "C017_Bomb_C",
        proj(
            ProjectileClass::C017Bomb,
            ProjectileHooks {
                construct: Some(bomb_construct),
                begin: Some(bomb_begin),
                step: Some(bomb_step),
                bounce_impact: Some(bomb_bounce),
                hit_enemy: Some(bomb_hit_enemy),
                ..ProjectileHooks::default()
            },
        ),
    )?;
    r.actors.enter(
        "WeaponModeMissile_C",
        proj(
            ProjectileClass::WeaponModeMissile,
            ProjectileHooks {
                make: Some(|_w, _f, _r| ProjectileExt::Missile(Missile::default())),
                begin: Some(missile_begin),
                step: Some(missile_step),
                hit_enemy: Some(missile_hit_enemy),
                ..ProjectileHooks::default()
            },
        ),
    )?;
    // A ParentProjectilePhysics child with no function or graph of its own (components and defaults only).
    r.actors.enter("C017_Spit_Bolt_C", proj(ProjectileClass::C017SpitBolt, ProjectileHooks::default()))?;
    r.actors.enter(
        "C017_CannonBall_C",
        world(
            WorldActorClass::C017CannonBall,
            WorldActorHooks {
                size: Some(SizeHooks::default()),
                spawn: Some(ball_spawn),
                step: Some(ball_step),
                activate: Some(ball_activate),
                ..WorldActorHooks::default()
            },
        ),
    )?;
    r.actors.enter(
        "C017_CannonAim_C",
        world(
            WorldActorClass::C017CannonAim,
            WorldActorHooks {
                size: Some(SizeHooks::default()),
                spawn: Some(aim_spawn),
                step: Some(aim_step),
                activate: Some(turret_fire),
                ..WorldActorHooks::default()
            },
        ),
    )?;
    r.actors.enter(
        "C017_ShockwaveActor_C",
        world(
            WorldActorClass::C017ShockwaveActor,
            WorldActorHooks {
                size: Some(SizeHooks::default()),
                hit_shapes: Some(stomp_shapes),
                spawn: Some(stomp_spawn),
                begin_play: Some(stomp_begin_play),
                step: Some(stomp_step),
                ..WorldActorHooks::default()
            },
        ),
    )?;
    r.actors.enter(
        "C017_DownThrusterAttack_C",
        ActorEntry {
            elemental: Some(ElementalParts { box_: true, overlap: true, ..ElementalParts::default() }),
            ..world(
                WorldActorClass::C017DownThrusterAttack,
                WorldActorHooks {
                    size: Some(SizeHooks::default()),
                    spawn: Some(thruster_spawn),
                    hit_shapes: Some(thruster_shapes),
                    step: Some(thruster_step),
                    ..WorldActorHooks::default()
                },
            )
        },
    )?;
    r.conditions.enter("C017_ScrapCondition_C", Condition::Fn(scrap_condition))?;
    r.actors.enter(
        "C017_ThrownScrap_C",
        proj(
            ProjectileClass::C017ThrownScrap,
            ProjectileHooks {
                make: Some(|_w, _f, _r| ProjectileExt::ThrownScrap(ThrownScrap::default())),
                begin: Some(scrap_begin),
                bounce_impact: Some(scrap_bounce),
                hit_enemy: Some(scrap_hit_enemy),
                ..ProjectileHooks::default()
            },
        ),
    )?;
    // foreignOwner (a copied C017_Charge_Scrap_Eat_Montage): the item's code reads the spawner's place, facing,
    // velocity, team, id, spawnedActors and ability slot 0, the grabber's key and Comp_Armor; no GiantState. The
    // Giant's own pickup hook below reads no Creator (@25071-25300), as the build.
    r.actors.enter(
        "Item_C017_Scrap_C",
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(
                ActorClass::Item(ItemClass::ItemC017Scrap),
                ActorHooks::Item(ItemHooks { picked_up: Some(scrap_picked_up), ..ItemHooks::default() }),
            )
        },
    )?;
    r.actors.enter(
        "Item_C017_Art_C",
        ActorEntry::new(
            ActorClass::Item(ItemClass::ItemC017Art),
            ActorHooks::Item(ItemHooks {
                begin: Some(art_begin),
                step: Some(art_step),
                picked_up: Some(art_picked_up),
                ..ItemHooks::default()
            }),
        ),
    )?;
    r.pickups.enter("irongiant", PickupEntry { picked_up: pickup_picked_up, tick: Some(pickup_tick) })?;
    Ok(())
}
