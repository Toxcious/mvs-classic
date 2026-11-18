//! The Disguise: Buff_TakeFace, Buff_Disguise, AnimNotify_EquipFace and the conditions that
//! read the buff, with the second mesh the buff puts on its owner: a copy of the victim's skeleton with an anim
//! instance of its own (a montage play beside the fighter's: `Disguise::copy`) and clones of the victim's hit and
//! hurt shapes.
//!
//! Source: Buff_Disguise (U@ = its ubergraph, SDM@ = SetupDisguiseMesh), Buff_TakeFace, AnimNotify_EquipFace, the
//! condition graphs. Shared: the owner is any fighter (Pawn_Arya is the one pawn whose data reaches it).
//!
//! What runs here and what is left to the owner's pawn (CharHooks):
//! - the copy's notifies run through the owner's handlers with the owner as `f`, under the keys of instance
//!   `COPY_INST` (`fighter::anim::montages` knows the range); its BranchWindows are not filed: BranchGroupBlocked /
//!   BranchGroupEndBlocked (`foreign_window_begin` / `foreign_window_end`);
//! - "BPI Activate Branch Override" is `CharHooks::activate_branch_override`, AryaDisguiseHasAttackBranch's read is
//!   `CharHooks::disguise_has_attack_branches`; without the hook the first does nothing and the second is false;
//! - the pawn's zeroing of its own shapes during its CopyCat montages is the pawn's (`combat::phases` hit_spheres and
//!   `combat::hit::hurt` name the place).
//!
//! Unverified: the second mesh is ticked right behind the pawn's Mesh (`fighter::tick` mesh tick); the order of the
//! two is the level's tick set order, which only a live read gives.
//! Deviation: the copy's anim instance is its montage alone. The victim's AnimBP on the second mesh (locomotion under
//! the montage's blends, slot mixes, layers, the hit rotation) gives its shapes no pose here: outside a montage the
//! mesh is at scale zero, and inside one the montage's slot track is taken at full weight.

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::boxes::{self, BoxSampler, WorldSphere, sample_of};
use crate::combat::buffs::{BuffExt, buff_mut, buff_of, end_buff, spawn_buff};
use crate::combat::packet::DataVec;
use crate::combat::shapes::{Disguise, ForeignPlay};
use crate::combat::{BuffClass, BuffRef};
use crate::consts;
use crate::events::{DisguiseFxKind, Event, NotifyRef, UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::anim::box_pose::{put_pose, take_pose};
use crate::fighter::anim::branches::{foreign_window_begin, foreign_window_end};
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::montages::{
    COPY_INST, KEY_STRIDE, MontageKey, drop_state, instance_data, montage_data, new_instance, notify_data,
    notify_ticks, running, state_mut, state_ref, truthy,
};
use crate::fighter::anim::notify_sink::{begin_of, end_of, instant_of};
use crate::fighter::anim::taunts::taunt_info_montage;
use crate::fighter::anim::update::nm;
use crate::fighter::anim::visibility::{add_invisibility, invisibility_count, remove_invisibility};
use crate::fighter::anim::{MontageSlot, MontageState, OpenPose, slot_jump_to_section};
use crate::fighter::common::{buff_def, char_data, knock_back_shapes, latent_frames, warn_text};
use crate::fighter::control::{
    AbilityOver, ability_row, data_name_in, emit, override_ability, reset_ability, trigger_ability,
    trigger_ability_check,
};
use crate::fighter::match_side::{surface_sfx, surface_socket};
use crate::fighter::moves::V2;
use crate::fighter::tick::FrameMode;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks, CondFn, Condition, NotifyHandler};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_anim::montage::{Host, Montage, Play};
use mvs_data::boxes::BoxKind;
use mvs_data::value::{Props, Value};

const CLS: Name = name!("Buff_Disguise_C");
/// The key of the copy's notify 0 (`COPY_KEY`).
const COPY_KEY: i32 = COPY_INST * KEY_STRIDE;

/// The EFighter entry name of a character key (the buff's two maps are keyed by the enum). A character without an
/// entry here cannot be copied: the steal is refused with a warning.
fn efighter(key: &str) -> Option<Name> {
    Some(match key {
        "shaggy" => name!("Shaggy"),
        "superman" => name!("Superman"),
        "wonderwoman" => name!("WonderWoman"),
        "harley" => name!("HarleyQuinn"),
        "lebron" => name!("C016"),
        "irongiant" => name!("C017"),
        "morty" => name!("C019"),
        "arya" => name!("Arya"),
        "batman" => name!("Batman"),
        "creature" => name!("ReinDog"),
        "blackadam" => name!("C021"),
        "bugs" => name!("BugsBunny"),
        "rick" => name!("C020"),
        "taz" => name!("C015"),
        "steven" => name!("StevenUniverse"),
        "garnet" => name!("Garnet"),
        "finn" => name!("Finn"),
        "jake" => name!("Jake"),
        "tomandjerry" => name!("TomAndJerry"),
        "velma" => name!("Velma"),
        "marvin" => name!("C018"),
        "stripe" => name!("C023B"),
        "gizmo" => name!("C023A"),
        _ => return None,
    })
}

/// OnOwnerMontageEnded's two montages (U@26729).
const COPYCAT: [Name; 2] = [name!("Arya_CopyCat_Air_montage"), name!("Arya_CopyCat_Ground_montage")];

/// `mapRow`: the value of a `{ Key, Value }` map's row `EFighter::<name>`.
fn map_row<'a>(rows: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    rows?.as_arr()?.iter().find_map(|r| {
        let o = r.as_obj()?;
        let k = o.get("Key")?.as_str()?;
        (k.strip_prefix("EFighter::") == Some(name)).then(|| o.get("Value")).flatten()
    })
}

fn pnum(p: Option<&Props>, k: &str) -> Option<Real> {
    p?.get(k)?.as_f64().map(crate::math::narrow)
}

fn dis(w: &World, f: FighterId) -> Option<&Disguise> {
    w.fighter(f).shapes.disguise.as_ref()
}

fn dis_mut(w: &mut World, f: FighterId) -> Option<&mut Disguise> {
    w.fighter_mut(f).shapes.disguise.as_mut()
}

/// The copy's play number, `None` without a disguise or a copy (`d.copy === cp` compares these).
fn copy_uid(w: &World, f: FighterId) -> Option<u32> {
    dis(w, f)?.copy.as_ref().map(|c| c.m.montage.uid)
}

fn copy_of(w: &World, f: FighterId) -> Option<&ForeignPlay> {
    dis(w, f)?.copy.as_ref()
}

fn copy_mut(w: &mut World, f: FighterId) -> Option<&mut ForeignPlay> {
    dis_mut(w, f)?.copy.as_mut()
}

/// `defOf(f, b)` read for DestroyDisguiseAfterAnimation (`def ?? {}`; the key's default true).
fn destroy_after(w: &World, f: FighterId, b: Option<BuffRef>) -> bool {
    match b.and_then(|_| buff_def(w, ActorRef::Fighter(f), CLS)).and_then(|d| d.get("DestroyDisguiseAfterAnimation")) {
        None | Some(Value::Null) => true,
        v => truthy(v),
    }
}

// ---- the UI state

