//! The anim notify classes of a fighter's montages, entered by name: the
//! hitframes and the sweet spot, the sounds, effects and sub-meshes, the branch windows, the move commands and
//! launches, the charge, armor and the rest of the per-notify state; the actor and stage branches of the classes a
//! spawned actor's or a stage actor's montage plays. A class entered with `fighterNotifies` is `strict`: a begin
//! or an instant call it has no handler for is reported.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::actor_montage::proxy_notify;
use crate::combat::buffs::{apply_buff_notify, buff_signal, remove_buff, remove_notify_buffs, turn_off_hit_boxes};
use crate::combat::defense::{ArmorPack, add_armor_pack, end_rage, end_rage_charge, remove_armor, start_rage_charge};
use crate::combat::disguise::copy_running;
use crate::combat::elem::{instigator_charge, item_set_charge_variables};
use crate::combat::hit::{LaunchOnHit, hit_props, set_active_hit_props, set_hit_pause, share_hit_set};
use crate::combat::polymorph::poly_running;
use crate::consts::{AIR_CONTROL, GRAVITY_SCALE, HIT_NO_WALL_SLIDE_CD, MAX_EVADE_COUNT};
use crate::events::{Event, NotifyRef, PopKind, PopParams, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::anim::{
    ChargeBtn, ChargeRamp, MontageSlot, NewActiveHitOpenPose, ResolveBufferedKind, ResolveBufferedW, ResolveUnpressedW,
    active_instance, add_invisibility, answers_no_press, branch_notify_start, cond, cond_reset, conds_ok,
    hit_boxes_off_at, new_active_hit, notify_data, owner_of, queued_for, remove_invisibility, resolve,
    resolve_buffered, resolve_unpressed, set_sub_mesh, set_submesh_always_visible, set_window_end, slot_name,
    stopped_owner, submesh_notify, taunt_slot_entry,
};
use crate::fighter::body::impart_base;
use crate::fighter::common::{actor_is_a, latent_frames, timer_frames, warn_once};
use crate::fighter::control::{
    ExecuteKind, adjust_block_cooldowns, air_attack_cool_down, charge_filed, charge_held, charge_play_rate,
    enable_invulnerability, end_charge_loop_sound, evade_triggered, execute, increment_air_special, release_charge,
    start_charge_totals, trigger_ability, update_charge_values, update_evade_values,
};
use crate::fighter::match_side::{TutorialEvent, play_vo, sfx, throw_item, tutorial_event_of};
use crate::fighter::moves::{
    MoveCtl, MoveEnd, V2, add_move_cmd, adjust_stick_influence, clamp_vel, complete_remove_move, evade_move_vector,
    evade_play_rate, is_falling, is_flying, nav_jump, net_impulse, set_can_walk_off_ledges, set_crouching,
    set_is_friction_modified, update_air_control,
};
use crate::fighter::tick::{SurfaceTrace, controller_rumble, remove_controller_rumble};
use crate::fighter::{
    add_active_attack, equip_add, equip_remove, facing, grounded, in_hitstun, is_dashing, max_walk,
    remove_active_attack, set_facing, set_grounded, update_hurt_boxes,
};
use crate::input::{AttackBtn, Dir};
use crate::math as js;
use crate::name;
use crate::names::Name;
use crate::registry::actor_notifies::{
    actor_continuous_begin, actor_continuous_end, actor_hitframe_begin, actor_hitframe_end, actor_one_shot,
    actor_play_vfx_begin, actor_play_vfx_end, actor_surface,
};
use crate::registry::entries::ActorHooks;
use crate::registry::props::{arr, flag, flag_or, name, name_of, num, num_or, obj, text, truthy};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::spawner_notifies::cond_list;
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::broadcast_charge_end;
use crate::world::items::item_def;
use crate::world::spawning::destroy_projectile;
use indexmap::IndexSet;
use mvs_data::GameData;
use mvs_data::fighter::NotifyTick;
use mvs_data::value::{Props, Value};

/// `KEY_STRIDE`: the keys of a montage instance's notifies are `instance * KEY_STRIDE + index`.
const KEY_STRIDE: i32 = 4096;

/// `C.SFX_ARMOR_COOLDOWN`, `C.SFX_EVADE`.
const SFX_ARMOR_COOLDOWN: Name = name!("sfx_shared_combat_armor_cooldown");
const SFX_EVADE: Name = name!("sfx_shared_combat_evade");

// ---- reads

fn push(w: &mut World, f: FighterId, e: Event) {
    w.fighter_mut(f).events.push(e);
}

fn fid(w: &World, f: FighterId) -> i32 {
    w.fighter(f).id
}

/// `p[k] ?? d` of a name member (`d` the default literal).
fn name_or(w: &World, p: &Props, k: &str, d: Name) -> Name {
    match p.get(k) {
        None | Some(Value::Null) => d,
        Some(Value::Str(s)) => name(w, s),
        Some(_) => Name::NONE,
    }
}

/// The strings of an array member (`(p[k] ?? []) as string[]`).
fn strs<'a>(p: &'a Props, k: &str) -> Vec<&'a str> {
    arr(p, k).iter().filter_map(Value::as_str).collect()
}

fn names(w: &World, p: &Props, k: &str) -> Vec<Name> {
    strs(p, k).into_iter().map(|s| name(w, s)).collect()
}

/// `assetName`: a soft reference's name, `None` for `''`, `'None'` or a value that is no string.
fn asset_name(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty() && *s != "None")
}

/// `p[k] ?? ZERO2` of a vector member; a member without `X` or `Y` reads `NaN` there, as `undefined` does.
fn v2(p: &Props, k: &str) -> V2 {
    match obj(p, k) {
        None => V2 { x: 0.0, y: 0.0 },
        Some(o) => V2 { x: num(o, "X").unwrap_or(Real::NAN), y: num(o, "Y").unwrap_or(Real::NAN) },
    }
}

fn v2_value(v: V2) -> Value {
    let mut o = Props::new();
    o.insert("X".into(), Value::Num(f64::from(v.x)));
    o.insert("Y".into(), Value::Num(f64::from(v.y)));
    Value::Obj(o)
}

fn tick_of(w: &World, f: FighterId, n: NotifyRef) -> NotifyTick {
    let data = w.data.clone();
    notify_data(w, &data, f, n).tick
}

fn has_buffs(w: &World, f: FighterId) -> bool {
    !w.fighter(f).buffs.buffs.is_empty()
}

fn dead(w: &World, f: FighterId) -> bool {
    w.fighter(f).life.dead > 0.0
}

fn key_real(k: i32) -> Real {
    crate::real(k)
}

/// `montageMeshSpawn`: the props of the SubMeshSpawn notify that is on for `mesh` in a montage the fighter plays
/// (its main one and its grouped ones), read by the renderer.
pub fn montage_mesh_spawn<'a>(w: &World, data: &'a GameData, f: FighterId, mesh: &str) -> Option<&'a Props> {
    let m = &w.fighter(f).montages;
    let grouped = m.grouped.iter().flat_map(|(l, _)| l.iter().map(|e| &e.m));
    for pl in m.montage.iter().chain(grouped) {
        for &i in &pl.active {
            let Ok(index) = u32::try_from(i) else { continue };
            let n = notify_data(w, data, f, NotifyRef { montage: pl.montage.name, index });
            if n.cls == "AnimNotify_SubMeshSpawn_C" && text(&n.p, "Mesh") == Some(mesh) {
                return Some(&n.p);
            }
        }
    }
    None
}

/// `meshOf`: the mesh an effect of the notify spawns on, a Disguise copy's (`copy`) or a Polymorph's (`poly`).
fn mesh_of(w: &World, f: FighterId) -> (bool, bool) {
    if copy_running(w, f) {
        (true, false)
    } else if poly_running(w, f) {
        (false, true)
    } else {
        (false, false)
    }
}

// ---- the hitframes

/// `sweetSpot`: a SweetSpotHitframe's sweet spot as a Hitframe's packet: its `SweetSpot_*` members without the
/// prefix (`SweetSpot_Tags` its HitBoxTags) and its own On Hit Effect Override.
fn sweet_spot(p: &Props) -> Props {
    let mut out = Props::new();
    for (k, v) in p {
        if let Some(rest) = k.strip_prefix("SweetSpot_") {
            let key = if k == "SweetSpot_Tags" { "HitBoxTags" } else { rest };
            out.insert(key.to_owned(), v.clone());
        }
    }
    if let Some(v) = p.get("Sweet Spot On Hit Effect Override").filter(|v| truthy(v)) {
        out.insert("OnHitEffectOverride".into(), v.clone());
    }
    out
}

/// `sweetKey`: the sweet spot's key beside its hitframe's. The keys are
/// integers, so it is half a stride on, a key no notify of the instance has (an instance has fewer than 2048 notifies).
fn sweet_key(idx: i32) -> i32 {
    idx + KEY_STRIDE / 2
}

/// The key that shares its hit set with `key` (`sweet.hitSet = own.hitSet`): the sweet spot's for a hitframe's, the
/// hitframe's for a sweet spot's. Only a pair `sweet_begin` made holds both keys, so the caller checks the partner is live.
pub(crate) fn sweet_partner(key: i32) -> i32 {
    if key.rem_euclid(KEY_STRIDE) >= KEY_STRIDE / 2 { key - KEY_STRIDE / 2 } else { sweet_key(key) }
}

