//! Perk buff classes that bind the shared hooks outside movement: Buff_RefreshingKnockOut
//! and Buff_InMemoriam (PlayerKnockedOut), Buff_Perk_ArmorOnDeath (PlayerState Deaths), Buff_KnockOutBounce's
//! BlastBoxExitKnockbackExplosion (BlastBoxPlayerExit CheckForBlastBoxBounce), and the projectile damage /
//! knockback boosts, whose copies on a fighter's actors (GiveBuffToSpawnedActors, the projectile buffs
//! grantSpawnedBuffs) answer the actor's dealt pass (World.applyProjectileHit).
//! Read: Buff_RefreshingKnockOut, Buff_InMemoriam, Buff_Perk_ArmorOnDeath, Buff_KnockOutBounce,
//! Buff_Perk_ProjectileDamageBoost_Small, Buff_Perk_ProjectileKnockbackBoost_Small, BlastBoxExitKnockbackExplosion,
//! BlastBoxPlayerExit (CheckForBlastBoxBounce).

use super::{PerkExt, Vals, def, enter, entry, ext, ext_mut, flag, kind, num, set_ext, stacks};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_cls;
use crate::combat::defense::{ArmorPack, add_armor_pack, remove_armor};
use crate::combat::elem::{ChargeVals, instigator_charge};
use crate::combat::hits::apply_dealer_hit;
use crate::combat::packet::DataVec;
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::{Event, PopKind};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_is_projectile, latent_frames, lit, lookup_owner, timer_frames};
use crate::fighter::control::dealer_dodged;
use crate::fighter::moves::{V2, reset_air_options};
use crate::fighter::{actor_y, no_hit};
use crate::math::{max, ue};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

// ---------------------------------------------------------------- Buff_RefreshingKnockOut
// BeginPlay @517 (authority) binds the game mode's PlayerKnockedOut; the handler's gates: the owner a
// BP_FighterCharacter, not IsMovingOnGround @777, the loser's player data and pawn and damage manager valid, its
// MostRecentAttacker == the owner's controller @1695 (the port: the loser's mostRecentAttacker is the owner). Then
// MulticastRefresh(S > 1) @1743: BPI_ResetAirOptions(S > 1, S > 1, true) @1870 and (not dedicated) BPI_PopText
// @2802, "Air Moves Refreshed" with the flag, else "Special Attacks Refreshed" (life 0.5, colour (0, 1, 0.25, 1),
// offset (0, 0, 300) on the owner: hud popText kinds airRefreshed / specialsRefreshed).
// Unverified: with one stack only the air specials come back.
fn refreshing_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::PlayerKnockedOut || w.fighter(f).body.on_floor {
        return false;
    }
    let Some(v) = a.victim else { return false };
    if w.fighter(v).attackers.most_recent_attacker != Some(f) {
        return false;
    }
    let two = stacks(w, b) > 1.0;
    reset_air_options(w, f, two, two, true);
    let kind = if two { PopKind::AirRefreshed } else { PopKind::SpecialsRefreshed };
    pop(w, f, kind);
    false
}

/// A `popText` event of the owner's, stacks 0.
pub(crate) fn pop(w: &mut World, f: FighterId, kind: PopKind) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText { fighter: id, kind, stacks: 0.0, gain: None, params: None });
}

/// A `sound` event of the owner's, at a place when `at` is given.
pub(crate) fn sound(w: &mut World, f: FighterId, ev: Name, at: Option<[Real; 2]>) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: id,
        ev,
        x: at.map(|p| p[0]),
        y: at.map(|p| p[1]),
        hit: None,
        pooled: false,
        key: None,
    });
}

// ---------------------------------------------------------------- Buff_InMemoriam
// BeginPlay @1359 (authority) -> @645: GetByFighter(GetOwner()) reads its own Team Index and Owner Index, then binds
// PlayerKnockedOut @1290; a failed lookup (an owner that is not a fighter: the copy GrantBuffsFromInstigator puts
// on a spawned actor) K2_DestroyActor @626. The handler @10: the loser is not the owner, its team is the owner's:
// Active = true, the EndActive timer (re)set to ActiveDuration + S x DurationPerStack @537. EndActive @1408: Active
// = false. DealtDamageModCondition: Active.
fn memoriam_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    set_ext(w, b, PerkExt::Memoriam { active: false, at: -1.0 });
}