/// SetUIMulticast(On, Value, SetMax) (U@25344): off removes the HUD state "Disguise"; on adds it with the count (a
/// value below 0 shows no number: Arya as the victim), the max = Value with SetMax, else the last max. PopText: two
/// constants, the empty text U@25561 and "Face Stolen!" U@25581; Clamp(Value, 0, 5) == 5 (U@25797, U@25895)
/// selects the second U@26301. So the steal from a victim without a row in NormalAttackCountPerCharacter
/// (DefaultNormalAttackCount 5) shows it; one with a row (C017 3, C019 2, Arya -1) and every copied neutral (at most
/// remaining - 1) do not. The literals 0 and 5 are the graph's, not the CDO's.
fn set_ui(w: &mut World, f: FighterId, on: bool, value: Real, set_max: bool) {
    let Some(d) = dis_mut(w, f).filter(|_| on) else {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("Disguise")));
        return;
    };
    if set_max {
        d.ui_max = value;
    }
    let max_value = d.ui_max;
    let clamp = min(max(value, 0.0), 5.0);
    let op = UiStateOp::Add(UiStateParams {
        id: name!("Disguise"),
        state_type: 2,
        display_type: i32::from(value < 0.0),
        current_value: if value < 0.0 { -1.0 } else { clamp },
        max_value,
        timer: -1.0,
        icon_material: name!("MI_State_FaceSteal"),
        active: true,
        pop_text: if clamp == 5.0 { name!("Face Stolen!") } else { Name::NONE },
        auto_remove_time: -1.0,
        subicon: None,
    });
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

// ---- the steal

/// InitDisguiseServer(victim), called by Buff_TakeFace right behind SpawnBuffActor.
fn init_disguise(w: &mut World, f: FighterId, b: BuffRef, from: FighterId) {
    let key = w.names.str(w.fighter(from).ch).to_owned();
    let Some(name) = efighter(&key) else {
        warn_text(w, &format!("Buff_Disguise: no EFighter entry for {key}, the steal is refused"));
        end_buff(w, f, b);
        return;
    };
    let names = &w.names;
    let def = buff_def(w, ActorRef::Fighter(f), CLS);
    let from_ch = char_data(w, w.fighter(from).ch);
    let pawn = &from_ch.fighter.pawn;
    let first = |k: &str| -> Option<&Props> {
        pawn.get(k)?
            .as_arr()?
            .iter()
            .filter_map(Value::as_obj)
            .find(|r| pnum(Some(r), "RequiredDirectionInput") == Some(4.0))
    };
    let (g, a) = (first("AttacksNormalGround"), first("AttacksNormalAir"));
    let montage =
        |r: Option<&Props>| r.and_then(|r| r.get("AttackMontage")?.as_str()).map_or(Name::NONE, |s| nm(names, s));
    let cd_of = |r: Option<&Props>| pnum(r, "AbilitySlot").unwrap_or(-1.0) > -1.0;
    let pstr = |k: &str| pawn.get(k).and_then(Value::as_str);
    // GetTauntInfo(victim): ground = the montage of the victim's first TauntDataMap key whose Slug is in
    // TauntMontageCache (a TauntData: a sticker is never cached; `taunt_info_montage`), else SpotDodgeAnim; air =
    // AirEvade, else the first Evades row of direction 4.
    let taunt =
        taunt_info_montage(w, from).unwrap_or_else(|| pstr("SpotDodgeAnim").map_or(Name::NONE, |s| nm(names, s)));
    let air_taunt = match pstr("AirEvade") {
        Some(s) => nm(names, s),
        None => montage(first("Evades")),
    };
    let count =
        map_row(def.and_then(|d| d.get("NormalAttackCountPerCharacter")), names.str(name)).and_then(Value::as_f64);
    let dflt = pnum(def, "DefaultNormalAttackCount").unwrap_or(5.0);
    // DisguiseSubmeshes: the victim's sub meshes with its SetupInitialVisibility state (SDM).
    let base = char_data(w, w.fighter(from).base_ch);
    let sub_hidden = base.def.sub_hidden.iter().map(|s| data_name_in(names, s)).collect();
    let d = Disguise {
        from,
        character: name,
        remaining: crate::math::narrow(
            count.unwrap_or_else(|| f64::from(pnum(def, "NormalAttacksRemaining").unwrap_or(5.0))),
        ),
        ui_max: dflt,
        activated: false,
        neutral: false,
        taunt: false,
        shown: false,
        scaled: false,
        ground: montage(g),
        air: montage(a),
        taunt_ground: taunt,
        taunt_air: air_taunt,
        ground_cd: cd_of(g),
        air_cd: cd_of(a),
        owner_num: pnum(def, "OwnerMontageNumber").unwrap_or(-1.0),
        start_num: 0.0,
        seq: None,
        copy: None,
        wait: None,
        destroy_in: -1,
        sub_hidden,
        hurt_off: Default::default(),
        aim: 0.0,
    };
    // The Cooldowns row of the victim (U@7978-8809): nine values written into the owner's Abilities[0] in place,
    // then OverrideAbility(Abilities[0], 0).
    // Unverified: the row is not written back when the buff ends.
    let cd = map_row(def.and_then(|d| d.get("Cooldowns")), names.str(name)).and_then(Value::as_obj).cloned();
    w.fighter_mut(f).shapes.disguise = Some(d);
    set_ui(w, f, true, crate::math::narrow(count.unwrap_or(f64::from(dflt))), true);
    if let (Some(cd), Some(mut row)) = (cd, ability_row(w, f, 0)) {
        let n = |k: &str| cd.get(k).and_then(Value::as_f64).unwrap_or(f64::from(Real::NAN));
        let flag = |k: &str| truthy(cd.get(k));
        row.cooldown = crate::math::narrow(n("Cooldown"));
        row.icon = cd.get("Icon").and_then(Value::as_str).map(|s| data_name_in(&w.names, s));
        row.max_count = crate::math::narrow(n("MaxCount"));
        row.start_count = crate::math::narrow(n("StartCount"));
        row.count_cooldown = crate::math::narrow(n("CountCooldown"));
        row.count_spent_per_trigger = crate::math::narrow(n("CountSpentPerTrigger"));
        row.count_refreshed_per_cooldown = crate::math::narrow(n("CountRefreshedPerCooldown"));
        row.ally_cooldown_visible = flag("AllyCooldownVisible");
        row.foe_cooldown_visible = flag("FoeCooldownVisible");
        let fi = w.fighter(f);
        let alternate_cooldowns = match fi.abilities.ability_over.as_ref().and_then(|o| o.get(&0)) {
            Some(o) => o.alternate_cooldowns.clone(),
            None => char_data(w, fi.ch).def.abilities.first().and_then(|a| {
                a.alternate_cooldowns.as_ref().map(|v| v.iter().map(|&x| crate::math::narrow(x)).collect())
            }),
        };
        override_ability(w, f, 0, AbilityOver { row, alternate_cooldowns });
    }
}

// ---- the copy's notify sink

/// `copyRunning`: whether a notify of fighter `f` runs inside the sink of its Disguise
/// copy (its MeshComp is the DisguiseMesh; `inSink === f`, set only while the sink's calls run).
pub fn copy_running(w: &World, f: FighterId) -> bool {
    w.scratch.copy_sink == Some(f)
}

/// `sinkOf`'s `call`: a handler run with the copy's sink marked as running.
fn in_sink(w: &mut World, f: FighterId, body: impl FnOnce(&mut World)) {
    let was = w.scratch.copy_sink.replace(f);
    body(w);
    w.scratch.copy_sink = was;
}

/// The copy's play as the player sees it (`sinkOf`): its montage (the victim's), its state found by number on each
/// access, BranchWindows to the owner's foreign window hooks and every other notify to the owner's handlers.
struct CopyHost<'w, 'm> {
    w: &'w mut World,
    f: FighterId,
    key: MontageKey,
    m: &'m Montage,
    spare: MontageState,
}