const CONDITIONAL_FIELDS: [&str; 32] = [
    "Damage",
    "BaseKnockBack",
    "KnockBackScaling",
    "KnockBackDirection",
    "HitReaction",
    "HitPauseTime",
    "ChargeHitPauseMult",
    "HitStunMult",
    "OnHitSound",
    "OnHitEffectOverride",
    "DirectionalKnockback",
    "VelocityInsteadOfFacing",
    "KnockBackMirror",
    "IgnoreFacing",
    "DoLOSCheck",
    "ForceKnockback",
    "QueueHitPauseUntilPose",
    "HitOnlyOneFighter",
    "ArmorPierce",
    "DestroyProjectiles",
    "ReflectProjectiles",
    "ControllerRumble",
    "AttackDecayType",
    "AttackDecayName",
    "AdditiveKBMode",
    "KBScalingStartingDmg",
    "HitEachActorOnlyOnce",
    "IgnoreKBI",
    "VacuumInfo",
    "HitBoxScale",
    "ElementalBuffs",
    "OnHitBuffs",
];

/// `conditionalPacket`: AnimNotify_Hitframe_Conditional's packet when its conditions hold, its `Conditional*`
/// members, the KBI multiplier and the vacuum and socket members cleared.
fn conditional_packet(p: &Props) -> Props {
    let mut out = Props::new();
    out.insert("HitBoxTags".into(), p.get("HitBoxTags").cloned().unwrap_or(Value::Null));
    for k in CONDITIONAL_FIELDS {
        if let Some(v) = p.get(&format!("Conditional{k}")) {
            out.insert(k.to_owned(), v.clone());
        }
    }
    if let Some(v) = p.get("ConditionalKBIMultiplier") {
        out.insert("KBI_Multiplier".into(), v.clone());
    }
    out.insert("VacuumKnockback".into(), Value::Bool(false));
    out.insert("Direct Knockback Toward Socket".into(), Value::Bool(false));
    out.insert("KnockbackTargetSocket".into(), Value::Str("None".into()));
    out.insert("KnockbackSocketOffset".into(), v2_value(V2 { x: 0.0, y: 0.0 }));
    out.insert("Account for Velocity with Vacuum Knockback".into(), Value::Bool(false));
    out
}

/// `hitframeBegin` of notify `n` at `idx`, its packet `over` where a handler builds one (else the notify's).
pub(crate) fn hitframe_begin_with(w: &mut World, f: FighterId, n: NotifyRef, idx: i32, over: Option<Props>) {
    let data = w.data.clone();
    let p = over.as_ref().unwrap_or(&notify_data(w, &data, f, n).p);
    if num(p, "AttackDecayType") == Some(3.0) {
        let d = &mut w.fighter_mut(f).decay;
        d.current_decay_sequence = d.attack_count;
    }
    hit_boxes_on(w, f, n, idx, over);
}

pub(crate) fn hitframe_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    hitframe_begin_with(w, f, n, idx, None);
}

/// `hitBoxesOn`: the notify's hit on the list, the fading montages' hits of its tags taken off.
fn hit_boxes_on(w: &mut World, f: FighterId, n: NotifyRef, idx: i32, over: Option<Props>) {
    if has_buffs(w, f) && turn_off_hit_boxes(w, f) {
        return;
    }
    let data = w.data.clone();
    let p = over.as_ref().unwrap_or(&notify_data(w, &data, f, n).p);
    let tags = strs(p, "HitBoxTags");
    for i in 0..w.fighter(f).montages.fading.len() {
        let Some(hits) = w.fighter_mut(f).montages.fading[i].hits.take() else { continue };
        let mut keep = Vec::with_capacity(hits.len());
        for h in hits {
            let hp = hit_props(w, f, &h);
            if !strs(&hp, "HitBoxTags").iter().any(|t| tags.contains(t)) {
                keep.push(h);
                continue;
            }
            w.fighter_mut(f).montages.active_hits.shift_remove(&h.idx);
        }
        w.fighter_mut(f).montages.fading[i].hits = Some(keep);
    }
    let prev = w.fighter(f).shapes.disguise.as_ref().and_then(|d| d.copy.as_ref()?.prev);
    let owner = owner_of(w, f, key_real(idx)).map_or(Name::NONE, |m| slot_name(w, f, m));
    let fi = w.fighter(f);
    let pose = NewActiveHitOpenPose { anim: fi.anim.anim, time: fi.anim.anim_time };
    let mut h = new_active_hit(
        n,
        idx,
        &IndexSet::new(),
        fi.charge.total_charged_time,
        fi.decay.attack_count,
        owner,
        Some(pose),
    );
    if let Some(cp) = prev {
        h.copy_pose = Some(cp);
    }
    w.fighter_mut(f).montages.active_hits.insert(idx, h);
    if let Some(p) = over {
        set_active_hit_props(w, f, idx, p);
    }
}

/// `hitframeEnd` of notify `n` at `idx`, the tags its packet names.
fn hitframe_end_tags(w: &mut World, f: FighterId, n: NotifyRef, idx: i32, tags: &[Name]) {
    let kept = w.fighter(f).montages.fade_ends.is_some()
        && tick_of(w, f, n) == NotifyTick::BranchingPoint
        && (0..KEY_STRIDE).contains(&idx);
    if has_buffs(w, f) && !kept {
        buff_signal(w, f, BuffSignal::HitBoxesDeactivated, BuffSignalArg::default());
    }
    w.fighter_mut(f).decay.has_hit_a_fighter = false; // HitBoxesOff @7250
    hit_boxes_off_at(w, f, tags, Some(idx), !kept);
    w.fighter_mut(f).montages.active_hits.shift_remove(&idx);
}

pub(crate) fn hitframe_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let tags = names(w, &notify_data(w, &data, f, n).p, "HitBoxTags");
    hitframe_end_tags(w, f, n, idx, &tags);
}

// The SweetSpotHitframe's own begin and end are its Hitframe's (`f.begin({ ...n, cls: 'AnimNotify_Hitframe_C' })`,
// a class the sink neither defers nor polls, so the call is its handler's), then its sweet spot's at its own key.
fn sweet_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    hitframe_begin(w, f, n, idx);
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if arr(p, "SweetSpot_Tags").is_empty() {
        return;
    }
    let k = sweet_key(idx);
    hitframe_begin_with(w, f, n, k, Some(sweet_spot(p)));
    let hits = &w.fighter(f).montages.active_hits;
    if hits.contains_key(&idx) && hits.contains_key(&k) {
        share_hit_set(w, f, idx, k);
    }
}

fn sweet_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    hitframe_end(w, f, n, idx);
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if !arr(p, "SweetSpot_Tags").is_empty() {
        let tags = names(w, p, "SweetSpot_Tags");
        hitframe_end_tags(w, f, n, sweet_key(idx), &tags);
    }
}

fn conditional_hitframe_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if num(p, "AttackDecayType") == Some(3.0) {
        let d = &mut w.fighter_mut(f).decay;
        d.current_decay_sequence = d.attack_count;
    }
    let conds = cond_list(w, p, "Conditions");
    if !conds_ok(w, f, conds.as_deref(), false) {
        hit_boxes_on(w, f, n, idx, None);
    } else if !flag(p, "ConditionalHitboxDisableOverride") {
        hit_boxes_on(w, f, n, idx, Some(conditional_packet(p)));
    }
}

pub(crate) fn update_hitframe_damage_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let tags = strs(p, "HitBoxTags");
    let keep_unused = flag(p, "Keep Unused Hit Boxes");
    let mut hit_set = IndexSet::new();
    let keys: Vec<i32> = w.fighter(f).montages.active_hits.keys().copied().collect();
    for k in keys {
        let Some(h) = w.fighter(f).montages.active_hits.get(&k) else { continue };
        hit_set.extend(h.hit_set.iter().copied());
        let mut hp = hit_props(w, f, h);
        let ht = strs(&hp, "HitBoxTags");
        let left: Vec<Value> = if keep_unused {
            ht.iter().filter(|t| !tags.contains(t)).map(|t| Value::Str((*t).to_owned())).collect()
        } else {
            Vec::new()
        };
        if left.is_empty() {
            w.fighter_mut(f).montages.active_hits.shift_remove(&k);
        } else if left.len() != ht.len() {
            hp.insert("HitBoxTags".into(), Value::Arr(left));
            set_active_hit_props(w, f, k, hp);
        }
    }
    let owner = owner_of(w, f, key_real(idx)).map_or(Name::NONE, |m| slot_name(w, f, m));
    let h = new_active_hit(n, idx, &hit_set, 0.0, 0, owner, None);
    w.fighter_mut(f).montages.active_hits.insert(idx, h);
}

// ---- small handlers of the first groups

fn friction_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    friction(w, f, n, true);
}

fn friction_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    friction(w, f, n, false);
}

fn friction(w: &mut World, f: FighterId, n: NotifyRef, add: bool) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let modify = name_or(w, p, "Modify Name", name!("None"));
    set_is_friction_modified(w, f, num_or(p, "Friction Mult", 0.0), modify, add);
}

fn anim_is_attacking_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let on = matches!(notify_data(w, &data, f, n).p.get("IsAttacking"), Some(Value::Bool(true)));
    let c = &mut w.fighter_mut(f).charge;
    if on {
        c.anim_attack_off = 0;
    } else if c.anim_attack_off != 2 {
        c.anim_attack_off = 1;
    }
}

