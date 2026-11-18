//! Pawn_Arya's own logic: the stolen-face override pack (CreateStolenAbilityAttacks), "BPI
//! Activate Branch Override", OnBranchGroupBlocked / OnBranchGroupEndBlocked, OnEvadedAttack, OnPlayerKnockedOut and
//! the pawn's latent Delays (tick), BeginPlay (the backstab buff), BPI_Cleanup (reset), matchReset, "Movement Stop
//! Anim On Disguise", the reads normalHitboxesOff and disguiseHasAttackBranches; the notifies
//! AnimNotify_AryaDeactivateHitboxes, AnimNotify_AryaSetAssassinating, AnimNotify_AryaSwordGlowOn; the buffs
//! Buff_AryaBackStab and Buff_BackStab.
//!
//! Her attacks themselves are data: the start tables, branch groups and montages of the build, played by the shared
//! code. The Disguise is shared (`combat::disguise`) and calls the pawn through the hooks below.
//!
//! Not here, by the graph:
//! - ReceiveTick @8370 (SetSubmeshAlwaysVisible), UpdateSwordAnim (bound to the skin's apply @8718) and the sword's
//!   master pose (@13616): drawing;
//! - BPI_Use2DHitboxes: her `Use 2D Hitboxes` is false (class default, no live writer): the data's flag;
//! - the BPI_Arya functions of the ricochet, the ally pivot, the ally bounce, the overlapping and nearby fighter
//!   reads and "holding enemy": called by classes her live data never reaches;
//! - the bot injections (SetDynamicSubtree at the end of BeginPlay): the bots' crate.

use super::state::{BACKSTAB, EVADE_NEUTRAL, arya, arya_mut, arya_of, arya_w};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::spawn_area::spawn_object;
use crate::combat::buffs::{BuffExt, buff_mut, buff_of, spawn_buff};
use crate::combat::disguise::stop_copy_montage;
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::montages::{any_notify, slot_name};
use crate::fighter::anim::overrides::{add_override_pack, remove_override_pack};
use crate::fighter::anim::{OverridePack, OverrideRow, complete_window, file_window};
use crate::fighter::common::{latent_frames, peers};
use crate::fighter::control::pawn_table;
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks, NotifyHandler};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use mvs_data::fighter::AttackEntry;
use mvs_data::value::Value;

const TURN: Name = name!("Buff_TurnEnemyAwayOnHit_C");
const SNACK: Name = name!("Buff_ScoobySnack_C");
const HAS_BRANCH: &str = "AryaDisguiseHasAttackBranch_C";
const AIR_SPECIAL_LIMIT: &str = "AirSpecialLimitCondition_C";

/// The hooks of this file in Arya's CharHooks (`mod.rs` own: normalHitboxesOff, disguiseHasAttackBranches,
/// activateBranchOverride, branchGroupBlocked, branchGroupEndBlocked, evadedAttack, playerKnockedOut, tick,
/// movementStopAnim, reset, matchReset). The pawn has no death or EndPlay event of its own: the DoOnce, its delay and
/// the bind go on through a ring-out and the respawn.
pub fn hooks(h: &mut CharHooks) {
    // "BPI Disable Normal Hitboxes" @5232.
    h.normal_hitboxes_off = Some(|w, f| arya(w, f).hitboxes_off);
    // "BPI Disguise Has Attack Branches": BranchOverrideActive, a tracked window and a count above 0.
    h.disguise_has_attack_branches = Some(|w, f| {
        let s = arya(w, f);
        s.branch_override_active && !s.tracked.is_empty() && s.active_branches > 0
    });
    h.activate_branch_override = Some(activate_branch_override);
    h.branch_group_blocked = Some(branch_group_blocked);
    h.branch_group_end_blocked = Some(branch_group_end_blocked);
    h.evaded_attack = Some(evaded_attack);
    h.player_knocked_out = Some(player_knocked_out);
    h.tick = Some(tick);
    h.movement_stop_anim = Some(movement_stop_anim);
    h.reset = Some(reset);
    h.match_reset = Some(match_reset);
}

/// Two rows as `JSON.stringify` of the row sees them (the dedupe key of `stolenPack`).
fn same_row(a: &OverrideRow, b: &OverrideRow) -> bool {
    let (x, y) = (&a.attack, &b.attack);
    a.aerial_move == b.aerial_move
        && x.required_button_input == y.required_button_input
        && x.required_direction_input == y.required_direction_input
        && x.attack_montage == y.attack_montage
        && x.ability_slot == y.ability_slot
        && x.display_name == y.display_name
        && x.conditions == y.conditions
        && x.allow_pre_attack_rotation == y.allow_pre_attack_rotation
}