const BRANCH_WINDOW: &str = "AnimNotify_BranchWindow_C";

impl<'m> Host<'m, MontageKey> for CopyHost<'_, 'm> {
    fn m(&self) -> &'m Montage {
        self.m
    }
    fn s(&mut self) -> &mut MontageState {
        if let Some(s) = state_mut(self.w, self.f, self.key.uid) {
            return s;
        }
        &mut self.spare
    }
    fn begin(&mut self, idx: usize) {
        let (f, n, r) = (self.f, &self.m.notifies[idx], self.r(idx));
        if n.cls == BRANCH_WINDOW {
            foreign_window_begin(self.w, f, r, self.key.name, crate::math::narrow(n.dur));
        } else {
            in_sink(self.w, f, |w| begin_of(w, f, n, r, COPY_KEY + ix(idx)));
        }
    }
    fn end(&mut self, idx: usize) {
        let (f, n, r) = (self.f, &self.m.notifies[idx], self.r(idx));
        if n.cls == BRANCH_WINDOW {
            foreign_window_end(self.w, f, r);
        } else {
            in_sink(self.w, f, |w| end_of(w, f, n, r, COPY_KEY + ix(idx)));
        }
    }
    fn instant(&mut self, idx: usize) {
        let (f, n, r) = (self.f, &self.m.notifies[idx], self.r(idx));
        in_sink(self.w, f, |w| instant_of(w, f, n, r));
    }
}

impl CopyHost<'_, '_> {
    fn r(&self, idx: usize) -> NotifyRef {
        NotifyRef { montage: self.key.name, index: u32::try_from(idx).expect("a notify index") }
    }
}

fn ix(i: usize) -> i32 {
    i32::try_from(i).expect("a notify index")
}

/// Runs `body` over the player of the copy `uid` (kept in limbo if a handler drops it while it runs).
fn with_copy<R>(w: &mut World, f: FighterId, uid: u32, body: impl FnOnce(&mut CopyHost<'_, '_>) -> R) -> Option<R> {
    let data = w.data.clone();
    let m = instance_data(&data, w, f, uid)?;
    let key = state_ref(w, f, uid)?.montage;
    Some(running(w, f, uid, |w| body(&mut CopyHost { w, f, key, m, spare: MontageState::new(key, false) })))
}

/// The pose of the copy (`cp.m.pose()`): its current anim and time.
fn copy_pose(w: &World, f: FighterId) -> Option<(Name, Real)> {
    let uid = copy_uid(w, f)?;
    crate::fighter::anim::update::inst_pose(&w.data, w, f, uid, None)
}

/// `setPrev`.
fn set_prev(w: &mut World, f: FighterId) {
    let p = copy_pose(w, f);
    if let Some(c) = copy_mut(w, f) {
        c.prev = p;
    }
}

/// `endStates`: the states of a copy that was stopped end now (its Terminate and the dispatch behind it).
fn end_states(w: &mut World, f: FighterId, uid: u32) {
    if state_ref(w, f, uid).is_none_or(|s| s.active.is_empty()) {
        return;
    }
    with_copy(w, f, uid, |h| {
        h.end_branching();
        h.end_queued();
    });
}

/// `dropCopy`.
fn drop_copy(w: &mut World, f: FighterId, uid: u32) {
    end_states(w, f, uid);
    let c = &mut w.fighter_mut(f).charge;
    if c.charge_notify >= COPY_KEY && c.charge_notify < COPY_KEY + KEY_STRIDE {
        c.charge_notify = -1;
    }
    if copy_uid(w, f) == Some(uid) {
        let cp = dis_mut(w, f).and_then(|d| d.copy.take()).expect("the copy");
        drop_state(w, f, cp.m);
    }
}

/// CreateProxyObjectForPlayMontage(DisguiseMesh, montage, rate 1, start 0): Montage_Play on the second mesh's anim
/// instance.
/// Deviation: an instance still in the list from a stop is over for the port at once; its queued states would end at
/// the mesh's next dispatch.
fn start_copy(w: &mut World, f: FighterId, name: Name, neutral: bool, ground: bool) -> bool {
    let from = dis(w, f).expect("a disguise").from;
    if montage_data(w, from, name).is_none() {
        return false;
    }
    if let Some(uid) = copy_uid(w, f) {
        drop_copy(w, f, uid);
    }
    let m = new_instance(w, f, name, ground);
    dis_mut(w, f).expect("a disguise").copy =
        Some(ForeignPlay { m, stopped: false, blend_out: false, prev: None, neutral: Some(neutral) });
    set_prev(w, f);
    true
}

/// Set Disguise Visibility(on): on a change CurrentlyDisguised = on and the owner's AddInvisibility /
/// RemoveInvisibility.
fn set_visibility(w: &mut World, f: FighterId, on: bool) {
    let Some(d) = dis_mut(w, f) else { return };
    if d.shown == on {
        return;
    }
    d.shown = on;
    if on {
        add_invisibility(w, f);
    } else {
        remove_invisibility(w, f);
    }
}

/// SetDisguisedMirrorHitBoxEnabled (U@28236): the owner's handler flag = the victim's while a copy plays, false after
/// it (and from then on: nothing writes the owner's own value back).
fn set_mirror(w: &mut World, f: FighterId, on: bool) {
    let v = on && {
        let from = dis(w, f).expect("a disguise").from;
        char_data(w, w.fighter(from).ch)
            .fighter
            .components
            .pawn
            .get("Comp_Pawn_HitBoxManager_GEN_VARIABLE")
            .and_then(|c| match &c.p {
                Value::Obj(p) => p.get("IsHitBoxMirroringEnabled"),
                _ => None,
            })
            .and_then(Value::as_bool)
            .unwrap_or(true)
    };
    w.fighter_mut(f).vis.hit_mirror = Some(v);
}

fn branch_override(w: &mut World, f: FighterId, on: bool) {
    if let Some(h) = w.fighter(f).hooks.char.activate_branch_override {
        h(w, f, on);
    }
}

/// PlayDisguiseNeutralAttackServer(onGround) (U@1666) and MulticastDisguiseNeutralAttack (U@27034).
fn play_neutral(w: &mut World, f: FighterId, ground: bool) {
    let d = dis(w, f).expect("a disguise");
    let (name, neutral, from) = (if ground { d.ground } else { d.air }, d.neutral, d.from);
    if neutral || montage_data(w, from, name).is_none() {
        // Delay(0) and again, for ever while the montage is not valid (U@1611 -> 1109)
        if !neutral {
            let key = w.names.str(w.fighter(from).ch);
            let kind = if ground { "ground" } else { "air" };
            warn_text(w, &format!("Buff_Disguise: no {kind} neutral montage for {key} ({})", w.names.str(name)));
        }
        dis_mut(w, f).expect("a disguise").wait = Some((true, ground));
        return;
    }
    dis_mut(w, f).expect("a disguise").neutral = true;
    set_mirror(w, f, true);
    let d = dis_mut(w, f).expect("a disguise");
    d.scaled = true;
    d.remaining -= 1.0;
    let v = if d.character == name!("Arya") { -1.0 } else { max(d.remaining, 0.0) };
    set_ui(w, f, true, v, false);
    branch_override(w, f, true);
    start_copy(w, f, name, true, ground);
    set_visibility(w, f, true); // SetDisguiseVisibilityFlow(true): the instance has its active montage at once
    if let Some(d) = dis_mut(w, f) {
        d.start_num = d.owner_num;
    }
}

