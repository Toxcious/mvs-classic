//! The perk TeamFightCloud: Buff_C015_TeamFightCloud ([`CloudState`]),
//! Buff_C015AttachToDogpile ([`AttachExt`], an `attachToAlly` child: `combat::attach_ally`), and its parts of the
//! joined worldTick and timerTick.
//!
//! Taz's TeamFightCloud perk (Perk_C015_TeamFightCloud -> Buff_C015_TeamFightCloud_C, self-only; offsets of its
//! ubergraph), Buff_C015AttachToDogpile (a Buff_AttachToAlly child) and C015Dogpile's part (FirstHitConnected @1082,
//! SetMinimumRemainingAlpha @3629). The values are the perk table's (the buffs' class defaults). In a 2v2 a team mate
//! in Taz's dogpile cloud who presses a neutral attack is carried in it, invulnerable and unseen, and the cloud grows
//! bigger, lasts longer, hits five times and Taz takes two armor packs; the mate leaves at any press (the cloud goes
//! back to its own size and time) or is thrown up at the cloud's end.

use super::dogpile::world_scale;
use super::dogpile::{dogpile_of, dogpile_owner, p_num, set_remaining_dogpile_time, update_dogpile, upgrade_dogpile};
use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActor;
use crate::actors::{ActorId, ActorRef};
use crate::combat::attach_ally::{self, AttachOverrides, AttachState};
use crate::combat::buffs::{BuffExt, buff, buff_mut, end_buff, inst_view, spawn_buff};
use crate::combat::defense::{add_armor_pack, remove_armor};
use crate::combat::perks::armor_pack;
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::Event;
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::common::peers;
use crate::fighter::control::enable_invulnerability;
use crate::fighter::moves::Impulse;
use crate::fighter::{
    FighterId, actor_y, add_active_attack, cap_radius, capsule_half, no_collision, remove_active_attack,
};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

const CLOUD: Name = name!("Buff_C015_TeamFightCloud_C");
const ATTACH: Name = name!("Buff_C015AttachToDogpile_C");
const ATTACH_CLASSES: [Name; 1] = [ATTACH];

/// The perk buff's variables (`CloudState`).
#[derive(State, Debug, Default)]
pub struct CloudState {
    /// DogpileActor while it is in the actors' list; `None` once it is gone (then `gone`) or before the first.
    pub dogpile: Option<ActorId>,
    /// DogpileActor gone from the actors' list: the object as it was at
    /// its end, kept so the trace's record writes it whole.
    pub gone: Option<Box<WorldActor>>,
    /// HitboxConnected.
    pub connected: bool,
    /// AllyFighter.
    pub ally: Option<FighterId>,
    /// AllyAttachBuff (its OnEndPlay bound to OnAttachBuffDestroyed); the fighter it is on (`attachOn`) is its
    /// `fighter`.
    pub attach: Option<BuffRef>,
    /// OriginalMinimumScale, OriginalMaximumScale, OriginalDuration, OriginalTotalHitsCount (AfterDogpileCreated
    /// @3442-3280).
    pub orig_min: Real,
    pub orig_max: Real,
    pub orig_dur: Real,
    pub orig_hits: Real,
    /// The fighters whose InputTriggered is bound to OnAllyInputRecieved.
    pub inputs: Vec<FighterId>,
    /// The clouds whose FirstHitConnected and OnEndPlay are bound (live ones).
    pub bound: Vec<ActorId>,
    /// The cloud whose AllyOverlap begin / end overlap events are bound.
    pub overlap: Option<ActorId>,
    /// The fighters the bound AllyOverlap overlaps now (the port's overlap state, [`overlaps`]).
    pub inside: Vec<FighterId>,
    /// UILabels' keys (Map_Add order).
    pub labels: Vec<FighterId>,
    /// The cloud during its own EndPlay (OnDogpileEnd runs inside it): IsValid is still true for an actor in its
    /// EndPlay (it is marked pending kill after RouteEndPlay; stock 4.26 UWorld::DestroyActor). Set and cleared inside
    /// one call: `None` between calls.
    pub ending: Option<ActorId>,
}

