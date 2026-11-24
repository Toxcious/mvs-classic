//! Stripe's bomb: Comp_C023B_Bomb (SetBomb, ActivateBomb, BombTimerEnd's
//! unattached branch -> BombExplotion (its attached branch is dormant: Comp_C023B_EnemyAttach's attach path is),
//! DropBomb @452 -> C023B_DroppedBomb, ExplosionLaunch, CreateBombExplosion's SphereOverlapActors, Buff_Fire @320,
//! TurnOffHitBoxes, BombMovementClense, ResetOnDeath), C023B_DroppedBomb ([`DroppedBomb`], an item that flies by
//! `actors::item_bounce`), the notifies AnimNotify_C023B_EquipBomb / _ActivateBomb / _BiteDamage_BombLaunch, the
//! conditions C023B_HasBomb_Condition / C023B_NoBomb_Condition; joined to timerTick, knockedBack, hitStunEnded,
//! hitEnemy and death. Its variables are `StripeState::bomb` (state.rs).
//!
//! Offsets are Comp_C023B_Bomb's (its function's, or ExecuteUbergraph_Comp_C023B_Bomb's for the custom events) unless
//! C023B_DroppedBomb is named. Sim space: x = -UE Y, y = UE Z; a KnockBackDirectionVector's X is UE Y.

use super::state::{StripeBombState, stripe, stripe_of, stripe_w};
use crate::Real;
use crate::State;
use crate::actors::item::{Item, ItemDef, ItemExt, num};
use crate::actors::item_bounce::Bounce;
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::buffs::{remove_buff, spawn_buff};
use crate::combat::hit::{remove_hitstun_from_knock_back, set_active_hit_props, stun_changed};
use crate::combat::hits::{direct_hit, refused_ally_hit};
use crate::combat::packet::HitPacket;
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::branches::clear_queued_attack;
use crate::fighter::anim::grouped::stop_anim_montage;
use crate::fighter::anim::montages::{hit_boxes_off_tags, new_active_hit, start_montage};
use crate::fighter::anim::slot_mix::set_sub_mesh;
use crate::fighter::common::{latent_frames, warn_text};
use crate::fighter::control::{actor_spawn_ability, attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::reset_jump_counts;
use crate::fighter::traces::{OBJ_DAMAGE_DESTRUCTIBLE, OBJ_PAWN, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, facing, in_air, in_hitstun, invulnerable};
use crate::math::ue::size2d;
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ItemHooks, NotifyHandler};
use crate::registry::props::{num as pnum, obj};
use crate::registry::{ActorEntry, ActorHooks, CharHooks, Condition, Registry, RegistryError};
use crate::world::World;
use indexmap::IndexSet;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::{Props, Value};
use std::sync::Arc;

/// The class.
pub const DROPPED_BOMB: &str = "C023B_DroppedBomb_C";
const EXPLOTION_MONTAGE: &str = "C023B_BombExplotion_Montage";
/// SetSubMeshVisibility(Bomb) @1673 / @1978.
const BOMB_MESH: &str = "Bomb";
/// Hit Box Damage's packet on the body box: the activeHits key of HitBoxesOn(Hit Box Tags) (below every notify's key
/// and every lingering Hitframe's; clearMontageState leaves it).
const BODY_KEY: i32 = -(1 << 30);

/// `DroppedBomb extends BouncingItem` (C023B_DroppedBomb_C).
#[derive(State, Debug, Default)]
pub struct DroppedBomb {
    /// The bouncing flight's state (`BouncingItem`).
    pub bounce: Bounce,
    /// Detonated (MulticastDetonation @4235).
    pub detonated: bool,
    /// DetonateTime (CDO 2; MulticastTimerValues @3980): the TimerEffect's TimerOffset 1 - it / 5.
    pub detonate_time: Real,
    /// BombDuration (@4007): the TimerEffect's Duration.
    pub bomb_duration: Real,
    /// Detonate_Timer (K2_SetTimerDelegate(Detonate, DetonateTime) @946): the World tick of the timer pass it fires in,
    /// -1 none; its set tick and Rate (remaining time, the PickUp @1487).
    pub detonate_at: i32,
    pub detonate_set: i32,
    pub detonate_rate: Real,
    /// Frames left of Detonate's Delay(0.1) @491 before DestroySpawnedActor, 0 none running.
    pub boom: i32,
    /// HitBoxTags = [ExplosionHitBox] (@436): the ExplosionHitBox sphere is the hit box.
    pub explosion_box: bool,
}

/// The component's class defaults (Comp_C023B_Bomb_GEN_VARIABLE).
fn comp(w: &World, f: FighterId) -> Props {
    crate::fighter::control::pawn_comp_in(&w.data, w, f, "Comp_C023B_Bomb_GEN_VARIABLE").cloned().unwrap_or_default()
}

fn bomb_time(w: &World, f: FighterId) -> Real {
    pnum(&comp(w, f), "BombTime").unwrap_or(5.0)
}

// ------------------------------------------------------------------ BombTimer

/// K2_SetTimer(self, BombTimerEnd, rate, no loop) (SetBomb @90 / @399) from the pawn's tick groups (a notify) or the
/// item's PickUp: pending, so it fires in the timer pass of tick + timerTicks(rate). A rate <= 0 sets nothing and
/// returns an invalid handle.
// Unverified: K2_SetTimer by function name finds the object's running timer for that function and replaces it (the
// dynamic-delegate form's rule, taken as the same for the name form).
fn set_bomb_timer(w: &mut World, f: FighterId, rate: Real) {
    if rate <= 0.0 {
        clear_bomb_timer(w, f);
        return;
    }
    let tick = w.fighter(f).clock.tick;
    let at = crate::combat::hit::knockback::timer_count(rate);
    let s = &mut stripe_w(w, f).bomb;
    s.timer_valid = true;
    s.timer_set = tick;
    s.timer_rate = rate;
    s.timer = tick + at;
    s.active = true;
}