/// PlayDisguiseAttack(onGround) (U@22842 -> 308): the copied taunt.
fn play_taunt(w: &mut World, f: FighterId, destroy_after: bool, ground: bool) {
    let d = dis(w, f).expect("a disguise");
    if d.taunt {
        return;
    }
    let (name, from) = (if ground { d.taunt_ground } else { d.taunt_air }, d.from);
    if montage_data(w, from, name).is_none() {
        let key = w.names.str(w.fighter(from).ch);
        let kind = if ground { "ground" } else { "air" };
        warn_text(w, &format!("Buff_Disguise: no {kind} taunt montage for {key} ({})", w.names.str(name)));
        dis_mut(w, f).expect("a disguise").wait = Some((false, ground));
        return;
    }
    let d = dis_mut(w, f).expect("a disguise");
    d.taunt = true;
    d.start_num = d.owner_num;
    set_mirror(w, f, true);
    dis_mut(w, f).expect("a disguise").scaled = true;
    if destroy_after {
        set_ui(w, f, false, 0.0, false);
    }
    start_copy(w, f, name, false, ground);
    set_visibility(w, f, true);
}

/// The owner's MontageNavigation.JumpToSectionLogic("EndDisguiseAttack", null): its active montage (a montage
/// without the section: nothing).
fn jump_end_disguise_attack(w: &mut World, f: FighterId) {
    if let Some(slot) = active_instance(w, f) {
        slot_jump_to_section(w, f, slot, name!("EndDisguiseAttack"));
    }
}

/// MulticastHandleSectionJump (U@6445 -> U@27998).
fn end_jump(w: &mut World, f: FighterId) {
    if dis(w, f).is_some_and(|d| d.owner_num == d.start_num) {
        jump_end_disguise_attack(w, f);
    }
}

/// The end block of a copied neutral (U@6557).
fn neutral_end_block(w: &mut World, f: FighterId) {
    let Some(d) = dis_mut(w, f).filter(|d| d.neutral) else { return };
    d.neutral = false;
    set_mirror(w, f, false);
    dis_mut(w, f).expect("a disguise").scaled = false;
    set_visibility(w, f, false);
    end_jump(w, f);
}

/// The taunt copy's end (U@22886 -> U@12043). True: K2_DestroyActor.
fn taunt_end(w: &mut World, f: FighterId, destroy_after: bool) -> bool {
    let Some(d) = dis_mut(w, f).filter(|d| d.taunt) else { return false };
    d.taunt = false;
    set_mirror(w, f, false);
    let d = dis_mut(w, f).expect("a disguise");
    d.scaled = false;
    if d.owner_num != d.start_num {
        return false;
    }
    jump_end_disguise_attack(w, f);
    destroy_after
}

/// UPlayMontageCallbackProxy's three events for the copy's play (`proxyEvent`): OnBlendOut then OnCompleted for a
/// play that ends by itself, OnInterrupted alone for one stopped before its blend-out, OnBlendOut then OnInterrupted
/// for one stopped inside it. `neutral`: the play's (`cp.neutral`); `done`: Completed or Interrupted.
fn proxy_event(w: &mut World, f: FighterId, neutral: bool, done: bool) {
    let b = buff_of(w, f, CLS);
    if !neutral {
        let da = destroy_after(w, f, b);
        if taunt_end(w, f, da)
            && let Some(b) = b
        {
            end_buff(w, f, b);
        }
        return;
    }
    branch_override(w, f, false);
    neutral_end_block(w, f);
    if !done {
        return;
    }
    // U@6518, behind the block whether or not it ran: the count's test, Delay(0.1), K2_DestroyActor
    if dis(w, f).is_some_and(|d| d.remaining <= 0.0) {
        set_ui(w, f, false, 0.0, false);
        let n = crate::math::to_int32(latent_frames(0.1));
        dis_mut(w, f).expect("a disguise").destroy_in = n;
    }
}

/// StopDisguiseAttack(seq) (U@22902): one of the two flags set; a valid seq must be CurrentAnimSequence; the second
/// mesh's Montage_Stop(0.0, null), whose interrupt reaches the proxy inside the call (bQueueMontageEvents is clear
/// outside Montage_Advance); then the neutral's end block or the taunt's end.
/// Unverified: a stop made from a handler of the copy's own Advance has its event queued to that mesh's dispatch in
/// the build; here it runs in the call.
fn stop_disguise_attack(w: &mut World, f: FighterId, seq: Option<Name>) {
    let d = dis(w, f).expect("a disguise");
    if !d.neutral && !d.taunt {
        return;
    }
    if seq.is_some() && seq != d.seq {
        return;
    }
    stop_copy_montage(w, f);
    let b = buff_of(w, f, CLS);
    let Some(d) = dis(w, f) else { return };
    if d.neutral {
        branch_override(w, f, false);
        neutral_end_block(w, f);
    } else {
        let da = destroy_after(w, f, b);
        if taunt_end(w, f, da)
            && let Some(b) = b
        {
            end_buff(w, f, b);
        }
    }
}

/// `stopCopyMontage`: Montage_Stop(0.0) on the second mesh's anim instance (also the
/// owner pawn's "Movement Stop Anim On Disguise" path: CharHooks code calls it). Its notify states end at that
/// mesh's next update (`copy_tick`).
pub fn stop_copy_montage(w: &mut World, f: FighterId) {
    let Some(cp) = copy_mut(w, f).filter(|c| !c.stopped) else { return };
    cp.stopped = true;
    cp.m.ended = true; // halt
    let (blend_out, neutral) = (cp.blend_out, cp.neutral.unwrap_or(false));
    // an instance already in its own blend-out keeps OnInterrupted for its end (`copy_tick`)
    if !blend_out {
        proxy_event(w, f, neutral, true);
    }
}

/// `copyTick`: the second mesh's anim instance update, behind the owner's Mesh in the
/// fighter's mesh tick.
pub fn copy_tick(w: &mut World, f: FighterId, mode: FrameMode) {
    let Some(cp) = copy_of(w, f) else { return };
    let (uid, stopped, neutral) = (cp.m.montage.uid, cp.stopped, cp.neutral.unwrap_or(false));
    if stopped {
        // the dispatch behind the stop ends its queued states (under a hit pause too); its Terminate the others
        if mode == FrameMode::Paused {
            if !cp.m.active.is_empty() {
                with_copy(w, f, uid, |h| h.end_queued());
            }
            return;
        }
        let late = cp.blend_out;
        drop_copy(w, f, uid);
        if late {
            proxy_event(w, f, neutral, true);
        }
        return;
    }
    if mode == FrameMode::Dead {
        return;
    }
    // Under a hit pause the mesh ticks dilated (0.005), as the pawn's Mesh: the montage moves by that share of the
    // frame with its notifies and NotifyTicks.
    let dil = if mode == FrameMode::Paused { consts::HITPAUSE_DILATION } else { 1.0 };
    set_prev(w, f);
    let prev = copy_of(w, f).and_then(|c| c.prev);
    let traces = w.fighter(f).frame.surface_traces.len();
    let (alive, now) = with_copy(w, f, uid, |h| {
        let alive = h.advance(consts::DT * dil, false, dil != 1.0);
        let m = h.m;
        let now = h
            .s()
            .pose(m, None)
            .map(|(si, gi, t)| (nm(&h.w.names, &m.slots[si].segments[gi].anim), crate::math::narrow(t)));
        (alive, now)
    })
    .expect("the copy's montage in its victim's data");
    flush_copy_traces(w, f, prev, now, traces);
    // ended or stopped by one of its own handlers
    if copy_uid(w, f) != Some(uid) || copy_of(w, f).is_some_and(|c| c.stopped) {
        return;
    }
    let cp = copy_of(w, f).expect("the copy");
    if cp.m.blending_out() && !cp.blend_out {
        copy_mut(w, f).expect("the copy").blend_out = true;
        proxy_event(w, f, neutral, false);
        if copy_uid(w, f) != Some(uid) {
            return;
        }
    }
    if !alive {
        let blend_out = copy_of(w, f).expect("the copy").blend_out;
        drop_copy(w, f, uid);
        if !blend_out {
            proxy_event(w, f, neutral, false);
        }
        proxy_event(w, f, neutral, true);
        return;
    }
    notify_ticks(w, f, MontageSlot::Foreign, uid, COPY_INST);
}