/// The link Buff_C015AttachToDogpile keeps to the perk buff that spawned it (`cloud`).
#[derive(State, Copy, PartialEq, Debug)]
pub struct CloudLink {
    pub owner: FighterId,
    pub buff: BuffRef,
}

/// Buff_C015AttachToDogpile_C's extension: the parent's `AttachState` (`combat::attach_ally`) and its own `cloud`.
#[derive(State, Debug, Default)]
pub struct AttachExt {
    pub attach: AttachState,
    pub cloud: Option<CloudLink>,
}

fn cloud(w: &World, b: BuffRef) -> Option<&CloudState> {
    match &buff(w, b)?.ext {
        BuffExt::TeamFightCloud(e) => Some(e),
        _ => None,
    }
}

fn cloud_mut(w: &mut World, b: BuffRef) -> Option<&mut CloudState> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::TeamFightCloud(e) => Some(e),
        _ => None,
    }
}

fn attach_ext(w: &World, b: BuffRef) -> Option<&AttachExt> {
    match &buff(w, b)?.ext {
        BuffExt::AttachToDogpile(e) => Some(e),
        _ => None,
    }
}

fn attach_ext_mut(w: &mut World, b: BuffRef) -> Option<&mut AttachExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::AttachToDogpile(e) => Some(e),
        _ => None,
    }
}

/// IsValid(the cloud): in the actors' list and not dead, or in its own EndPlay.
fn valid(w: &World, e: &CloudState, a: Option<ActorId>) -> bool {
    a.is_some_and(|id| dogpile_owner(w, id).is_some_and(|(_, dead)| !dead || e.ending == Some(id)))
}

/// IsValid(AllyAttachBuff): a live buff still in its fighter's list.
fn attach_valid(w: &World, e: &CloudState) -> bool {
    e.attach.and_then(|b| buff(w, b)).is_some_and(|x| x.frames > 0.0)
}

// ---------------------------------------------------------------- the input label

/// CreateUIPopUp(Fighter) @5052: a locally controlled Fighter not in UILabels -> UI_Functions PopInputLabel with
/// Action 0 (NormalAttack), Modifier 1 (Neutral), an empty Label, PlayerIndex the Fighter's, AutomaticInputType, Tint
/// (0.68, 0.8, 0.85, 1), TextFontInfo Display 1 16, KeyFontInfo NormsCond-ExtraBoldItalic 16, SizeIconSize 60 x 60, no
/// custom icon; positioning Lifetime 0, Location (0, -20, 300), Justification 1, ScreenSpace 3, attached to the
/// Fighter. A valid widget is Map_Add'ed, SetVisibility(Visible), Enter (@6146-6243). PopInputLabel makes none for a
/// bot's pawn. RemoveUIPopUp(Fighter) @6940: locally controlled and in the map -> SetVisibility(Hidden), Exit,
/// Map_Remove. The sim keeps the map's keys for every Fighter and sends the CloudLabel events; the UI draws them for
/// its own local, non-bot player alone, so a key whose widget the build would not make shows nothing.
/// Deviation: the build runs CreateUIPopUp on the server alone (AfterDogpileCreated is bound with authority @569), so
/// online a client's mate never sees the label; the port's pages each show it to their own player.
fn create_label(w: &mut World, f: FighterId, b: BuffRef, ally: FighterId) {
    let Some(e) = cloud_mut(w, b) else { return };
    if e.labels.contains(&ally) {
        return;
    }
    e.labels.push(ally);
    let (fighter, owner) = (w.fighter(ally).id, w.fighter(f).id);
    w.fighter_mut(f).events.push(Event::CloudLabel { fighter, owner, on: true, hide: None });
}