fn item_use_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let Some(it) = w.fighter(f).items.held_item else { return };
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if !arr(p, "ActivationConditions").is_empty() {
        warn_once(w, name!("AnimNotify_ItemUse: activation conditions not ported"));
    }
    if flag(p, "UpdateCharge") {
        let c = instigator_charge(w, f);
        item_set_charge_variables(w, ActorRef::Item(it), c); // ActivateItem @724, ahead of ActivateObject
    }
    let can_use = item_def(w, it).is_none_or(|d| flag_or(d, "CanUseItem", true));
    let activates = w.actors.items.get(it).is_some_and(|x| {
        let e = w.registry.actors.get(w.names.str(x.cls));
        e.is_some_and(|e| matches!(e.hooks, ActorHooks::Item(h) if h.activate.is_some()))
    });
    if can_use || activates {
        crate::actors::item::queue_item_use(w, it, f);
    } else {
        throw_item(w, f, 0.0, 0.0, None);
    }
}

fn override_mirror_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if let Some(h) = w.fighter(f).hooks.char.set_facing_override {
        h(w, f, true);
    }
}

fn override_mirror_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if let Some(h) = w.fighter(f).hooks.char.set_facing_override {
        h(w, f, false);
    }
}

fn rumble_name(w: &World, f: FighterId, n: NotifyRef) -> Option<Name> {
    let data = w.data.clone();
    name_of(w, &notify_data(w, &data, f, n).p, "ControllerRumble")
}

fn rumble_kill(w: &World, f: FighterId, n: NotifyRef) -> bool {
    let data = w.data.clone();
    flag(&notify_data(w, &data, f, n).p, "Kill On Notify End")
}

fn notify_conds_ok(w: &mut World, f: FighterId, n: NotifyRef, direct: bool) -> bool {
    let data = w.data.clone();
    let conds = cond_list(w, &notify_data(w, &data, f, n).p, "Conditions");
    conds_ok(w, f, conds.as_deref(), direct)
}

fn rumble_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let r = rumble_name(w, f, n);
    controller_rumble(w, f, r);
}

fn rumble_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if rumble_kill(w, f, n) {
        let r = rumble_name(w, f, n);
        remove_controller_rumble(w, f, r);
    }
}

fn cond_rumble_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if notify_conds_ok(w, f, n, false) {
        let r = rumble_name(w, f, n);
        controller_rumble(w, f, r);
    }
}

fn cond_rumble_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if notify_conds_ok(w, f, n, false) && rumble_kill(w, f, n) {
        let r = rumble_name(w, f, n);
        remove_controller_rumble(w, f, r);
    }
}

fn actor_noop(_w: &mut World, _a: ActorRef, _n: NotifyRef, _idx: u32) {}

fn pop_text_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let fighter = fid(w, f);
    let kind = PopKind::NotifyText;
    push(w, f, Event::PopText { fighter, kind, stacks: 0.0, gain: None, params: Some(PopParams::Notify(n)) });
}

// ---- effects

fn play_vfx_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let (copy, poly) = mesh_of(w, f);
    let (fighter, left) = (fid(w, f), facing(w, f) < 0.0);
    push(w, f, Event::NotifyVfx { fighter, n, on: true, left, copy, poly });
}

fn play_vfx_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    if !flag(&notify_data(w, &data, f, n).p, "KillOnNotifyEnd") {
        return;
    }
    let (copy, poly) = mesh_of(w, f);
    let (fighter, left) = (fid(w, f), facing(w, f) < 0.0);
    push(w, f, Event::NotifyVfx { fighter, n, on: false, left, copy, poly });
}

fn condition_vfx_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if notify_conds_ok(w, f, n, true) {
        play_vfx_begin(w, f, n, idx);
    }
}

fn emote_vfx_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let slot = w.fighter(f).controls.taunt_slot.unwrap_or(0);
    let Some(s) = taunt_slot_entry(w, f, slot).and_then(|e| e.sticker.as_ref()) else { return };
    if s.ps.is_empty() {
        return;
    }
    let (ps, scale) = (name(w, &s.ps), s.scale);
    let fighter = fid(w, f);
    push(w, f, Event::EmoteVfx { fighter, n, on: true, ps, scale: scale.map(crate::math::narrow) });
}

fn emote_vfx_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let fighter = fid(w, f);
    push(w, f, Event::EmoteVfx { fighter, n, on: false, ps: Name::NONE, scale: [1.0, 1.0, 1.0] });
}

fn particle(w: &mut World, f: FighterId, n: NotifyRef, on: bool) {
    let (copy, poly) = mesh_of(w, f);
    let (fighter, left) = (fid(w, f), facing(w, f) < 0.0);
    push(w, f, Event::ParticleNotify { fighter, n, on, left, copy, poly });
}

fn timed_particle_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    particle(w, f, n, true);
}

fn timed_particle_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    particle(w, f, n, false);
}

fn particle_instant(w: &mut World, f: FighterId, n: NotifyRef) {
    particle(w, f, n, true);
}

// ---- attacking flags, hurt boxes, sub-meshes

fn branch_manager(w: &World, f: FighterId, n: NotifyRef) -> bool {
    let data = w.data.clone();
    flag(&notify_data(w, &data, f, n).p, "NotifyBranchManager")
}

fn is_attacking_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if branch_manager(w, f, n) {
        add_active_attack(w, f); // Received_NotifyBegin @290-533
    }
}

fn is_attacking_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if branch_manager(w, f, n) {
        remove_active_attack(w, f); // Received_NotifyEnd @290-533
    }
}

fn cond_is_attacking_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if notify_conds_ok(w, f, n, false) && branch_manager(w, f, n) {
        add_active_attack(w, f);
    }
}

fn cond_is_attacking_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if notify_conds_ok(w, f, n, false) && branch_manager(w, f, n) {
        remove_active_attack(w, f);
    }
}

fn no_wall_bounce_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).hit.no_wall_bounce = true;
}

fn no_wall_bounce_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).hit.no_wall_bounce = false;
}

fn walk_off_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_can_walk_off_ledges(w, f, false);
}

fn walk_off_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_can_walk_off_ledges(w, f, true);
}

fn hurt_boxes(w: &mut World, f: FighterId, n: NotifyRef, begin: bool) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let tags = names(w, p, "HurtBoxTags");
    let on = flag(p, "ToggleOn");
    update_hurt_boxes(w, f, &tags, if begin { on } else { !on });
}

fn hurt_box_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    hurt_boxes(w, f, n, true);
}

fn hurt_box_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    hurt_boxes(w, f, n, false);
}

fn submesh_vis_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    submesh_notify(w, f, n, true);
}

fn submesh_vis_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    submesh_notify(w, f, n, false);
}

fn obj_nonempty(p: &Props, k: &str) -> bool {
    obj(p, k).is_some_and(|o| !o.is_empty())
}

fn submesh_spawn_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if obj_nonempty(p, "SkinMesdhOverride") || obj_nonempty(p, "SkinOverride") {
        warn_once(w, name!("SubMeshSpawn: skin override maps not ported"));
    }
    if let Some(mesh) = p.get("Mesh").filter(|v| truthy(v)).and_then(Value::as_str) {
        let e = crate::fighter::anim::MontageMesh {
            mesh: name(w, mesh),
            anim: name_or(w, p, "Animation", Name::NONE),
            looping: flag(p, "LoopAnimation"),
            socket: name_or(w, p, "Socket", name!("None")),
            age: 0.0,
        };
        w.fighter_mut(f).montages.montage_meshes.push(e);
    }
}

fn submesh_spawn_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let Some(mesh) = name_of(w, &notify_data(w, &data, f, n).p, "Mesh") else { return };
    let list = &mut w.fighter_mut(f).montages.montage_meshes;
    if let Some(at) = list.iter().position(|e| e.mesh == mesh) {
        list.remove(at);
    }
}

fn submesh_anim_change_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if obj_nonempty(p, "SkinMesdhOverride") {
        warn_once(w, name!("SubMeshAnimChange: SkinMesdhOverride not ported"));
    }
    let mesh = name_of(w, p, "Mesh");
    let anim = name_or(w, p, "Animation", Name::NONE);
    let looping = flag(p, "LoopAnimation");
    let list = &mut w.fighter_mut(f).montages.montage_meshes;
    let len = crate::real(u32::try_from(list.len()).unwrap_or(u32::MAX));
    let want = js::min(len - 1.0, num_or(p, "MeshIndex", 0.0));
    let (mut count, mut found) = (0.0, None);
    for (i, e) in list.iter().enumerate() {
        if Some(e.mesh) != mesh {
            continue;
        }
        count += 1.0;
        found = Some(i);
        if count - 1.0 == want {
            break;
        }
    }
    let Some(i) = found else { return };
    let e = &mut list[i];
    e.anim = anim;
    e.looping = looping;
    e.age = 0.0;
}

// ---- branch windows

fn branch_window_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let montage = active_instance(w, f).map(|m| slot_name(w, f, m));
    branch_notify_start(w, f, montage, Some(idx));
    if let Some(c) = w.fighter_mut(f).branching.win_cleared.as_mut()
        && let Some(k) = c.iter().position(|&i| i == idx)
    {
        c.remove(k);
    }
    let data = w.data.clone();
    let dur = notify_data(w, &data, f, n).dur;
    set_window_end(w, f, idx, crate::math::narrow(dur));
}