/// CreateStolenAbilityAttacks (ReceiveBeginPlay @8783): the four start tables in the order AttacksNormalGround
/// @442, AttacksNormalAir @1127, AttacksSpecialGround @1812, AttacksSpecialAir @2497, each from its last row to its
/// first; a copy of the row gets AryaDisguiseHasAttackBranch added to its Conditions (AddUnique @488; the special
/// air rows AirSpecialLimitCondition too @2639) and joins StolenAbilityAttacks with AerialMove false, true, false,
/// true (AddUnique @621: an equal row is not added twice). StolenFaceAbilityOverride.Overrides = the list @2841;
/// its Priority is the class default's (2). The same for every pawn of a data file.
pub fn stolen_pack(w: &World, f: FighterId) -> OverridePack {
    let data = w.data.clone();
    let mut rows: Vec<OverrideRow> = Vec::new();
    let mut table = |name: &str, aerial: bool, more: Option<&str>| {
        let list: Vec<AttackEntry> = pawn_table(&data, w, f, name);
        for e in list.iter().rev() {
            let mut attack = e.clone();
            let has = |c: &str, l: &[Option<String>]| l.iter().any(|x| x.as_deref() == Some(c));
            if !has(HAS_BRANCH, &attack.conditions) {
                attack.conditions.push(Some(HAS_BRANCH.to_string()));
            }
            if let Some(m) = more
                && !has(m, &attack.conditions)
            {
                attack.conditions.push(Some(m.to_string()));
            }
            let row = OverrideRow { aerial_move: aerial, attack };
            if rows.iter().any(|r| same_row(r, &row)) {
                continue;
            }
            rows.push(row);
        }
    };
    table("AttacksNormalGround", false, None);
    table("AttacksNormalAir", true, None);
    table("AttacksSpecialGround", false, None);
    table("AttacksSpecialAir", true, Some(AIR_SPECIAL_LIMIT));
    // no default: a data file without the class default's pack is not Arya's
    let ch = w.names.str(w.fighter(f).ch);
    let priority = data.chars[ch]
        .fighter
        .pawn
        .get("StolenFaceAbilityOverride")
        .and_then(Value::as_obj)
        .and_then(|p| p.get("Priority"))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("arya: the pawn data has no StolenFaceAbilityOverride.Priority ({ch})"));
    OverridePack { priority: crate::math::narrow(priority), overrides: rows, from_data: false }
}

/// "BPI Activate Branch Override"(Activate) @6011 (Buff_Disguise calls it around a copied neutral): nothing
/// unless the value changes. On: AddOverridePack(StolenFaceAbilityOverride) @6073, then the flag. Off:
/// CompleteBranchNotify(each tracked window, the pawn's own anim instance) @6420, the list cleared, the count 0,
/// RemoveOverridePack @6613, then the flag. A completed window can start an attack, whose montage start ends the
/// copy and comes back here with the flag still set: the loop reads the list as it stands, as the graph's does.
fn activate_branch_override(w: &mut World, f: FighterId, on: bool) {
    if on == arya(w, f).branch_override_active {
        return;
    }
    if on {
        let pack = stolen_pack(w, f);
        add_override_pack(w, f, pack);
        arya_w(w, f).branch_override_active = true;
        return;
    }
    let mut i = 0;
    while let Some(&n) = arya(w, f).tracked.get(i) {
        complete_window(w, f, n);
        i += 1;
    }
    let s = arya_w(w, f);
    s.tracked.clear();
    s.active_branches = 0;
    let pack = stolen_pack(w, f);
    remove_override_pack(w, f, &pack);
    arya_w(w, f).branch_override_active = false;
}

/// A window with Branches or BranchGroups.
fn has_branches(w: &World, f: FighterId, n: NotifyRef) -> bool {
    let Some(nt) = any_notify(w, f, n) else { return false };
    let len = |k: &str| nt.p.get(k).and_then(Value::as_arr).map_or(0, <[Value]>::len);
    len("Branches") > 0 || len("BranchGroups") > 0
}