fn remove_label(w: &mut World, f: FighterId, b: BuffRef, ally: Option<FighterId>) {
    let Some(ally) = ally else { return };
    let Some(e) = cloud_mut(w, b) else { return };
    let Some(i) = e.labels.iter().position(|&x| x == ally) else { return };
    e.labels.remove(i);
    let (fighter, owner) = (w.fighter(ally).id, w.fighter(f).id);
    w.fighter_mut(f).events.push(Event::CloudLabel { fighter, owner, on: false, hide: Some(true) });
}

// ---------------------------------------------------------------- the graph

/// UpdateDogpile's armor @3622: the Instigator's Comp_Armor AddArmorPack(ArmorPack) or RemoveArmor(ArmorPack.
/// ArmorType).
fn armor(w: &mut World, f: FighterId, b: BuffRef, add: bool) {
    let pack = armor_pack(w, inst_view(w, b), "ArmorPack");
    if add {
        add_armor_pack(w, f, &pack);
    } else {
        remove_armor(w, f, pack.kind);
    }
}

/// UpdateDogpile(TotalHits, WeakHitBox, FinalHitBox, AddArmor) @9575: the cloud valid -> its WeakHitCount,
/// WeakHitBoxDamage, FinalHitBoxDamage; then the armor.
fn update_dogpile_call(w: &mut World, f: FighterId, b: BuffRef, hits: Real, add: bool) {
    let Some(e) = cloud(w, b) else { return };
    if !valid(w, e, e.dogpile) {
        return;
    }
    let id = e.dogpile.expect("a valid cloud");
    let d = inst_view(w, b);
    let weak = d.obj("NewWeakHitBox").cloned().unwrap_or_default();
    let fin = d.obj("NewFinalHitBox").cloned().unwrap_or_default();
    update_dogpile(w, id, hits, &weak, &fin);
    armor(w, f, b, add);
}

/// UpgradeDogpileMulticast(Dogpile, MinScale, MaxScale, MaximumDuration, MinimumDurationAlpha, AddArmor) @7907: the
/// cloud valid; its Instigator (Taz) BPI_SetRemainingDogpileTime(Max - FClamp(Alpha, 0, 1 - MinAlpha) x Max) @8274;
/// the timeline's play rate 1 / Max, HitBoxMaxScale, HitBoxMinScale, SetMinimumRemainingAlpha(MinAlpha)
/// (`dogpile::upgrade_dogpile`); then @240 the armor as UpdateDogpile's.
#[allow(clippy::too_many_arguments, reason = "the multicast's own parameters")]
fn upgrade_dogpile_call(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    a: Option<ActorId>,
    lo: Real,
    hi: Real,
    dur: Real,
    min_alpha: Real,
    add: bool,
) {
    let Some(e) = cloud(w, b) else { return };
    if !valid(w, e, a) {
        return;
    }
    let id = a.expect("a valid cloud");
    let alpha = dogpile_of(w, id).map_or(0.0, |s| s.alpha);
    let al = min(max(alpha, 0.0), 1.0 - min_alpha);
    let (owner, _) = dogpile_owner(w, id).expect("a valid cloud");
    set_remaining_dogpile_time(w, owner, dur - (al * dur));
    upgrade_dogpile(w, id, lo, hi, dur, min_alpha);
    armor(w, f, b, add);
}

/// OnDogpileActorOverlap(OtherActor) @4362: with no AllyFighter, a fighter other than the Instigator, of his team
/// (the PlayerData's TeamIndex), the Instigator with a Comp_State_Manager -> AllyFighter = it; without
/// HitboxConnected, CreateUIPopUp(it) @481 and its InputTriggered bound to OnAllyInputRecieved @527.
fn actor_overlap(w: &mut World, f: FighterId, b: BuffRef, o: FighterId) {
    let team = w.fighter(f).team.team;
    let ot = w.fighter(o).team.team;
    let Some(e) = cloud_mut(w, b) else { return };
    if e.ally.is_some() || o == f || ot != team {
        return;
    }
    e.ally = Some(o);
    if e.connected {
        return;
    }
    create_label(w, f, b, o);
    if let Some(e) = cloud_mut(w, b)
        && !e.inputs.contains(&o)
    {
        e.inputs.push(o);
    }
}