/// `f.queued` without window `idx` (`null` once it has none left).
fn drop_window(w: &mut World, f: FighterId, idx: i32) {
    let b = &mut w.fighter_mut(f).branching;
    if let Some(q) = b.queued.as_mut() {
        q.windows.retain(|&i| i != idx);
        if q.windows.is_empty() {
            b.queued = None;
        }
    }
}

fn queued_has(w: &World, f: FighterId, idx: i32) -> bool {
    w.fighter(f).branching.queued.as_ref().is_some_and(|q| q.windows.contains(&idx))
}

fn branch_window_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if let Some(e) = w.fighter_mut(f).branching.win_ends.as_mut() {
        e.shift_remove(&idx);
    }
    let late = stopped_owner(w, f, key_real(idx));
    if late.is_some() && (in_hitstun(w, f) || dead(w, f)) {
        if queued_has(w, f, idx) {
            drop_window(w, f, idx);
        }
        return;
    }
    let own: Option<MontageSlot> = late.or_else(|| owner_of(w, f, key_real(idx)));
    if own.is_none() {
        return;
    }
    if queued_has(w, f, idx) {
        let Some(q) = w.fighter(f).branching.queued.as_ref() else { return };
        let p = queued_for(q, idx);
        drop_window(w, f, idx);
        let (force, dir) = (p.force, p.dir);
        if let Some(r) = resolve_buffered(w, f, &ResolveBufferedW { n, i: idx }, &p) {
            if r.kind == ResolveBufferedKind::Branch {
                w.fighter_mut(f).branching.queued = None;
            }
            let kind = match r.kind {
                ResolveBufferedKind::Branch => ExecuteKind::Branch,
                ResolveBufferedKind::Jump => ExecuteKind::Jump,
                ResolveBufferedKind::Dodge => ExecuteKind::Dodge,
            };
            execute(w, f, kind, r.br.as_ref(), force, dir);
        }
    } else {
        let cleared = w.fighter(f).branching.win_cleared.as_ref().is_some_and(|c| c.contains(&idx));
        if !cleared && answers_no_press(w, f, n) {
            if in_hitstun(w, f) || dead(w, f) {
                return; // Trigger Current Attack Client starts nothing there
            }
            if let Some(br) = resolve_unpressed(w, f, &ResolveUnpressedW { n, i: idx }) {
                w.fighter_mut(f).branching.queued = None; // the trigger clears the maps
                execute(w, f, ExecuteKind::Branch, Some(&br), None, Dir::Default);
            }
        }
    }
}

fn camera_shake_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if let Some(v) = p.get("CameraShake").filter(|v| truthy(v)).and_then(Value::as_str) {
        let cls = name(w, v);
        push(w, f, Event::CameraShake { cls });
    }
}

// ---- sounds

fn ak_event(w: &World, f: FighterId, n: NotifyRef) -> Option<Name> {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    p.get("AkEvent").filter(|v| truthy(v)).and_then(Value::as_str).map(|s| name(w, s))
}

fn ak_key(idx: i32) -> SoundKey {
    SoundKey { tag: name!(":"), n: idx }
}

/// An EAkCurveInterpolation value without its enum's prefix, as a name.
fn curve_name(w: &World, s: &str) -> Name {
    match s {
        "Log3" => name!("Log3"),
        "Sine" => name!("Sine"),
        "Log1" => name!("Log1"),
        "InvSCurve" => name!("InvSCurve"),
        "Linear" => name!("Linear"),
        "SCurve" => name!("SCurve"),
        "Exp1" => name!("Exp1"),
        "SineRecip" => name!("SineRecip"),
        "Exp3" => name!("Exp3"),
        "LastFadeCurve" => name!("LastFadeCurve"),
        "Constant" => name!("Constant"),
        _ => name(w, s),
    }
}

fn ak_continuous_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if let Some(ev) = ak_event(w, f, n) {
        let fighter = fid(w, f);
        push(w, f, Event::SoundStart { fighter, ev, key: ak_key(idx) });
    }
}

fn ak_continuous_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let raw = match p.get("FadeCurve") {
        None | Some(Value::Null) => "Linear".to_owned(),
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Num(x)) => x.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(_) => String::new(),
    };
    let curve = curve_name(w, &raw.replacen("EAkCurveInterpolation::", "", 1));
    let fade = num_or(p, "TransitionDurationInMs", 100.0) / 1000.0;
    let fighter = fid(w, f);
    let e = Event::SoundStop { fighter, key: ak_key(idx), fade: Some(fade), ev: None, curve: Some(curve), cut: false };
    push(w, f, e);
}

fn vo_effort_continuous_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if !dead(w, f) {
        ak_continuous_begin(w, f, n, idx);
    }
}

fn sound(fighter: i32, ev: Name) -> Event {
    Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None }
}

fn ak_one_shot(w: &mut World, f: FighterId, n: NotifyRef) {
    if let Some(ev) = ak_event(w, f, n) {
        let fighter = fid(w, f);
        push(w, f, sound(fighter, ev));
    }
}

fn ak_one_shot_cond(w: &mut World, f: FighterId, n: NotifyRef) {
    let Some(ev) = ak_event(w, f, n) else { return };
    let data = w.data.clone();
    let c = name_of(w, &notify_data(w, &data, f, n).p, "Condition").unwrap_or(Name::NONE);
    if cond(w, f, c) {
        let fighter = fid(w, f);
        push(w, f, sound(fighter, ev));
    }
}

fn ak_surface(w: &mut World, f: FighterId, n: NotifyRef) {
    let Some(ev) = ak_event(w, f, n) else { return };
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let sock = name_or(w, &nt.p, "TraceFrom", name!("Root"));
    let queued = nt.tick == NotifyTick::Queued;
    w.fighter_mut(f).frame.surface_traces.push(SurfaceTrace { ev, sock, queued });
}

fn ak_vo(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let Some(alias) = p.get("AliasToPlay").filter(|v| truthy(v)).and_then(Value::as_str) else { return };
    let prob = num_or(p, "Probability", 1.0);
    if w.fighter_mut(f).rng.next() <= prob {
        let alias = name(w, alias);
        play_vo(w, f, alias, &[], false);
    }
}

// ---- equip, invisibility, crouch

fn holding_item(w: &mut World, f: FighterId, on: bool) {
    if let Some(h) = w.fighter(f).hooks.char.holding_item {
        h(w, f, on);
    }
}

fn equip_hook(w: &mut World, f: FighterId, n: NotifyRef, begin: bool) {
    if let Some(h) = w.fighter(f).hooks.char.equip {
        h(w, f, n, begin);
    }
}

fn sub_mesh_anim(w: &mut World, f: FighterId, tag: Name, anim: Option<Name>, looping: bool) {
    let fighter = fid(w, f);
    push(w, f, Event::SubMeshAnim { fighter, tag, anim, looping });
}

fn equip_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if num(p, "RandomWeight") != Some(1.0) || !arr(p, "RandomMeshTags").is_empty() {
        warn_once(w, name!("AnimNotify_Equip: random weight / tags not handled"));
    }
    let tag_s = match p.get("MeshTag") {
        None | Some(Value::Null) => "None",
        Some(v) => v.as_str().unwrap_or(""),
    };
    let tag = name(w, tag_s);
    let on = flag(p, "EquipAtStart");
    set_sub_mesh(w, f, tag, on);
    holding_item(w, f, on && tag_s.eq_ignore_ascii_case("rockmesh"));
    if !on {
        sub_mesh_anim(w, f, tag, None, false);
    } else if let Some(a) = asset_name(p.get("Animation")) {
        let a = name(w, a);
        sub_mesh_anim(w, f, tag, Some(a), flag(p, "LoopAnimation"));
    }
    if on {
        equip_add(w, f, tag);
    } else {
        equip_remove(w, f, tag);
    }
    tutorial_event_of(w, f, if on { TutorialEvent::Equip } else { TutorialEvent::Unequip }); // @1507 / @1838
    equip_hook(w, f, n, true);
}

fn equip_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let tag_s = match p.get("MeshTag") {
        None | Some(Value::Null) => "None",
        Some(v) => v.as_str().unwrap_or(""),
    };
    let tag = name(w, tag_s);
    if flag(p, "UnequipAtEnd") {
        set_sub_mesh(w, f, tag, false);
        equip_remove(w, f, tag);
        if tag_s.eq_ignore_ascii_case("rockmesh") {
            holding_item(w, f, false);
        }
        sub_mesh_anim(w, f, tag, None, false);
    } else {
        if flag(p, "EquipAtEnd") {
            set_sub_mesh(w, f, tag, true);
            equip_add(w, f, tag);
            tutorial_event_of(w, f, TutorialEvent::Equip); // @1293
        }
        if let Some(a) = asset_name(p.get("EndAnimation")) {
            let a = name(w, a);
            sub_mesh_anim(w, f, tag, Some(a), flag(p, "EndLoopAnimation"));
        }
    }
    equip_hook(w, f, n, false);
}

fn invisibility_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    add_invisibility(w, f);
}

fn invisibility_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    remove_invisibility(w, f);
}

fn crouch_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_crouching(w, f, true);
    w.fighter_mut(f).crouch.crouch_locked = true;
}

fn crouch_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).crouch.crouch_locked = false;
    set_crouching(w, f, false);
}

// ---- move commands and launches