fn memoriam_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::PlayerKnockedOut {
        return false;
    }
    let Some(v) = a.victim else { return false };
    if v == f || w.fighter(v).team.team != w.fighter(f).team.team {
        return false;
    }
    let d = def(w, b);
    let len = num(d, "ActiveDuration") + stacks(w, b) * num(d, "DurationPerStack");
    // Set in the ring-out (World.checkBlast, behind the frame's timer pass): counted from the next one.
    let at = crate::real(w.fighter(f).clock.tick) + timer_frames(Some(len));
    if let Some(PerkExt::Memoriam { active, at: e }) = ext_mut(w, b) {
        *active = true;
        *e = at;
    }
    false
}

fn memoriam_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let tick = crate::real(w.fighter(f).clock.tick);
    if let Some(PerkExt::Memoriam { active, at }) = ext_mut(w, b)
        && *at >= 0.0
        && tick >= *at
    {
        *active = false;
        *at = -1.0;
    }
    false
}

fn memoriam_dealt(
    w: &mut World,
    _v: ActorRef,
    _o: FighterId,
    _dl: FighterId,
    _hit: &HitPacket,
    b: BuffRef,
    _hb: bool,
) -> bool {
    matches!(ext(w, b), Some(PerkExt::Memoriam { active: true, .. }))
}

/// The copy on a spawned actor: GetByFighter fails, K2_DestroyActor @626.
fn memoriam_actor_begin(_w: &mut World, _a: ActorRef, _i: u32, _from: Option<FighterId>) -> bool {
    true
}

// ---------------------------------------------------------------- Buff_Perk_ArmorOnDeath
// BeginPlay @629 (authority, owner a Pawn): its PlayerState's BPI_GetDeaths @865 (Fighter.kos: PandaGameMode
// TryUpdateIndividualScore's +1 per ring-out) > 0 -> Delay(0.2) @955 -> ApplyPerkArmorServer @563: if not Applied,
// Applied = true and ApplyPerkArmorMulticast(true) @613 -> Comp_Armor AddArmorPack(Armor) @391; then
// Delay(Duration + S x DurationPerkStack) @137 -> ApplyPerkArmorMulticast(false) @15 -> RemoveArmor(type) @508.
// EndPlay @1038: RemoveArmor(type). The buff is spawned again at each respawn (`add_perks`), so the armor comes
// 0.2 s after every respawn but the first life's.
// Unverified: the Delays are counted from the buff's first tick (combat/buffs tick_buffs runs `tick` once a frame),
// as the port's other BeginPlay Delays.
fn armor_death_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let phase = if w.fighter(f).stats.kos > 0 { 0 } else { 2 };
    set_ext(w, b, PerkExt::ArmorDeath { phase, left: latent_frames(0.2), applied: false });
}

fn armor_death_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(PerkExt::ArmorDeath { phase, left, .. }) = ext_mut(w, b) else { return false };
    if *phase == 2 {
        return false;
    }
    *left -= 1.0;
    if *left > 0.0 {
        return false;
    }
    let phase = *phase;
    let d = def(w, b);
    let pack = armor_pack(w, d, "Armor");
    if phase == 0 {
        let len = num(d, "Duration") + stacks(w, b) * num(d, "DurationPerkStack");
        let applied = matches!(ext(w, b), Some(PerkExt::ArmorDeath { applied: true, .. }));
        if !applied {
            if let Some(PerkExt::ArmorDeath { applied, .. }) = ext_mut(w, b) {
                *applied = true;
            }
            add_armor_pack(w, f, &pack);
        }
        if let Some(PerkExt::ArmorDeath { phase, left, .. }) = ext_mut(w, b) {
            *phase = 1;
            *left = latent_frames(len);
        }
    } else {
        remove_armor(w, f, pack.kind);
        if let Some(PerkExt::ArmorDeath { phase, .. }) = ext_mut(w, b) {
            *phase = 2;
        }
    }
    false
}

fn armor_death_end(w: &mut World, f: FighterId, b: BuffRef) {
    let kind = armor_pack(w, def(w, b), "Armor").kind;
    remove_armor(w, f, kind);
}

/// `armorPack`: an armor struct of a class's defaults (`field`: Armor, Buff_DodgeArmor's
/// 'Armor Pack') as `combat::defense` takes it.
pub fn armor_pack(w: &World, d: impl Vals, field: &str) -> ArmorPack {
    let a = d.val(field).and_then(Value::as_obj);
    let kind = a.and_then(|a| a.get("ArmorType")).and_then(Value::as_str).unwrap_or("None");
    ArmorPack {
        kind: if kind == "None" { name!("None") } else { lit(w, kind) },
        can_override: flag(a, "CanOverride"),
        amount: num(a, "ArmorAmount"),
        max_hits: num(a, "MaxHits"),
        threshold: num(a, "KnockBackThreshold"),
        kb_res: num(a, "KnockBackResistanceMultiplier"),
        dmg_res: num(a, "DamageResistanceMultiplier"),
        hs_res: num(a, "HitStunResistanceMultiplier"),
    }
}