/// OnDogpileActorEndOverlap(OtherActor) @6304: a fighter other than the Instigator, of his team, the AllyFighter ->
/// @15: AllyFighter = null, RemoveUIPopUp(it), its InputTriggered unbound @72.
fn actor_end_overlap(w: &mut World, f: FighterId, b: BuffRef, o: FighterId) {
    let team = w.fighter(f).team.team;
    let ot = w.fighter(o).team.team;
    let Some(e) = cloud_mut(w, b) else { return };
    if o == f || ot != team || Some(o) != e.ally {
        return;
    }
    e.ally = None;
    remove_label(w, f, b, Some(o));
    if let Some(e) = cloud_mut(w, b) {
        e.inputs.retain(|&x| x != o);
    }
}

/// The port's AllyOverlap (sphere 200 on the cloud's ScalingComponent: radius 200 x the cloud's scale, at the cloud's
/// location) against a fighter's root capsule (the Pawn object type the sphere's Custom profile overlaps), in the play
/// plane. A fighter whose collision is off (dead, a pull, a buff's) overlaps nothing.
/// Deviation: the build's overlap events come with the moves of the cloud (attached to Taz), of the mate's capsule and
/// with the scale's change, inside those moves; the port tests once a frame in the timer pass ([`overlap_tick`]),
/// after the fighters' and the cloud's moves. BaseFighter's inner Capsule (also of the Pawn type) is not tested: the
/// root capsule is the larger one in every pawn. AfterDogpileCreated's HitBox loop (@1729-2104:
/// HitBox.GetOverlappingActors, the sphere of the same size on the same component, whose profile also overlaps the
/// HurtBox type) is folded into the AllyOverlap loop: a mate whose hurt boxes alone reach the HitBox is not found
/// there.
fn overlaps(w: &World, id: ActorId, o: FighterId) -> bool {
    if w.fighter(o).life.dead > 0.0 || no_collision(w, o) {
        return false;
    }
    let Some(s) = dogpile_of(w, id) else { return false };
    let r = 200.0 * world_scale(w, id);
    let (hh, cr) = (capsule_half(w, o), cap_radius(w, o));
    let sg = max(hh - cr, 0.0);
    let cy = actor_y(w, o);
    let dy = if s.y < cy - sg {
        cy - sg - s.y
    } else if s.y > cy + sg {
        s.y - cy - sg
    } else {
        0.0
    };
    let dx = s.x - w.fighter(o).body.x;
    dx * dx + dy * dy <= (r + cr) * (r + cr)
}

/// AfterDogpileCreated(SpawnedActor) @3878 (the Instigator's Comp_Spawner AfterSpawnedActor, bound in BeginPlay with
/// authority @569-693): a C015Dogpile_C (IsSoftRef and the cast) that has hit nobody -> HitboxConnected = false (and
/// its multicast @9555), DogpileActor = it, its OnEndPlay bound to OnDogpileEnd @4227, the originals kept
/// @3442-3106, the HitBox overlap loop (folded, above), FirstHitConnected bound to OnDogpileHitboxConnected @2150,
/// AllyOverlap's begin / end bound @2214 / @2300, then every BP_FighterCharacter AllyOverlap overlaps ->
/// OnDogpileActorOverlap (@2420-2917).
fn dogpile_created(w: &mut World, f: FighterId, b: BuffRef, id: ActorId) {
    if dogpile_of(w, id).is_none_or(|s| s.has_hit_enemy) {
        return;
    }
    let orig_min = p_num(w, id, "HitBoxMinScale").unwrap_or(1.0);
    let orig_max = p_num(w, id, "HitBoxMaxScale").unwrap_or(1.0);
    let orig_dur = p_num(w, id, "MainDuration").unwrap_or(3.0);
    let orig_hits = p_num(w, id, "WeakHitCount").unwrap_or(3.0);
    let Some(e) = cloud_mut(w, b) else { return };
    e.connected = false;
    e.dogpile = Some(id);
    e.gone = None;
    if !e.bound.contains(&id) {
        e.bound.push(id);
    }
    e.orig_min = orig_min;
    e.orig_max = orig_max;
    e.orig_dur = orig_dur;
    e.orig_hits = orig_hits;
    e.overlap = Some(id);
    e.inside.clear();
    for o in peers(w).to_vec() {
        if !overlaps(w, id, o) {
            continue;
        }
        if let Some(e) = cloud_mut(w, b) {
            e.inside.push(o);
        }
        actor_overlap(w, f, b, o);
    }
}