/// OnBranchGroupBlocked @11863 (bound in CreateStolenAbilityAttacks @2877-2964): with the override on, a window
/// not tracked yet @11873, a Buff_Disguise whose IsPlayingNeutralAttack is set @12175 and the window's anim
/// instance on that buff's DisguiseMesh @12249 (the copy's player is the one other instance the port has):
/// BranchNotifyStart(the window, the pawn's own anim instance, its GetCurrentActiveMontage, Duration) @12403, the
/// window tracked @12475, and the count + 1 for a window with Branches or BranchGroups @12543-12821.
// Unverified: with no active montage on the pawn's own instance the window is filed under no montage (an empty name).
fn branch_group_blocked(w: &mut World, f: FighterId, n: NotifyRef, _montage: Name, duration: Real) {
    let s = arya(w, f);
    if !s.branch_override_active || s.tracked.contains(&n) {
        return;
    }
    if !w.fighter(f).shapes.disguise.as_ref().is_some_and(|d| d.neutral) {
        return;
    }
    let cur = active_instance(w, f).map(|slot| slot_name(w, f, slot));
    file_window(w, f, n, cur, duration);
    arya_w(w, f).tracked.push(n);
    if has_branches(w, f, n) {
        arya_w(w, f).active_branches += 1;
    }
}

/// OnBranchGroupEndBlocked @12918: a tracked window leaves the list @12988, CompleteBranchNotify(the window, the
/// pawn's own anim instance) @13090, and the count - 1 for a window with Branches or BranchGroups @13144-13422.
/// No test of the override flag.
fn branch_group_end_blocked(w: &mut World, f: FighterId, n: NotifyRef) {
    let s = arya_w(w, f);
    let Some(k) = s.tracked.iter().position(|x| *x == n) else { return };
    s.tracked.remove(k);
    complete_window(w, f, n);
    if has_branches(w, f, n) {
        arya_w(w, f).active_branches -= 1;
    }
}

/// OnEvadedAttack(AttackingActor) @8798 (bound to the pawn's own EvadedAttack @8621): with the Mesh's current
/// montage Arya_Evade_Neutral_Montage @8866, SpawnBuffActor(Buff_TurnEnemyAwayOnHit_C, self, self, 0 stacks,
/// 5.0 s) and the same on every ally of Comp_Actor_Destructible.GetAllies with herself as Instigator @575. The
/// attacker is not read.
/// GetAllies: the other fighters of her team that are not knocked out (unverified).
fn evaded_attack(w: &mut World, f: FighterId, _attacker: Option<FighterId>) {
    if active_instance(w, f).map(|slot| slot_name(w, f, slot)) != Some(EVADE_NEUTRAL) {
        return;
    }
    spawn_buff(w, f, TURN, 5.0, Some(0), Some(f), true, None);
    let team = w.fighter(f).team.team;
    let all: Vec<FighterId> = peers(w).to_vec();
    for o in all {
        if o != f && w.fighter(o).team.team == team && w.fighter(o).life.dead <= 0.0 {
            spawn_buff(w, o, TURN, 5.0, Some(0), Some(f), true, None);
        }
    }
}

/// OnPlayerKnockedOut(LoserIndex, ...) @8984, bound to the game mode's PlayerKnockedOut by BeginPlay's Delay
/// (`ko_bound`). A DoOnce @9029: closed at entry @9044 and opened by the Delay(2.0) @10971 alone, so every path
/// that leaves ahead of that delay leaves the handler closed for the rest of the pawn's life:
/// - the loser's player data, pawn, BaseFighter cast or NotPlayerCharacter fail (a fighter of the match passes);
/// - the loser's DamageManager.MostRecentAttacker has no valid pawn @10308 (a ring-out nobody caused);
/// - a loser of another team whose attacker is not this pawn @11640.
///
/// A loser of her team (herself too) @10532: the attacker's pawn dead (BPI_IsDead @10912): the delay alone;
/// else, for a loser other than herself @11232, SpawnBuffActor(Buff_ScoobySnack_C, self, self, 5 stacks, 14.0 s)
/// @11246 and the delay; herself: the delay alone.
/// A loser of another team with this pawn as the attacker: Comp_Pawn_SpawnObjectRandomArea.SpawnObject() @11650
/// (the pie, `actors`) and the delay.
fn player_knocked_out(w: &mut World, f: FighterId, loser: FighterId) {
    let s = arya_w(w, f);
    if !s.ko_bound || s.ko_closed {
        return;
    }
    s.ko_closed = true;
    let Some(att) = w.fighter(loser).attackers.most_recent_attacker else { return };
    if w.fighter(loser).team.team == w.fighter(f).team.team {
        if w.fighter(att).life.dead <= 0.0 && loser != f {
            spawn_buff(w, f, SNACK, 14.0, Some(5), Some(f), true, None);
        }
    } else {
        if att != f {
            return;
        }
        spawn_object(w, f);
    }
    arya_w(w, f).ko_delay = latent_frames(2.0);
}