// ---------------------------------------------------------------- the projectile boosts
// Buff_Perk_ProjectileDamageBoost_Small DealtDamageModCondition: the DamageDealer's Comp_Actor_Destructible valid and
// (its IsProjectile or the dealer implements BPI_Projectiles). Buff_Perk_ProjectileKnockbackBoost_Small's: the
// dealer casts to SpawnedActorParent. _Medium / _Large are their children with defaults alone (their SuperStruct).
// On the fighter the dealer is the pawn: neither holds, the copy on its actors carries the mod.
// The class test: `actor_is_projectile` (BPI_Projectiles among its interfaces, or its Destructible's IsProjectile);
// a thrown item as World dealerIsProjectile has it (IsProjectile set from the throw on).
// Unverified: a Destructible is taken as on every spawned actor (SpawnedActorParent's); every port actor that deals a
// hit as a SpawnedActorParent child.

/// `projectileDealer`.
pub fn projectile_dealer(w: &World, a: ActorRef) -> bool {
    if matches!(a, ActorRef::Item(_)) {
        return true;
    }
    let own = lookup_owner(w, a);
    let Some(cls) = actor_cls(w, a) else { return false };
    actor_is_projectile(w, ActorRef::Fighter(own), cls)
}

fn never(_w: &mut World, _v: ActorRef, _o: FighterId, _dl: FighterId, _h: &HitPacket, _b: BuffRef, _hb: bool) -> bool {
    false
}

fn dmg_boost_actor(w: &mut World, a: ActorRef, _i: u32, _v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    projectile_dealer(w, a)
}

fn kb_boost_actor(_w: &mut World, _a: ActorRef, _i: u32, _v: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    true
}

const DMG_BOOST: BuffHooks = BuffHooks { dealt_cond: Some(never), ..NO_HOOKS };
const DMG_BOOST_ACTOR: ActorBuffHooks = ActorBuffHooks { dealt_cond: Some(dmg_boost_actor), ..NO_ACTOR };
const KB_BOOST_ACTOR: ActorBuffHooks = ActorBuffHooks { dealt_cond: Some(kb_boost_actor), ..NO_ACTOR };

/// No hooks (a `const` `Default`).
pub(crate) const NO_HOOKS: BuffHooks = BuffHooks {
    begin: None,
    tick: None,
    on: None,
    take_any_damage: None,
    end: None,
    branch_group_blocked: None,
    branch_group_end_blocked: None,
    actor_collision_off: None,
    timer: None,
    max_stacks: None,
    stacks: None,
    cleanse: None,
    status: None,
    dealt_cond: None,
    taken_cond: None,
    attach_parent: None,
};

/// No actor hooks.
pub(crate) const NO_ACTOR: ActorBuffHooks = ActorBuffHooks {
    begin: None,
    tick: None,
    overlap_timer: None,
    on: None,
    end: None,
    stacks: None,
    max_stacks: None,
    hurt_sphere: None,
    hitbox_sphere: None,
    values: None,
    taken_cond: None,
    dealt_cond: None,
};

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    enter(
        r,
        &[
            kind(C::BuffRefreshingKnockOut, BuffHooks { on: Some(refreshing_on), ..NO_HOOKS }, None),
            kind(
                C::BuffInMemoriam,
                BuffHooks {
                    begin: Some(memoriam_begin),
                    on: Some(memoriam_on),
                    timer: Some(memoriam_timer),
                    dealt_cond: Some(memoriam_dealt),
                    ..NO_HOOKS
                },
                Some(ActorBuffHooks { begin: Some(memoriam_actor_begin), ..NO_ACTOR }),
            ),
            kind(
                C::BuffPerkArmorOnDeath,
                BuffHooks {
                    begin: Some(armor_death_begin),
                    tick: Some(armor_death_tick),
                    end: Some(armor_death_end),
                    ..NO_HOOKS
                },
                None,
            ),
            // Getters only (GetScale, GetMinimumKnockback, ...), read by check_for_blast_box_bounce.
            kind(C::BuffKnockOutBounce, NO_HOOKS, None),
            kind(C::BuffPerkProjectileDamageBoostSmall, DMG_BOOST, Some(DMG_BOOST_ACTOR)),
            kind(C::BuffPerkProjectileDamageBoostMedium, DMG_BOOST, Some(DMG_BOOST_ACTOR)),
            kind(C::BuffPerkProjectileDamageBoostLarge, DMG_BOOST, Some(DMG_BOOST_ACTOR)),
            kind(C::BuffPerkProjectileKnockbackBoostSmall, DMG_BOOST, Some(KB_BOOST_ACTOR)),
            kind(C::BuffPerkProjectileKnockbackBoostLarge, DMG_BOOST, Some(KB_BOOST_ACTOR)),
        ],
    )
}