pub(crate) fn move_command_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    if flag(p, "OnlyPerformOnGround") && !grounded(w, f) {
        return;
    }
    let is_evade = flag(p, "IsEvade");
    let bv = evade_move_vector(w, f, v2(p, "MoveVector"));
    // `p.IsEvade && bv !== mv ? { ...p, MoveVector: bv } : p`: an equal vector is the same read either way.
    let mv = if is_evade {
        let mut q = p.clone();
        q.insert("MoveVector".into(), v2_value(bv));
        adjust_stick_influence(w, f, &q)
    } else {
        adjust_stick_influence(w, f, p)
    };
    let er = if is_evade { evade_play_rate(w, f) } else { 1.0 };
    let total = if er == 1.0 { nt.dur } else { f64::from(crate::math::narrow(nt.dur / f64::from(er))) };
    let end = MoveEnd {
        stop: flag_or(p, "StopOnEnd", true),
        clamp: flag_or(p, "ClampSpeedOnEnd", false),
        min: v2(p, "SpeedClampMin"),
        max: v2(p, "SpeedClampMax"),
    };
    let ctl = MoveCtl {
        speed: num_or(p, "ControlSpeed", 0.0),
        max: num_or(p, "ControlAngleMax", 0.0),
        min: num_or(p, "ControlAngleMin", 0.0),
        friction: num_or(p, "ControlSpeedFriction", 0.0),
        decay: num_or(p, "ControlSpeedDecay", 0.0),
    };
    let (xy, z) = (flag_or(p, "XYOverride", true), flag_or(p, "ZOverride", true));
    let (block_down, skate) = (flag_or(p, "BlockDownMovement", false), flag_or(p, "WallSkate", false));
    add_move_cmd(w, f, idx, crate::math::narrow(total), mv, xy, z, &end, block_down, skate, &ctl);
}

pub(crate) fn move_command_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if w.fighter(f).branching.defer_queued_ends && tick_of(w, f, n) != NotifyTick::BranchingPoint {
        w.fighter_mut(f).branching.pending_move_ends.push(idx);
    } else {
        complete_remove_move(w, f, idx);
    }
}

fn cond_move_command_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if notify_conds_ok(w, f, n, true) {
        move_command_begin(w, f, n, idx);
    }
}

fn cond_move_command_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if w.fighter(f).branching.defer_queued_ends && tick_of(w, f, n) != NotifyTick::BranchingPoint {
        w.fighter_mut(f).branching.pending_ends.push((n, idx));
        return;
    }
    if notify_conds_ok(w, f, n, true) {
        move_command_end(w, f, n, idx);
    }
}

fn retain_velocity_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let fi = w.fighter(f);
    let (evading, vx) = (fi.evade.evading, fi.body.vx);
    if !(grounded(w, f) && !is_dashing(w, f) && !evading && vx.abs() >= max_walk(w, f) - 400.0) {
        return;
    }
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    let zero = V2 { x: 0.0, y: 0.0 };
    let end = MoveEnd { stop: flag_or(p, "Stop on End", false), clamp: false, min: zero, max: zero };
    let (xy, z) = (flag_or(p, "XYOverride", true), flag_or(p, "ZOverride", true));
    add_move_cmd(
        w,
        f,
        -1,
        crate::math::narrow(nt.dur),
        v2(p, "MoveVector"),
        xy,
        z,
        &end,
        false,
        false,
        &MoveCtl::default(),
    );
}

/// A member of `p.LaunchDirection ?? { X: 0, Y: 0, Z: 0 }`.
fn launch_dir(p: &Props, k: &str) -> Real {
    match obj(p, "LaunchDirection") {
        None => 0.0,
        Some(o) => num(o, k).unwrap_or(Real::NAN),
    }
}

fn launch_self_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if flag(p, "LaunchOnHit") {
        w.fighter_mut(f).hit.launch_on_hit = Some(LaunchOnHit {
            speed_clamp: flag(p, "SpeedClamp"),
            min: v2(p, "SpeedClampMin"),
            max: v2(p, "SpeedClampMax"),
            can_fast_fall: flag(p, "CanFastFall"),
            dir_y: num_in_or(p, "LaunchDirection", "Y"),
            dir_z: num_in_or(p, "LaunchDirection", "Z"),
            xy: flag(p, "XYOverride"),
            z: flag(p, "ZOverride"),
        });
        return;
    }
    let fi = w.fighter(f);
    let k = 1.0 + (num_or(p, "ChargeMult", 1.0) - 1.0) * fi.charge.charge_pct; // Lerp(1, ChargeMult, GetChargeValue)
    let down = fi.crouch.pushing_down && flag(p, "CanFastFall");
    let clamp = flag(p, "SpeedClamp");
    if if clamp { !down } else { down } {
        let (vx, vy) = clamp_vel(w, f, v2(p, "SpeedClampMin"), v2(p, "SpeedClampMax"), k);
        net_impulse(w, f, vx, vy, true, true, 0.0, false, 0.0);
    } else if !clamp {
        let (y, z) = (launch_dir(p, "Y"), launch_dir(p, "Z"));
        let x = obj(p, "LaunchDirection").and_then(|o| num(o, "X")).unwrap_or(0.0);
        let (xy, zo) = (flag(p, "XYOverride"), flag(p, "ZOverride"));
        // MakeVector(X, Y, Z) @3077, NetImpulse @3178
        net_impulse(w, f, y * facing(w, f) * k, z * k, xy, zo, 0.0, false, x * k);
    }
}

/// `p[k]?.[sub] ?? 0`.
fn num_in_or(p: &Props, k: &str, sub: &str) -> Real {
    obj(p, k).and_then(|o| num(o, sub)).unwrap_or(0.0)
}

fn launch_self_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    if flag(&notify_data(w, &data, f, n).p, "LaunchOnHit") {
        w.fighter_mut(f).hit.launch_on_hit = None;
    }
}

fn cond_launch_self_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let conds = cond_list(w, &notify_data(w, &data, f, n).p, "Conditions").unwrap_or_default();
    for c in conds {
        if !cond(w, f, c.unwrap_or(Name::NONE)) {
            return;
        }
    }
    launch_self_begin(w, f, n, idx);
}

fn gravity_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let g = num_or(&notify_data(w, &data, f, n).p, "NewGravityScale", 0.0);
    let m = &mut w.fighter_mut(f).moves;
    m.net_gravity = Some(g);
    m.gravity_mod_count += 1;
}

fn gravity_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let m = &mut w.fighter_mut(f).moves;
    if m.gravity_mod_count > 0 {
        m.gravity_mod_count -= 1;
    }
    if m.gravity_mod_count == 0 {
        m.net_gravity = Some(GRAVITY_SCALE);
    }
}

// ---- the charge

fn charge_hold_ui_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let fi = w.fighter_mut(f);
    let total = match fi.charge.charge_attack_multiplier {
        Some(m) if flag(&nt.p, "Use Charge Multiplier") => crate::math::narrow(nt.dur * f64::from(m)),
        _ => crate::math::narrow(nt.dur),
    };
    fi.charge.set_charge_ui((fi.clock.time, total));
}

fn charge_hold_ui_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let fi = w.fighter_mut(f);
    fi.charge.set_charge_ui((fi.clock.time, 0.0)); // PreAttackCharge(0, false, false)
}

/// `INPUT_BUTTONS[i]`: attack, special, jump, dodge.
fn input_button(i: Real) -> Option<ChargeBtn> {
    match i {
        0.0 => Some(ChargeBtn::Attack(AttackBtn::Attack)),
        1.0 => Some(ChargeBtn::Attack(AttackBtn::Special)),
        2.0 => Some(ChargeBtn::Jump),
        3.0 => Some(ChargeBtn::Dodge),
        _ => None,
    }
}

fn charging_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    let local = if flag(p, "ModifyMontageWithChargeMultiplier") {
        w.fighter(f).charge.charge_attack_multiplier.unwrap_or(1.0)
    } else {
        1.0
    };
    if let Some(s) = p.get("Ak Charge Sound").filter(|v| truthy(v)).and_then(Value::as_str) {
        // The key `f.id + ':charge' + seq`: the tag `:charge` in `chargeSoundKey`, the number its
        // `chargeSoundSeq`, the fighter the event's.
        let m = &mut w.fighter_mut(f).montages;
        m.charge_sound_seq += 1;
        m.charge_sound_key = name!(":charge");
        let key = SoundKey { tag: name!(":charge"), n: m.charge_sound_seq };
        let (fighter, ev) = (fid(w, f), name(w, s));
        push(w, f, Event::Rtpc { fighter, name: name!("cha_charge_loop"), value: 0.0, interp_ms: 0.0 });
        push(w, f, Event::SoundStart { fighter, ev, key });
    }
    charge_play_rate(w, f, 1.0 / local); // @1427, ahead of StartAttackChargeServer (@1737)
    let fi = w.fighter_mut(f);
    fi.montages.charge_ramp =
        Some(ChargeRamp { n: 0, at: 0.05, span: crate::math::narrow(nt.dur * f64::from(local) - 0.1), pending: true });
    fi.charge.charge_notify = idx;
    fi.charge.charge_released = false;
    let pressed = num_or(p, "PressedButton", 0.0);
    let btn = input_button(pressed);
    if btn.is_none() {
        warn_once(w, name!("AnimNotify_ChargingAttack: PressedButton is not a button the port tracks"));
    }
    w.fighter_mut(f).branching.charge_btn = btn;
    charge_filed(w, f);
    let fi = w.fighter_mut(f);
    fi.charge.charge_start_time = fi.clock.time;
    start_charge_totals(w, f, n); // ChargeTotalTime = ChargeMultiplier x TotalDuration, TotalChargedTime, the ChargeTimer
    let c = &mut w.fighter_mut(f).charge;
    c.charge_pct = 1.0;
    c.charge_mult = num_or(p, "MaxKnockBackMultiplier", 1.0);
    c.charge_dmg_mult = num_or(p, "MaxDamageMultiplier", 1.0);
    c.charge_buff_mult = num_or(p, "MaxBuffStackMultiplier", 1.0);
    if let Some(h) = w.fighter(f).hooks.char.charge_started {
        h(w, f, n); // its ChargeStarted broadcast (@167)
    }
    if flag(p, "OnlyEndOnNotifyEnd") {
        w.fighter_mut(f).charge.charge_released = true; // input never bound
    } else if resolve(w, f, key_real(idx)).is_some_and(|r| r.n == n) && charge_held(w, f) == Some(false) {
        release_charge(w, f, None);
    }
}