/// K2_ClearAndInvalidateTimerHandle(BombTimer).
fn clear_bomb_timer(w: &mut World, f: FighterId) {
    let s = &mut stripe_w(w, f).bomb;
    s.timer_valid = false;
    s.timer = 0;
    s.active = false;
}

/// K2_GetTimerRemainingTimeHandle(BombTimer) (142b26730): 0 for an invalid handle, -1 once it fired, the Rate in the
/// frame it was set (pending), else the Rate less the deltas counted since the timer pass of that frame.
pub fn bomb_time_left(w: &World, f: FighterId, s: &StripeBombState) -> Real {
    if !s.timer_valid {
        return 0.0;
    }
    if s.timer == 0 {
        return -1.0;
    }
    let tick = w.fighter(f).clock.tick;
    if tick == s.timer_set {
        return s.timer_rate;
    }
    s.timer_rate - crate::real(tick - s.timer_set - 1) * DELTA_SECONDS
}

// ------------------------------------------------------------------ the component's functions

/// EquipBombVisuals(Equip, TimeAmount) @720: the pawn's BPI_C023B_SetBombValue(Equip) @819 (HasBomb); Equip: the
/// TimerEmitter (PS_Timer on fml_un_R_wrist, scale 2: Duration BombTime, TimerOffset 1 - TimeAmount / 5, Team the
/// owner's colour value), SetSubMeshVisibility(Bomb, true) @1673 and sfx_cha_C023b_bomb_timer on the owner @1753;
/// else the TimerEmitter destroyed when valid @1874 and SetSubMeshVisibility(Bomb, false) @1978 (PrintString "Bomb
/// off" @2038 is a development print).
// Deviation: Equip spawns a second TimerEmitter over a live one (the variable is overwritten, the old one plays on); the
// port keeps one (fuse_at / fuse_offset are the newest).
fn equip_bomb_visuals(w: &mut World, f: FighterId, equip: bool, time_amount: Real) {
    stripe_w(w, f).bomb.equipped = equip;
    if equip {
        let tick = w.fighter(f).clock.tick;
        let s = &mut stripe_w(w, f).bomb;
        s.fuse_offset = 1.0 - (time_amount / 5.0);
        s.fuse_at = tick;
        set_sub_mesh(w, f, name!("Bomb"), true);
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        own_sound(w, f, name!("sfx_cha_C023b_bomb_timer"), x, y);
    } else {
        stripe_w(w, f).bomb.fuse_at = -1;
        set_sub_mesh(w, f, name!("Bomb"), false);
    }
    let _ = BOMB_MESH;
}

/// SetBomb(FromItem, OverrideTime) @0, with authority: FromItem -> BombTimer = K2_SetTimer(OverrideTime) @90; else
/// K2_SetTimer(BombTime) @399 and the AbilityManager's TriggerAbilityServer(2) @560. Then EquipBombVisuals(true,
/// FromItem ? OverrideTime : BombTime) @235 and EventKnockedBack bound to the DamageManager's KnockedBack @353.
/// Callers: AnimNotify_C023B_EquipBomb (false, 0.0), C023B_DroppedBomb's PickUp (true, the time).
pub fn set_bomb(w: &mut World, f: FighterId, from_item: bool, override_time: Real) {
    if stripe_of(w, f).is_none() {
        return; // GetComponentByClass(Comp_C023B_Bomb) on another pawn: none
    }
    if from_item {
        set_bomb_timer(w, f, override_time);
    } else {
        let t = bomb_time(w, f);
        set_bomb_timer(w, f, t);
        crate::fighter::control::trigger_ability(w, f, 2);
    }
    let t = if from_item { override_time } else { bomb_time(w, f) };
    equip_bomb_visuals(w, f, true, t);
    stripe_w(w, f).bomb.knocked_bound = true;
}

/// ActivateBomb @0: with BombTimer active, the timer cleared and invalidated @44, BombTimerEnd @64, EventKnockedBack
/// unbound @172 (BombTimerEnd unbound it already). Caller: AnimNotify_C023B_ActivateBomb.
pub fn activate_bomb(w: &mut World, f: FighterId) {
    let Some(s) = stripe_of(w, f) else { return };
    if s.bomb.timer == 0 {
        return;
    }
    clear_bomb_timer(w, f);
    bomb_timer_end(w, f);
    stripe_w(w, f).bomb.knocked_bound = false;
}