/// The pawn's latent actions, behind its ReceiveTick. BeginPlay's Delay(1.0) @8663 -> @35: with more than one
/// player in the PlayerDataManager @126 and a game mode of the parent class @191, OnPlayerKnockedOut is bound
/// @266-289. The DoOnce's Delay(2.0) -> @331.
// Unverified: GetPlayerCount is taken as the match's fighters (the bots are players of the manager).
fn tick(w: &mut World, f: FighterId) {
    let many = peers(w).len() > 1;
    let s = arya_w(w, f);
    if s.begin_delay > 0.0 {
        s.begin_delay -= 1.0;
        if s.begin_delay == 0.0 {
            s.ko_bound = many;
        }
    }
    if s.ko_delay > 0.0 {
        s.ko_delay -= 1.0;
        if s.ko_delay == 0.0 {
            s.ko_closed = false;
        }
    }
}

/// ReceiveBeginPlay @8498 after the parent's, on the authority: CreateStolenAbilityAttacks @8783 (`stolen_pack`),
/// the capture of the Mesh's HitBox and HurtBox shapes @8470 (state: the box set's own values),
/// SpawnBuffActor(Buff_AryaBackStab_C, self, self, 0 stacks, 0.0 s) @8553, the EvadedAttack bind @8621 and
/// Delay(1.0) @8663 (the state's `begin_delay`, pending from the first).
fn begin_play(w: &mut World, f: FighterId) {
    spawn_buff(w, f, BACKSTAB, 0.0, Some(0), Some(f), true, None);
}

/// "Movement Stop Anim On Disguise"(Montage, BlendOut) @14243 behind the parent's multicast: with a
/// Buff_Disguise, its DisguiseMesh's anim instance Montage_Stop(BlendOut, Montage).
// Deviation: the copy stops at once (`stop_copy_montage` is Montage_Stop(0.0)); the blend-out time the notify passes
// (the copied montage's default) is not played out on the second mesh.
fn movement_stop_anim(w: &mut World, f: FighterId, montage: Name, _blend_out: Real) {
    let hit = w
        .fighter(f)
        .shapes
        .disguise
        .as_ref()
        .and_then(|d| d.copy.as_ref())
        .is_some_and(|c| c.m.montage.name == montage);
    if hit {
        stop_copy_montage(w, f);
    }
}

/// BPI_Cleanup @13519 (BaseFighter ResetFighterLogic calls it): IsAssassinating and BranchOverrideActive false,
/// TrackedDisguiseNotifies cleared. The pack stays on the override manager and the count as it was: the next
/// "BPI Activate Branch Override"(false) finds the flag clear and does nothing.
/// Then the port's own step: the fighter's spawn has dropped every buff (deviation), and Buff_AryaBackStab has
/// DestroyBuffWhenOwnerDefeated false: the build's pawn keeps the actor of its BeginPlay through a defeat, so it is
/// given again here.
fn reset(w: &mut World, f: FighterId) {
    let s = arya_w(w, f);
    s.is_assassinating = false;
    s.branch_override_active = false;
    s.tracked.clear();
    if buff_of(w, f, BACKSTAB).is_none() {
        spawn_buff(w, f, BACKSTAB, 0.0, Some(0), Some(f), true, None);
    }
}

/// A match reset spawns a new pawn: a new state and its BeginPlay again (the World's reset calls this behind the
/// fighter's spawn, which drops the buffs, and ahead of the passive rows' buffs). Also the first spawn of a match.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = crate::chars::CharState::Arya(super::AryaState::default());
    begin_play(w, f);
}

// ---- the notifies

/// AnimNotify_AryaDeactivateHitboxes (Arya_CopyCat_Ground_montage -0.0001 + 3.8332 s, Arya_CopyCat_Air_montage
/// 0.0001 + 0.4999 s): Received_NotifyBegin / End on an owner with BPI_Arya -> "BPI Disable Normal
/// Hitboxes"(true / false).
fn hitboxes_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = arya_mut(w, f) {
        s.hitboxes_off = true;
    }
}

fn hitboxes_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = arya_mut(w, f) {
        s.hitboxes_off = false;
    }
}