fn charging_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let late = w.fighter(f).montages.fade_ends.is_some() && nt.tick == NotifyTick::BranchingPoint;
    if !late {
        if let Some(h) = w.fighter(f).hooks.char.charge_end {
            h(w, f, n);
        }
        broadcast_charge_end(w, f); // a run-time binder's broadcast
    }
    let cn = w.fighter(f).charge.charge_notify;
    if cn == idx {
        if flag(&nt.p, "OnlyEndOnNotifyEnd") {
            update_charge_values(w, f, n);
            w.fighter_mut(f).charge.set_charge_ui((0.0, 0.0));
        }
        w.fighter_mut(f).charge.charge_released = true;
    } else if cn >= 0 && (idx >= KEY_STRIDE || cn >= KEY_STRIDE) {
        if let Some(cur) = resolve(w, f, key_real(cn)).map(|r| r.n)
            && flag(&notify_data(w, &data, f, cur).p, "OnlyEndOnNotifyEnd")
        {
            update_charge_values(w, f, cur);
            w.fighter_mut(f).charge.set_charge_ui((0.0, 0.0));
        }
        w.fighter_mut(f).charge.charge_released = true;
    }
    if w.fighter(f).charge.charge_pct > 0.7 {
        tutorial_event_of(w, f, TutorialEvent::ChargeUp);
    }
    if late {
        if let Some(e) = w.fighter_mut(f).montages.fade_ends.as_mut() {
            e.push(n);
        }
    } else {
        charge_play_rate(w, f, 1.0); // Received_NotifyEnd @258, its first statement
        end_charge_loop_sound(w, f, n);
    }
}

fn ability_index_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let i = num_or(&notify_data(w, &data, f, n).p, "AbilityIndex", 0.0);
    trigger_ability(w, f, js::to_int32(i)); // TriggerAbilityActual(AbilityIndex)
}

// ---- armor, hit pause, cooldowns, rage, buffs

fn armor_kind(w: &World, p: &Props) -> Name {
    name_or(w, p, "ArmorType", name!("None"))
}

fn armor_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if crate::real(w.fighter(f).evade.evade_count) >= MAX_EVADE_COUNT {
        // BPI_ArmorDodgeCooldown @108020-108184, under IsLocallyControlled (every pawn of an offline match): the sound
        // and the broker's ShowArmorUnavailableForPlayer (the HUD reads the count).
        sfx(w, f, Some(SFX_ARMOR_COOLDOWN));
        let e = &mut w.fighter_mut(f).evade;
        e.armor_unavailable = e.armor_unavailable.wrapping_add(1);
        return;
    }
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let a = ArmorPack {
        kind: armor_kind(w, p),
        can_override: flag_or(p, "CanOverride", true),
        amount: num_or(p, "ArmorAmount", 40.0),
        max_hits: num_or(p, "MaxHits", 2.0),
        threshold: num_or(p, "KnockBackThreshold", 80000.0),
        kb_res: num_or(p, "KnockBackResistanceMultiplier", 0.0),
        dmg_res: num_or(p, "DamageResistanceMultiplier", 1.0),
        hs_res: num_or(p, "HitStunResistanceMultiplier", 0.0),
    };
    add_armor_pack(w, f, &a);
    if flag_or(p, "CostDodgeMeter", true) {
        update_evade_values(w, f);
    }
}

fn armor_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let kind = armor_kind(w, &notify_data(w, &data, f, n).p);
    remove_armor(w, f, kind);
}

fn hit_pause_pose_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let h = &mut w.fighter_mut(f).hit;
    h.in_hit_pause_pose = true;
    let (q, sec) = (h.queued_hit_pause, h.queued_hit_pause_sec);
    if q > 0.0 {
        set_hit_pause(w, f, q, sec);
    }
}

fn hit_pause_pose_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).hit.in_hit_pause_pose = false;
}

fn air_attack_cd_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let cd = num_or(&notify_data(w, &data, f, n).p, "AirAttackCoolDown", 0.0);
    air_attack_cool_down(w, f, cd);
}

fn rage_charge_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    start_rage_charge(w, f);
}

fn rage_charge_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    end_rage_charge(w, f);
}

fn apply_buff_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    apply_buff_notify(w, f, n);
}

fn apply_buff_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    if flag(&notify_data(w, &data, f, n).p, "DestroyBuffsOnNotifyEnd") {
        remove_notify_buffs(w, f, n);
    }
}

fn remove_rage_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    end_rage(w, f); // EndRageMulticast (the duration timer is left running)
    tutorial_event_of(w, f, TutorialEvent::UniqueCharacterMechanic1); // @471
}

fn increment_air_special_begin(_w: &mut World, _f: FighterId, _n: NotifyRef, _idx: i32) {}

fn increment_air_special_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if w.fighter(f).hit.kb_mode || is_falling(w, f) || is_flying(w, f) {
        increment_air_special(w, f);
    }
}

fn outline_off_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).life.outline_count -= 1;
}

fn outline_off_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).life.outline_count += 1;
}

fn ability_trigger_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let slot = w.fighter(f).abilities.attack_slot;
    trigger_ability(w, f, slot);
}

fn no_wall_slide_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let fi = w.fighter(f);
    let tick = fi.clock.tick;
    if tick < fi.wall.no_wall_slide_until {
        return;
    }
    let data = w.data.clone();
    let cd = num_or(&notify_data(w, &data, f, n).p, "HitNoWallSlideCD", HIT_NO_WALL_SLIDE_CD);
    w.fighter_mut(f).wall.no_wall_slide_until = js::to_int32(crate::real(tick) + latent_frames(cd));
}

fn air_control_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let v = num_or(&notify_data(w, &data, f, n).p, "NewAirControl", 0.0);
    w.fighter_mut(f).air.air_control = v;
}

fn air_control_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    update_air_control(w, f, AIR_CONTROL);
}

/// `evadeFromAttack`.
fn evade_from_attack(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    update_evade_values(w, f);
    sfx(w, f, Some(SFX_EVADE));
    let key = w.names.str(w.fighter(f).ch);
    let vo = w.data.chars.get(key).and_then(|c| c.def.sfx.vo_evade.as_deref()).filter(|s| !s.is_empty());
    let vo = vo.map(|s| name(w, s));
    sfx(w, f, vo);
    if in_hitstun(w, f) || dead(w, f) {
        let fi = w.fighter_mut(f);
        fi.evade.evade_count = js::max(fi.evade.evade_count - 1.0, 0.0);
        fi.platforms.evading_through_platform = false;
        return;
    }
    enable_invulnerability(w, f, true, false);
    update_evade_values(w, f);
    evade_triggered(w, f, 5.0);
    if w.fighter(f).evade.evade_invuln > 0.0 {
        w.fighter_mut(f).evade.evade_invuln = 0.0;
        enable_invulnerability(w, f, false, false);
    }
    let t = w.fighter(f).evade.evade_invuln_time;
    if t > 0.0 {
        w.fighter_mut(f).evade.evade_invuln = timer_frames(Some(t));
    } else {
        enable_invulnerability(w, f, false, false);
    }
}

fn section_jump_single_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if !notify_conds_ok(w, f, n, true) {
        return;
    }
    let Some(m) = owner_of(w, f, key_real(idx)).or_else(|| active_instance(w, f)) else { return };
    let data = w.data.clone();
    let section = name_or(w, &notify_data(w, &data, f, n).p, "JumpToSection", name!("None"));
    nav_jump(w, f, m, section);
}

// ---- the actor branches of the shared classes

/// A spawned actor's owner and class (`a.owner`, `a.cls`).
fn owner_cls(w: &World, a: ActorRef) -> Option<(FighterId, Name)> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| Some((p.owner?, p.cls))),
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|x| Some((x.owner?, x.cls))),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|x| (x.owner, x.cls)),
        _ => None,
    }
}

fn destroy_meeseeks_begin(w: &mut World, a: ActorRef, _n: NotifyRef, _idx: u32) {
    let Some((owner, cls)) = owner_cls(w, a) else { return };
    let Some(base) = actor_is_a(w, ActorRef::Fighter(owner), cls, name!("Meeseeks_Base_C")) else {
        warn_once(w, name!("DestroyUnactivatedMeeseeks: no class chain"));
        return;
    };
    let activated = match a {
        // The class's own `Activated`: the runner's is in its state, not the movement flag's.
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| match &p.ext {
            crate::actors::ProjectileExt::MeeseeksRun(r) => r.activated,
            _ => p.activated,
        }),
        _ => false,
    };
    if !base || activated {
        return;
    }
    match a {
        ActorRef::Projectile(id) => destroy_projectile(w, id, true),
        ActorRef::Item(id) => {
            if let Some(x) = w.actors.items.get_mut(id) {
                x.dead = true; // K2_DestroyActor
            }
        }
        ActorRef::Actor(id) => {
            if let Some(x) = w.actors.actors.get_mut(id) {
                x.dead = true;
            }
        }
        _ => {}
    }
}