/// The bound AllyOverlap's begin and end overlap events (OnDogpileHitboxActorOverlap @4273 / EndOverlap @6280), once
/// a frame in Taz's timer pass: the ends first, then the begins, each in the fighters' order.
fn overlap_tick(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(id) = cloud(w, b).and_then(|e| e.overlap) else { return };
    if dogpile_owner(w, id).is_none_or(|(_, dead)| dead) {
        if let Some(e) = cloud_mut(w, b) {
            e.overlap = None;
            e.inside.clear();
        }
        return;
    }
    let now: Vec<FighterId> = peers(w).iter().copied().filter(|&o| overlaps(w, id, o)).collect();
    let Some(e) = cloud_mut(w, b) else { return };
    let was = std::mem::replace(&mut e.inside, now.clone());
    for &o in &was {
        if !now.contains(&o) {
            actor_end_overlap(w, f, b, o);
        }
    }
    for &o in &now {
        if !was.contains(&o) {
            actor_overlap(w, f, b, o);
        }
    }
}

/// OnAllyInputRecieved(InputType, InputDirection, ...) @9771: without HitboxConnected, a Neutral (4) NormalAttack (0)
/// -> @9364 its InputTriggered unbound; @8723 RemoveUIPopUp(AllyFighter); UpdateDogpile(NewTotalHitsCount,
/// NewWeakHitBox, NewFinalHitBox, true); UpgradeDogpileMulticast(DogpileActor, NewHitBoxMinScale, NewHitBoxMaxScale,
/// MaximumDuration, MinimumDurationAlpha, true); AllyOverlap's begin / end unbound @8871 / @8957; @8593 the
/// AllyFighter's SpawnBuffActor(Buff_C015AttachToDogpile_C, AllyFighter, the Instigator, 0, 0.0, true, false) ->
/// AllyAttachBuff, its OnEndPlay bound to OnAttachBuffDestroyed @4320. So Taz takes the armor pack twice
/// (CanOverride: two packs).
fn ally_input(w: &mut World, f: FighterId, b: BuffRef, a: &BuffSignalArg) {
    let Some(p) = a.presser else { return };
    let Some(e) = cloud_mut(w, b) else { return };
    if !e.inputs.contains(&p) {
        return;
    }
    if e.connected || a.dir != Some(4.0) || a.input != Some(0) {
        return;
    }
    e.inputs.retain(|&x| x != p);
    let ally = e.ally;
    remove_label(w, f, b, ally);
    let d = inst_view(w, b);
    let n = |k: &str, v: Real| d.get(k).and_then(Value::as_f64).unwrap_or(f64::from(v));
    let (hits, lo, hi, dur, ma) = (
        n("NewTotalHitsCount", 5.0),
        n("NewHitBoxMinScale", 1.0),
        n("NewHitBoxMaxScale", 1.0),
        n("MaximumDuration", 3.0),
        n("MinimumDurationAlpha", 0.0),
    );
    update_dogpile_call(w, f, b, crate::math::narrow(hits), true);
    let dp = cloud(w, b).and_then(|e| e.dogpile);
    upgrade_dogpile_call(
        w,
        f,
        b,
        dp,
        crate::math::narrow(lo),
        crate::math::narrow(hi),
        crate::math::narrow(dur),
        crate::math::narrow(ma),
        true,
    );
    if let Some(e) = cloud(w, b)
        && valid(w, e, e.dogpile)
        && e.overlap == e.dogpile
        && let Some(e) = cloud_mut(w, b)
    {
        e.overlap = None;
        e.inside.clear();
    }
    let Some(ally) = cloud(w, b).and_then(|e| e.ally) else {
        return; // SpawnBuffActor on None: nothing; AllyAttachBuff keeps the last call's value (no valid buff)
    };
    let nb = spawn_buff(w, ally, ATTACH, 0.0, Some(0), Some(f), true, None);
    if let Some(e) = cloud_mut(w, b) {
        e.attach = nb;
    }
    if let Some(nb) = nb
        && let Some(x) = attach_ext_mut(w, nb)
    {
        x.cloud = Some(CloudLink { owner: f, buff: b });
    }
}