/// `mirrorSectionJump`: MontageNavigation's AdditionalMeshes: a section jump on the owner
/// is made on the second mesh's current montage as well (Montage_JumpToSection: a montage without the section stays
/// where it is). `None`: the jump to the current section's end (JumpToSectionsEnd).
pub fn mirror_section_jump(w: &mut World, f: FighterId, section: Option<Name>) {
    let Some(uid) = copy_of(w, f).filter(|c| !c.stopped).map(|c| c.m.montage.uid) else { return };
    let data = w.data.clone();
    let Some(md) = instance_data(&data, w, f, uid) else { return };
    let sec = section.map(|s| w.names.str(s).to_owned());
    let Some(s) = state_mut(w, f, uid) else { return };
    match sec {
        None => {
            let i = s.section_index_at(md, s.pos);
            s.jump_to_section_end(md, i);
        }
        Some(sec) => {
            s.jump_to_section(md, &sec);
        }
    }
}

/// `ownerMontageEvent`: OnOwnerMontageEnded (U@26729), bound to the owner mesh's
/// OnMontageEnded and OnMontageBlendingOut.
pub fn owner_montage_event(w: &mut World, f: FighterId, name: Name) {
    if let Some(d) = dis_mut(w, f)
        && COPYCAT.contains(&name)
    {
        d.owner_num += 1.0;
    }
}

/// `setCopyAim`: BPI_SetAimDirection on the second mesh's anim instance, from a notify of
/// its montage.
pub fn set_copy_aim(w: &mut World, f: FighterId, v: Real) {
    if let Some(d) = dis_mut(w, f) {
        d.aim = v;
    }
}

/// `disguiseShown`: OnFighterVisibilityUpdated's `shown` (U@21861): the second mesh is
/// drawn (and the owner's own meshes are not).
pub fn disguise_shown(w: &World, f: FighterId) -> bool {
    dis(w, f).is_some_and(|d| d.activated && d.shown) && invisibility_count(w, f) == 1.0
}

// ---- the clone shapes

/// The Y place of the Disguise's second mesh.
pub fn disguise_mesh_y(w: &World, f: FighterId) -> Real {
    match dis(w, f) {
        Some(d) => clone_y(w, f, d.from),
        None => crate::fighter::actor_y(w, f),
    }
}

/// The second mesh hangs on the owner's capsule at Z + (victim half height - owner half height), + CrouchOffset
/// (owner half height - owner radius) while the owner crouches (SDM@1001-1621, U@16850-17120), template sizes; the
/// victim's box tracks are about its own capsule centre, so that is where their origin is. X: the owner's.
fn clone_y(w: &World, f: FighterId, from: FighterId) -> Real {
    let fi = w.fighter(f);
    let o = &char_data(w, fi.base_ch).def;
    let v = &char_data(w, w.fighter(from).base_ch).def;
    let crouch = if fi.capsule.cmc_crouched { o.cap_half - o.cap_radius } else { 0.0 };
    crate::fighter::actor_y(w, f)
        + crate::math::narrow((v.cap_half - o.cap_half + crouch) * f64::from(fi.body.size_scale))
}

/// The victim's box sampler.
fn sampler_of(w: &World, from: FighterId) -> &BoxSampler {
    let ch = w.fighter(from).ch;
    w.fixed.samplers.get(&ch).unwrap_or_else(|| panic!("no box sampler for {}", w.names.str(ch)))
}

/// `clonePose`: the victim's box tracks at the copy's pose (`at`: a pose kept from an earlier update), at the
/// owner's scale, in a buffer of the World's pool ([`put_pose`] hands it back). None: the clones are not there
/// (scale zero, unverified; `any_scale` reads the pose all the same) or the pose has no tracks.
fn clone_pose(w: &World, f: FighterId, at: Option<(Name, Real)>, any_scale: bool) -> Option<Vec<f32>> {
    let d = dis(w, f)?;
    if !d.scaled && !any_scale {
        return None;
    }
    let (anim, time) = match at {
        Some(p) => p,
        None => copy_pose(w, f)?,
    };
    let ch = w.fighter(d.from).ch;
    let stride = usize::try_from(char_data(w, ch).boxes.stride).expect("a box stride");
    let mut out = take_pose(w);
    out.resize(stride, 0.0);
    if !sample_of(w, ch, w.names.str(anim), time, &mut out) {
        put_pose(w, out);
        return None;
    }
    let k = w.fighter(f).body.size_scale;
    if k != 1.0 {
        sampler_of(w, d.from).scale(&mut out, k);
    }
    Some(out)
}

/// `flushCopyTraces`: the Ak_SurfaceEffect notifies of the copy's montage this update: Received_Notify traces from
/// MeshComp.GetSocketLocation(TraceFrom), and MeshComp is the second mesh, so the socket is the victim's skeleton's at
/// the copy's pose (a queued one's after the update, a branching-point one's before it), placed as the clones are.
/// The handler files them in the fighter's surface traces from `from` on; they are traced here, in the second mesh's
/// tick, and taken off the list.
/// Deviation: at scale zero (after the end block of the copied neutral, in its blend-out) the build's socket is at the
/// component's origin; the port takes the pose's root track for it.
fn flush_copy_traces(w: &mut World, f: FighterId, prev: Option<(Name, Real)>, now: Option<(Name, Real)>, from: usize) {
    if w.fighter(f).frame.surface_traces.len() <= from {
        return;
    }
    let list = w.fighter_mut(f).frame.surface_traces.split_off(from);
    for t in &list {
        let Some(d) = dis(w, f) else { break };
        let (vic, scaled) = (d.from, d.scaled);
        let sock = w.names.str(t.sock).to_owned();
        let b = sampler_of(w, vic);
        let i = surface_socket(w, b, &sock);
        let at = if t.queued { now } else { prev };
        let pose = at.and_then(|at| clone_pose(w, f, Some(at), true));
        let (Some(i), Some(pv)) = (i, pose.as_ref()) else {
            warn_text(
                w,
                &if i.is_none() {
                    format!("surface trace socket not sampled on the disguise mesh: {sock}")
                } else {
                    format!("surface trace: no disguise pose to read {sock} from")
                },
            );
            if let Some(p) = pose {
                put_pose(w, p);
            }
            // the port's fallback, as for the pawn's Mesh
            let oy = w.fighter(f).body.y - crate::fighter::cap_half(w, f);
            surface_sfx(w, f, t.ev, oy, false, None, None);
            continue;
        };
        let o = if scaled { b.offsets[i as usize] as usize } else { 0 };
        let (px, py) = (crate::real(pv[o]), crate::real(pv[o + 1]));
        let fi = w.fighter(f);
        let x = fi.body.x + px * fi.body.facing_dir;
        let oy = clone_y(w, f, vic) + py;
        if let Some(p) = pose {
            put_pose(w, p);
        }
        surface_sfx(w, f, t.ev, oy, false, None, Some(x));
    }
}