fn launch_projectile_begin(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    let Some((owner, cls)) = owner_cls(w, a) else { return };
    if actor_is_a(w, ActorRef::Fighter(owner), cls, name!("SpawnedActorParent_C")) != Some(true) {
        return;
    }
    let ActorRef::Projectile(id) = a else {
        warn_once(w, name!("LaunchProjectile on an actor with no ProjectileMovement in the port"));
        return;
    };
    let data = w.data.clone();
    let p = &crate::actors::actor_montage::actor_notify_data(w, &data, a, n).p;
    let speed = num_or(p, "Speed", 0.0);
    let (y, z) = (num_in_or(p, "Direction", "Y"), num_in_or(p, "Direction", "Z"));
    let use_facing = flag(p, "UseInstigatorFacingDirection");
    let Some(pr) = w.actors.projectiles.get_mut(id) else { return };
    pr.moving = true; // activateMovement
    pr.max_speed = js::max(speed, pr.max_speed);
    let mut dy = y;
    let left = pr.facing < 0.0;
    if use_facing && !left {
        dy = -dy; // (X too: the depth, not kept)
    }
    pr.vx = -dy * speed;
    pr.vy = z * speed;
}

fn projectile_gravity_begin(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    let ActorRef::Projectile(id) = a else { return };
    let data = w.data.clone();
    let g = num_or(&crate::actors::actor_montage::actor_notify_data(w, &data, a, n).p, "Gravity", 0.0);
    if let Some(pr) = w.actors.projectiles.get_mut(id) {
        pr.grav_scale = g;
    }
}

fn proxy_instant(w: &mut World, a: ActorRef, n: NotifyRef) {
    proxy_notify(w, a, n, true);
}

fn proxy_begin(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    proxy_notify(w, a, n, true);
}

fn proxy_end(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    proxy_notify(w, a, n, false);
}

fn stage_proxy_instant(w: &mut World, s: u32, n: NotifyRef) {
    proxy_notify(w, ActorRef::Stage(s), n, true);
}

fn stage_proxy_begin(w: &mut World, s: u32, n: NotifyRef, _idx: u32) {
    proxy_notify(w, ActorRef::Stage(s), n, true);
}

fn stage_proxy_end(w: &mut World, s: u32, n: NotifyRef, _idx: u32) {
    proxy_notify(w, ActorRef::Stage(s), n, false);
}

// ---- the last groups

fn flying_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let fi = w.fighter_mut(f);
    fi.moves.flying = true;
    fi.hit.kb_mode = false;
    set_grounded(w, f, false);
}

fn flying_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let b = if grounded(w, f) { w.fighter(f).platforms.base } else { None };
    let fi = w.fighter_mut(f);
    fi.moves.flying = false;
    fi.hit.kb_mode = false;
    set_grounded(w, f, false);
    if let Some(b) = b {
        impart_base(w, f, b);
    }
}

fn section_jump_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let conds = cond_list(w, &notify_data(w, &data, f, n).p, "Conditions").unwrap_or_default();
    for c in conds.into_iter().flatten() {
        cond_reset(w, f, c);
    }
}

fn block_cooldown_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    adjust_block_cooldowns(w, f, true);
}

fn block_cooldown_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    adjust_block_cooldowns(w, f, false);
}

fn submesh_tag(w: &World, f: FighterId, n: NotifyRef) -> Name {
    let data = w.data.clone();
    name_or(w, &notify_data(w, &data, f, n).p, "SubMesh", name!("None"))
}

fn always_visible_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let tag = submesh_tag(w, f, n);
    set_submesh_always_visible(w, f, tag, true);
}

fn always_visible_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let tag = submesh_tag(w, f, n);
    set_submesh_always_visible(w, f, tag, false);
}

fn invulnerable_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    enable_invulnerability(w, f, true, false);
}

fn invulnerable_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    enable_invulnerability(w, f, false, false);
}

fn rotation_reverse_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let v = -facing(w, f);
    set_facing(w, f, v);
}

/// `removeBuffOfClass`: no class, no buff (IsBuffOfClass is false for every buff).
fn remove_buff_of(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    if let Some(c) = asset_name(notify_data(w, &data, f, n).p.get("Buff")) {
        let c = name(w, c);
        remove_buff(w, f, Some(c), None);
    }
}

fn remove_buff_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    remove_buff_of(w, f, n);
}

fn remove_buff_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    if flag(&notify_data(w, &data, f, n).p, "RemoveOnEnd") {
        remove_buff_of(w, f, n);
    }
}

// ---- the entries

type Hook = fn(&mut World, FighterId, NotifyRef, i32);
type Instant = fn(&mut World, FighterId, NotifyRef);

const fn h(begin: Option<Hook>, end: Option<Hook>) -> NotifyHandler {
    NotifyHandler {
        begin,
        end,
        instant: None,
        strict: false,
        actor_begin: None,
        actor_end: None,
        actor_instant: None,
        stage_begin: None,
        stage_end: None,
        stage_instant: None,
    }
}

fn be(b: Hook, e: Hook) -> NotifyHandler {
    h(Some(b), Some(e))
}

fn b(b: Hook) -> NotifyHandler {
    h(Some(b), None)
}

fn e(e: Hook) -> NotifyHandler {
    h(None, Some(e))
}

fn inst(i: Instant) -> NotifyHandler {
    NotifyHandler { instant: Some(i), ..NotifyHandler::default() }
}

fn none() -> NotifyHandler {
    NotifyHandler::default()
}

fn strict(mut n: NotifyHandler) -> NotifyHandler {
    n.strict = true;
    n
}