/// OnAttachBuffDestroyed @9025: with HitboxConnected nothing; else UpgradeDogpileMulticast(DogpileActor, the
/// originals, 0.05, false), then (authority) UpdateDogpile(OriginalTotalHitsCount, NewWeakHitBox, NewFinalHitBox,
/// false): the build hands the new packets back, not the originals (OriginalWeakHitBox / OriginalFinalHitBox are kept
/// @3226 / @3106 and read nowhere), and RemoveArmor runs twice.
fn attach_destroyed(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = cloud(w, b) else { return };
    if e.connected {
        return;
    }
    let (dp, lo, hi, dur, hits) = (e.dogpile, e.orig_min, e.orig_max, e.orig_dur, e.orig_hits);
    upgrade_dogpile_call(w, f, b, dp, lo, hi, dur, 0.05, false);
    update_dogpile_call(w, f, b, hits, false);
}

/// OnDogpileHitboxConnected @9524 -> OnDogpileAttackConnected @9539: HitboxConnected = true (and its multicast @9433),
/// the AllyFighter valid -> RemoveUIPopUp(it). The mate's InputTriggered stays bound: OnAllyInputRecieved tests
/// HitboxConnected. The cloud's FirstHitConnected (`dogpile` calls it).
pub fn first_hit(w: &mut World, id: ActorId) {
    let Some((f, _)) = dogpile_owner(w, id) else { return };
    let Some(b) = cloud_buff(w, f, id) else { return };
    let Some(e) = cloud_mut(w, b) else { return };
    e.connected = true;
    let ally = e.ally;
    remove_label(w, f, b, ally);
}

/// OnDogpileEnd @9169 (the cloud's OnEndPlay, `dogpile` calls it): OnDogpileEndMulticast (@1610: the Instigator's
/// Comp_Armor valid -> @1406 RemoveArmor(ArmorPack.ArmorType), then @857 Exit on every UILabels value: the map is not
/// emptied, so a mate whose label was up at the end gets none in the next cloud until a RemoveUIPopUp takes the key
/// out); AllyAttachBuff valid -> its K2_DestroyActor (its EndPlay, and OnAttachBuffDestroyed inside it, on this cloud
/// still valid), then the AllyFighter valid -> LaunchCharacter((0, 0, 2500), false, true) @9301; AllyFighter = null
/// @9352 (behind the launch: a destroyed buff with no AllyFighter returns before it).
pub fn dogpile_end(w: &mut World, id: ActorId) {
    let Some((f, _)) = dogpile_owner(w, id) else { return };
    if let Some(b) = cloud_buff(w, f, id) {
        dogpile_end_bound(w, f, b, id);
    }
    // The cloud gone from the actors' list: the perk buffs that name it keep the object as it is now.
    let snap = w.actors.actors.get(id).map(|a| Box::new(a.clone()));
    let serials: Vec<u32> = w.fighter(f).buffs.buffs.iter().filter(|x| x.cls == CLOUD).map(|x| x.serial).collect();
    for serial in serials {
        let b = BuffRef { fighter: f, serial };
        if let Some(e) = cloud_mut(w, b)
            && e.dogpile == Some(id)
        {
            e.dogpile = None;
            e.gone.clone_from(&snap);
        }
    }
}