/// The clones are ordered behind every shape of the owner (they are added to the actor later).
/// Unverified: the order.
const ORDER: i32 = 2048;

/// `cloneHitSpheres`: the hit shapes of the clones carrying a tag (every live tag is
/// resolved on both sets while the second mesh is scaled), pushed behind the owner's own. `two`: the owner's "Use 2D
/// Hitboxes" (GetHitBoxes reads the owner's flag): the components tagged tag + "2D". `hs`: the tag's HitBoxScale,
/// `lim` the caller's clamp of a scale. Returns whether a clone carries the tag at all, so that a tag only the victim
/// has is not reported as missing. A clone of a detector is made on the profile HitBox (SetCollisionProfileName
/// HitBox, SDM@12343): it meets every test.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn clone_hit_spheres(
    w: &World,
    f: FighterId,
    tag: &str,
    two: bool,
    hs: &DataVec,
    lim: &dyn Fn(Real) -> Real,
    at: Option<OpenPose>,
    out: &mut Vec<WorldSphere>,
) -> bool {
    let Some(d) = dis(w, f) else { return false };
    let from_ch = char_data(w, w.fighter(d.from).ch);
    let b = sampler_of(w, d.from);
    let shapes = &from_ch.boxes.shapes;
    let list = if two { b.tagged(&format!("{tag}2D")) } else { b.tagged(tag) };
    // The victim's shapes on the KnockBack profile (its FullBody; knock_back_shapes) have object type WorldStatic,
    // not 16 or 30, so they have no clone (SDM@11433-11587).
    let kb = knock_back_shapes(from_ch);
    let cloned = |i: u32| {
        let sh = &shapes[i as usize];
        sh.kind == BoxKind::Hit && !kb.contains(&sh.name)
    };
    if !list.iter().any(|&i| cloned(i)) {
        return !two && !b.tagged(&format!("{tag}2D")).is_empty();
    }
    let Some(pose) = clone_pose(w, f, at.map(|p| (p.anim, p.time)), false) else { return true };
    let fi = w.fighter(f);
    let (x, y, facing) = (fi.body.x, clone_y(w, f, d.from), fi.body.facing_dir);
    let scale = lim(hs.x.unwrap_or(1.0));
    let at = |k: usize| crate::real(pose[k]);
    for &i in list {
        if !cloned(i) {
            continue;
        }
        let sh = &shapes[i as usize];
        let o = b.offsets[i as usize] as usize;
        let order = Some(ORDER + sh.order.unwrap_or_else(|| ix(i as usize)));
        if sh.is_box == Some(true) {
            let mut v = [0.0; 9];
            for k in 0..3 {
                let part = match k {
                    0 => hs.x,
                    1 => hs.y,
                    _ => hs.z,
                };
                let s = lim(part.unwrap_or(1.0));
                v[k * 3] = at(o + 3 + k * 3) * facing * s;
                v[k * 3 + 1] = at(o + 4 + k * 3) * s;
                v[k * 3 + 2] = at(o + 5 + k * 3) * s;
            }
            out.push(WorldSphere {
                x: x + at(o) * facing,
                y: y + at(o + 1),
                z: Some(at(o + 2)),
                r: 0.0,
                order,
                bx: Some(v),
                ..WorldSphere::default()
            });
        } else {
            out.push(WorldSphere {
                x: x + at(o) * facing,
                y: y + at(o + 1),
                z: if two { None } else { Some(at(o + 2)) },
                r: crate::math::narrow(sh.radius * f64::from(scale) * f64::from(at(o + 3))),
                order,
                ..WorldSphere::default()
            });
        }
    }
    put_pose(w, pose);
    true
}

/// `cloneOff`: the `off` set of the clone hurt tests: the clones UpdateHurtBoxes turned off and the victim's shapes
/// that are not cloned. The clone list is VictimFighter.Mesh.GetChildrenComponents(true) (U@9199 / U@10411 /
/// U@11235), so a shape on the root capsule (Pawn_BatmanV2's HurtBoxGlide, a CollisionCylinder child) has no clone.
fn clone_off(w: &World, d: &Disguise) -> indexmap::IndexSet<i32> {
    let shapes = &char_data(w, w.fighter(d.from).ch).boxes.shapes;
    let mut off = d.hurt_off.clone();
    for (i, sh) in shapes.iter().enumerate() {
        if sh.on_capsule == Some(true) {
            off.insert(ix(i));
        }
    }
    off
}

/// The clone hurt shapes' test frame: the pose, the place, the facing and the off set.
fn clone_hurt<R>(
    w: &World,
    vic: FighterId,
    test: impl FnOnce(&BoxSampler, &[f32], Real, Real, Real, &indexmap::IndexSet<i32>) -> R,
) -> Option<R> {
    let pose = clone_pose(w, vic, None, false)?;
    let d = dis(w, vic).expect("a disguise");
    let off = clone_off(w, d);
    let fi = w.fighter(vic);
    let r = test(sampler_of(w, d.from), &pose, fi.body.x, clone_y(w, vic, d.from), fi.body.facing_dir, &off);
    put_pose(w, pose);
    Some(r)
}

/// `cloneHurtHit`: the clone hurt shapes (the owner's components for the length of a
/// copy, beside its own): the first sphere that touches one.
pub fn clone_hurt_hit(w: &mut World, vic: FighterId, sp: &[WorldSphere]) -> Option<WorldSphere> {
    clone_hurt(w, vic, |b, pose, x, y, fc, off| boxes::spheres_hit_hurt(b, sp, pose, x, y, fc, None, Some(off)))
        .flatten()
}

/// `cloneFirstHurt`: the place of the first clone hurt shape a sphere touches, -1 none.
pub fn clone_first_hurt(w: &mut World, vic: FighterId, s: &WorldSphere) -> Real {
    clone_hurt(w, vic, |b, pose, x, y, fc, off| boxes::first_hurt(b, s, pose, x, y, fc, None, Some(off)))
        .unwrap_or(-1.0)
}

/// `cloneBoxHitsHurt`: the same shapes against a box hitbox.
pub fn clone_box_hits_hurt(w: &mut World, vic: FighterId, cx: Real, cy: Real, hx: Real, hy: Real) -> bool {
    clone_hurt(w, vic, |b, pose, x, y, fc, off| {
        boxes::box_hits_hurt(b, cx, cy, hx, hy, pose, x, y, fc, None, Some(off))
    })
    .unwrap_or(false)
}

/// `cloneHurtsTouched`: for a caller that keeps the begin of an overlap per component: the
/// places of the clones touched are added to `out` from `base` on.
pub fn clone_hurts_touched(w: &mut World, vic: FighterId, s: &WorldSphere, base: Real, out: &mut Vec<Real>) {
    let mut touched = Vec::new();
    let got = clone_hurt(w, vic, |b, pose, x, y, fc, off| {
        boxes::hurts_touched(b, s, pose, x, y, fc, None, Some(off), &mut touched)
    });
    if got.is_some() {
        out.extend(touched.iter().map(|&k| base + k));
    }
}