/// BombTimerEnd @0 (the timer's fire, ActivateBomb): EventKnockedBack unbound @94, EquipBombVisuals(false) @135,
/// CreateBombExplosion @226, the owner's SpawnBuffActor(Buff_Fire_C, owner, owner, 3, 0.0, false, false) @320,
/// BombMovementClense @404, then with the pawn's BPI_C023B_CheckAttached false (always: the enemy attach is dormant)
/// MontageServer(C023B_BombExplotion_Montage) @598 -> MontageMulticast -> BP_FighterCharacter.PlayAnimMontage (@2165,
/// rate 1, section None).
/// ACharacter::PlayAnimMontage is a plain Montage_Play on the pawn's mesh, past the branch manager: the port's
/// start_montage with BeginAttack not set.
// Deviation: what else the port's start_montage does for an attack start rides along (the attack count, the charge
// values, the hit flag reset, the ground / air section by `grounded`: the montage has one section).
// Unverified: MontageServer / MontageMulticast are taken as running at once (the listen server's own calls).
fn bomb_timer_end(w: &mut World, f: FighterId) {
    stripe_w(w, f).bomb.knocked_bound = false;
    equip_bomb_visuals(w, f, false, 0.0);
    create_bomb_explosion(w, f);
    spawn_buff(w, f, name!("Buff_Fire_C"), 0.0, Some(3), Some(f), false, Some(f));
    bomb_movement_clense(w, f);
    start_montage(w, f, name!("C023B_BombExplotion_Montage"), &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
    let _ = EXPLOTION_MONTAGE;
}

/// CreateBombExplosion(HitBoxHandler) @0: ExplosionVisuals @5 (@419: not a dedicated server: ExplosionEffect
/// (PS_C023B_DynamiteExplosion) at the owner's location, rotation 0, scale 1, and sfx_cha_C023b_bomb_explode on the
/// owner), then SphereOverlapActors(the owner's location, 250, [Pawn, Destructible, DamageDestructible], no class, the
/// owner ignored) @137, and per actor whose Comp_Actor_Destructible TeamIndex differs from the owner's @566:
/// ExplosionDamage.KnockBackDirectionVector = (attached ? 1 : (owner's Y >= actor's Y ? -1 : 1), 1) @1281,
/// ExplosionDamage.IgnoreFacing = !attached @1317, HitBoxHandler.pfgDoDamage(null, the actor, ExplosionDamage, null,
/// [], On Hit Buffs Param) @1404 (the pawn's own handler: a hit of the fighter, its HitEnemyPlayer and OnHitBuffs).
/// Called by perks.rs (Buff_C023B_Dynamite's OnSelfBounce binding).
// Unverified: pfgDoDamage's gates on a fighter are taken as World.itemHits' are for the handler's owner: the dodge pop
// (@7381) for an invulnerable victim, then the hit (directHit: apply_hit with no hit box). DoLOSCheck (true in
// ExplosionDamage) is not tested (as direct_hit's other callers).
// pfgDoDamage @4922-5257: with no HitBox (no IgnoreHitStun tag) and IsWallBounce false, the call deals nothing while
// the owner's CheckHitstunActive is true (a hitstun or a buff stun source: `in_hitstun`), behind the dodge branch
// (@4608) and ahead of the hit. The fuse's end comes ahead of BombMovementClense's hitstun removal.
// Deviation: an actor that is not a fighter (Destructible is not gathered by the traces; a DamageDestructible body) is
// not hit (warned): no ported actor of the roster has one in reach of the query's types here.
// Deviation: Destructible (object type 5) is left out of the query: the traces gather no body of that type (the query
// would warn and meet nothing more).
pub fn create_bomb_explosion(w: &mut World, f: FighterId) {
    let p = comp(w, f);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let fighter = w.fighter(f).id;
    let vfx = p.get("ExplosionEffect").and_then(Value::as_str).unwrap_or("PS_C023B_DynamiteExplosion");
    let vfx = w.names.get(vfx).unwrap_or(name!("PS_C023B_DynamiteExplosion"));
    w.fighter_mut(f).events.push(Event::ProjectileDeath {
        cls: name!("Comp_C023B_Bomb_C"),
        fighter,
        vfx: Some(vfx),
        x: fx,
        y: fy,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    own_sound(w, f, name!("sfx_cha_C023b_bomb_explode"), fx, fy);
    let types = [crate::real(OBJ_PAWN), crate::real(OBJ_DAMAGE_DESTRUCTIBLE)];
    let found = sphere_overlap_actors(w, fx, fy, 250.0, &types, &[Some(ActorRef::Fighter(f))], -1.0);
    let dmg = obj(&p, "ExplosionDamage").cloned().unwrap_or_default();
    let rows = p.get("On Hit Buffs Param").cloned();
    for o in found {
        let ActorRef::Fighter(vic) = o else {
            warn_text(w, "stripe bomb: CreateBombExplosion met an actor that is no fighter (not hit)");
            continue;
        };
        if w.fighter(vic).team.team == w.fighter(f).team.team
            || w.fighter(vic).life.dead > 0.0
            || crate::combat::phases::no_hit_by(w, f, vic)
        {
            continue;
        }
        // @1103-1188: UE Y = -x, so the owner's Y >= the actor's is f.x <= vic.x.
        let dir_x = if fx <= w.fighter(vic).body.x { -1.0 } else { 1.0 };
        if invulnerable(w, vic) {
            let (id, tick) = (w.fighter(f).decay.attack_count, w.flow.tick);
            attack_dodged(
                w,
                ActorRef::Fighter(f),
                vic,
                id,
                pnum(&dmg, "BaseKnockBack").unwrap_or(0.0),
                tick,
                Some(f),
                false,
            );
            continue;
        }
        dodge_over(w, ActorRef::Fighter(f), vic);
        if in_hitstun(w, f) {
            continue;
        }
        let mut q = dmg.clone();
        let dir = dir_value(dir_x, 1.0);
        q.insert("KnockBackDirectionVector".into(), dir.clone());
        q.insert("KnockBackDirection".into(), dir);
        q.insert("IgnoreFacing".into(), Value::Bool(true));
        if let Some(v) = dmg.get("OnHitAudio") {
            q.insert("OnHitSound".into(), v.clone());
        }
        if let Some(v) = dmg.get("QueueHitPauseUntilPoses") {
            q.insert("QueueHitPauseUntilPose".into(), v.clone());
        }
        if let Some(r) = &rows {
            q.insert("OnHitBuffs".into(), r.clone());
        }
        let pk = HitPacket::from_props(&w.names, &q, None);
        direct_hit(w, f, vic, &pk);
    }
}

/// `{ X, Y }` as a data vector.
fn dir_value(x: Real, y: Real) -> Value {
    let mut o = Props::new();
    o.insert("X".into(), Value::Num(f64::from(x)));
    o.insert("Y".into(), Value::Num(f64::from(y)));
    Value::Obj(o)
}

/// BombMovementClense @0: the owner's Comp_Actor_Parent children that are BuffParentActors of StateInfo.Group 11
/// destroyed @670 (RemoveBuff's test by group: HELD_GROUP); then with the HitstunHandler's CheckHitstunActive @888:
/// RemoveAllHitStunSources @1014 (both source counters 0, OnIsHitStunned(false)) and RemoveHitstunFromKnockBack @1121;
/// then the BranchManager's ClearQueuedAttack @1229, StopAnimMontage(null) @1265 and
/// ConditionallyDispatchQueuedAnimEvents @1302 (nothing queued outside an anim update: a no-op here).
// Unverified: CheckHitstunActive is taken as `in_hitstun`. RemoveAllHitStunSources zeroes the counters and leaves the
// HitstunTimer / BuffstunTimer pending in the build (their fire finds no source); the port's hitstun is its timer, so
// the hitstun and the buff stun are ended here (hitstun 0, the buff stun's state off).
fn bomb_movement_clense(w: &mut World, f: FighterId) {
    remove_buff(w, f, None, Some(name!("StateGroups::NewEnumerator12")));
    if in_hitstun(w, f) {
        let x = w.fighter_mut(f);
        x.attackers.stun_source = false;
        x.buffs.buff_stunned = false;
        let pulled = w.fighter(f).moves.pull.is_some();
        stun_changed(w, f, pulled);
        remove_hitstun_from_knock_back(w, f);
        let x = w.fighter_mut(f);
        x.hit.hitstun = 0.0;
        x.buffs.buff_stun = 0.0;
        x.hit.hitstun_paused = false;
        x.hit.hitstun_pending = false;
    }
    clear_queued_attack(w, f);
    stop_anim_montage(w, f);
}

/// DropBomb(knock_back, KnockBackDirection) @0 (EventKnockedBack): with BombTimer active, TimeRemainingOnDrop = its
/// remaining time @82, the timer cleared @109, EquipBombVisuals(false) @129, the Comp_Spawner's
/// CompSpawnActor(C023B_DroppedBomb_C, owner, transform (0, 0, 150), direction (0, KnockBackDirection.X >= 0 ? 1 : -1,
/// 1), 800, 800) @452 and EventKnockedBack unbound @668. `dir_x`: the knockback's direction in sim axes (its X is
/// -dir_x).
/// The spawn as World's item spawn takes a Comp_Spawner item (socket None: the owner's location - (0, 0, 100) plus the
/// translation; the direction mirrored by the owner's facing, unverified there), then the item's BeginPlay, its
/// instigator's buffs and AfterSpawnedActor.
// Deviation: the spawn is made inside the knockback hit (the KnockedBack broadcast), where the build's CompSpawnActor
// runs; the port's World lists it there, so it moves in this frame's item pass when that pass is still to come.
fn drop_bomb(w: &mut World, f: FighterId, dir_x: Real) {
    if stripe(w, f).bomb.timer == 0 {
        return;
    }
    let left = bomb_time_left(w, f, &stripe(w, f).bomb);
    stripe_w(w, f).bomb.time_remaining_on_drop = left;
    clear_bomb_timer(w, f);
    equip_bomb_visuals(w, f, false, 0.0);
    let data = Arc::clone(&w.data);
    match crate::world::items::resolve_def(w, Some(f), ItemClass::C023BDroppedBomb.name()) {
        None => warn_text(w, "spawned actor not implemented (C023B_DroppedBomb_C)"),
        Some(src) => {
            let a = ItemDef::at(&data, src);
            actor_spawn_ability(w, f, a.p);
            let (x, y) = (w.fighter(f).body.x, actor_y(w, f) - 100.0 + 150.0);
            let it = make_bomb(w, f, src, x, y, (if -dir_x >= 0.0 { 1.0 } else { -1.0 }, 1.0), 800.0, 800.0);
            let attack_id = w.fighter(f).decay.attack_count;
            w.actors.items.get_mut(it).expect("the bomb just made").attack_id = attack_id;
            crate::world::items::add_item(w, it, Some(f), None, true);
            crate::world::spawning::item_spawned(w, Some(it), f);
        }
    }
    stripe_w(w, f).bomb.knocked_bound = false;
}

/// ExplosionLaunch @0 (AnimNotify_C023B_BiteDamage_BombLaunch, C023B_BombExplotion_Montage t 0.167): the owner's
/// BPI_GetMoveAxisValues (0, 0 on a failed cast @2658) and BPI_GetIsInAir @253; the direction (@299-883, the same
/// @963-1547 for Hit Box Damage): both axes 0 -> (0, 1); else (MoveX x -0.8, grounded ? max(MoveY, 0.4) : (MoveY is 1
/// or -1 ? MoveY : MoveY + 0.2)), written to HitLaunchSelf and 'Hit Box Damage'. Then the owner's HitBoxHandler
/// pfgDoDamage(null, owner, HitLaunchSelf) @1698 (his own team: DamageAndLaunch's refused branch, whose ForceKnockback
/// launches him: `refused_ally_hit`, which sends no NotifyAllyOfHit for the instigator himself),
/// SetMostRecentFriendlyFire(owner) @1892, HitBoxesOn('Hit Box Tags', 'Hit Box Damage', scale 1) @2008, HitSound bound
/// to HitEnemyPlayer @2225, the AnimBP's BPI_C023B_SetKnockBackOverride(true) @2345, TurnOffHitBoxes bound to the
/// HitstunHandler's HitStunEnded @2477 and UpdateHitOnBounce to the DamageManager's KnockedBack @2612.
/// MoveX / MoveY: BaseFighter's MoveXAxisValue / MoveYAxisValue as they stand.
// Unverified: the self-hit's AttackDecayType 5 and AdditiveKBMode NewEnumerator2 are not read on the refused branch
// (refused_ally_hit registers no hit).
fn explosion_launch(w: &mut World, f: FighterId) {
    if stripe_of(w, f).is_none() {
        return;
    }
    let p = comp(w, f);
    let (mx, my) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
    let (mut dx, mut dy) = (0.0, 1.0);
    if !(mx == 0.0 && my == 0.0) {
        dx = mx * -0.8;
        dy = if !in_air(w, f) {
            max(my, 0.4)
        } else if my == 1.0 || my == -1.0 {
            my
        } else {
            my + 0.2
        };
    }
    {
        let s = &mut stripe_w(w, f).bomb;
        s.body_dir_x = dx;
        s.body_dir_y = dy;
    }
    let mut me = obj(&p, "HitLaunchSelf").cloned().unwrap_or_default();
    me.insert("KnockBackDirectionVector".into(), dir_value(dx, dy));
    let pk = HitPacket::from_props(&w.names, &me, None);
    refused_ally_hit(w, f, f, &pk, false, ActorRef::Fighter(f));
    // SetMostRecentFriendlyFire(owner) @1892: himself, of his own team (read by BFBounce: launch's try_bounce).
    w.fighter_mut(f).attackers.most_recent_friendly_fire = Some(f);
    body_hit_boxes_on(w, f);
    let s = &mut stripe_w(w, f).bomb;
    s.hit_sound_bound = true;
    s.kb_override = true;
    s.stun_end_bound = true;
    s.bounce_bound = true;
}

/// The packet of 'Hit Box Damage' with the direction last written, in a Hitframe's names (a Hitframe's
/// KnockBackDirection / OnHitSound / QueueHitPauseUntilPose).
fn body_packet(w: &World, f: FighterId) -> Props {
    let p = comp(w, f);
    let s = &stripe(w, f).bomb;
    let mut d = obj(&p, "Hit Box Damage").cloned().unwrap_or_default();
    let dir = dir_value(s.body_dir_x, s.body_dir_y);
    d.insert("KnockBackDirectionVector".into(), dir.clone());
    d.insert("KnockBackDirection".into(), dir);
    if let Some(v) = d.get("OnHitAudio").cloned() {
        d.insert("OnHitSound".into(), v);
    }
    if let Some(v) = d.get("QueueHitPauseUntilPoses").cloned() {
        d.insert("QueueHitPauseUntilPose".into(), v);
    }
    let tags = p.get("Hit Box Tags").cloned().unwrap_or_else(|| Value::Arr(vec![Value::Str("BodyHitBox".into())]));
    d.insert("HitBoxTags".into(), tags);
    let mut one = Props::new();
    for k in ["X", "Y", "Z"] {
        one.insert(k.into(), Value::Num(1.0));
    }
    d.insert("HitBoxScale".into(), Value::Obj(one));
    d
}

/// The tags of 'Hit Box Tags' (default BodyHitBox).
fn body_tags(w: &World, f: FighterId) -> Vec<String> {
    match comp(w, f).get("Hit Box Tags").and_then(Value::as_arr) {
        Some(a) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        None => vec!["BodyHitBox".to_owned()],
    }
}

/// Comp_Pawn_HitBoxHandler HitBoxesOn(Tags, Damage, (1, 1, 1)) -> HitBoxesOnWithMultipleScales (the notifies'
/// hitBoxesOn's read): TurnOffHitboxCheck true ends the call; else the boxes of the tags take the packet, the hit list
/// of its AttackTag cleared (ClearActorsHitByTag with HitEachActorOnlyOnce, HitBoxHandler @1587 / @2007), AttackID the
/// handler's. BodyHitBox carries the IgnoreHitStun tag (the hitstun gate of the overlap): it deals while its owner is
/// in the launch's hitstun.
// Unverified: the AttackID is taken as the fighter's attack count (a notify's), the open pose as none (the box is
// tested from the next pose on).
fn body_hit_boxes_on(w: &mut World, f: FighterId) {
    if !w.fighter(f).buffs.buffs.is_empty() && crate::combat::buffs::turn_off_hit_boxes(w, f) {
        return;
    }
    let props = body_packet(w, f);
    // A notify of no montage: the packet is the hit's own (`ActiveHit::p`), the reference only names its class.
    let n = NotifyRef { montage: name!("Comp_C023B_Bomb_C"), index: 0 };
    let id = w.fighter(f).decay.attack_count;
    let mut h = new_active_hit(n, BODY_KEY, &IndexSet::new(), 0.0, id, Name::NONE, None);
    h.p = Some(Arc::new(props));
    w.fighter_mut(f).montages.active_hits.insert(BODY_KEY, h);
}

/// TurnOffHitBoxes @0 (HitStunEnded, UpdateHitOnBounce's other attacker): HitBoxesOff('Hit Box Tags') @71, HitSound
/// unbound @210, the AnimBP's BPI_C023B_SetKnockBackOverride(false) @330, TurnOffHitBoxes unbound @462,
/// UpdateHitOnBounce unbound @597.
fn turn_off_body_hit_boxes(w: &mut World, f: FighterId) {
    let tags = body_tags(w, f);
    let t: Vec<&str> = tags.iter().map(String::as_str).collect();
    hit_boxes_off_tags(w, f, &t, None, true);
    let s = &mut stripe_w(w, f).bomb;
    s.hit_sound_bound = false;
    s.kb_override = false;
    s.stun_end_bound = false;
    s.bounce_bound = false;
}

/// UpdateHitOnBounce(knock_back, KnockBackDirection, ChargeDamageMultiplier, Attacker, ComingFromLeft) @0 (the
/// DamageManager's KnockedBack while bound): BP_FighterCharacter's BPI_ResetJump @79; Attacker is the owner @136 ->
/// 'Hit Box Damage'.KnockBackDirectionVector = KnockBackDirection @188, UpdateHitBoxDamage('Hit Box Tags', it) @295
/// (the boxes keep their hit list) and broadcast OnSelfBounce @419; else TurnOffHitBoxes @443.
/// A bounce reaches it (launch's try_bounce): an enemy-instigated one with the enemy as Attacker; one with no
/// MostRecentAttackerActor and his own MostRecentFriendlyFire (ExplosionLaunch's) through the refused self hit,
/// Attacker the owner.
fn update_hit_on_bounce(w: &mut World, f: FighterId, dir_x: Real, dir_y: Real, dealer: Option<ActorRef>) {
    reset_jump_counts(w, f);
    if dealer == Some(ActorRef::Fighter(f)) {
        {
            let s = &mut stripe_w(w, f).bomb;
            s.body_dir_x = -dir_x;
            s.body_dir_y = dir_y;
        }
        if w.fighter(f).montages.active_hits.contains_key(&BODY_KEY) {
            let props = body_packet(w, f);
            set_active_hit_props(w, f, BODY_KEY, props);
        }
        super::perks::self_bounce(w, f);
    } else {
        turn_off_body_hit_boxes(w, f);
    }
}

/// ResetOnDeath @0 (bound to the pawn's OnExitedArena at BeginPlay): BombTimer cleared @0, EquipBombVisuals(false) @20,
/// EventKnockedBack unbound @134.
// Unverified: OnExitedArena is taken as the pawn's death hook (pawnDeath, in the ring-out after BaseFighter's own
// death).
pub fn death(w: &mut World, f: FighterId) {
    clear_bomb_timer(w, f);
    equip_bomb_visuals(w, f, false, 0.0);
    stripe_w(w, f).bomb.knocked_bound = false;
}

/// BombTimer's fire (BombTimerEnd) in the timer pass.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let s = &stripe(w, f).bomb;
    if s.timer == 0 || w.fighter(f).clock.tick < s.timer {
        return;
    }
    let s = &mut stripe_w(w, f).bomb;
    s.timer = 0; // fired: the handle stays valid (remaining -1), the timer is no longer active
    s.active = false;
    bomb_timer_end(w, f);
}

/// The DamageManager's KnockedBack: EventKnockedBack (DropBomb) and UpdateHitOnBounce, while bound.
// Deviation: the two in this order; the invocation list's order (bind order) is not followed.
fn knocked_back(w: &mut World, f: FighterId, _kb: Real, dir_x: Real, dir_y: Real, dealer: Option<ActorRef>) {
    if stripe(w, f).bomb.knocked_bound {
        drop_bomb(w, f, dir_x);
    }
    if stripe(w, f).bomb.bounce_bound {
        update_hit_on_bounce(w, f, dir_x, dir_y, dealer);
    }
}

/// The HitstunHandler's HitStunEnded: TurnOffHitBoxes while bound.
fn hit_stun_ended(w: &mut World, f: FighterId) {
    if stripe(w, f).bomb.stun_end_bound {
        turn_off_body_hit_boxes(w, f);
    }
}

/// A sound pushed on the fighter's own event list (`f.events.push`).
fn own_sound(w: &mut World, f: FighterId, ev: Name, x: Real, y: Real) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: id,
        ev,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// The HitBoxHandler's HitEnemyPlayer: HitSound @2316 while bound: PF_Post_Event_At_Location(
/// sfx_cha_C023b_bomb_body_hit, the hit player's location, the owner).
pub fn hit_enemy(w: &mut World, f: FighterId, victim: FighterId) {
    if stripe(w, f).bomb.hit_sound_bound {
        let (x, y) = (w.fighter(victim).body.x, actor_y(w, victim));
        own_sound(w, f, name!("sfx_cha_C023b_bomb_body_hit"), x, y);
    }
}

/// The hooks this file joins in Stripe's CharHooks (`knocked_back`, `hit_stun_ended`).
pub fn hooks(h: &mut CharHooks) {
    h.knocked_back = Some(knocked_back);
    h.hit_stun_ended = Some(hit_stun_ended);
}

// ------------------------------------------------------------------ the notifies and the conditions

/// Received_NotifyBegin @93: the owner's Comp_C023B_Bomb SetBomb(false, 0.0).
fn equip_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_bomb(w, f, false, 0.0);
}

/// @93: ActivateBomb().
fn activate_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    activate_bomb(w, f);
}