fn dogpile_end_bound(w: &mut World, f: FighterId, b: BuffRef, id: ActorId) {
    let Some(e) = cloud_mut(w, b) else { return };
    e.bound.retain(|&x| x != id);
    if e.overlap == Some(id) {
        e.overlap = None;
        e.inside.clear();
    }
    e.ending = Some(id);
    let kind = armor_pack(w, inst_view(w, b), "ArmorPack").kind;
    remove_armor(w, f, kind);
    let labels = cloud(w, b).map(|e| e.labels.clone()).unwrap_or_default();
    let owner = w.fighter(f).id;
    for o in labels {
        let fighter = w.fighter(o).id;
        w.fighter_mut(f).events.push(Event::CloudLabel { fighter, owner, on: false, hide: None });
    }
    if let Some(e) = cloud(w, b)
        && attach_valid(w, e)
    {
        let at = e.attach.expect("a valid attach buff");
        end_buff(w, at.fighter, at);
        if let Some(ally) = cloud(w, b).and_then(|e| e.ally) {
            w.fighter_mut(ally).moves.impulse =
                Some(Impulse { vx: 0.0, vy: 2500.0, xy: false, z: true, kind: 0, d: None });
        }
    }
    if let Some(e) = cloud_mut(w, b) {
        e.ally = None;
        e.ending = None;
    }
}

/// The live perk buff of the cloud's Taz that bound this cloud.
fn cloud_buff(w: &World, f: FighterId, id: ActorId) -> Option<BuffRef> {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|x| {
            x.frames > 0.0 && x.cls == CLOUD && matches!(&x.ext, BuffExt::TeamFightCloud(e) if e.bound.contains(&id))
        })
        .map(|x| BuffRef { fighter: f, serial: x.serial })
}

// ---------------------------------------------------------------- Buff_C015AttachToDogpile
// ReceiveBeginPlay @536: the parent's (`combat::attach_ally`), then on a BaseFighter owner Comp_Actor_Destructible
// AddInvulnerability @645, Comp_Pawn_BranchManager AddActiveAttack @703, EnableInvulnerability(true, false) @761 and
// (not a dedicated server) AddInvisibility @473. ReceiveEndPlay @10: the parent's, then RemoveInvulnerability @128,
// RemoveActiveAttack @186, EnableInvulnerability(false, false) @244, RemoveInvisibility @346. Its data: StateInfo Group
// Held (blocked by another Held buff or Buff_StevenBubbleEnemy, breaks the group 13 buffs), Stun 0.2 s IsGrappledAlly
// (UpdatePlayerStatus's buff stun, at least 0.3 s), TakenDamageMods (DoNothing), DestroyBuffWhenOwnerDefeated.
// AttachSocket FlightGrabSocket and AttachOffset (0, 0, -100) are read by nothing of the graph that reaches them (the
// grapple is to the root: `combat::attach_ally`).
// The perk's OnEndPlay binding (OnAttachBuffDestroyed) runs after the actor's whole EndPlay (AActor::EndPlay:
// ReceiveEndPlay, then OnEndPlay.Broadcast).

fn attach_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::AttachToDogpile(AttachExt::default());
    }
    let e = attach_ally::begin(w, f, from, AttachOverrides::default());
    if let Some(x) = attach_ext_mut(w, b) {
        x.attach = e;
    }
    w.fighter_mut(f).invuln.invuln_count += 1;
    add_active_attack(w, f);
    enable_invulnerability(w, f, true, false);
    add_invisibility(w, f);
}

fn attach_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    attach_ext(w, b).is_some_and(|e| attach_ally::on(f, &e.attach, s, a))
}