// ---------------------------------------------------------------- BlastBoxExitKnockbackExplosion
// BlastBoxPlayerExit CheckForBlastBoxBounce(loser) @704: the loser's MostRecentAttackerActor a BPI_Base_Fighter @255
// holding Buff_KnockOutBounce (BPI_CheckIfHasBuff @269-518): BlastBoxExitKnockbackExplosion at the attacker's
// location, scale GetScale (ScaleBeforeStacks + S x ScaleIncreasePerStack), owner the loser (its team the loser's,
// SpawnedActorParent @11258), Instigator the attacker @1205, MaximumKnockbackRadius / MinimumKnockback /
// MaximumKnockback from the buff's getters @854-1026.
// Its BeginPlay (@2300, authority): the loser's allies (GetAllies @2425); SphereOverlapActors(its location, Base Blast
// Radius x scale, BP_FighterCharacter) @2642; for each in array order: not among the loser's allies @168, not
// IsMovingOnGround @437; centre and radius of the game state (World.stageCenter) @547 / @693; d = VSize(centre -
// location), nothing when d - R <= 0 @930; (Y, Z) = centre - location: up = FMax(Z, -Z > R ? 0 : 500) @1223-1270,
// direction = GetRotated2D((Y, up), (Y > 0 ? 1 : -1) x (Z > 0 ? 25 : 10)) @1350-1570, BaseKnockBack = Lerp(min, max,
// (d - R) / MaximumKnockbackRadius) unclamped @1662-1717 (float32 steps, as the Kismet calls),
// KnockBackDirectionVector = direction @1753, then its handler's pfgDoDamage(null, the fighter, BaseHitbox) @1848.
// DamageAndLaunch credits the hit to the dealer's Instigator, the dealer being no fighter (LocalDmgDlrOwner,
// Comp_Pawn_DamageManager @8521-8595): the attacker (the dealer's owner); the loser's team is only the overlap's
// filter (and CanBeHitBy's, the explosion's TeamIndex). Then BoomVisuals (not dedicated, @2722-3503):
// PS_BatmanBombExplosion at 100 x scale / 250, sfx_shared_class_assassin_satchel_explode, the team colour the
// Instigator's.
// BaseHitbox and Base Blast Radius: the class defaults (the general perk table's "actors"; reach::GENERAL_ACTORS;
// AttackDecayType 5, never pushed on the attacker's AttackDecayArray; OnHitAudio sfx_shared_hit_lfe).
// Unverified: the explosion's own GrantBuffsFromInstigator walks the loser's buffs, which the defeat tail destroyed
// (every perk buff has DestroyBuffWhenOwnerDefeated): nothing.
// Unverified: SphereOverlapActors' order is taken as the fighters' order (capsulesInSphere), as the port's other
// sphere overlaps.
// Unverified: the KO-ing attacker himself is pushed (another team than the loser's).

/// A dealer no list holds (a literal dealer): the KO bounce explosion (a new actor each time, its
/// handler holding the dodge record; its BeginPlay copies the Instigator's charge values,
/// ReceiveChargeValuesFromInstigator, `charge`) and Buff_C016_DistanceShot's explosion (the buff actor's own handler,
/// `buff_actor`). Made for one explosion and dropped after it.
#[derive(Debug)]
pub struct PerkDealer {
    pub cls: Name,
    pub owner: FighterId,
    /// Its class defaults.
    pub p: Props,
    pub attack_id: i32,
    pub hit: HitPacket,
    pub x: Real,
    pub y: Real,
    pub facing: Real,
    pub charge: Option<ChargeVals>,
    /// A buff actor's own handler (`DealerActor.buffActor`): no attack decay.
    pub buff_actor: bool,
    /// Its handler's dodge variables (`fighter::control::dealer_dodged`), made at its first dodge; the explosion's
    /// loop holds the dealer shared.
    pub dodge: std::cell::RefCell<Option<crate::fighter::control::DodgeState>>,
}