/// AnimNotify_AryaSetAssassinating (the two up specials, 0.180 + 0.205 s): "BPI Set Assassinating"(true / false)
/// @5212. The variable's readers are the BPI getter's callers, none of them in her live data.
fn assassinating_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = arya_mut(w, f) {
        s.is_assassinating = true;
    }
}

fn assassinating_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = arya_mut(w, f) {
        s.is_assassinating = false;
    }
}

/// AnimNotify_AryaSwordGlowOn: "BPI SwordGlowOn" / "BPISwordGlowOff" -> AryaSwordGlowOn @7107 / Off @7122:
/// Emissive_Mult 80 @4915 / 0 @4135 on the sword's dynamic material. No variable: an event for the drawing.
fn glow(w: &mut World, f: FighterId, on: bool) {
    if arya_of(w, f).is_some() {
        let fighter = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::SwordGlow { fighter, on });
    }
}

fn glow_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    glow(w, f, true);
}

fn glow_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    glow(w, f, false);
}

// ---- the backstab buffs

/// Buff_AryaBackStab (DealtDamageMods DamageMod 2, Add) and its child Buff_BackStab, which has no graph of its own
/// (the passive row of her Comp_Pawn_PassiveBuffManager gives it to her allies). DealtDamageModCondition:
/// - false for no dealer @29, Victim == DamageDealer @43, a dealer that is no Pawn @160 or a victim that is no
///   BP_Fighter_Character @239;
/// - both face the same way (BPI_IsFacingLeft of each) @503, else false;
/// - with the two actor locations (world Y is the sim's -x, Z the height): facing right, Victim.Y < Dealer.Z @1134
///   (the dealer's height, as the bytecode has it: the pin is BreakVector's Z, not its Y); facing left,
///   Victim.Y > Dealer.Y @1172;
/// - true when one of the two holds, the teams differ @1050 and Victim != GetOwner() @575, @1400.
///
/// True: BackStabUIMulticast(Victim) @1452 -> Delay(0.0) @818 (one action: a call while it waits writes the event's
/// Victim and nothing more), whose resume pops the text (`backstab_tick`).
fn backstab_dealt(
    w: &mut World,
    victim: ActorRef,
    owner: FighterId,
    dealer: FighterId,
    _hit: &HitPacket,
    b: BuffRef,
    _first: bool,
) -> bool {
    let ActorRef::Fighter(v) = victim else { return false }; // @174-239: a projectile, item or actor HitActor
    if v == dealer {
        return false;
    }
    let left = w.fighter(v).body.facing_dir < 0.0;
    if (w.fighter(dealer).body.facing_dir < 0.0) != left {
        return false;
    }
    let vy = -w.fighter(v).body.x;
    let behind = if left { vy > (-w.fighter(dealer).body.x) } else { vy < actor_y(w, dealer) };
    if !(w.fighter(v).team.team != w.fighter(dealer).team.team && behind && v != owner) {
        return false;
    }
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::BackStab { made: true, pop: Some(v) };
    }
    true
}

/// @27: with AllowPop and a locally controlled owner the pop text on the victim, AllowPop false and Delay(1.0) ->
/// AllowPop true @15. Who is locally controlled is the UI's: the event goes out at every resume and the UI keeps
/// the 1.0 s.
fn backstab_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::BackStab { pop, .. } = &mut x.ext else { return false };
    let Some(v) = pop.take() else { return false };
    let (fighter, victim) = (w.fighter(f).id, w.fighter(v).id);
    w.fighter_mut(f).events.push(Event::Backstab { fighter, victim });
    false
}

/// This file's registry entries (the three notifies, the two backstab buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let pair = |b, e| NotifyHandler { begin: Some(b), end: Some(e), ..NotifyHandler::default() };
    r.notifies.enter("AnimNotify_AryaDeactivateHitboxes_C", pair(hitboxes_begin, hitboxes_end))?;
    r.notifies.enter("AnimNotify_AryaSetAssassinating_C", pair(assassinating_begin, assassinating_end))?;
    r.notifies.enter("AnimNotify_AryaSwordGlowOn_C", pair(glow_begin, glow_end))?;
    let hooks = BuffHooks { dealt_cond: Some(backstab_dealt), tick: Some(backstab_tick), ..BuffHooks::default() };
    for class in [BuffClass::BuffAryaBackStab, BuffClass::BuffBackStab] {
        let n = class.registered_name().expect("a registered class");
        r.buffs.enter(n, BuffEntry::new(class, hooks))?;
    }
    Ok(())
}