/// `cloneHurtBoxes`: AnimNotify_HurtBoxUpdate's UpdateHurtBoxes by tag reaches the clones
/// too (GetComponentsByTag on the owner); a shape with no clone is not among the owner's components.
pub fn clone_hurt_boxes(w: &mut World, f: FighterId, tags: &[Name], on: bool) {
    let Some(d) = dis(w, f) else { return };
    let shapes = &char_data(w, w.fighter(d.from).ch).boxes.shapes;
    let b = sampler_of(w, d.from);
    let mut flip = Vec::new();
    for &t in tags {
        for &i in b.tagged(w.names.str(t)) {
            let sh = &shapes[i as usize];
            if !sh.tags.iter().any(|x| x == "HurtBox") || sh.on_capsule == Some(true) {
                continue;
            }
            flip.push(ix(i as usize));
        }
    }
    let d = dis_mut(w, f).expect("a disguise");
    for i in flip {
        if on {
            d.hurt_off.shift_remove(&i);
        } else {
            d.hurt_off.insert(i);
        }
    }
}

/// `cloneSubMesh`: a sub mesh tag of the second mesh (DisguiseSubmeshes by tag): true when
/// the tag is one of the clones'. BannedTags (CDO: AryaBodyMesh -> DisguiseBodyMesh, AryaSword -> DisguiseSword,
/// AryaSheath -> DisguiseSheath): a clone whose tag is a key carries the value instead, so the key names the owner's
/// own mesh alone. `tag` in lower case; DisguiseSubmeshes is kept by the victim's own tag.
pub fn clone_sub_mesh(w: &mut World, f: FighterId, tag: Name, visible: bool) -> bool {
    let Some(d) = dis(w, f) else { return false };
    let tag_s = w.names.str(tag);
    let mut t = tag_s.to_owned();
    let banned = buff_def(w, ActorRef::Fighter(f), CLS).and_then(|d| d.get("BannedTags")).and_then(Value::as_arr);
    for r in banned.unwrap_or_default().iter().filter_map(Value::as_obj) {
        let (k, v) =
            (r.get("Key").and_then(Value::as_str).unwrap_or(""), r.get("Value").and_then(Value::as_str).unwrap_or(""));
        if v.to_lowercase() == tag_s {
            t = k.to_lowercase();
        } else if k.to_lowercase() == tag_s {
            return false;
        }
    }
    if !char_data(w, w.fighter(d.from).base_ch).def.sub_meshes.contains(&t) {
        return false;
    }
    let t = data_name_in(&w.names, &t);
    let d = dis_mut(w, f).expect("a disguise");
    if visible {
        d.sub_hidden.shift_remove(&t);
    } else {
        d.sub_hidden.insert(t);
    }
    true
}

/// `disguiseMeshRead`: BPI_CheckIsDisguised (BaseFighter @305-351): the fighter has the
/// buff and it plays a copied neutral. Then Comp_Spawner reads a socket on the second mesh (@651-720), whichever
/// montage the Spawner is of.
pub fn disguise_mesh_read(w: &World, f: FighterId) -> bool {
    dis(w, f).is_some_and(|d| d.neutral) && buff_of(w, f, CLS).is_some()
}

/// `copySpawnPoint`: a Spawner notify while [`disguise_mesh_read`]: the point (socket +
/// the notify's translation) on the second mesh at the request, for the World's spawn; `None`: no socket (the actor's
/// location, as for any spawner) or no track for it (warned).
pub fn copy_spawn_point(w: &mut World, f: FighterId, p: &Props) -> Option<V2> {
    let d = dis(w, f)?;
    let sock = p.get("Socket").and_then(Value::as_str).filter(|s| *s != "None")?;
    let b = sampler_of(w, d.from);
    let si = w.names.get(&format!("socket:{sock}")).and_then(|n| b.by_name.get(&n).copied());
    let pose = si.and_then(|_| clone_pose(w, f, None, false));
    let (Some(si), Some(pose)) = (si, pose) else {
        warn_text(w, &format!("spawn socket missing on the disguise mesh: {sock}"));
        return None;
    };
    let o = b.offsets[si as usize] as usize;
    let t = p.get("Transforms").and_then(Value::as_obj).and_then(|t| t.get("Translation")).and_then(Value::as_obj);
    let fi = w.fighter(f);
    let r = V2 {
        x: fi.body.x + (crate::real(pose[o]) + pnum(t, "Y").unwrap_or(0.0)) * fi.body.facing_dir,
        y: clone_y(w, f, d.from) + crate::real(pose[o + 1]) + pnum(t, "Z").unwrap_or(0.0),
    };
    put_pose(w, pose);
    Some(r)
}

// ---- the registry

/// `has`: the disguise while the buff lives.
fn has(w: &World, f: FighterId) -> Option<&Disguise> {
    dis(w, f).filter(|_| buff_of(w, f, CLS).is_some())
}

/// Buff_TakeFace BeginPlay (U@1641): on the hit fighter, Instigator = the stealer. Both need a
/// Comp_Actor_Destructible (every fighter has one). Same team: the stealer's TriggerAbilityCheck(3), failed: the buff
/// stays and nothing else happens; passed: TriggerAbilityActual(3). Then (U@399), unless the stealer has a
/// Buff_Disguise already (@618) or the hit fighter is a NotPlayerCharacter (@857 / @903: a minion), SpawnBuffActor(
/// Buff_Disguise, stealer, stealer, 0 stacks, 0.0) and InitDisguiseServer(hit fighter); K2_DestroyActor (@941 / @955
/// to @10 either way).
fn take_face_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _: Option<Real>) {
    let Some(from) = from else { return };
    if w.fighter(from).team.team == w.fighter(f).team.team {
        if !trigger_ability_check(w, from, 3) {
            return;
        }
        trigger_ability(w, from, 3);
    }
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::TakeFace { done: true };
    }
    if !w.fighter(f).life.not_player_character {
        disguise_as(w, from, f, false);
    }
}

/// The K2_DestroyActor of BeginPlay, at the buff's first tick (its spawn is still on the stack in its begin).
fn take_face_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    crate::combat::buffs::buff(w, b).is_some_and(|x| matches!(x.ext, BuffExt::TakeFace { done: true }))
}

/// Buff_Disguise ReceiveTick and the graph's latent actions.
fn disguise_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if dis(w, f).is_none() {
        return false;
    }
    let da = destroy_after(w, f, Some(b));
    let d = dis_mut(w, f).expect("a disguise");
    if d.destroy_in >= 0 {
        d.destroy_in -= 1;
        if d.destroy_in < 0 {
            return true;
        }
    }
    if let Some((neutral, ground)) = d.wait.take() {
        if neutral {
            play_neutral(w, f, ground);
        } else {
            play_taunt(w, f, da, ground);
        }
    }
    let aim = w.fighter(f).pose.aim_direction;
    if let Some(d) = dis_mut(w, f).filter(|d| d.activated) {
        d.aim = aim; // U@17885: the owner's anim instance's AimDirection, each tick
    }
    false
}

/// The owner's KnockedBack (U@27833 -> U@27848).
fn disguise_on(w: &mut World, f: FighterId, _b: BuffRef, sig: BuffSignal, _a: &BuffSignalArg) -> bool {
    let Some(d) = dis(w, f).filter(|_| sig == BuffSignal::KnockedBack) else { return false };
    if d.neutral || d.taunt {
        stop_disguise_attack(w, f, None);
    } else if d.shown {
        set_visibility(w, f, false);
    }
    false
}