fn attach_end(w: &mut World, f: FighterId, b: BuffRef) {
    let mut e = attach_ext_mut(w, b).map(|x| std::mem::take(&mut x.attach)).unwrap_or_default();
    attach_ally::end(w, f, &mut e);
    if let Some(x) = attach_ext_mut(w, b) {
        x.attach = e;
    }
    let c = &mut w.fighter_mut(f).invuln.invuln_count;
    *c = (*c - 1).max(0);
    remove_active_attack(w, f);
    enable_invulnerability(w, f, false, false);
    remove_invisibility(w, f);
    let Some(c) = attach_ext(w, b).and_then(|x| x.cloud) else { return };
    let live = buff(w, c.buff).is_some_and(|x| x.frames > 0.0);
    if live && cloud(w, c.buff).is_some_and(|e| e.attach == Some(b)) {
        attach_destroyed(w, c.owner, c.buff);
    }
}

#[allow(clippy::too_many_arguments, reason = "the hook's signature")]
fn attach_taken_cond(
    w: &mut World,
    owner: FighterId,
    b: BuffRef,
    _attacker: Option<ActorRef>,
    _boxed: bool,
    _hit: &HitPacket,
    by: Option<FighterId>,
) -> bool {
    attach_ext(w, b).is_some_and(|e| attach_ally::taken_cond(w, owner, &e.attach, by))
}

fn attach_parent(w: &World, _f: FighterId, b: BuffRef) -> Option<FighterId> {
    attach_ext(w, b).and_then(|e| attach_ally::attach_parent(&e.attach))
}

// ---------------------------------------------------------------- Buff_C015_TeamFightCloud
// ReceiveBeginPlay @3863 -> @569 (authority): the Instigator's (Taz's own: the perk spawns it on him) Comp_Spawner
// AfterSpawnedActor bound to AfterDogpileCreated (the AfterSpawnedActor signal of his spawns). DelayAfterLeaving
// @9519 has no caller.

fn cloud_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::TeamFightCloud(CloudState::default());
    }
}

fn cloud_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    match s {
        BuffSignal::AfterSpawnedActor => {
            if let Some(ActorRef::Actor(id)) = a.actor
                && dogpile_of(w, id).is_some()
            {
                dogpile_created(w, f, b, id);
            }
        }
        BuffSignal::FighterInputTriggered => ally_input(w, f, b, a),
        _ => {}
    }
    false
}

/// perks_cloud.rs's part of the joined worldTick: the mates carried in his cloud follow him
/// (`attach_ally::attach_follow`), after the fighters' mesh ticks.
pub fn world_tick(w: &mut World, f: FighterId) {
    attach_ally::attach_follow(w, f, &ATTACH_CLASSES);
}

/// perks_cloud.rs's part of the joined timerTick: the bound AllyOverlap's overlap events ([`overlap_tick`]), after the
/// cloud's own move in the actors' step.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let serials: Vec<u32> =
        w.fighter(f).buffs.buffs.iter().filter(|x| x.frames > 0.0 && x.cls == CLOUD).map(|x| x.serial).collect();
    for serial in serials {
        let b = BuffRef { fighter: f, serial };
        if buff(w, b).is_some_and(|x| x.frames > 0.0) {
            overlap_tick(w, f, b);
        }
    }
}

/// This file's registry entries (Buff_C015AttachToDogpile_C, Buff_C015_TeamFightCloud_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let attach = BuffHooks {
        begin: Some(attach_begin),
        on: Some(attach_on),
        end: Some(attach_end),
        taken_cond: Some(attach_taken_cond),
        attach_parent: Some(attach_parent),
        ..BuffHooks::default()
    };
    r.buffs.enter(
        "Buff_C015AttachToDogpile_C",
        BuffEntry { active_attack: true, ..BuffEntry::new(BuffClass::BuffC015AttachToDogpile, attach) },
    )?;
    let hooks = BuffHooks { begin: Some(cloud_begin), on: Some(cloud_on), ..BuffHooks::default() };
    r.buffs.enter("Buff_C015_TeamFightCloud_C", BuffEntry::new(BuffClass::BuffC015TeamFightCloud, hooks))?;
    Ok(())
}