/// `checkForBlastBoxBounce`.
pub fn check_for_blast_box_bounce(w: &mut World, loser: FighterId) {
    const EXPLOSION: &str = "BlastBoxExitKnockbackExplosion_C";
    let Some(att) = w.fighter(loser).attackers.most_recent_attacker else { return };
    let Some(serial) = w
        .fighter(att)
        .buffs
        .buffs
        .iter()
        .find(|x| x.class == BuffClass::BuffKnockOutBounce && x.frames > 0.0)
        .map(|x| x.serial)
    else {
        return;
    };
    let b = BuffRef { fighter: att, serial };
    let Some(cd) = super::perk_actor_def(w, EXPLOSION).map(|a| a.p.clone()) else {
        return;
    };
    let d = def(w, b);
    let s = entry(w, b).map_or(0.0, |x| x.stacks);
    let scale = num(d, "ScaleBeforeStacks") + s * num(d, "ScaleIncreasePerStack");
    let radius = num(d, "MaximumKnockbackRadius");
    let min = num(d, "MinimumKnockbackBeforeStacks") + s * num(d, "MinimumKnockbackPerStack");
    let mx = num(d, "MaximumKnockbackBeforeStacks") + s * num(d, "MaximumKnockbackPerStack");
    let blast = num(Some(&cd), "Base Blast Radius") * scale;
    let base = cd
        .get("BaseHitbox")
        .and_then(Value::as_obj)
        .map_or_else(HitPacket::default, |p| HitPacket::from_props(&w.names, p, None));
    let x = w.fighter(att).body.x;
    let y = actor_y(w, att);
    let [cx, cy, cz] = w.fixed.stage_center.loc;
    let rr = w.fixed.stage_center.r;
    let attack_id = base.attack_id.unwrap_or(0);
    // receiveCharge: the values only with the class's ReceiveChargeValuesFromInstigator.
    let charge = instigator_charge(w, att).filter(|_| flag(Some(&cd), "ReceiveChargeValuesFromInstigator"));
    let dealer = PerkDealer {
        cls: name!("BlastBoxExitKnockbackExplosion_C"),
        owner: att,
        p: cd,
        attack_id,
        hit: base.clone(),
        x,
        y,
        facing: 1.0,
        charge,
        buff_actor: false,
        dodge: std::cell::RefCell::new(None),
    };
    for vic in w.sphere_trace_multi(x, y, blast) {
        // The loser's allies (its team: a fighter out of play is not overlapped) and CanBeHitBy's team test alike.
        if w.fighter(vic).team.team == w.fighter(loser).team.team || w.fighter(vic).body.on_floor {
            continue;
        }
        // UE: the explosion at (0, -x, y); centre - location, float32 components.
        let (ux, uy, uz) = (cx, (cy + x), (cz - y));
        let dist = ue::size([ux, uy, uz]); // @1060 VSize
        let over = dist - rr;
        if over <= 0.0 {
            continue;
        }
        let up = max(uz, if (-uz) > rr { 0.0 } else { 500.0 });
        // @1570 GetRotated2D((Y, Up), angle): FMath::SinCos of the angle x pi / 180.
        let r = (if uy > 0.0 { 1.0 } else { -1.0 }) * (if uz > 0.0 { 25.0 } else { 10.0 });
        let (sn, cs) = ue::sin_cos(ue::deg_to_rad(r));
        let vx = (cs * uy) - (sn * up);
        let vy = (sn * uy) + (cs * up);
        let bkb = min + ((over / radius) * (mx - min));
        let mut hit = base.clone();
        hit.base_knock_back = Some(bkb);
        hit.knock_back_direction_vector = Some(DataVec { x: Some(vx), y: Some(vy), z: None });
        if w.fighter(vic).invuln.invuln_count > 0 {
            // AttackingActor: the explosion (its handler's owner)
            let tick = w.flow.tick;
            dealer_dodged(w, &dealer, vic, hit.attack_id.unwrap_or(0), bkb, tick);
            continue;
        }
        if w.fighter(vic).life.dead > 0.0 || no_hit(w, vic) {
            continue;
        }
        // IgnoreFacing: the vector's X is UE +Y, the sim's -x.
        let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
        apply_dealer_hit(w, &dealer, vic, &hit, at, V2 { x: -vx, y: vy });
    }
    let id = w.fighter(att).id;
    w.events.push(Event::ProjectileDeath {
        cls: name!("BlastBoxExitKnockbackExplosion_C"),
        fighter: id,
        vfx: Some(name!("PS_BatmanBombExplosion")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: blast / 250.0,
        tint: None,
        to: None,
    });
    w.events.push(Event::Sound {
        fighter: id,
        ev: name!("sfx_shared_class_assassin_satchel_explode"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}