/// Buff_Disguise ReceiveEndPlay.
fn disguise_end(w: &mut World, f: FighterId, _b: BuffRef) {
    let Some(d) = dis_mut(w, f) else { return };
    d.destroy_in = -1;
    d.wait = None;
    if d.neutral || d.taunt {
        // StopDisguiseAttack(null) (U@16174) with the actor on its way out: the flags and the end blocks, the taunt's
        // section jump too (U@22886 -> U@12043-12227); no second destroy
        if let Some(cp) = d.copy.as_mut().filter(|c| !c.stopped) {
            cp.stopped = true;
            cp.m.ended = true; // halt
        }
        if d.neutral {
            branch_override(w, f, false);
            neutral_end_block(w, f);
        } else {
            taunt_end(w, f, true);
        }
    }
    // the mesh component goes with the actor: its states end here
    if let Some(uid) = copy_uid(w, f) {
        drop_copy(w, f, uid);
    }
    set_ui(w, f, false, 0.0, false);
    reset_ability(w, f, 0); // AbilityManager.ResetAbility(0) (U@16451)
    set_visibility(w, f, false);
    emit(w, f, |fighter| Event::DisguiseFx { fighter, kind: DisguiseFxKind::Destroy });
    w.fighter_mut(f).shapes.disguise = None;
}

/// `disguiseAs`: Buff_TakeFace's spawn of the Disguise on `f` with `from` as the victim:
/// SpawnBuffActor(Buff_Disguise, f, f, 0, 0.0) and InitDisguiseServer(from). Nothing when f has the buff already
/// (TakeFace's own test, U@399). `override_`: Buff_Arya_Trophy's spawn (@2274-2587), which makes no such test: the
/// class's CanOverride (true in the data) ends the live disguise in SweepBuffState (`spawn_buff`) and the new one is
/// initialised from `from`.
pub fn disguise_as(w: &mut World, f: FighterId, from: FighterId, override_: bool) {
    if !override_ && buff_of(w, f, CLS).is_some() {
        return;
    }
    let b = spawn_buff(w, f, CLS, 0.0, Some(0), Some(f), true, None);
    if let Some(b) = b
        && buff_def(w, ActorRef::Fighter(f), CLS).is_some()
    {
        init_disguise(w, f, b, from);
    }
}

/// AnimNotify_EquipFace (state; NormalNeutral, Animation) Begin: the owner has the buff; not Activated ->
/// ActivateDisguise; PlayAttackMulticast(NormalNeutral, Animation): CurrentAnimSequence = Animation, onGround = not
/// (falling, jumping, a scheduled jump, a pending launch, flying), then the copied neutral or the copied taunt.
fn equip_face_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let Some(b) = buff_of(w, f, CLS).filter(|_| has(w, f).is_some()) else { return };
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let seq = p.get("Animation").and_then(Value::as_str).map(|s| nm(&w.names, s));
    let normal_neutral = truthy(p.get("NormalNeutral"));
    let d = dis_mut(w, f).expect("a disguise");
    if !d.activated {
        d.activated = true;
        emit(w, f, |fighter| Event::DisguiseFx { fighter, kind: DisguiseFxKind::Activate });
    }
    dis_mut(w, f).expect("a disguise").seq = seq;
    // U@24709-24965: not (IsFalling or Jumping or JumpScheduled or PendingLaunch or IsFlying or (MovementMode 6 and
    // custom mode 2)). The pawn's three flags as TauntPressedLogic's test reads them (`taunt_press`).
    // Unverified: the custom movement mode term is not read (no fighter of the port enters it by name).
    let fi = w.fighter(f);
    let ground = !(crate::fighter::in_air(w, f)
        || fi.moves.cmd_jumped
        || fi.moves.ground_jump.is_some()
        || fi.moves.jump_launched
        || (fi.hit.launch.is_some() && !fi.hit.free_launch)
        || fi.moves.flying);
    if normal_neutral {
        play_neutral(w, f, ground);
    } else {
        let da = destroy_after(w, f, Some(b));
        play_taunt(w, f, da, ground);
    }
}

/// AnimNotify_EquipFace End: NormalNeutral -> StopDisguiseAttack(Animation), else K2_DestroyActor.
fn equip_face_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let Some(b) = buff_of(w, f, CLS).filter(|_| has(w, f).is_some()) else { return };
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if truthy(p.get("NormalNeutral")) {
        let seq = p.get("Animation").and_then(Value::as_str).map(|s| nm(&w.names, s));
        stop_disguise_attack(w, f, seq);
    } else {
        end_buff(w, f, b);
    }
}

/// HasUnequipedFaceCondition: the buff, and NormalAttacksRemaining != 0.
fn has_unequiped_face(w: &mut World, f: FighterId) -> bool {
    has(w, f).is_some_and(|d| d.remaining != 0.0)
}

/// DoesNotHaveFaceCondition: HasUnequipedFaceCondition's negation.
fn does_not_have_face(w: &mut World, f: FighterId) -> bool {
    !has_unequiped_face(w, f)
}

/// AryaDisguiseAttackNotOnCooldown (IsAir false; the Air child sets it): without the buff false; else not (the
/// copied neutral of that kind uses a cooldown and TriggerAbilityCheck(0) fails).
fn attack_not_on_cooldown(w: &mut World, f: FighterId, air: bool) -> bool {
    let Some(d) = has(w, f) else { return false };
    let cd = if air { d.air_cd } else { d.ground_cd };
    !(cd && !trigger_ability_check(w, f, 0))
}

fn ground_attack_not_on_cooldown(w: &mut World, f: FighterId) -> bool {
    attack_not_on_cooldown(w, f, false)
}

fn air_attack_not_on_cooldown(w: &mut World, f: FighterId) -> bool {
    attack_not_on_cooldown(w, f, true)
}

/// AryaDisguiseHasAttackBranch: the pawn's own read of its override packs (CharHooks).
fn has_attack_branch(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).hooks.char.disguise_has_attack_branches.is_some_and(|h| h(w, f))
}

/// AryaNotDoingCopyCatAttack: without the buff true; else neither copy plays.
fn not_doing_copy_cat(w: &mut World, f: FighterId) -> bool {
    has(w, f).is_none_or(|d| !d.taunt && !d.neutral)
}

/// NotDisguisedAsArya: without the buff true; else DisguiseCharacter != Arya.
fn not_disguised_as_arya(w: &mut World, f: FighterId) -> bool {
    has(w, f).is_none_or(|d| d.character != name!("Arya"))
}

/// This file's registry entries: Buff_TakeFace, Buff_Disguise, AnimNotify_EquipFace and the conditions that read the
/// Disguise.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        "Buff_TakeFace_C",
        BuffEntry::new(
            BuffClass::BuffTakeFace,
            BuffHooks { begin: Some(take_face_begin), tick: Some(take_face_tick), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(
        "Buff_Disguise_C",
        BuffEntry::new(
            BuffClass::BuffDisguise,
            BuffHooks {
                tick: Some(disguise_tick),
                on: Some(disguise_on),
                end: Some(disguise_end),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_EquipFace_C",
        NotifyHandler { begin: Some(equip_face_begin), end: Some(equip_face_end), ..NotifyHandler::default() },
    )?;
    let conds: [(&'static str, CondFn); 7] = [
        ("HasUnequipedFaceCondition_C", has_unequiped_face),
        ("DoesNotHaveFaceCondition_C", does_not_have_face),
        ("AryaDisguiseAttackNotOnCooldown_C", ground_attack_not_on_cooldown),
        ("AryaDisguiseAirAttackNotOnCooldown_C", air_attack_not_on_cooldown),
        ("AryaDisguiseHasAttackBranch_C", has_attack_branch),
        ("AryaNotDoingCopyCatAttack_C", not_doing_copy_cat),
        ("NotDisguisedAsArya_C", not_disguised_as_arya),
    ];
    for (n, c) in conds {
        r.conditions.enter(n, Condition::Fn(c))?;
    }
    Ok(())
}