/// @93: ExplosionLaunch().
fn launch_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    explosion_launch(w, f);
}

/// Check: BPI_C023B_CheckBombValue (Pawn_C023B HasBomb) on the actor; a failed cast reads false (@149 / @178).
fn has_bomb(w: &mut World, f: FighterId) -> bool {
    stripe_of(w, f).is_some_and(|s| s.bomb.equipped)
}

fn no_bomb(w: &mut World, f: FighterId) -> bool {
    !has_bomb(w, f)
}

// ------------------------------------------------------------------ C023B_DroppedBomb

fn bomb_of(w: &World, it: ItemId) -> &DroppedBomb {
    match &w.actors.items.get(it).expect("a live item").ext {
        ItemExt::DroppedBomb(b) => b,
        _ => panic!("item {it:?}: not a C023B_DroppedBomb"),
    }
}

fn bomb_mut(w: &mut World, it: ItemId) -> &mut DroppedBomb {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::DroppedBomb(b) => b,
        _ => panic!("item {it:?}: not a C023B_DroppedBomb"),
    }
}

/// K2_GetTimerRemainingTimeHandle(Detonate_Timer), as `bomb_time_left` (World ticks).
fn detonate_left(w: &World, it: ItemId) -> Real {
    let b = bomb_of(w, it);
    if b.detonate_at < 0 {
        return if b.detonate_rate > 0.0 { -1.0 } else { 0.0 };
    }
    if w.flow.tick == b.detonate_set {
        return b.detonate_rate;
    }
    b.detonate_rate - crate::real(w.flow.tick - b.detonate_set - 1) * DELTA_SECONDS
}