fn actor_be(
    mut n: NotifyHandler,
    b: fn(&mut World, ActorRef, NotifyRef, u32),
    e: fn(&mut World, ActorRef, NotifyRef, u32),
) -> NotifyHandler {
    n.actor_begin = Some(b);
    n.actor_end = Some(e);
    n
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let n = &mut r.notifies;
    // notifies({ ... }) at the module's head
    n.enter("AnimNotify_SweetSpotHitframe_C", be(sweet_begin, sweet_end))?;
    n.enter("AnimNotify_SetGroundFriction_C", be(friction_begin, friction_end))?;
    n.enter("AnimNotify_SetAnimIsAttacking_C", b(anim_is_attacking_begin))?;
    n.enter("AnimNotify_ItemUse_C", b(item_use_begin))?;
    n.enter("AnimNotify_ResetDynamics", none())?;
    n.enter("AnimNotify_OverrideMirror_C", be(override_mirror_begin, override_mirror_end))?;
    n.enter("AnimNotify_ControllerRumble_C", actor_be(be(rumble_begin, rumble_end), actor_noop, actor_noop))?;
    n.enter(
        "AnimNotify_ConditionalControllerRumble_C",
        actor_be(be(cond_rumble_begin, cond_rumble_end), actor_noop, actor_noop),
    )?;
    n.enter("AnimNotify_PopText_C", b(pop_text_begin))?;

    // fighterNotifies({ ... }): the effects through the sounds
    n.enter("AnimNotify_ConditionPlayVFX_C", strict(be(condition_vfx_begin, play_vfx_end)))?;
    n.enter(
        "AnimNotify_PlayVFX_C",
        strict(actor_be(be(play_vfx_begin, play_vfx_end), actor_play_vfx_begin, actor_play_vfx_end)),
    )?;
    n.enter("AnimNotify_PlayEmoteVFX_C", strict(actor_be(be(emote_vfx_begin, emote_vfx_end), actor_noop, actor_noop)))?;
    n.enter("AnimNotifyState_TimedParticleEffect", strict(be(timed_particle_begin, timed_particle_end)))?;
    n.enter("AnimNotifyState_PlayTimedVFX", strict(be(timed_particle_begin, timed_particle_end)))?;
    n.enter("AnimNotify_PlayParticleEffect", strict(inst(particle_instant)))?;
    n.enter("AnimNotify_PlaySingleShotVFX", strict(inst(particle_instant)))?;
    n.enter("AnimNotify_isAttackingFlag_C", strict(be(is_attacking_begin, is_attacking_end)))?;
    n.enter("AnimNotify_ConditionalIsAttacking_C", strict(be(cond_is_attacking_begin, cond_is_attacking_end)))?;
    n.enter("AnimNotify_SetNoWallBounce_C", strict(be(no_wall_bounce_begin, no_wall_bounce_end)))?;
    n.enter("AnimNotify_StopWalkOffEdge_C", strict(be(walk_off_begin, walk_off_end)))?;
    n.enter("AnimNotify_HurtBoxUpdate_C", strict(be(hurt_box_begin, hurt_box_end)))?;
    n.enter("AnimNotify_SetSubmeshVisibility_C", strict(be(submesh_vis_begin, submesh_vis_end)))?;
    n.enter("AnimNotify_SubMeshSpawn_C", strict(be(submesh_spawn_begin, submesh_spawn_end)))?;
    n.enter("AnimNotify_SubMeshAnimChange_C", strict(b(submesh_anim_change_begin)))?;
    n.enter("AnimNotify_HitframeWithSpecificScales_C", strict(be(hitframe_begin, hitframe_end)))?;
    n.enter(
        "AnimNotify_Hitframe_C",
        strict(actor_be(be(hitframe_begin, hitframe_end), actor_hitframe_begin, actor_hitframe_end)),
    )?;
    n.enter("AnimNotify_Hitframe_Conditional_C", strict(be(conditional_hitframe_begin, hitframe_end)))?;
    // Received_NotifyEnd: HitBoxesOff(HitBoxTags)
    n.enter("AnimNotify_UpdateHitframeDamage_C", strict(be(update_hitframe_damage_begin, hitframe_end)))?;
    n.enter("AnimNotify_BranchWindow_C", strict(be(branch_window_begin, branch_window_end)))?;
    n.enter("AnimNotify_CameraShake_C", strict(b(camera_shake_begin)))?;
    n.enter(
        "Ak_Continuous_C",
        strict(actor_be(be(ak_continuous_begin, ak_continuous_end), actor_continuous_begin, actor_continuous_end)),
    )?;
    n.enter("Ak_VO_Effort_Continuous_C", strict(be(vo_effort_continuous_begin, ak_continuous_end)))?;
    n.enter("AnimNotify_Equip_C", strict(be(equip_begin, equip_end)))?;
    n.enter("AnimNotify_AddInvisibility_C", strict(be(invisibility_begin, invisibility_end)))?;
    n.enter("AnimNotify_Crouch_C", strict(be(crouch_begin, crouch_end)))?;
    let mut one_shot = strict(inst(ak_one_shot));
    one_shot.actor_instant = Some(actor_one_shot);
    n.enter("Ak_OneShot_C", one_shot)?;
    n.enter("Ak_VO_Effort_C", strict(inst(ak_one_shot)))?;
    n.enter("Ak_OneShotWithCondition_C", strict(inst(ak_one_shot_cond)))?;
    let mut surface = strict(inst(ak_surface));
    surface.actor_instant = Some(actor_surface);
    n.enter("Ak_SurfaceEffect_C", surface)?;
    n.enter("Ak_VO_C", strict(inst(ak_vo)))?;

    // fighterNotifies({ ... }): gravity through the wall slide
    n.enter("AnimNotify_SetGravity_C", strict(be(gravity_begin, gravity_end)))?;
    n.enter("AnimNotify_MoveCommand_C", strict(be(move_command_begin, move_command_end)))?;
    n.enter("AnimNotify_EvadeMoveCommand_C", strict(be(move_command_begin, move_command_end)))?;
    n.enter("AnimNotify_ConditionalMoveCommand_C", strict(be(cond_move_command_begin, cond_move_command_end)))?;
    n.enter("AnimNotify_RetainVelocityMoveCommand_C", strict(b(retain_velocity_begin)))?;
    n.enter("AnimNotify_ConditionalLaunchSelf_C", strict(be(cond_launch_self_begin, launch_self_end)))?;
    n.enter("AnimNotify_LaunchSelf_C", strict(be(launch_self_begin, launch_self_end)))?;
    n.enter("AnimNotify_ChargeHoldUI_C", strict(be(charge_hold_ui_begin, charge_hold_ui_end)))?;
    n.enter("AnimNotify_ChargingAttack_C", strict(be(charging_begin, charging_end)))?;
    n.enter("AnimNotify_AbilityTriggerWithIndex_C", strict(b(ability_index_begin)))?;
    n.enter("AnimNotify_Armor_C", strict(be(armor_begin, armor_end)))?;
    n.enter("AnimNotify_HitPausePoseFrames_C", strict(be(hit_pause_pose_begin, hit_pause_pose_end)))?;
    n.enter("AnimNotify_SetAirAttackCoolDown_C", strict(b(air_attack_cd_begin)))?;
    n.enter("AnimNotify_RageCharge_C", strict(be(rage_charge_begin, rage_charge_end)))?;
    n.enter("AnimNotify_ApplyBuff_C", strict(be(apply_buff_begin, apply_buff_end)))?;
    n.enter("AnimNotify_RemoveRage_C", strict(e(remove_rage_end)))?;
    n.enter("AnimNotify_IncrementAirSpecial_C", strict(be(increment_air_special_begin, increment_air_special_end)))?;
    n.enter("AnimNotify_TurnOffOutline_C", strict(be(outline_off_begin, outline_off_end)))?;
    n.enter("AnimNotify_AbilityTrigger_C", strict(b(ability_trigger_begin)))?;
    n.enter("AnimNotify_SetNoWallSlide_C", strict(b(no_wall_slide_begin)))?;

    n.enter("AnimNotify_SetAirControl_C", be(air_control_begin, air_control_end))?;
    n.enter("AnimNotify_EvadeFromAttack_C", b(evade_from_attack))?;

    // notifies({ ... }): the classes handled elsewhere or not at all, and the actors' own
    n.enter("AnimNotify_InputSectionJump_C", none())?;
    n.enter("AnimNotify_ConditionSectionJumpSingleCheck_C", b(section_jump_single_begin))?;
    n.enter("AnimNotify_ButtonPressedSectionJump_C", none())?;
    n.enter("AnimNotify_OffWallStopAnim_C", none())?;
    n.enter("AnimNotify_GroundAttack_C", none())?;
    n.enter("AnimNotify_SetCatch_C", none())?;
    n.enter("AnimNotify_OverrideWallSlideInputCheck_C", none())?;
    n.enter("AnimNotify_ServerAcceptClientPosition_C", none())?;
    n.enter(
        "AnimNotify_DestroyUnactivatedMeeseeks_C",
        NotifyHandler { actor_begin: Some(destroy_meeseeks_begin), ..NotifyHandler::default() },
    )?;
    n.enter(
        "AnimNotify_LaunchProjectile_C",
        NotifyHandler { actor_begin: Some(launch_projectile_begin), ..NotifyHandler::default() },
    )?;
    n.enter(
        "AnimNotify_SetProjectileGravity_C",
        NotifyHandler { actor_begin: Some(projectile_gravity_begin), ..NotifyHandler::default() },
    )?;
    n.enter(
        "AnimNotify_PlayMontageNotify",
        NotifyHandler {
            actor_instant: Some(proxy_instant),
            stage_instant: Some(stage_proxy_instant),
            ..NotifyHandler::default()
        },
    )?;
    n.enter(
        "AnimNotify_PlayMontageNotifyWindow",
        NotifyHandler {
            actor_begin: Some(proxy_begin),
            actor_end: Some(proxy_end),
            stage_begin: Some(stage_proxy_begin),
            stage_end: Some(stage_proxy_end),
            ..NotifyHandler::default()
        },
    )?;

    n.enter("AnimNotify_SetFlyingMoveMode_C", be(flying_begin, flying_end))?;
    n.enter("AnimNotify_ConditionSectionJump_C", strict(b(section_jump_begin)))?;
    n.enter("AnimNotify_BlockCooldown_C", be(block_cooldown_begin, block_cooldown_end))?;
    n.enter("AnimNotify_SetSubmeshAlwaysVisible_C", be(always_visible_begin, always_visible_end))?;
    n.enter("AnimNotify_SetPlayerInvulnerable_C", be(invulnerable_begin, invulnerable_end))?;
    n.enter("AnimNotify_SetRotationReverse_C", b(rotation_reverse_begin))?;
    n.enter("AnimNotify_RemoveBuff_C", be(remove_buff_begin, remove_buff_end))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweet_spot_renames_its_members() {
        let mut p = Props::new();
        p.insert("HitBoxTags".into(), Value::Arr(vec![Value::Str("A".into())]));
        p.insert("SweetSpot_Tags".into(), Value::Arr(vec![Value::Str("B".into())]));
        p.insert("SweetSpot_Damage".into(), Value::Num(9.0));
        p.insert("Sweet Spot On Hit Effect Override".into(), Value::Str("Fx".into()));
        let s = sweet_spot(&p);
        let keys: Vec<&str> = s.keys().map(String::as_str).collect();
        assert_eq!(keys, ["HitBoxTags", "Damage", "OnHitEffectOverride"]);
        assert_eq!(strs(&s, "HitBoxTags"), ["B"]);
    }

    #[test]
    fn conditional_packet_takes_the_conditional_members() {
        let mut p = Props::new();
        p.insert("HitBoxTags".into(), Value::Arr(vec![Value::Str("A".into())]));
        p.insert("Damage".into(), Value::Num(5.0));
        p.insert("ConditionalDamage".into(), Value::Num(7.0));
        p.insert("ConditionalKBIMultiplier".into(), Value::Num(2.0));
        let c = conditional_packet(&p);
        assert_eq!(num(&c, "Damage"), Some(7.0));
        assert_eq!(num(&c, "KBI_Multiplier"), Some(2.0));
        assert_eq!(text(&c, "KnockbackTargetSocket"), Some("None"));
        assert!(!flag(&c, "VacuumKnockback"));
        assert_eq!(c.len(), 8);
    }

    #[test]
    fn the_buttons_by_index() {
        assert_eq!(input_button(1.0), Some(ChargeBtn::Attack(AttackBtn::Special)));
        assert_eq!(input_button(3.0), Some(ChargeBtn::Dodge));
        assert_eq!(input_button(4.0), None);
        assert_eq!(input_button(0.5), None);
    }
}