/// Detonate @1031 (Detonate_Timer, HitEnemyPlayer), with authority: IsHeld -> the owner's
/// Comp_ItemManager.ThrowItem((0, 0, 0)) @256 (dropped); HitBoxOn? -> false and UpdateHitbox @344; HitBoxOn? = true,
/// HitBoxDamage = ExplosionHitStruct, HitBoxTags = [ExplosionHitBox], UpdateHitbox @463 (HitBoxesOn: the hit list of
/// the tag cleared, ExplosionHitStruct has HitEachActorOnlyOnce); MulticastDetonation @477 (Detonated;
/// PS_TomJerryRocket_Explosion at the bomb with the instigator's colour); Delay(0.1) @491 -> DestroySpawnedActor @580
/// (a running Delay takes no second call).
fn detonate(w: &mut World, it: ItemId) {
    let (held, holder) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.held, i.holder)
    };
    if held
        && holder >= 0
        && let Some(h) = w.by_id(holder)
        && w.fighter(h).items.held_item == Some(it)
    {
        crate::world::items::drop_held_item(w, h);
    }
    let hit = crate::world::items::item_def(w, it)
        .and_then(|p| obj(p, "ExplosionHitStruct"))
        .map(|p| HitPacket::from_props(&w.names, p, None));
    let i = w.actors.items.get_mut(it).expect("a live item");
    i.hit_on = true;
    if let Some(h) = hit {
        i.hit = h;
    }
    i.hit_set.clear();
    i.hit_actors.clear();
    if let ItemExt::DroppedBomb(b) = &mut i.ext {
        b.explosion_box = true;
    }
    // UpdateHitbox's SetCollisionEnabled: the fighters the sphere overlaps are hit at once (itemBoxActivated). A hit's
    // HitEnemyPlayer runs the parent's tail (Durability - 1, the box off) and Detonate again: the box on again with
    // its hit list cleared, so the same fighter is hit a second time, and that tail (Durability 0) destroys the bomb; a
    // destroyed actor's components are unregistered (no overlap: Durability < 1 here).
    // Deviation: the nested re-hit follows the bytecode and the native overlap rule (the end overlap of the box off, a
    // new begin overlap of the box on), not seen live; the port's item hits one enemy per pass (item_hits), so in 2v2
    // the second enemy in the sphere is not reached.
    let (dead, dur) = (i.dead, i.durability);
    if !dead && dur >= 1.0 {
        crate::world::items::item_box_activated(w, it);
    }
    bomb_mut(w, it).detonated = true;
    let i = w.actors.items.get(it).expect("a live item");
    let (x, y, owner) = (i.x, i.y, i.owner);
    let fighter = owner.map_or(-1, |o| w.fighter(o).id);
    w.events.push(Event::ProjectileDeath {
        cls: name!("C023B_DroppedBomb_C"),
        fighter,
        vfx: Some(name!("PS_TomJerryRocket_Explosion")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    let b = bomb_mut(w, it);
    if b.boom == 0 {
        b.boom = to_int32(latent_frames(0.1));
    }
}

/// Comp_Spawner HandleActorPostSpawn with EnableProjectileMovement (Finn's launchGem's read, no momentum): the velocity
/// direction x InitialSpeed (the direction not normalised), MaxSpeed max(the launch, the call's).
// Unverified: the direction's Y mirrored by the owner's facing (Comp_Spawner's transform relative to the owner).
#[allow(clippy::too_many_arguments, reason = "the function's parameters")]
fn make_bomb(
    w: &mut World,
    f: FighterId,
    src: crate::actors::item::DefSrc,
    x: Real,
    y: Real,
    dir: (Real, Real),
    speed: Real,
    max_speed: Real,
) -> ItemId {
    let data = Arc::clone(&w.data);
    let fc = facing(w, f);
    let sb = crate::world::blast::spawn_box(w);
    let a = ItemDef::at(&data, src);
    let class = ItemClass::C023BDroppedBomb;
    let mut it = Item::make(
        w,
        name!("C023B_DroppedBomb_C"),
        class,
        Some(f),
        src,
        a,
        x,
        y,
        0.0,
        fc,
        dir.0 * speed * fc,
        dir.1 * speed,
        &sb,
        false,
    );
    it.max_speed = max(speed * size2d([dir.0, dir.1]), max_speed);
    // EnableProjectileMovement: the ProjectileMovement flies from the spawn (HitBoxOn? false in the CDO).
    it.flying = true;
    it.ext = ItemExt::DroppedBomb(DroppedBomb {
        bounce: Bounce::new(ItemDef::at(&data, src)),
        detonate_time: num(a.p, "DetonateTime").unwrap_or(2.0),
        detonate_at: -1,
        ..DroppedBomb::default()
    });
    w.actors.items.insert(it)
}

/// A Comp_Spawner notify naming the class (none in the data: DropBomb's spawn is above).
fn make(w: &mut World, f: FighterId, r: &SpawnRequest, x: Real, y: Real) -> ItemId {
    let n = crate::world::spawning::request_props(w, f, r);
    let (dy, dz) = match n.get("InitialMovementDirection").and_then(Value::as_obj) {
        Some(d) => (pnum(d, "Y").unwrap_or(0.0), pnum(d, "Z").unwrap_or(0.0)),
        None => (0.0, 0.0),
    };
    let src = crate::world::items::resolve_def(w, Some(f), ItemClass::C023BDroppedBomb.name())
        .expect("C023B_DroppedBomb_C in its spawner's data");
    let it = make_bomb(
        w,
        f,
        src,
        x,
        y,
        (dy, dz),
        pnum(&n, "InitialSpeed").unwrap_or(0.0),
        pnum(&n, "MaxSpeed").unwrap_or(0.0),
    );
    w.actors.items.get_mut(it).expect("the bomb just made").attack_id = r.attack_id;
    it
}

fn sub() -> ItemExt {
    ItemExt::DroppedBomb(DroppedBomb { detonate_at: -1, ..DroppedBomb::default() })
}

/// ReceiveBeginPlay @612 after the parent's, with authority: the owner's Comp_C023B_Bomb GetDropTimeRemaining ->
/// MulticastTimerValues(TimeRemainingOnDrop, BombTime) @891 (DetonateTime, BombDuration, the TimerEffect's
/// parameters), then Detonate_Timer = K2_SetTimerDelegate(Detonate, DetonateTime) @946 (set inside the spawn: pending,
/// fires in the timer pass of tick + timerTicks(rate); a rate <= 0 sets none).
fn begin(w: &mut World, it: ItemId) -> bool {
    let owner = w.actors.items.get(it).and_then(|i| i.owner);
    if let Some(o) = owner
        && let Some(s) = stripe_of(w, o)
    {
        let (left, bt) = (s.bomb.time_remaining_on_drop, bomb_time(w, o));
        let b = bomb_mut(w, it);
        b.detonate_time = left;
        b.bomb_duration = bt;
    }
    let tick = w.flow.tick;
    let rate = bomb_of(w, it).detonate_time;
    if rate > 0.0 {
        let at = crate::combat::hit::knockback::timer_count(rate);
        let b = bomb_mut(w, it);
        b.detonate_set = tick;
        b.detonate_rate = rate;
        b.detonate_at = tick + at;
    }
    false
}

/// Detonate's Delay(0.1), counted from the item's own step of the frames after.
// Unverified: its first count against the latent pass is not read.
fn step(w: &mut World, it: ItemId) -> bool {
    let b = bomb_mut(w, it);
    if b.boom > 0 {
        b.boom -= 1;
        return b.boom == 0;
    }
    false
}

fn timer(w: &mut World, it: ItemId) -> bool {
    let b = bomb_of(w, it);
    if b.detonate_at >= 0 && w.flow.tick >= b.detonate_at {
        bomb_mut(w, it).detonate_at = -1; // fired (remaining -1)
        detonate(w, it);
    }
    false
}

/// HitEnemyPlayer @1782: the parent's (item_hits: Item.spend), then Detonate @1810.
fn item_hit_enemy(w: &mut World, it: ItemId, _vic: crate::fighter::FighterId) -> bool {
    detonate(w, it);
    false
}

/// PickUp(GrabbingActor) @1036, the class's own: Detonated -> nothing; a grabber with BPI_C023B (a Stripe) ->
/// SetBomb(true, BombTimer active and its remaining > 0 ? max(its remaining, Detonate_Timer's) : Detonate_Timer's)
/// @1571 / @1711, then DestroySpawnedActor @580; any other grabber -> the parent's PickUp @1762.
// Deviation: with Detonated the grabber's Comp_ItemManager still names the item as HeldItem (EquipOnSpawn) with no
// attachment; the port leaves the grabber with nothing held.
fn pick_up_override(w: &mut World, it: ItemId, f: FighterId) -> bool {
    if bomb_of(w, it).detonated {
        return true;
    }
    let Some(s) = stripe_of(w, f) else { return false };
    let mine = bomb_time_left(w, f, &s.bomb);
    let timer_on = s.bomb.timer != 0;
    let left = detonate_left(w, it);
    set_bomb(w, f, true, if timer_on && mine > 0.0 { max(mine, left) } else { left });
    crate::world::items::destroy_item(w, it);
    true
}

/// This file's registry entries (C023B_DroppedBomb_C, the notifies and conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_C023B_EquipBomb_C",
        NotifyHandler { begin: Some(equip_begin), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C023B_ActivateBomb_C",
        NotifyHandler { begin: Some(activate_begin), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C023B_BiteDamage_BombLaunch_C",
        NotifyHandler { begin: Some(launch_begin), ..NotifyHandler::default() },
    )?;
    r.conditions.enter("C023B_HasBomb_Condition_C", Condition::Fn(has_bomb))?;
    r.conditions.enter("C023B_NoBomb_Condition_C", Condition::Fn(no_bomb))?;
    let hooks = ItemHooks {
        make: Some(make),
        sub: Some(sub),
        begin: Some(begin),
        step: Some(step),
        timer: Some(timer),
        hit_enemy: Some(item_hit_enemy),
        pick_up_override: Some(pick_up_override),
        ..ItemHooks::default()
    };
    r.actors
        .enter(DROPPED_BOMB, ActorEntry::new(ActorClass::Item(ItemClass::C023BDroppedBomb), ActorHooks::Item(hooks)))
}
