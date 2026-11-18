//! Buffs (`FighterBuff`, the shared and the characters' buff classes): a fighter's list of
//! buff entries, the buff stun, the conditions' state. A class's own fields (the entry's `ext` bag) are a [`BuffExt`]
//! variant.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated tests are kept for their NaN result")]

use std::borrow::Cow;

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::BuffClass;
use crate::combat::HitPacket;
use crate::combat::conditions::CondState;
use crate::combat::decay::DecayHit;
use crate::combat::defense::{ArmorPack, absorb_temp_health, add_armor_pack, calculate_armor, remove_armor};
use crate::combat::hit::{
    ffa_damage, note_attacker, receive_light_hit, record_damage, remove_hitstun_from_knock_back, timer_ticks,
};
use crate::combat::holds::{apply_stun_buffs, hold_begin, hold_end, hold_tick, is_hold};
use crate::combat::packet::DataVec;
use crate::combat::pop_text::{GraphPopText, pop_text_on};
use crate::consts::{DAMAGE_DECAY_MOD, DELTA_SECONDS, OFF_LEDGE_TRACE_LENGTH};
use crate::events::{Event, NotifyRef, PopKind, SoundKey, UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::anim::cond;
use crate::fighter::common::{actor_by, buff_def, latent_frames, peers, timer_frames, warn_once};
use crate::fighter::control::{adjust_block_cooldowns, apply_ability_buffs, apply_charge_buffs};
use crate::fighter::match_side::{ground_trace, post_vo, sfx};
use crate::fighter::moves::{
    Impulse, V2, recalc_misc_movement, recalc_move_folds, recall_move, reset_jump_counts, set_flying_mode,
};
use crate::fighter::{actor_y, set_facing, signal_bound, stand_y};
use crate::math::ue;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg, FighterSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::{DelegateCall, DelegateList, PawnList, broadcast_bound};
use crate::world::stage::ComponentMove;
use indexmap::IndexMap;
use mvs_data::value::{Props, Value};

/// A buff entry by its fighter and its serial (not the entry itself): stays valid while the entry lives,
/// whatever the list's order does.
#[derive(State, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BuffRef {
    pub fighter: FighterId,
    pub serial: u32,
}

/// What a buff class adds to its entry (its `ext`), a variant per class that has one.
#[derive(State, Debug, Default)]
pub enum BuffExt {
    #[default]
    None,
    /// `Buff_ScoobySnack_C`: `{ by }`, the instigator.
    ScoobySnack { by: Option<FighterId> },
    /// `Buff_Shaggy_DyingRage_C`: `DyingRageState`.
    DyingRage { bound: bool, activated: bool },
    /// A perk buff class's own variables (`combat::perks`).
    Perk(crate::combat::perks::PerkExt),
    /// `Buff_JumpIncrease_C`: `{ life }`, the latent ticks left of its Delay(6.0).
    JumpIncrease { life: Real },
    /// `Buff_MoveCompTo_C` and `Buff_MoveCompTo_WithStun_C`: `{ move }`, the running MoveComponentTo.
    MoveCompTo { mv: Option<ComponentMove> },
    /// `Buff_Thorns_C`: `{ per }`, ReflectedDamagePerStack.
    Thorns { per: Real },
    /// `Buff_Ice_C`: `IceExt` (OldIceLevel, Frozen, AllyBreakCount).
    Ice { old: Real, frozen: bool, ally_breaks: Real },
    /// `Buff_FearStack_C`: `{ by }`, the Instigator.
    FearStack { by: Option<FighterId> },
    /// `Buff_PieHeal_C`: `{ left }`, the latent ticks left of its Delay(2.0).
    PieHeal { left: Real },
    /// `Buff_Invisibility_C` and `Buff_Invisibility_Fog_C` (`actors::smoke_bomb`): CurrentlyInvisible (`hidden`) and
    /// the owner's BaseFighter.Occluded this buff set (`occ`).
    Occlusion { hidden: bool, occ: bool },
    /// `Buff_SpeedUp_C`: DelayDestroy's Delay(1.0), the latent ticks left (`None`: not called).
    SpeedUp { left: Option<Real> },
    /// `Buff_DecrementEvadeCount_C`: CurrentDecrements (`n`), the ticks left of the running
    /// Delay(DecrementEvadeOnInterval) (`next`) and of Delay(MaxLifetime) (`life`).
    Evade { n: Real, next: Real, life: Real },
    /// `Buff_PassiveApplyBuffonHit_C`: Active, from the last dealt pass.
    PassiveApply { active: bool },
    /// `Buff_BatmanOnHitApplyBuff_C`: `{ active }`, its DealtDamageModCondition's last answer.
    BatmanOnHit { active: bool },
    /// `Buff_DelayKnockUp_C`: `BombState` (`chars::batman::bomb`).
    DelayKnockUp(crate::chars::batman::bomb::BombState),
    /// `Buff_Superman_FireSlam_C`: `{ wall }`, ActiveFireWall as the wall projectile's spawn count, -1 none.
    FireSlam { wall: i32 },
    /// `Buff_Superman_SniperPunch_C`: `{ bound }`, written by SetMultipliersServer (until then the entry has no ext).
    SniperPunch { bound: bool },
    /// `Buff_BasketballPass_C` and `Buff_IgnitedBasketballPass_C`: `PassState` (`chars::lebron::ball`).
    BasketballPass(crate::chars::lebron::ball::PassState),
    /// `Buff_Arya_Trophy_C`: `TrophyState` (`chars::arya::perks`): the latent ticks left of its BeginPlay Delay(1.0)
    /// (`delay`, 0 once it ran), PlayerKnockedOut bound (`bound`), its Team Index (`team`).
    AryaTrophy { delay: Real, bound: bool, team: i32 },
    /// `Buff_AryaBackStab_C` and `Buff_BackStab_C` (`chars::arya::pawn`): `made`: the `ext` object exists (its
    /// first true DealtDamageModCondition made it; an entry with `BuffExt::None` reads the same as `made` false);
    /// `pop`, the victim BackStabUIMulticast waits to pop the text over.
    BackStab { made: bool, pop: Option<FighterId> },
    /// `Buff_TakeFace_C` (`combat::disguise`): `{ done }`, its BeginPlay passed and its K2_DestroyActor waits for the
    /// buff's first tick (an entry with `done` false reads as no extension).
    TakeFace { done: bool },
    /// `Buff_Creature_FireStorm_C` (`chars::creature::perks`): `StormState`, its AfterSpawnedActor bind made.
    FireStorm { bound: bool },
    /// `C021_Buff_ElectricChainVictim_C` on a fighter (`chars::black_adam::chain`): `VictimExt`, the tick of the next
    /// damage tick (`at`) and the Instigator (`by`).
    ChainVictim { at: Real, by: Option<FighterId> },
    /// `Buff_C021_EarthSmashVictim_C`: `Pillar` (`chars::black_adam::bolt`).
    EarthSmashVictim(crate::chars::black_adam::bolt::Pillar),
    /// `Buff_C021_SetHomingTowardsTarget_C` (`chars::black_adam::field`): `{ done }`, its one scan made.
    SetHoming { done: bool },
    /// `Buff_Damage_Electric_C` (`combat::electric`): `ElectricState`.
    Electric(crate::combat::electric::ElectricState),
    /// The gauntlet classes (`combat::gauntlet`): `GauntletState`, from its begin with an Instigator or its first
    /// hit.
    Gauntlet(crate::combat::gauntlet::GauntletState),
    /// `Buff_NullifyNextHit_C` (`chars::wonder_woman::actors`).
    NullifyNextHit(crate::chars::wonder_woman::actors::NullifyNextHit),
    /// `Buff_Perk_WonderWoman_FastFallSlam_C`: `SlamState` (`chars::wonder_woman::perks`).
    FastFallSlam(crate::chars::wonder_woman::perks::Slam),
    /// `Buff_Perk_WonderWoman_ProjectileBlock_C`: `BlockState` (`chars::wonder_woman::perks`).
    ProjectileBlock(crate::chars::wonder_woman::perks::Block),
    /// `Buff_ConfettiCount_C` (`chars::harley::actors`).
    ConfettiCount(crate::chars::harley::actors::ConfettiCount),
    /// `Buff_ConfettiExplosionCount_C`: `ExplosionState` (`chars::harley::actors`).
    ConfettiExplosion(crate::chars::harley::actors::ConfettiExplosion),
    /// `Buff_C019_Cook_C`: `CookState` (`chars::morty::perks`).
    Cook(crate::chars::morty::perks::Cook),
    /// `Buff_C017_AllyBurst_C`: `AllyBurst` (`chars::iron_giant::actors`).
    AllyBurst(crate::chars::iron_giant::actors::AllyBurst),
    /// `Buff_C017_Ally_Pilot_C`: `PilotHold` (`chars::iron_giant::turret`).
    AllyPilot(crate::chars::iron_giant::turret::PilotHold),
    /// `C017_TempHealth_DamageReflect_Passive_C` (`chars::iron_giant::perks`).
    DamageReflect(crate::chars::iron_giant::perks::DamageReflect),
    /// `Buff_HeldByPlasmaBall_C`: `BallHold` (`chars::iron_giant::plasma`).
    HeldByPlasmaBall(crate::chars::iron_giant::plasma::BallHold),
    /// `Buff_C017_FastWeaponMode_C`: `FastState` (`chars::iron_giant::perks`).
    FastWeaponMode(crate::chars::iron_giant::perks::FastWeapon),
    /// `Buff_C017_ScorchedEarth_C`: `ScorchState` (`chars::iron_giant::perks`).
    ScorchedEarth(crate::chars::iron_giant::perks::Scorch),
    /// `Buff_StevenSharedShield_C`: `{ steven, charges }` (`chars::steven::shield`).
    StevenSharedShield(crate::chars::steven::shield::SharedShield),
    /// `Buff_BubbleCount_C`: `CountExt` (`chars::steven::tether`).
    BubbleCount(crate::chars::steven::tether::BubbleCount),
    /// `Buff_StevenBubbleEnemy_C`: `BubbleExt` (`chars::steven::tether`).
    StevenBubbleEnemy(crate::chars::steven::tether::BubbleEnemy),
    /// `Buff_Steven_GreenThumbDamageBoost_C`: `{ orig, last, pos, rate }` (`chars::steven::watermelon`).
    GreenThumbBoost(crate::chars::steven::watermelon::GreenThumbBoost),
    /// `Buff_Garnet_ElectricGroove_C`: `{ from }` (`chars::garnet::groove`).
    ElectricGroove(crate::chars::garnet::groove::Groove),
    /// `Buff_GarnetStackingSinging_C`: `SingExt` (`chars::garnet::sing`).
    StackingSinging(crate::chars::garnet::sing::SingExt),
    /// `Buff_Chicken_C`: `{ left, block, inst }` (`chars::taz::chicken`).
    Chicken(crate::chars::taz::chicken::ChickenExt),
    /// `Buff_C015AttachToDogpile_C`: `AttachState` and `cloud` (`chars::taz::perks_cloud`).
    AttachToDogpile(crate::chars::taz::perks_cloud::AttachExt),
    /// `Buff_C015_TeamFightCloud_C`: `CloudState` (`chars::taz::perks_cloud`).
    TeamFightCloud(crate::chars::taz::perks_cloud::CloudState),
    /// `Buff_Tasty_C`: `{ inst }` (`chars::taz::tasty`).
    Tasty(crate::chars::taz::tasty::TastyExt),
    /// `Buff_ApplyTasty_C` on a fighter: `{ inst }` (`chars::taz::tasty`).
    ApplyTasty(crate::chars::taz::tasty::ApplyTastyExt),
    /// `Buff_TastyOnHit_C` on a fighter: `{ inst, open }` (`chars::taz::tasty`).
    TastyOnHit(crate::chars::taz::tasty::TastyOnHitExt),
    /// `Buff_TastyBlock_C`: `{ due, duration }` (`chars::taz::tasty`).
    TastyBlock(crate::chars::taz::tasty::TastyBlockExt),
    /// `Buff_c015_ApplybuffOnOverlap_C` (the lick): `LickExt` (`chars::taz::tasty`).
    Lick(crate::chars::taz::tasty::LickExt),
    /// `Buff_Charm_C`: `CharmExt` (`chars::bugs::charm`).
    Charm(crate::chars::bugs::charm::CharmExt),
    /// `Buff_Tunneling_C`: `TunnelingExt` (`chars::bugs::tunnel`).
    Tunneling(crate::chars::bugs::tunnel::TunnelingExt),
    /// `Buff_TunnelTeleport_C`: `TeleportExt` (`chars::bugs::tunnel`).
    TunnelTeleport(crate::chars::bugs::tunnel::TeleportExt),
    /// `C020_Buff_Silenced_C` on a fighter: `SilencedExt` (`chars::rick`).
    C020Silenced(crate::chars::rick::SilencedExt),
    /// `Buff_Teleporting3_C`: `TeleportExt` (`chars::rick::teleport`).
    Teleporting3(crate::chars::rick::teleport::TeleportExt),
    /// `Buff_PortalBuff_C`: `{ left }` (`chars::rick::teleport`).
    PortalBuff(crate::chars::rick::teleport::PortalBuffExt),
    /// `Buff_Finn_GemOnCharge_C`: `GemVars` (`chars::finn::perks`).
    GemOnCharge(crate::chars::finn::perks::GemVars),
    /// `Buff_Finn_GoingOutOfBusiness_C`: `SaleVars` (`chars::finn::perks`).
    GoingOutOfBusiness(crate::chars::finn::perks::SaleVars),
    /// `Finn_Shop_Boots_Buff_C` and `Finn_Shop_Boots_Buff_Ally_C`: `BootsExt` (`chars::finn::shop`).
    ShopBoots(crate::chars::finn::shop::BootsExt),
    /// `Finn_Shop_ArmorBuns_Buff_C` (`chars::finn::shop`).
    ArmorBuns(crate::chars::finn::shop::BunsExt),
    /// `Buff_EvidenceCount_C`: `CountState` (`chars::velma::evidence`).
    EvidenceCount(crate::chars::velma::evidence::CountExt),
    /// `Buff_Studied_C`: `StudiedState` (`chars::velma::perks`).
    Studied(crate::chars::velma::perks::StudiedExt),
    /// `Buff_Arrested_C`: `ArrestedExt` (`chars::velma::police`).
    Arrested(crate::chars::velma::police::ArrestedExt),
    /// `Buff_VelmaHealOverTimeBlast_C`: `BlastExt` (`chars::velma::beam`).
    HealOverTimeBlast(crate::chars::velma::beam::BlastExt),
    /// `Buff_VelmaAttachToAlly_C`: `AttachState` and its own (`chars::velma::capture`).
    VelmaAttachToAlly(crate::chars::velma::capture::CaptureExt),
    /// `Buff_SlowIntoStun_C`: `SlowExt` (`chars::velma::gadgets`).
    SlowIntoStun(crate::chars::velma::gadgets::SlowExt),
    /// `C023B_DashProjectileBlock_C`: `{ closed }` (`chars::stripe::dash`).
    DashProjectileBlock(crate::chars::stripe::dash::BlockExt),
    /// `Buff_Jake_WeaponMaster_C`: `MasterState` (`chars::jake::weapons`).
    JakeWeaponMaster(crate::chars::jake::weapons::MasterExt),
    /// `Buff_Jake_SafetyNet_C`: `SafetyNetState` (`chars::jake::perks`).
    JakeSafetyNet(crate::chars::jake::perks::SafetyNetExt),
    /// `Buff_AirSuperiority_C`: `AirState` (`chars::jake::perks`).
    AirSuperiority(crate::chars::jake::perks::AirExt),
    /// `Buff_Cheese_C`: `{ delay, bound }` (`chars::tom_and_jerry::cheese`).
    Cheese(crate::chars::tom_and_jerry::cheese::CheeseExt),
    /// `Buff_C018HomingVictim_C`: `HomingVictimExt` (`chars::marvin::homing`).
    C018HomingVictim(crate::chars::marvin::homing::HomingVictimExt),
    /// `Buff_DynamicMoveToActor_C`, `Buff_StunnedDynamicMoveToActor_C` and their children: `DynamicMove`
    /// (`combat::dynamic_move`).
    DynamicMove(crate::combat::dynamic_move::DynamicMove),
    /// `Buff_MoveToActor2_C`: `MoveToActor2` (`combat::move_to_actor_2`).
    MoveToActor2(crate::combat::move_to_actor_2::MoveToActor2),
    /// `C023A_BowSlow_C`: `{ adjusted }` (`chars::gizmo::bow`).
    BowSlow(crate::chars::gizmo::bow::BowSlowExt),
    /// `Buff_C023A_Backpack_C`: `{ bound, spawned }` (`chars::gizmo::perks`).
    Backpack(crate::chars::gizmo::perks::BackpackExt),
    /// `C023A_DanceCount_Debuff_C`: `{ from, wait }` (`chars::gizmo::perks`).
    DanceCount(crate::chars::gizmo::perks::DanceCountExt),
    /// `C023A_Music_Buff_C` and `C023A_Music_DeBuff_C`: `{ from }` (`chars::gizmo::music`).
    GizmoMusic(crate::chars::gizmo::music::MusicExt),
    /// `C023A_Car_Buff_C`: `{ base, car }` (`chars::gizmo::car`).
    C023ACarBuff(crate::chars::gizmo::car::CarBuffExt),
    /// `Car_Buff_C` (the item cars'): `{ base, car, canJump }` (`items::car`).
    ItemCarBuff(crate::items::car::ItemCarBuffExt),
    /// `Mutator_InstantKill_C`: `{ left }` (`combat::mutator_buffs`).
    InstantKill(crate::combat::mutator_buffs::InstantKillExt),
    /// `Mutator_GrowWithDamage_C`: `{ dealt }` (`combat::grow_with_damage`).
    GrowWithDamage(crate::combat::grow_with_damage::GrowExt),
}

impl BuffExt {
    /// The dodge variables of the buff actor's own Comp_Pawn_HitBoxHandler (DodgeState), for a
    /// class whose actor deals a hit (`ActorRef::Buff`); `None` for every other class.
    pub fn dodge_mut(&mut self) -> Option<&mut Option<crate::fighter::control::DodgeState>> {
        match self {
            BuffExt::DelayKnockUp(e) => Some(&mut e.dodge),
            BuffExt::BasketballPass(e) => Some(&mut e.dodge),
            BuffExt::EarthSmashVictim(e) => Some(&mut e.dodge),
            BuffExt::FastFallSlam(e) => crate::chars::wonder_woman::perks::slam_dodge_mut(e),
            BuffExt::ConfettiExplosion(e) => crate::chars::harley::actors::explosion_dodge_mut(e),
            _ => None,
        }
    }

    /// GetOwner of the buff actor whose own Comp_Pawn_HitBoxHandler deals a hit (`ActorRef::Buff`): the Instigator
    /// its class credits the hit to; `None` for every other class.
    pub fn actor_owner(&self) -> Option<FighterId> {
        match self {
            BuffExt::DelayKnockUp(e) => e.by,
            BuffExt::BasketballPass(e) => e.by,
            BuffExt::EarthSmashVictim(e) => e.by,
            BuffExt::FastFallSlam(e) => crate::chars::wonder_woman::perks::slam_owner(e),
            BuffExt::ConfettiExplosion(e) => crate::chars::harley::actors::explosion_owner(e),
            BuffExt::Arrested(e) => crate::chars::velma::police::arrested_owner(e),
            BuffExt::Electric(e) => e.by,
            _ => None,
        }
    }
}

/// `own`: the variables the entry's own graph has written over its class's defaults (`setBuffValues`), in the order
/// the writes came.
#[derive(Debug, Default)]
pub struct BuffOwn {
    pub values: Props,
}

impl Clone for BuffOwn {
    fn clone(&self) -> Self {
        BuffOwn { values: self.values.clone() }
    }

    /// A save over a save keeps the map's and the keys' storage.
    fn clone_from(&mut self, s: &Self) {
        self.values.clone_from(&s.values);
    }
}

impl crate::state::StateHash for BuffOwn {
    fn hash_state(&self, h: &mut crate::state::StateHasher) {
        hash_props(&self.values, h);
    }
}

/// A data record into the state hash, its keys in order.
pub(crate) fn hash_props(p: &Props, h: &mut crate::state::StateHasher) {
    h.write_len(p.len());
    for (k, v) in p {
        h.write_bytes(k.as_bytes());
        hash_value(v, h);
    }
}

/// A data value into the state hash, by its kind and contents.
fn hash_value(v: &Value, h: &mut crate::state::StateHasher) {
    match v {
        Value::Null => h.write_u32(0),
        Value::Bool(b) => {
            h.write_u32(1);
            h.write_u32(u32::from(*b));
        }
        Value::Num(n) => {
            h.write_u32(2);
            h.write_u64(n.to_bits());
        }
        Value::Str(s) => {
            h.write_u32(3);
            h.write_bytes(s.as_bytes());
        }
        Value::Arr(a) => {
            h.write_u32(4);
            h.write_len(a.len());
            for x in a {
                hash_value(x, h);
            }
        }
        Value::Obj(o) => {
            h.write_u32(5);
            h.write_len(o.len());
            for (k, x) in o {
                h.write_bytes(k.as_bytes());
                hash_value(x, h);
            }
        }
        Value::F32s(a) => {
            h.write_u32(6);
            h.write_len(a.len());
            for x in a {
                h.write_u32(x.to_bits());
            }
        }
    }
}

/// `FighterBuff`.
#[derive(State, Debug)]
pub struct FighterBuff {
    pub serial: u32,
    pub cls: Name,
    pub class: BuffClass,
    pub frames: Real,
    pub stacks: Real,
    pub stack_frames: Real,
    pub heal: Option<Real>,
    pub heal_amt: Option<Real>,
    pub born: Option<Real>,
    pub rescue: Option<Real>,
    pub by: Option<Real>,
    pub pulled: Option<bool>,
    pub notify: Option<NotifyRef>,
    pub fire: Option<Real>,
    pub fire_dmg: Option<Real>,
    pub src: Option<FighterId>,
    pub ext: BuffExt,
    pub stun: bool,
    pub stun_by: Option<FighterId>,
    pub mods_off: Option<bool>,
    pub own: Option<BuffOwn>,
    pub gate: Option<Real>,
    pub add_age: Option<Real>,
    pub ally_hit: bool,
    pub swept: bool,
}

/// `BuffDecay`: a buff's duration decay (`addBuffDurationDecay`).
#[derive(State, Debug)]
pub struct BuffDecay {
    pub cls: Name,
    pub timer: Real,
    pub stacks: Real,
}

/// `struggle`: a hold's button mash.
#[derive(State, Debug, Default)]
pub struct Struggle {
    pub inc: Real,
    pub max: Real,
    pub count: Real,
    pub avail: Real,
    pub period: Real,
    pub left: Real,
}

/// The fighter's buffs and buff stun.
#[derive(State, Debug, Default)]
pub struct Buffs {
    pub buffs: Vec<FighterBuff>,
    /// The next entry's serial.
    pub next_serial: u32,
    pub speed_mod: Real,
    pub buff_stunned: bool,
    pub buff_stun: Real,
    pub buff_stun_len: Real,
    pub buff_stun_age: Real,
    pub suspended: bool,
    pub grappled: bool,
    pub struggle: Option<Struggle>,
    /// `stunPicks`: calls of `apply_stun_buffs` so far, compared within one call of `tick_buffs` (not state).
    #[state(skip)]
    pub stun_picks: u32,
    /// The times the list was replaced by a filtered copy (a `filter` on `f.buffs`): a walk
    /// begun before a replacement goes on over the list it began with (not state).
    #[state(skip)]
    pub generation: u32,
    /// `condObjs`: the condition classes' state, by class name.
    pub cond_objs: Option<IndexMap<Name, CondState>>,
}

// ---- the entry list

/// The registry entry of a buff class (`BUFFS.get(cls)`).
pub(crate) fn kind_of(w: &World, class: BuffClass) -> Option<&'static BuffEntry> {
    let r: &'static Registry = w.registry;
    class.registered_name().and_then(|n| r.buffs.get(n))
}

/// The entry `b`, while it is in its fighter's list.
pub fn buff(w: &World, b: BuffRef) -> Option<&FighterBuff> {
    w.fighter(b.fighter).buffs.buffs.iter().find(|x| x.serial == b.serial)
}

/// The entry `b` to write, while it is in its fighter's list.
pub fn buff_mut(w: &mut World, b: BuffRef) -> Option<&mut FighterBuff> {
    w.fighter_mut(b.fighter).buffs.buffs.iter_mut().find(|x| x.serial == b.serial)
}

/// Every entry of the list now, as references (`[...f.buffs]`), in a buffer of the World's pool (`put_refs` hands
/// it back; a walk inside a walk takes another).
fn refs(w: &mut World, f: FighterId) -> Vec<BuffRef> {
    let mut v = w.scratch.buff_refs.pop().unwrap_or_default();
    v.clear();
    v.extend(w.fighter(f).buffs.buffs.iter().map(|x| BuffRef { fighter: f, serial: x.serial }));
    v
}

/// A buffer of `refs` or `calc_order` back to the World's pool.
fn put_refs(w: &mut World, v: Vec<BuffRef>) {
    w.scratch.buff_refs.push(v);
}

/// `for (const b of f.buffs)` with a body that may push to the list or replace it: the walk sees what is pushed
/// while the list it began with is the fighter's, and goes on over that one once it is replaced.
struct Walk {
    arr: Vec<BuffRef>,
    generation: u32,
    i: usize,
}

impl Walk {
    fn new(w: &mut World, f: FighterId) -> Self {
        let arr = refs(w, f);
        Walk { arr, generation: w.fighter(f).buffs.generation, i: 0 }
    }

    /// The walk's buffer back to the pool.
    fn done(self, w: &mut World) {
        put_refs(w, self.arr);
    }

    fn next(&mut self, w: &World, f: FighterId) -> Option<BuffRef> {
        let bs = &w.fighter(f).buffs;
        if bs.generation == self.generation && bs.buffs.len() > self.arr.len() {
            let n = self.arr.len();
            self.arr.extend(bs.buffs[n..].iter().map(|x| BuffRef { fighter: f, serial: x.serial }));
        }
        let r = self.arr.get(self.i).copied()?;
        self.i += 1;
        Some(r)
    }
}

/// The list replaced by its live entries (`f.buffs = f.buffs.filter((b) => b.frames > 0)`).
fn filter_live(w: &mut World, f: FighterId) {
    let bs = &mut w.fighter_mut(f).buffs;
    bs.buffs.retain(|b| b.frames > 0.0);
    bs.generation = bs.generation.wrapping_add(1);
}

// ---- a buff's values

/// A buff actor's values: its class defaults under what its own graph has written (`instDef`), read a key at a
/// time; `base` none: a class no fighter of the match has.
#[derive(Clone, Copy, Debug, Default)]
pub struct Def<'a> {
    pub base: Option<&'a Props>,
    pub own: Option<&'a Props>,
}

impl<'a> Def<'a> {
    /// A class's defaults alone (`buffDef`).
    pub fn of(p: Option<&'a Props>) -> Self {
        Def { base: p, own: None }
    }

    /// `d.k` (`{ ...d, ...own }[k]`).
    pub fn get(&self, k: &str) -> Option<&'a Value> {
        self.own.and_then(|o| o.get(k)).or_else(|| self.base.and_then(|d| d.get(k)))
    }

    /// `d` itself was found (`instDef(...) !== undefined`).
    pub fn some(&self) -> bool {
        self.base.is_some() || self.own.is_some()
    }

    /// A number (`d.k`, none for undefined, null or another kind).
    pub fn num(&self, k: &str) -> Option<Real> {
        self.get(k).and_then(Value::as_f64).map(crate::math::narrow)
    }

    /// `!!d.k`.
    pub fn flag(&self, k: &str) -> bool {
        truthy(self.get(k))
    }

    /// An object member (`d.k`).
    pub fn obj(&self, k: &str) -> Option<&'a Props> {
        self.get(k).and_then(Value::as_obj)
    }

    /// The whole record (`{ ...d, ...own }`).
    pub fn merged(&self) -> Option<Cow<'a, Props>> {
        match (self.base, self.own) {
            (b, None) => b.map(Cow::Borrowed),
            (b, Some(o)) => {
                let mut m = b.cloned().unwrap_or_default();
                for (k, v) in o {
                    m.insert(k.clone(), v.clone());
                }
                Some(Cow::Owned(m))
            }
        }
    }
}

/// JavaScript's truthiness of a data value (`undefined` for none).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// A number of a property bag.
fn pnum(p: Option<&Props>, k: &str) -> Option<Real> {
    p.and_then(|p| p.get(k)).and_then(Value::as_f64).map(crate::math::narrow)
}

/// `a || b` over two numbers.
fn or_num(a: Option<Real>, b: Option<Real>) -> Option<Real> {
    match a {
        Some(x) if x != 0.0 && !x.is_nan() => a,
        _ => b,
    }
}

/// The values of a buff of `owner`'s class `cls` with the entry's own (`instDef`), the entry given.
fn def_view<'a>(w: &'a World, owner: ActorRef, cls: Name, own: Option<&'a BuffOwn>) -> Def<'a> {
    Def { base: buff_def(w, owner, cls), own: own.map(|o| &o.values) }
}

/// `instDef(f, b)`: the live buff's values, read a key at a time.
pub fn inst_view(w: &World, b: BuffRef) -> Def<'_> {
    match buff(w, b) {
        Some(x) => def_view(w, ActorRef::Fighter(b.fighter), x.cls, x.own.as_ref()),
        None => Def::default(),
    }
}

/// `instDef`: a buff actor's values, its class defaults under what its own graph has written.
pub fn inst_def_of(w: &World, b: BuffRef) -> Option<Cow<'_, Props>> {
    inst_view(w, b).merged()
}

/// `setBuffValues`: a buff's graph writing its own variables. The graph's own
/// UpdatePlayerStatus / UpdateCurrentStates call, where it has one, is the caller's (`update_states`): a write alone
/// changes nothing on the fighter.
pub fn set_buff_values(w: &mut World, b: BuffRef, values: &Props) {
    let Some(x) = buff_mut(w, b) else { return };
    let own = x.own.get_or_insert_with(BuffOwn::default);
    for (k, v) in values {
        own.values.insert(k.clone(), v.clone());
    }
}

// ---- the class flags

/// `buffThruAtk`'s 'ground' | 'air' | 'rotation'.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuffThruAtkK {
    Ground,
    Air,
    Rotation,
}

/// `buffThruAtk`: a live buff whose class raises that ThruATK counter (BuffKind.thruAtk).
pub fn buff_thru_atk(w: &World, f: FighterId, k: BuffThruAtkK) -> bool {
    let i = match k {
        BuffThruAtkK::Ground => 0,
        BuffThruAtkK::Air => 1,
        BuffThruAtkK::Rotation => 2,
    };
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .any(|b| b.frames > 0.0 && kind_of(w, b.class).and_then(|e| e.thru_atk).is_some_and(|t| t[i]))
}

/// `buffBlockFastFall`: a live buff whose class sets BPI_SetBlockFastFall(true) in its
/// BeginPlay and (false) in its EndPlay.
pub fn buff_block_fast_fall(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.frames > 0.0 && kind_of(w, b.class).is_some_and(|e| e.block_fast_fall))
}

/// `buffActiveAttack`: a live buff whose class holds an AddActiveAttack of its own.
pub fn buff_active_attack(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.frames > 0.0 && kind_of(w, b.class).is_some_and(|e| e.active_attack))
}

/// `buffCollisionOff`: a live buff whose class turns the owner's whole collision off.
/// The registry entry's `actorCollisionOff` is a hook that answers true for such a class.
pub fn buff_collision_off(w: &World, f: FighterId) -> bool {
    for b in &w.fighter(f).buffs.buffs {
        if b.frames <= 0.0 {
            continue;
        }
        let Some(h) = kind_of(w, b.class).and_then(|e| e.hooks.actor_collision_off) else { continue };
        if h(w, f, BuffRef { fighter: f, serial: b.serial }) {
            return true;
        }
    }
    false
}

/// The class name without its `_C`.
fn bare(s: &str) -> &str {
    s.strip_suffix("_C").unwrap_or(s)
}

/// BuffParentActor.IsBuffOfClass (@0-266): a buff actor of class `buff` is of class `of` when `of` is that class
/// (EqualEqual_SoftClassReference @190) or one of its parents (ClassIsChildOf @152), the data's
/// [`mvs_data::BuffSupers`]. Both are class names as the data writes them (`Buff_BasketballPass_C`).
// A store without the parents (the one the trace runs read) leaves this the exact class.
pub fn is_buff_of_class(w: &World, buff: &str, of: &str) -> bool {
    buff == of || buff_parents(w, buff).iter().any(|p| p == of)
}

/// [`is_buff_of_class`] with both names taken with or without their `_C` (CheckHasBuff's readers:
/// `hasBuff` / `buffStacks` compare the names so).
fn is_buff_of_bare(w: &World, buff: &str, of: &str) -> bool {
    let of = bare(of);
    bare(buff) == of || buff_parents(w, buff).iter().any(|p| bare(p) == of)
}

/// A buff class's Blueprint parents, nearest first (none where the store has no BuffSupers entry).
fn buff_parents<'w>(w: &'w World, buff: &str) -> &'w [String] {
    w.data.buff_supers.get(buff).map_or(&[], Vec::as_slice)
}

/// `buffStacks`: BPI_CheckIfHasBuff's stacks (Comp_State_Manager CheckHasBuff):
/// CurrentStackAmount of the last buff actor of the class or a child of it ([`is_buff_of_class`] @757), not the
/// number of actors.
pub fn buff_stacks(w: &World, f: FighterId, cls: &str) -> Real {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .rev()
        .find(|x| is_buff_of_bare(w, w.names.str(x.cls), cls))
        .map_or(0.0, |b| b.stacks)
}

/// `hasBuff`: BPI_CheckIfHasBuff's HasBuff (Comp_State_Manager CheckHasBuff @757-840): a
/// live buff actor of the class or a child of it ([`is_buff_of_class`]), whatever its CurrentStackAmount.
pub fn has_buff(w: &World, f: FighterId, cls: Name) -> bool {
    let of = w.names.str(cls);
    w.fighter(f).buffs.buffs.iter().any(|x| x.frames > 0.0 && is_buff_of_bare(w, w.names.str(x.cls), of))
}

// ---- the notify's buffs

/// The properties of a notify of `f`'s montages (or of a montage its second meshes play).
fn notify_p(w: &World, f: FighterId, n: NotifyRef) -> Option<&Props> {
    crate::fighter::anim::montages::any_notify_in(&w.data, w, f, n).map(|x| &x.p)
}

/// The interned name of a data string (a class or condition the data names).
fn data_name(w: &World, s: &str) -> Option<Name> {
    w.names.get(s)
}

/// `applyBuffNotify`: AnimNotify_ApplyBuff begin. BuffSelf: SpawnBuffActor on the owner
/// (after an optional Condition check); otherwise BuffAllAllies spawns it on every other fighter of the same team
/// (@999-2015), which the world does (`applyAllyBuffs`). With DestroyBuffsOnNotifyEnd the spawned actor is filed
/// under the notify (Comp_State_Manager AddNotifySpawnedBuffs) for the notify's end.
pub fn apply_buff_notify(w: &mut World, f: FighterId, n: NotifyRef) {
    let Some(p) = notify_p(w, f, n) else { return };
    let buff_self = match p.get("BuffSelf") {
        None | Some(Value::Null) => true,
        v => truthy(v),
    };
    if !buff_self {
        if truthy(p.get("BuffAllAllies")) && truthy(p.get("Buff")) {
            w.fighter_mut(f).requests.ally_buff_requests.push(n);
        }
        return;
    }
    let condition = p.get("Condition").and_then(Value::as_str).filter(|c| !c.is_empty() && *c != "None");
    let condition = condition.map(|c| data_name(w, c).unwrap_or(Name::NONE));
    let cls = p.get("Buff").and_then(Value::as_str).filter(|c| !c.is_empty()).map(|c| data_name(w, c));
    let duration = pnum(Some(p), "BuffDuration").unwrap_or(1.0);
    // Stacks goes to SpawnBuffActor's StackCount as it is (@396): the notify's default 0 starts the buff at 0 stacks.
    let stacks = pnum(Some(p), "Stacks").unwrap_or(0.0);
    let destroy = truthy(p.get("DestroyBuffsOnNotifyEnd"));
    if let Some(c) = condition
        && !cond(w, f, c)
    {
        return;
    }
    let Some(cls) = cls else { return };
    let Some(cls) = cls else {
        // A class no data of the match has: buffDef's warning, no actor.
        warn_once(w, name!("applyBuffNotify: a buff class the match has no data for"));
        return;
    };
    #[allow(clippy::cast_possible_truncation, reason = "a notify's Stacks is an integer")]
    let b = spawn_buff(w, f, cls, duration, Some(stacks as i32), None, true, None);
    if let Some(b) = b
        && destroy
        && let Some(x) = buff_mut(w, b)
    {
        x.notify = Some(n);
    }
}

/// `removeNotifyBuffs`: AnimNotify_ApplyBuff's Received_NotifyEnd with
/// DestroyBuffsOnNotifyEnd: Comp_State_Manager RemoveNotifySpawnedBuffs(notify, Stacks) (ubergraph 8324, 5047-5611):
/// every buff actor filed under this notify is destroyed when Stacks is 0, else loses that many stacks.
pub fn remove_notify_buffs(w: &mut World, f: FighterId, n: NotifyRef) {
    let stacks = notify_p(w, f, n).and_then(|p| pnum(Some(p), "Stacks")).unwrap_or(0.0);
    let mut any = false;
    let mut walk = Walk::new(w, f);
    while let Some(r) = walk.next(w, f) {
        let Some(b) = buff(w, r) else { continue };
        if b.notify != Some(n) || b.frames <= 0.0 {
            continue;
        }
        if stacks != 0.0 {
            let x = buff_mut(w, r).expect("the walked entry");
            x.stacks -= stacks;
            let (class, s) = (x.class, x.stacks);
            buff_status_update(w, f, class, s, false, false);
            if s > 0.0 {
                update_states(w, f); // RemoveStack @938-1009
                // the class's own RemoveStack
                let h = kind_of(w, class).and_then(|e| e.hooks.stacks);
                if !h.is_some_and(|h| h(w, f, r, false)) {
                    continue;
                }
            }
        }
        if let Some(x) = buff_mut(w, r) {
            x.frames = 0.0;
        }
        any = true;
    }
    walk.done(w);
    if any {
        tick_buffs(w, f, true);
    }
}

// ---- the duration decay

/// `addBuffDurationDecay`: AddBuffDurationDecay(BuffClassParam) @0-803: an entry: stacks + 1,
/// written only while the new count <= len(BuffDurationDecayLevels); none: timer = BuffDurationDecayTime, stacks 1.
fn add_buff_duration_decay(w: &mut World, f: FighterId, cls: Name, levels: Real, time: Real) {
    let list = &mut w.fighter_mut(f).moves.buff_decays;
    if let Some(e) = list.as_mut().and_then(|l| l.iter_mut().find(|x| x.cls == cls)) {
        let n = e.stacks + 1.0;
        if n <= levels {
            e.stacks = n;
        }
        return;
    }
    let ne = BuffDecay { cls, timer: time, stacks: 1.0 };
    match list {
        Some(l) => l.push(ne),
        None => *list = Some(vec![ne]),
    }
}

/// The length of a class's BuffDurationDecayLevels.
fn decay_levels(d: Def<'_>) -> Option<&[Value]> {
    d.get("BuffDurationDecayLevels").and_then(Value::as_arr)
}

/// `decayedBuffDuration`: GetDecayedBuffDuration @0-475: an entry: FMin(levels[stacks - 1],
/// the given duration); none: the given duration. Array_Get out of range gives 0.
fn decayed_buff_duration(w: &World, f: FighterId, cls: Name, d: Def, duration: Real) -> Real {
    let Some(e) = w.fighter(f).moves.buff_decays.as_ref().and_then(|l| l.iter().find(|x| x.cls == cls)) else {
        return duration;
    };
    let i = e.stacks - 1.0;
    let lv = if i >= 0.0 && i.fract() == 0.0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a whole index >= 0")]
        decay_levels(d).and_then(|l| l.get(i as usize)).and_then(Value::as_f64).unwrap_or(0.0)
    } else {
        0.0
    };
    min(crate::math::narrow(lv), duration)
}

/// `tickBuffDurationDecay`: TickBuffDurationDecay @0-1412: timer - 1; at <= 0, stacks - 1
/// and the timer back to the class's BuffDurationDecayTime; the entry is kept only while stacks > 0.
fn tick_buff_duration_decay(w: &mut World, f: FighterId) {
    let Some(list) = w.fighter(f).moves.buff_decays.as_ref() else { return };
    let times: Vec<Real> = list
        .iter()
        .map(|e| pnum(buff_def(w, ActorRef::Fighter(f), e.cls), "BuffDurationDecayTime").unwrap_or(0.0))
        .collect();
    let x = w.fighter_mut(f);
    let list = x.moves.buff_decays.as_mut().expect("checked above");
    let mut i = 0;
    list.retain_mut(|e| {
        let mut t = e.timer - 1.0;
        let mut n = e.stacks;
        if t <= 0.0 {
            n -= 1.0;
            t = times[i];
        }
        i += 1;
        if n > 0.0 {
            e.timer = t;
            e.stacks = n;
            true
        } else {
            false
        }
    });
    if list.is_empty() {
        x.moves.buff_decays = None;
    }
}

/// `buffDecayTick`: Comp_State_Manager HandleBuffDurationDecays' loop (ubergraph @5028 ->
/// @6985 -> @979: Delay(1.0), TickBuffDurationDecay, the same Delay again): a tick every latentFrames(1.0) frames
/// from the pawn's BeginPlay (`decay_t0`), in the end-of-frame latent pass.
/// Unverified: a buff whose life is a Delay on a ticking actor ends in that actor's tick, before the decay; which
/// kind each buff's life is is not read.
pub fn buff_decay_tick(w: &mut World, f: FighterId) {
    let x = w.fighter(f);
    if x.moves.buff_decays.is_none() {
        return;
    }
    let n = crate::real(x.clock.tick) - x.moves.decay_t0.unwrap_or(0.0);
    if n > 0.0 && n % latent_frames(1.0) == 0.0 {
        tick_buff_duration_decay(w, f);
    }
}

/// `buffDecayReset`: World.reset: a new pawn's Comp_State_Manager (an empty map, its own loop
/// from this tick).
pub fn buff_decay_reset(w: &mut World, f: FighterId) {
    let x = w.fighter_mut(f);
    x.moves.buff_decays = None;
    x.moves.decay_t0 = if x.clock.tick != 0 { Some(crate::real(x.clock.tick)) } else { None };
}

/// `isEnlarge`: a class whose graph is Buff_Enlarge's (the class, or Buff_Arcade_Enlarge,
/// which changes only the defaults).
pub fn is_enlarge(cls: Name) -> bool {
    cls == name!("Buff_Enlarge_C") || cls == name!("Buff_Arcade_Enlarge_C")
}

// ---- the spawn

/// The class's armor pack (`buffArmor`): Buff_ArmorPack's ArmorStruct, Buff_RespawnArmor's 'Armor Pack'.
fn buff_armor<'a>(d: Def<'a>) -> Option<&'a Props> {
    match d.get("ArmorStruct") {
        None | Some(Value::Null) => d.obj("Armor Pack"),
        Some(v) => v.as_obj(),
    }
}

/// The armor pack's ArmorType.
fn armor_type(w: &World, a: &Props) -> Name {
    a.get("ArmorType").and_then(Value::as_str).and_then(|s| data_name(w, s)).unwrap_or(name!("None"))
}

/// A Stun struct's StunDuration (`d.Stun?.StunDuration ?? 0`).
fn stun_duration(d: Def) -> Real {
    pnum(d.obj("Stun"), "StunDuration").unwrap_or(0.0)
}

/// A StateInfo list of names (`si.BlockerGroups` and the like).
fn name_list<'a>(si: &'a Props, k: &str) -> &'a [Value] {
    si.get(k).and_then(Value::as_arr).unwrap_or(&[])
}

/// `list.includes(s)`.
fn list_has(list: &[Value], s: &str) -> bool {
    list.iter().any(|v| v.as_str() == Some(s))
}

/// The pushed entry's defaults (`newBuffEntry`).
fn new_buff_entry(w: &mut World, f: FighterId, cls: Name, frames: Real, stacks: Real, stack_frames: Real) -> BuffRef {
    let class = BuffClass::of(&w.names, w.names.str(cls)).unwrap_or(BuffClass::Data(cls));
    let bs = &mut w.fighter_mut(f).buffs;
    let serial = bs.next_serial;
    bs.next_serial += 1;
    bs.buffs.push(FighterBuff {
        serial,
        cls,
        class,
        frames,
        stacks,
        stack_frames,
        heal: None,
        heal_amt: None,
        born: None,
        rescue: None,
        by: None,
        pulled: None,
        notify: None,
        fire: None,
        fire_dmg: None,
        src: None,
        ext: BuffExt::None,
        stun: false,
        stun_by: None,
        mods_off: None,
        own: None,
        gate: None,
        add_age: None,
        ally_hit: false,
        swept: false,
    });
    BuffRef { fighter: f, serial }
}

/// `spawnBuff`: Comp_State_Manager SpawnBuffActor: the duration through
/// GetDecayedBuffDuration, then SweepBuffState over the owner's buff actors (a live one of the class: CanOverride
/// destroys it; else the spawn is blocked, CanStack adds the stacks and CanRefresh restarts its life timer), the
/// StateInfo's blockers and breaks, then BuffParentActor's ReceiveBeginPlay and UpdatePlayerStatus(NewBuff).
/// `caller`: the fighter whose Comp_State_Manager makes the call and broadcasts NewBuffSpawned (@1427) and
/// BuffStacksAdded (SweepBuffState @1671); none: the buff owner's own.
/// Unverified: a call site that names no caller is taken as the owner's own manager.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
#[allow(clippy::too_many_lines, reason = "SpawnBuffActor and the BeginPlay, in their own order")]
pub fn spawn_buff(
    w: &mut World,
    f: FighterId,
    cls: Name,
    duration: Real,
    stack_count: Option<i32>,
    instigator: Option<FighterId>,
    break_if_blocked: bool,
    caller: Option<FighterId>,
) -> Option<BuffRef> {
    if cls.is_none() {
        return None; // a None class spawns no actor
    }
    let own = ActorRef::Fighter(f);
    let d = Def::of(buff_def(w, own, cls)); // warns for a class no fighter of the match has
    if !d.some() {
        return None;
    }
    // SpawnSpawnBuffActor @182-@241: the decay entry first, then GetDecayedBuffDuration's duration for every spawn.
    if d.flag("HasBuffDurationDecay") {
        #[allow(clippy::cast_precision_loss, reason = "a short list")]
        let levels = decay_levels(d).map_or(0.0, |l| l.len() as Real);
        let time = d.num("BuffDurationDecayTime").unwrap_or(0.0);
        add_buff_duration_decay(w, f, cls, levels, time);
    }
    let d = Def::of(buff_def(w, own, cls));
    let duration = decayed_buff_duration(w, f, cls, d, duration);
    // @291-339: DecayStackLevel above 2 -> BuffDecayUI(LocalBuffInstigator), ahead of SweepBuffState.
    if let Some(i) = instigator {
        let lv = w
            .fighter(f)
            .moves
            .buff_decays
            .as_ref()
            .and_then(|l| l.iter().find(|x| x.cls == cls))
            .map_or(0.0, |e| e.stacks);
        if lv > 2.0 {
            let id = w.fighter(i).id;
            w.fighter_mut(f).events.push(Event::PopText {
                fighter: id,
                kind: PopKind::BuffDecay,
                stacks: lv,
                gain: None,
                params: None,
            });
        }
    }
    let d = Def::of(buff_def(w, own, cls));
    let n = stack_count.map_or_else(|| d.num("SpawnedStackCount").unwrap_or(1.0), crate::real);
    let mgr = caller.unwrap_or(f);
    // BuffParentActor SetLifeTimer @4110-4164: any duration <= 0 leaves the actor with no life timer.
    let frames = if duration > 0.0 { timer_ticks(Some(duration)) } else { Real::INFINITY };
    let old = w
        .fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|b| b.cls == cls && b.frames > 0.0)
        .map(|b| BuffRef { fighter: f, serial: b.serial });
    // SweepBuffState @1364-2041 reads CanOverride, CanStack, PreventStacksWhenReapplied, CanRefresh and AlwaysRefresh
    // from the live actor.
    if let Some(old) = old {
        let od = inst_view(w, old);
        let od = if od.some() { od } else { d };
        if !od.flag("CanOverride") {
            let (stack, refresh, always) = (
                od.flag("CanStack") && !od.flag("PreventStacksWhenReapplied"),
                od.flag("CanRefresh"),
                od.flag("AlwaysRefresh"),
            );
            if stack {
                let od = od.merged().map(Cow::into_owned).unwrap_or_default();
                add_stack_gated(w, f, old, &od, n);
                // @1671: BuffStacksAdded(the live buff, StackCount) right after AddStack, whatever it did.
                if !w.fighter(mgr).buffs.buffs.is_empty() {
                    let arg = BuffSignalArg {
                        buff_cls: Some(cls),
                        buff_owner: Some(f),
                        stacks: Some(n),
                        ..BuffSignalArg::default()
                    };
                    buff_signal(w, mgr, BuffSignal::BuffStacksAdded, arg);
                }
            }
            let ob = buff(w, old)?;
            if ob.frames <= 0.0 {
                return None; // its BPI On Max Stacks destroyed it
            }
            // @1709-2041: CanRefresh && ((the LifeTimer active && (new > remaining || new <= 0)) || AlwaysRefresh).
            let of = ob.frames;
            if refresh && ((of != Real::INFINITY && (frames > of || duration <= 0.0)) || always) {
                buff_mut(w, old).expect("checked above").frames = frames + 1.0; // pending in this frame
                let class = buff(w, old).expect("checked above").class;
                buff_status_update(w, f, class, 0.0, false, false); // RefreshBuff: StatusUpdateMulticast(0, refreshed)
            }
            return None;
        }
        // K2_DestroyActor on the old one: its EndPlay runs first.
        let x = buff_mut(w, old).expect("found above");
        x.frames = 0.0;
        x.swept = true;
        let class = x.class;
        let kind = kind_of(w, class);
        let parent = !kind.is_some_and(|k| k.end_skips_parent);
        parent_loop_end(w, f, old, parent);
        if parent {
            buff_status_update(w, f, class, 0.0, false, true);
        }
        rescue_end(w, f, cls);
        if w.fighter(f).holds.hold.as_ref().is_some_and(|h| h.cls == class) {
            hold_end(w, f, true);
        }
        if let Some(end) = kind.and_then(|k| k.hooks.end) {
            end(w, f, old);
        }
        let d = Def::of(buff_def(w, own, cls));
        if let Some(a) = buff_armor(d) {
            let t = armor_type(w, a);
            remove_armor(w, f, t);
        }
        let bs = &mut w.fighter_mut(f).buffs;
        bs.buffs.retain(|b| b.serial != old.serial);
        bs.generation = bs.generation.wrapping_add(1);
    }
    let d = Def::of(buff_def(w, own, cls));
    if let Some(si) = d.obj("StateInfo") {
        let (bg, kg, bb, kb) = (
            name_list(si, "BlockerGroups"),
            name_list(si, "BreakGroups"),
            name_list(si, "BlockerBuffs"),
            name_list(si, "BreakBuffs"),
        );
        if !(bg.is_empty() && kg.is_empty() && bb.is_empty() && kb.is_empty()) {
            // SweepBuffState @2087-2885 collects the blockers and the buffs to break; @842-909 destroys the latter
            // only with !LocalBlocked || BreakIfBlocked.
            let mut blocked = false;
            let mut doomed = Vec::new();
            for b in &w.fighter(f).buffs.buffs {
                if b.frames <= 0.0 {
                    continue;
                }
                let g = buff_def(w, own, b.cls)
                    .and_then(|x| x.get("StateInfo"))
                    .and_then(Value::as_obj)
                    .and_then(|s| s.get("Group"))
                    .and_then(Value::as_str);
                let c = w.names.str(b.cls);
                if g.is_some_and(|g| list_has(bg, g)) || list_has(bb, c) {
                    blocked = true;
                }
                if g.is_some_and(|g| list_has(kg, g)) || list_has(kb, c) {
                    doomed.push(b.serial);
                }
            }
            if !doomed.is_empty() && (!blocked || break_if_blocked) {
                for b in &mut w.fighter_mut(f).buffs.buffs {
                    if doomed.contains(&b.serial) {
                        b.frames = 0.0;
                    }
                }
                tick_buffs(w, f, true);
            }
            if blocked {
                return None;
            }
        }
    }
    // Buff_HealOverTime ReceiveBeginPlay @628-703: a NotPlayerCharacter owner -> K2_DestroyActor @1484.
    // Deviation: the parent's EndPlay of that actor is not modelled.
    if cls == name!("Buff_HealOverTime_C") && w.fighter(f).life.not_player_character {
        return None;
    }
    // BuffParentActor ReceiveBeginPlay @1312-1431: SetStackLifeTimer(StackDecayBuffer, or StackDecayRate).
    let d = Def::of(buff_def(w, own, cls));
    let stack_frames = stack_life_begin_of(d);
    let min_stack_speed = d.num("MinStackSpeed");
    let has_add = d.get("NewStackApplyBuffer").is_some();
    let nb = new_buff_entry(w, f, cls, frames + 1.0, n, stack_frames);
    // BeginPlay @1454 SetMinStackTimer(MinStackSpeed); Buff_Ice BeginPlay @1169-1198: TimeSinceLastAdd = now.
    if let Some(s) = min_stack_speed
        && s != 0.0
        && !s.is_nan()
    {
        buff_mut(w, nb).expect("pushed").gate = Some(timer_frames(Some(s)));
    }
    if has_add {
        buff_mut(w, nb).expect("pushed").add_age = Some(0.0);
    }
    let class = buff(w, nb).expect("pushed").class;
    let kind = kind_of(w, class);
    buff_status_update(w, f, class, n, true, false); // BeginPlay: StatusUpdateMulticast(stacks, applied)
    let d = Def::of(buff_def(w, own, cls));
    let fire = d.get("HitDamageOnInterval").is_some() && d.num("DamageTickTime").is_some_and(|t| t > 0.0);
    parent_loop_begin(w, f, nb, fire); // BeginPlay @1593: StartLoopSoundMulticast
    let d = Def::of(buff_def(w, own, cls));
    // Buff_HealOverTime's loop; a registered class with its own healing does it in its begin.
    if d.get("HealingOnInterval").is_some() && kind.is_none() {
        heal_begin(w, f, nb);
    }
    if fire {
        fire_begin(w, f, nb, instigator);
    }
    if cls == name!("Buff_OnExitedArenaTeleportToAlly_C") {
        rescue_begin(w, f, nb, instigator);
    }
    if is_hold(cls) {
        hold_begin(w, f, cls, instigator);
    }
    if let Some(begin) = kind.and_then(|k| k.hooks.begin) {
        begin(w, f, nb, instigator, Some(duration)); // a registered class's own ReceiveBeginPlay
    }
    let d = Def::of(buff_def(w, own, cls));
    let (stun, destroy_on) = (stun_duration(d), d.flag("DestroyOnInstigatorInterrupt"));
    if let Some(i) = instigator {
        if stun > 0.0
            && let Some(x) = buff_mut(w, nb)
        {
            x.stun_by = Some(i);
        }
        // The actor's Instigator, kept for the class that reads it in the parent's graph; a hold has it in `hold.by`.
        if destroy_on
            && !is_hold(cls)
            && let Some(x) = buff_mut(w, nb)
        {
            x.src = Some(i);
        }
    }
    recalc_speed_mod(w, f, Some(nb)); // SpawnBuffActor -> UpdatePlayerStatus(NewBuff)
    apply_stun_buffs(w, f, Some(nb), false); // the same call's ApplyStunBuffs (@345)
    recalc_move_folds(w, f, Some(&nb)); // SetDefaultSpeed, ApplyJumpBuffs (@387), ApplyEvadeBuffs (@411)
    recalc_ability_mods(w, f, Some(nb));
    recalc_misc_movement(w, f, Some(&nb)); // ApplyMiscMovementBuffs (@459)
    recalc_charge_mod(w, f, Some(nb)); // ApplyChargeBuffs (@483)
    recalc_size_mod(w, f, Some(nb));
    recalc_head_size(w, f, Some(nb));
    if is_enlarge(cls) {
        let id = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::EnlargeFx { fighter: id, begin: true });
    }
    let d = Def::of(buff_def(w, own, cls));
    if let Some(a) = buff_armor(d) {
        let o = Some(a);
        let pack = ArmorPack {
            kind: armor_type(w, a),
            can_override: a.get("CanOverride").and_then(Value::as_bool).unwrap_or(true),
            amount: pnum(o, "ArmorAmount").unwrap_or(40.0),
            max_hits: pnum(o, "MaxHits").unwrap_or(2.0),
            threshold: pnum(o, "KnockBackThreshold").unwrap_or(80000.0),
            kb_res: pnum(o, "KnockBackResistanceMultiplier").unwrap_or(0.0),
            dmg_res: pnum(o, "DamageResistanceMultiplier").unwrap_or(1.0),
            hs_res: pnum(o, "HitStunResistanceMultiplier").unwrap_or(0.0),
        };
        add_armor_pack(w, f, &pack);
    }
    // @1427: broadcast NewBuffSpawned(the new buff, LocalBuffInstigator) on the caller, behind UpdatePlayerStatus.
    if !w.fighter(mgr).buffs.buffs.is_empty() {
        let arg =
            BuffSignalArg { buff_cls: Some(cls), buff_owner: Some(f), stacks: Some(n), ..BuffSignalArg::default() };
        buff_signal(w, mgr, BuffSignal::NewBuffSpawned, arg);
    }
    signal_bound(w, mgr, FighterSignal::NewBuffSpawned, false, n, cls, Some(f)); // a pawn component's binding
    Some(nb)
}

// ---- the mods of the live buffs (Comp_State_Manager CalculateBuffValues)

/// The buffs CalculateBuffValues walks: the new buff first, then the owner's live buff children.
fn calc_order(w: &mut World, f: FighterId, first: Option<BuffRef>) -> Vec<BuffRef> {
    let mut v = w.scratch.buff_refs.pop().unwrap_or_default();
    v.clear();
    if let Some(b) = first
        && buff(w, b).is_some()
    {
        v.push(b);
    }
    for b in &w.fighter(f).buffs.buffs {
        if Some(b.serial) != first.map(|x| x.serial) && b.frames > 0.0 {
            v.push(BuffRef { fighter: f, serial: b.serial });
        }
    }
    v
}

/// `recalcSpeedMod`: CalculateBuffValues' speed part: LocalCumulativeSpeedMod from 1 by each
/// buff's GetMoveMod by its SpeedModType (Multiply, Add, Override, DoNothing); a buff with UseSpeedOverride gives
/// LocalUseSpeedOverride and its SpeedFlatOverride (@11025-11172), and ApplyMovementSpeedBuffs then takes
/// CharacterSpeedModifier 1 (@246).
pub fn recalc_speed_mod(w: &mut World, f: FighterId, first: Option<BuffRef>) {
    let mut m = 1.0;
    let mut over = None;
    let order = calc_order(w, f, first);
    for &b in &order {
        let d = inst_view(w, b);
        if d.flag("UseSpeedOverride") {
            over = Some(d.num("SpeedFlatOverride").unwrap_or(0.0));
        }
        let t = mod_type(d.get("SpeedModType"));
        if t == 0.0 {
            continue;
        }
        let mm = move_mod_of(d, buff(w, b).map_or(0.0, |x| x.stacks));
        m = if t == 1.0 {
            m * mm
        } else if t == 2.0 {
            m + mm
        } else {
            mm
        };
    }
    put_refs(w, order);
    let x = w.fighter_mut(f);
    match over {
        None => {
            x.buffs.speed_mod = m;
            x.moves.speed_override = None;
        }
        Some(o) => {
            x.buffs.speed_mod = 1.0;
            x.moves.speed_override = Some(o);
        }
    }
}

/// `moveMod`: BuffParentActor GetMoveMod: SpeedMod, with CanStack by SpeedModPerStackType.
pub fn move_mod(d: &Props, stacks: i32) -> Real {
    move_mod_of(Def::of(Some(d)), crate::real(stacks))
}

/// GetMoveMod over a buff's values.
pub fn move_mod_of(d: Def, stacks: Real) -> Real {
    let base = d.num("SpeedMod").unwrap_or(1.0);
    if d.flag("CanStack") {
        per_stack_mod(base, base, d.num("SpeedModPerStack").unwrap_or(0.0), d.get("SpeedModPerStackType"), stacks)
    } else {
        base
    }
}

/// `chargeModOf`: BuffParentActor GetChargeMod (@0-397): GetMoveMod's shape over ChargeMod,
/// but its Multiply case adds the power to ChargeModPerStack (@170).
pub fn charge_mod_of(d: Def, stacks: Real) -> Real {
    let base = d.num("ChargeMod").unwrap_or(1.0);
    let per = d.num("ChargeModPerStack").unwrap_or(0.0);
    if d.flag("CanStack") { per_stack_mod(base, per, per, d.get("ChargeModPerStackType"), stacks) } else { base }
}

/// The per-stack switch of GetMoveMod and GetChargeMod: 0 the base; 1 `pow` + per ** stacks; 2 base + stacks x per;
/// 3 stacks x per (float32 steps).
fn per_stack_mod(base: Real, pw: Real, per: Real, t: Option<&Value>, stacks: Real) -> Real {
    let t = mod_type(t);
    if t == 1.0 {
        pw + ue::powf(per, stacks)
    } else if t == 2.0 {
        base + (stacks * per)
    } else if t == 3.0 {
        stacks * per
    } else {
        base
    }
}

/// `recalcChargeMod`: CalculateBuffValues' charge part (@184, @8363-8875); ApplyChargeBuffs
/// (@483) hands it on.
pub fn recalc_charge_mod(w: &mut World, f: FighterId, first: Option<BuffRef>) {
    let mut m = 1.0;
    let order = calc_order(w, f, first);
    for &b in &order {
        let d = inst_view(w, b);
        let t = mod_type(d.get("ChargeModType"));
        if t == 0.0 {
            continue;
        }
        let c = charge_mod_of(d, buff(w, b).map_or(0.0, |x| x.stacks));
        m = if t == 1.0 {
            c * m
        } else if t == 2.0 {
            c + m
        } else {
            c
        };
    }
    put_refs(w, order);
    apply_charge_buffs(w, f, m);
}

/// `recalcSizeMod`: CalculateBuffValues' size part (@8876-10476); ApplySizeBuffs (@2461)
/// hands a value that differs by more than 1e-4 to Comp_SizeManager.SetSizeScale. One axis is kept (the build's
/// size buffs are uniform).
pub fn recalc_size_mod(w: &mut World, f: FighterId, first: Option<BuffRef>) {
    let mut m = 1.0;
    let order = calc_order(w, f, first);
    for &b in &order {
        let d = inst_view(w, b);
        m = size_mod_step_of(m, d, buff(w, b).map_or(0.0, |x| x.stacks));
    }
    put_refs(w, order);
    if (m - w.fighter(f).body.size_scale).abs() > 1e-4 {
        set_size_scale_to(w, f, m);
    }
}

/// NormalizeMultiplierFloats' percent change of one value.
fn size_pct(x: Real) -> Real {
    if x < 0.0 { -(100.0 - (x * 100.0)) } else { (x * 100.0) - 100.0 }
}

/// `sizeModStep`: one buff of CalculateBuffValues' size loop: its GetSizeMod X (with CanStack
/// + stacks x SizeModsPerStack) joins the cumulative value by NormalizeMultiplierFloats unless it is 1 within 1e-6.
pub fn size_mod_step(modifier: Real, d: &Props, stacks: i32) -> Real {
    size_mod_step_of(modifier, Def::of(Some(d)), crate::real(stacks))
}

/// The X of a SizeMods struct's MultSizeMod.
fn mult_size_x(d: Def, k: &str) -> Option<Real> {
    pnum(d.obj(k).and_then(|s| s.get("MultSizeMod")).and_then(Value::as_obj), "X")
}

/// `sizeModStep` over a buff's values.
pub fn size_mod_step_of(modifier: Real, d: Def, stacks: Real) -> Real {
    let mut v = mult_size_x(d, "SizeMods").unwrap_or(1.0);
    if d.flag("CanStack") {
        v += mult_size_x(d, "SizeModsPerStack").unwrap_or(0.0) * stacks;
    }
    if (v - 1.0).abs() <= 1e-6 {
        return modifier;
    }
    normalize_multiplier_floats(modifier, v)
}

/// `normalizeMultiplierFloats`: Comp_State_Manager NormalizeMultiplierFloats (@0-979): 0 when
/// either is 0 within 1e-6, else the two percent changes added (A + B - 1), never below 0, in float32 steps.
pub fn normalize_multiplier_floats(a: Real, b: Real) -> Real {
    if a.abs() <= 1e-6 || b.abs() <= 1e-6 {
        return 0.0;
    }
    max(((100.0 + size_pct(a)) + size_pct(b)) / 100.0, 0.0)
}

/// `setSizeScale`: Comp_SizeManager SetSizeScale: SetActorScale3D(scale) and
/// OnSizeScaleChanged. The capsule keeps its centre; a walking capsule is put back at its walking height (Buff_Enlarge
/// EndPlay @503-1173 sweeps the owner down to the floor when it IsMovingOnGround).
/// Deviation: the game resolves the grown capsule's floor penetration in the next movement tick and the EndPlay sweep
/// uses the scaled capsule times the size mod again; walls, ceilings and platforms the grown capsule reaches into are
/// not pushed out of here. The mesh offsets and actor-space spawn offsets are not scaled.
pub fn set_size_scale(w: &mut World, f: FighterId, k: i32) {
    set_size_scale_to(w, f, crate::real(k));
}

/// `setSizeScale` with a scale that is no whole number (the size mods').
pub fn set_size_scale_to(w: &mut World, f: FighterId, k: Real) {
    let x = w.fighter_mut(f);
    if k == x.body.size_scale {
        return;
    }
    x.body.size_scale = k;
    // SetActorScale3D(k) writes over a Polymorph's non-uniform scale.
    x.body.actor_scale = None;
    // `f.ch`: the base definition's capsule and mesh scale, times k.
    let mut base = crate::fighter::ScaledDef::of(crate::combat::hit::def_of(w, w.fighter(f).ch));
    if let Some(m) = w.fighter(f).base_mesh_scale {
        base.mesh_scale = m;
    }
    w.fighter_mut(f).scaled = if k == 1.0 {
        base
    } else {
        crate::fighter::ScaledDef {
            cap_half: (base.cap_half * k),
            cap_radius: (base.cap_radius * k),
            crouched_half: (base.crouched_half * k),
            mesh_scale: (base.mesh_scale * k),
        }
    };
    if w.fighter(f).body.on_floor {
        let y = stand_y(w, f);
        w.fighter_mut(f).body.y = y;
    }
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::SizeScale { fighter: id, scale: k });
}

/// `recalcHeadSize`: CalculateBuffValues' LocalHeadSizeMultiplier (@276 1.0, @10787-10921:
/// times each buff's HeadSizeMultiplier that is not exactly 1); UpdatePlayerStatus @555-725 writes a new value and
/// hands it to BPI_SetHeadScale.
pub fn recalc_head_size(w: &mut World, f: FighterId, first: Option<BuffRef>) {
    let mut m = 1.0;
    let order = calc_order(w, f, first);
    for &b in &order {
        let v = inst_view(w, b).num("HeadSizeMultiplier").unwrap_or(1.0);
        if v != 1.0 {
            m *= v;
        }
    }
    put_refs(w, order);
    let a = &mut w.fighter_mut(f).pose;
    if m != a.head_scale.unwrap_or(1.0) {
        a.head_scale = Some(m);
    }
}

/// `recalcAbilityMods`: CalculateBuffValues' LocalAbilityMods (@6948..7225), then
/// ApplyAbilityBuffs.
pub fn recalc_ability_mods(w: &mut World, f: FighterId, first: Option<BuffRef>) {
    let mut mult = 1.0;
    let order = calc_order(w, f, first);
    for &b in &order {
        let d = inst_view(w, b);
        let base = pnum(d.obj("AbilityMods"), "AbilityCooldownMultiplier").unwrap_or(1.0);
        // BuffParentActor.GetAbilityMods
        let m = if d.flag("CanStack") {
            let per = pnum(d.obj("AbilityModsPerStack"), "AbilityCooldownMultiplier").unwrap_or(0.0);
            base + (buff(w, b).map_or(0.0, |x| x.stacks) * per)
        } else {
            base
        };
        if (m - 1.0).abs() > 1e-6 {
            mult *= m;
        }
    }
    put_refs(w, order);
    apply_ability_buffs(w, f, mult);
}

// ---- the heal, the rescue, the fire

/// `healBegin`: Buff_HealOverTime ReceiveBeginPlay (ubergraph 489): FireVFXOn,
/// BPI_PopText('HEALING'), the heal loop at once (ApplyHealing(HealingOnInterval), Delay(HealingTickTime)), and
/// Delay(MaxLifetime) -> K2_DestroyActor beside the parent's life timer.
/// Unverified: the first Delay is taken to count the spawn frame; the negative damage is taken to pass Comp_Armor's
/// CalculateTempHealthDamage unchanged; RecordDamage is not fed.
pub fn heal_begin(w: &mut World, f: FighterId, b: BuffRef) {
    let tick = crate::real(w.fighter(f).clock.tick);
    let d = inst_view(w, b);
    let (on, every, life) =
        (d.num("HealingOnInterval").unwrap_or(0.0), d.num("HealingTickTime").unwrap_or(0.0), d.num("MaxLifetime"));
    if let Some(x) = buff_mut(w, b) {
        x.born = Some(tick);
    }
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: id,
        kind: PopKind::Healing,
        stacks: 0.0,
        gain: None,
        params: None,
    });
    apply_healing(w, f, on);
    let Some(x) = buff_mut(w, b) else { return };
    x.heal = Some(max(latent_frames(every) - 1.0, 1.0));
    if let Some(l) = life
        && l > 0.0
        && x.frames > latent_frames(l)
    {
        x.frames = latent_frames(l);
    }
}

/// `rescueBegin`: Buff_OnExitedArenaTeleportToAlly ReceiveBeginPlay (ubergraph 1825): the
/// rescue's Delay(Duration 4), taken to count the spawn frame as `heal_begin`'s.
pub fn rescue_begin(w: &mut World, f: FighterId, b: BuffRef, instigator: Option<FighterId>) {
    let tick = crate::real(w.fighter(f).clock.tick);
    let by = instigator.map_or(-1.0, |i| crate::real(w.fighter(i).id));
    let dur = inst_view(w, b).num("Duration").unwrap_or(0.0);
    let Some(x) = buff_mut(w, b) else { return };
    x.born = Some(tick);
    x.by = Some(by);
    x.rescue = Some(max(latent_frames(dur) - 1.0, 1.0));
}

/// `rescueEnd`: Buff_OnExitedArenaTeleportToAlly ReceiveDestroyed -> RemoveBuffVisuals:
/// sfx_cha_C019_teleport_ally_end at the owner.
pub fn rescue_end(w: &mut World, f: FighterId, b: Name) {
    if b != name!("Buff_OnExitedArenaTeleportToAlly_C") {
        return;
    }
    let (x, y, id) = (w.fighter(f).body.x, actor_y(w, f), w.fighter(f).id);
    w.fighter_mut(f).events.push(sound_at(id, name!("sfx_cha_C019_teleport_ally_end"), Some(x), Some(y)));
}

/// A `sound` event of the fighter.
fn sound_at(fighter: i32, ev: Name, x: Option<Real>, y: Option<Real>) -> Event {
    Event::Sound { fighter, ev, x, y, hit: None, pooled: false, key: None }
}

/// A sound event of a class's data (`d.k`, posted when set).
fn data_sound(w: &mut World, f: FighterId, ev: Option<Name>, at: bool) {
    let Some(ev) = ev.filter(|e| !e.is_none()) else { return };
    let id = w.fighter(f).id;
    let (x, y) = if at { (Some(w.fighter(f).body.x), Some(actor_y(w, f))) } else { (None, None) };
    w.fighter_mut(f).events.push(sound_at(id, ev, x, y));
}

/// A data string as a name (`d.k`, none when unset or empty).
fn data_str(w: &World, d: Def, k: &str) -> Option<Name> {
    d.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).and_then(|s| data_name(w, s))
}

/// `endBuff`: K2_DestroyActor on one of this fighter's buffs from outside its own graph: its
/// EndPlay runs in the call.
pub fn end_buff(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(x) = buff_mut(w, b) else { return };
    if x.frames <= 0.0 {
        return;
    }
    x.frames = 0.0;
    tick_buffs(w, f, true);
}

/// `HELD_GROUP`: StateGroups byte 11 (NewEnumerator12 is 11), the Held group.
pub const HELD_GROUP: &str = "StateGroups::NewEnumerator12";

/// The StateInfo.Group of a buff's class.
fn group_of(w: &World, f: FighterId, cls: Name) -> Option<&str> {
    buff_def(w, ActorRef::Fighter(f), cls)
        .and_then(|d| d.get("StateInfo"))
        .and_then(Value::as_obj)
        .and_then(|s| s.get("Group"))
        .and_then(Value::as_str)
}

/// `removeBuff`: Comp_State_Manager.RemoveBuff(BuffToRemove, BuffGroupToRemove): every valid
/// Buff_Parent actor of the class or a child of it ([`is_buff_of_class`] @846) or (a group other than 0) of the group
/// gets K2_DestroyActor (@986), its EndPlay inside the call. True: one was destroyed.
pub fn remove_buff(w: &mut World, f: FighterId, cls: Option<Name>, group: Option<Name>) -> bool {
    if cls.is_none() && group.is_none() {
        return false;
    }
    let mut any = false;
    let list = refs(w, f);
    for &r in &list {
        let Some(b) = buff(w, r) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        let by_group = group.is_some_and(|g| group_of(w, f, b.cls) == Some(w.names.str(g)));
        let of_class = cls.is_some_and(|c| c == b.cls || is_buff_of_class(w, w.names.str(b.cls), w.names.str(c)));
        if of_class || by_group {
            end_buff(w, f, r);
            any = true;
        }
    }
    put_refs(w, list);
    any
}

/// `breakBuffStates`: Comp_State_Manager.BreakBuffStates(StateGroups, Buffs) @0-1124: every
/// valid Buff_Parent actor whose exact class is in Buffs or whose StateInfo.Group is in StateGroups gets
/// K2_DestroyActor (@978) inside the loop. True: one was destroyed.
pub fn break_buff_states(w: &mut World, f: FighterId, states: &[&str], buffs: &[Name]) -> bool {
    if states.is_empty() && buffs.is_empty() {
        return false;
    }
    let mut any = false;
    let list = refs(w, f);
    for &r in &list {
        let Some(b) = buff(w, r) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        let g = group_of(w, f, b.cls);
        if buffs.contains(&b.cls) || g.is_some_and(|g| states.contains(&g)) {
            end_buff(w, f, r);
            any = true;
        }
    }
    put_refs(w, list);
    any
}

/// `passiveFrom`: a live registered buff on this fighter whose Instigator is `o`.
pub fn passive_from(w: &World, f: FighterId, o: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.frames > 0.0 && b.src == Some(o) && kind_of(w, b.class).is_some())
}

/// `turnOffHitBoxes`: Comp_State_Manager.TurnOffHitboxCheck (@2166-2196): the first buff whose
/// DealtDamageMods.TurnOffHitBoxes is true (the live values).
pub fn turn_off_hit_boxes(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| {
        b.frames > 0.0 && {
            let d = def_view(w, ActorRef::Fighter(f), b.cls, b.own.as_ref());
            truthy(d.obj("DealtDamageMods").and_then(|m| m.get("TurnOffHitBoxes")))
        }
    })
}

/// `buffOf`: the live buff of a class on this fighter.
pub fn buff_of(w: &World, f: FighterId, cls: Name) -> Option<BuffRef> {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|b| b.cls == cls && b.frames > 0.0)
        .map(|b| BuffRef { fighter: f, serial: b.serial })
}

/// `hitByAllyBound`: one of this fighter's live buffs has bound its
/// Comp_Pawn_HitBoxHandler.HitByAlly (`ally_hit`) or its Comp_Pawn_DamageManager.HitByAlly (BuffKind.damageHitByAlly).
pub fn hit_by_ally_bound(w: &World, f: FighterId) -> bool {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .any(|b| b.frames > 0.0 && (b.ally_hit || kind_of(w, b.class).is_some_and(|e| e.damage_hit_by_ally)))
}

/// `lightHitRefused`: DamageAndLaunch @718 -> @8047 for a buff actor's own light hit on an
/// owner whose DamageDisabled makes CanBeHitBy refuse it: the DamageManager's HitByAlly, then nothing.
/// Deviation: CalculateArmor @7560, which runs ahead of the team test for a hit without Buff_Elemental_NoArmorDamage, is
/// not made for it.
fn light_hit_refused(w: &mut World, f: FighterId, hit: HitPacket) {
    if hit_by_ally_bound(w, f) {
        let arg = BuffSignalArg { hit: Some(Box::new(hit)), ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::DamageHitByAlly, arg);
    }
}

/// `buffSignal`: a delegate of this fighter that a registered buff class has bound
/// (BuffKind.on), to the buffs there when the signal is sent. BuffParentActor's handler of the owner's DamageManager
/// KnockedBack @5633: DestroyOnOwnerInterrupt -> K2_DestroyActor, else the class's own.
pub fn buff_signal(w: &mut World, f: FighterId, sig: BuffSignal, arg: BuffSignalArg) {
    let mut any = false;
    let list = refs(w, f);
    for &r in &list {
        let Some(b) = buff(w, r) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        let (cls, class) = (b.cls, b.class);
        if sig == BuffSignal::KnockedBack
            && truthy(buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| d.get("DestroyOnOwnerInterrupt")))
        {
            buff_mut(w, r).expect("found above").frames = 0.0;
            any = true;
            continue;
        }
        let Some(on) = kind_of(w, class).and_then(|e| e.hooks.on) else { continue };
        if !on(w, f, r, sig, &arg) {
            continue;
        }
        if let Some(x) = buff_mut(w, r) {
            x.frames = 0.0;
        }
        any = true;
    }
    put_refs(w, list);
    if any {
        tick_buffs(w, f, true);
    }
}

/// `buffTakeAnyDamage`: the owner's OnTakeAnyDamage to the buff actors whose class binds it.
/// Unverified: the buffs' place among the delegate's other bound functions is taken as first after
/// Comp_Actor_Damage's.
pub fn buff_take_any_damage(w: &mut World, f: FighterId, damage: Real, by: Option<FighterId>) {
    let mut any = false;
    let list = refs(w, f);
    for &r in &list {
        let Some(b) = buff(w, r) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        let Some(h) = kind_of(w, b.class).and_then(|e| e.hooks.take_any_damage) else { continue };
        if !h(w, f, r, damage, by) {
            continue;
        }
        if let Some(x) = buff_mut(w, r) {
            x.frames = 0.0;
        }
        any = true;
    }
    put_refs(w, list);
    if any {
        tick_buffs(w, f, true);
    }
}

/// `applyHealing`: ApplyDamage(owner, -FMin(CurrentDamage, HealAmount)); Comp_Actor_Damage
/// OnTakeAnyDamage passes the negative amount on to RecordDamage's BPI_UpdateFFATeamDamage (@549).
pub fn apply_healing(w: &mut World, f: FighterId, amount: Real) {
    let h = min(w.fighter(f).stats.damage, amount);
    let x = w.fighter_mut(f);
    x.stats.damage -= h; // CurrentDamage is a float
    ffa_damage(w, f, -h);
    if h != 0.0 {
        signal_bound(w, f, FighterSignal::TakeAnyDamage, false, -h, Name::NONE, None);
    }
}

/// `fireBegin`: Buff_Fire ReceiveBeginPlay (ubergraph 941): two looping timers of
/// DamageTickTime, InitialVisuals @1147, the parent's loop sound (StartLoopSoundMulticast @1593).
/// Unverified: the looping re-arm is ExpireTime += Rate (4.26 source, not read in the exe).
pub fn fire_begin(w: &mut World, f: FighterId, b: BuffRef, instigator: Option<FighterId>) {
    let tick = crate::real(w.fighter(f).clock.tick);
    let d = inst_view(w, b);
    let dmg = pnum(d.obj("HitDamageOnInterval"), "Damage").unwrap_or(0.0);
    let every = timer_frames(d.num("DamageTickTime"));
    let start = data_str(w, d, "SFX_Loop_Start");
    let Some(x) = buff_mut(w, b) else { return };
    x.born = Some(tick);
    x.src = instigator; // SpawnBuffActor @842: the actor's Instigator (RecordDamage credits it)
    x.fire_dmg = Some(dmg);
    x.fire = Some(every);
    data_sound(w, f, start, false);
    fire_visuals(w, f, b);
}

/// `fireVisuals`: InitialVisuals @1800 (also after every AddStack, @2844): SFX_Ignite.
/// Deviation: PS_HitImpact_TEMPLATE at the owner and the PS_Debuff_Fire_FOE loop on the mesh are not drawn.
fn fire_visuals(w: &mut World, f: FighterId, b: BuffRef) {
    let ev = data_str(w, inst_view(w, b), "SFX_Ignite");
    data_sound(w, f, ev, false);
}

/// The respawn's end of a buff (`fireEnd`): BuffParentActor's StopLoopSound for a live
/// buff ([`parent_loop_end`]), then the Fire path's own ([`fire_loop_end`]).
pub fn fire_end(w: &mut World, f: FighterId, b: BuffRef) {
    if let Some(x) = buff(w, b)
        && x.frames > 0.0
    {
        let parent = !kind_of(w, x.class).is_some_and(|k| k.end_skips_parent);
        parent_loop_end(w, f, b, parent);
    }
    fire_loop_end(w, f, b);
}

/// `fireEnd`: BuffParentActor ReceiveEndPlay -> StopLoopSound: SFX_Loop_Stop on the buff
/// actor @4957, SFX_Loop_End at the owner's location @5132.
fn fire_loop_end(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(x) = buff(w, b) else { return };
    if x.fire.is_none() {
        return;
    }
    let d = inst_view(w, b);
    let (stop, end) = (data_str(w, d, "SFX_Loop_Stop"), data_str(w, d, "SFX_Loop_End"));
    data_sound(w, f, stop, false);
    data_sound(w, f, end, true);
}

// ---- BuffParentActor's loop sound (posted per class only: the classes below)

/// A class whose own code posts BuffParentActor's loop sound, each in its own form (its own key and kind of event):
/// the Fire path (`fire_begin` / `fire_loop_end`), Finn's shop buffs, Buff_HeldByPlasmaBall, C021's chain victim,
/// Buff_Damage_Electric and its child Buff_Low_Damage_Electric, the gauntlets and Garnet's bubbles, the polymorphs.
/// The shared post leaves it alone.
fn own_loop_sound(w: &World, class: BuffClass, cls: Name, fire: bool) -> bool {
    use BuffClass as C;
    fire || matches!(
        class,
        C::FinnShopBootsBuff
            | C::FinnShopBootsBuffAlly
            | C::FinnShopArmorBunsBuff
            | C::BuffHeldByPlasmaBall
            | C::C021BuffElectricChainVictim
            | C::BuffDamageElectric
            | C::BuffLowDamageElectric
            | C::BuffSupermanAllyIceGauntlet
            | C::BuffAllyElectricGauntlets
            | C::BuffFireGauntlets
            | C::BuffGarnetElecInsideBubble
            | C::BuffGarnetElecOutsideBubble
            | C::BuffGarnetFullPowerGauntlets
    ) || w.registry.polymorph_children.get(w.names.str(cls)).is_some()
}

/// The key of what a buff actor posts on itself: its class and its owner (a fighter holds one live buff of a class).
fn parent_loop_key(w: &World, f: FighterId, cls: Name) -> SoundKey {
    SoundKey { tag: cls, n: w.fighter(f).id }
}

/// BuffParentActor ReceiveBeginPlay @1593 StartLoopSoundMulticast -> StartLoopSound @5219: SFX_Loop_Start, when
/// valid, on the buff actor (PF_Post_Event(SFX_Loop_Start, self, self) @5258), on the buff's key. `fire`: the class
/// takes the Fire path.
fn parent_loop_begin(w: &mut World, f: FighterId, b: BuffRef, fire: bool) {
    let Some(x) = buff(w, b) else { return };
    let (class, cls) = (x.class, x.cls);
    if own_loop_sound(w, class, cls, fire) {
        return;
    }
    let Some(ev) = data_str(w, inst_view(w, b), "SFX_Loop_Start") else { return };
    let (fighter, key) = (w.fighter(f).id, parent_loop_key(w, f, cls));
    w.fighter_mut(f).events.push(Event::SoundStart { fighter, ev, key });
}

/// BuffParentActor ReceiveEndPlay @4282 -> StopLoopSound @4301 (@4913): SFX_Loop_Stop, when valid, on the buff actor
/// (@4957), SFX_Loop_End, when valid, at the owner's location (PF_Post_Event_At_Location @5132); `parent` false for a
/// class whose EndPlay skips the parent's. Then the actor's destroy: PF_Post_Event set StopWhenOwnerDestroyed, so
/// what the actor posted on itself stops (StopAll), SFX_Loop_Stop's own play part with the loop (`cut`).
// Unverified: the destroy's stop is put here, ahead of the class's own EndPlay (it stops the buff's key alone), and
// taken as no fade.
fn parent_loop_end(w: &mut World, f: FighterId, b: BuffRef, parent: bool) {
    let Some(x) = buff(w, b) else { return };
    let (class, cls, fire) = (x.class, x.cls, x.fire.is_some());
    if own_loop_sound(w, class, cls, fire) {
        return;
    }
    let d = inst_view(w, b);
    let (start, stop, end) =
        (data_str(w, d, "SFX_Loop_Start"), data_str(w, d, "SFX_Loop_Stop"), data_str(w, d, "SFX_Loop_End"));
    let (fighter, key) = (w.fighter(f).id, parent_loop_key(w, f, cls));
    let mut on_self = start.is_some();
    if parent {
        if let Some(ev) = stop {
            let e = Event::SoundStop { fighter, key, fade: None, ev: Some(ev), curve: None, cut: true };
            w.fighter_mut(f).events.push(e);
            on_self = true;
        }
        data_sound(w, f, end, true);
    }
    if on_self {
        let e = Event::SoundStop { fighter, key, fade: Some(0.0), ev: None, curve: None, cut: false };
        w.fighter_mut(f).events.push(e);
    }
}

/// A data enum value as the packet holds it: a number, or an enumerator's name.
fn data_enum(w: &World, v: &Value) -> Option<crate::combat::DataEnum> {
    match v {
        #[allow(clippy::cast_possible_truncation, reason = "an enum's byte value")]
        Value::Num(n) => Some(crate::combat::DataEnum::Value(*n as i32)),
        Value::Str(s) => w.names.get(s).map(crate::combat::DataEnum::Name),
        _ => None,
    }
}

/// The fields of HitDamageOnInterval a pulse reads.
struct Pulse {
    attack_id: Real,
    decay_type: Real,
    base_kb: Real,
    additive: Option<crate::combat::DataEnum>,
    scaling: Option<Real>,
    hit_stun: Option<Real>,
}

/// `firePulse`: DamageTickServer @2451: the buff actor's own HitBoxHandler.pfgDoDamage(owner,
/// HitDamageOnInterval, [Buff_Elemental_NoArmorDamage]) @2527, then Damage x PercentDecayPerTick @2631. The light path
/// of DamageAndLaunch with no armor, no hitstun, hitpause or knockback; HitByEnemy @3723 with the buff actor as the
/// DamageDealer. Then pfgDoDamage's NotifyAllyOfHit ([`buff_ally_hit`]), on either path of DamageAndLaunch.
/// An Instigator that is no fighter (a level actor's fire, `apply_level_hit`): DamageAndLaunch's LocalDmgDlrOwner is
/// the DamageDealer itself (@8521-8641), the buff actor, which becomes PreviousAttacker.
/// Unverified: HitStunMult as the broadcast passes it is the packet's.
fn fire_pulse(w: &mut World, f: FighterId, b: BuffRef, d: &TickDef) {
    let Some(x) = buff_mut(w, b) else { return };
    let base = x.fire_dmg.unwrap_or(0.0);
    x.fire_dmg = Some(base * d.decay);
    let src = x.src;
    data_sound(w, f, d.sfx_damage, false); // DamageTickLocal @2727
    if w.fighter(f).invuln.invuln_count > 0 {
        return;
    }
    let h = &d.pulse;
    // pfgDoDamage's LocalHitInformation: HitDamageOnInterval with the pulse's Damage.
    let hit =
        HitPacket { damage: Some(base), base_knock_back: Some(h.base_kb), hit_stun_mult: h.hit_stun, ..d.packet() };
    if w.fighter(f).invuln.damage_disabled {
        light_hit_refused(w, f, hit.clone());
        buff_ally_hit(w, f, src, hit);
        return;
    }
    #[allow(clippy::cast_possible_truncation, reason = "the data's integers")]
    let dh = DecayHit {
        attack_id: h.attack_id as i32,
        decay_type: h.decay_type as i32,
        decay_name: name!("None"),
        combo_name: name!("None"),
        base_knock_back: h.base_kb,
        launch_knock_back: None,
        additive_kb_mode: h.additive,
        knock_back_scaling: h.scaling,
        hit_stun_mult: h.hit_stun,
    };
    // A fire lit by a level actor (the Elements tutorial's fire walls, no Instigator): the buff actor becomes
    // PreviousAttacker, as the decode above has it.
    note_attacker(w, f, src.map_or(ActorRef::Buff(b), ActorRef::Fighter), &dh);
    // Attacker: the buff actor, no hit box. Deviation: the build passes the buff actor as the attacker object; the
    // Rust attacker refs name no buff actor, so the conditions are given none.
    let hp = d.packet();
    let mut dmg = apply_taken_damage_mods(w, f, base, 0.0, 0.0, src, false, None, Some(&hp))[0];
    light_hit_tail(w, f, &mut dmg, src, h.base_kb, h.hit_stun.unwrap_or(0.0));
    buff_ally_hit(w, f, src, hit);
    // pfgDoDamage @22407-22653: BFHitPlayer on the handler owner's owner, the buff's wearer (SpawnBuffActor @5, @710)
    crate::world::catch_up::hit_player(w, f);
}

/// pfgDoDamage's NotifyAllyOfHit (@8400 -> @21344; Comp_Pawn_HitBoxHandler NotifyAllyOfHit @0-937) for a buff actor's
/// own handler on its owner: behind IsValid(Instigator), Victim != Instigator and the Instigator's TeamIndex the
/// owner's, the owner's handler broadcasts HitByAlly(LocalHitInformation, the dealer handler's owner) @584; the
/// dealer handler's owner is the buff actor, no fighter. An invulnerable owner gets none (the dodge branch @4682 ->
/// @7381 pops ahead of it), as the callers return before.
/// ElementalBuffHitAlly @472 with the call's ElementalBuffs does nothing: Buff_Elemental_NoArmorDamage's
/// ElementalHitAlly calls its parent's, whose entry @3531 is a pop_flow.
/// Not ported: TrackActorsHitByTag and MostRecentHitActor of the buff actor's handler, which the port keeps no state
/// for.
/// Unverified: the buff actor's handler's HitAlly @886 is taken as bound by nothing.
fn buff_ally_hit(w: &mut World, f: FighterId, src: Option<FighterId>, hit: HitPacket) {
    let Some(s) = src else { return };
    if s == f || w.fighter(s).team.team != w.fighter(f).team.team {
        return;
    }
    if hit_by_ally_bound(w, f) {
        let arg = BuffSignalArg { hit: Some(Box::new(hit)), ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::HitByAlly, arg);
    }
}

/// The light hit's tail, from the combo decay to HitByEnemy, as the fire pulse and the buff actor's hit share it.
fn light_hit_tail(w: &mut World, f: FighterId, dmg: &mut Real, src: Option<FighterId>, base_kb: Real, hit_stun: Real) {
    if w.fighter(f).decay.combo_resistance {
        *dmg *= DAMAGE_DECAY_MOD;
    }
    *dmg = min(max(*dmg, 0.0), 10000.0);
    let dmg = *dmg;
    let taken = absorb_temp_health(w, f, dmg);
    record_damage(w, f, taken, src);
    ffa_damage(w, f, taken);
    receive_light_hit(w, f, taken, 0.0, 0.0); // a pause of 0 leaves a running one alone (takeHitPause)
    if let Some(m) = w.fighter(f).attackers.most_recent_attacker {
        let (mx, my) = (w.fighter(m).body.x, actor_y(w, m));
        let off = !ground_trace(w, m, mx, my, false, OFF_LEDGE_TRACE_LENGTH);
        w.fighter_mut(f).attackers.last_attacker_off_ledge = off;
    }
    if dmg > 0.0 {
        let id = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::PulseHit { fighter: id, pain_damage: dmg }); // @3360
    }
    let hit = HitPacket {
        damage: Some(dmg),
        base_knock_back: Some(base_kb),
        hit_stun_mult: Some(hit_stun),
        ..HitPacket::default()
    };
    hit_by_enemy(w, f, None, dmg, Some(&hit), None, None);
}

/// `hitByEnemy`: Comp_Pawn_DamageManager HitByEnemy (DamageAndLaunch @3723) for the buffs
/// bound to it, the team's pawns that bound this fighter's delegate, and the run-time binders.
/// Unverified: the order of the delegate's bound functions is the binding order in the build and is not read.
/// Unverified: the packet's KnockBackDirectionVector at the broadcast is taken as the direction the port's knockback
/// computed.
pub fn hit_by_enemy(
    w: &mut World,
    f: FighterId,
    dealer: Option<FighterId>,
    damage: Real,
    hit: Option<&HitPacket>,
    by: Option<ActorRef>,
    dir: Option<V2>,
) {
    if !w.fighter(f).buffs.buffs.is_empty() {
        let by_ = by.map(|a| actor_by(w, a, dealer));
        let arg = BuffSignalArg { dealer, hit: hit.map(|h| Box::new(h.clone())), by: by_, ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::HitByEnemy, arg);
    }
    let team = w.fighter(f).team.team;
    for i in 0..peers(w).len() {
        let o = peers(w)[i];
        if o != f
            && w.fighter(o).team.team == team
            && let Some(h) = w.fighter(o).hooks.char.ally_hit_by_enemy
        {
            h(w, o, f, damage);
        }
    }
    // the run-time binders: KnockBackDirectionVector in UE axes (X = -x, Y = y), 0 for none
    if !w.fighter(f).delegates.hit_by_enemy.is_empty() {
        let (dir_x, dir_y) = dir.map_or((0.0, 0.0), |d| (-d.x, d.y));
        let call = DelegateCall::HitByEnemy { hit, dir_x, dir_y, dealer, by };
        broadcast_bound(w, DelegateList::Pawn(f, PawnList::HitByEnemy), &call);
    }
}

/// `buffActorLightHit`: a buff actor's own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, this
/// fighter, a hit of Damage alone) (Buff_Thorns ProcBuff @1219), as the fire pulse with CalculateArmor (DamageAndLaunch
/// @7560). `pierce`: the packet's ArmorPierce; `no_armor`: Buff_Elemental_NoArmorDamage among its ElementalBuffs.
/// Then NotifyAllyOfHit ([`buff_ally_hit`]) on either path.
/// Deviation: as the fire pulse, the combo decay of such a hit is combo resistance alone.
/// `buff`: the buff actor whose handler deals the hit. With no fighter Instigator, LocalDmgDlrOwner is the buff actor
/// (@8521-8641), which becomes PreviousAttacker (@835-890, @8665-8728).
#[allow(clippy::too_many_arguments, reason = "the call's inputs, and the buff actor")]
pub fn buff_actor_light_hit(
    w: &mut World,
    wearer: FighterId,
    buff: BuffRef,
    f: FighterId,
    src: Option<FighterId>,
    damage: Real,
    pierce: bool,
    no_armor: bool,
) {
    if w.fighter(f).invuln.invuln_count > 0 {
        return;
    }
    let hit = HitPacket {
        damage: Some(damage),
        base_knock_back: Some(0.0),
        hit_stun_mult: Some(0.0),
        armor_pierce: Some(pierce),
        ..HitPacket::default()
    };
    if w.fighter(f).invuln.damage_disabled {
        light_hit_refused(w, f, hit.clone());
        buff_ally_hit(w, f, src, hit);
        return;
    }
    let dh = DecayHit {
        attack_id: 0,
        decay_type: 0,
        decay_name: name!("None"),
        combo_name: name!("None"),
        base_knock_back: 0.0,
        launch_knock_back: None,
        additive_kb_mode: None,
        knock_back_scaling: Some(0.0),
        hit_stun_mult: Some(0.0),
    };
    note_attacker(w, f, src.map_or(ActorRef::Buff(buff), ActorRef::Fighter), &dh);
    let ar = if no_armor { damage } else { calculate_armor(w, f, damage, 0.0, 0.0, 1.0, pierce, None, None).damage };
    let mut dmg = apply_taken_damage_mods(w, f, ar, 0.0, 0.0, src, false, None, None)[0];
    light_hit_tail(w, f, &mut dmg, src, 0.0, 0.0);
    buff_ally_hit(w, f, src, hit);
    // pfgDoDamage @22407-22653: BFHitPlayer on the handler owner's owner, the buff's wearer (SpawnBuffActor @5, @710)
    crate::world::catch_up::hit_player(w, wearer);
}

/// `defeatBuffs`: BaseFighter's defeat tail (ubergraph 18380-19022): K2_DestroyActor on every
/// buff child with DestroyBuffWhenOwnerDefeated, one at a time (each one's EndPlay inside the loop).
/// Unverified: the loop's order is the build's array of child actors; taken as the port's list order.
pub fn defeat_buffs(w: &mut World, f: FighterId) {
    let list = refs(w, f);
    for &r in &list {
        let Some(b) = buff(w, r) else { continue };
        let on = match buff_def(w, ActorRef::Fighter(f), b.cls).and_then(|d| d.get("DestroyBuffWhenOwnerDefeated")) {
            None | Some(Value::Null) => true,
            v => truthy(v),
        };
        if b.frames <= 0.0 || !on {
            continue;
        }
        buff_mut(w, r).expect("found above").frames = 0.0;
        tick_buffs(w, f, true);
    }
    put_refs(w, list);
}

/// What `tickBuffs` reads of a buff's values at the start of its turn (`const d = instDef(f, b) ?? {}`).
struct TickDef {
    heal_on: Real,
    heal_every: Real,
    damage_tick: Option<Real>,
    decay: Real,
    sfx_damage: Option<Name>,
    pulse: Pulse,
    stack_rate: Option<Real>,
    stun: Real,
    armor: Option<Name>,
}

impl TickDef {
    fn of(w: &World, d: Def) -> Self {
        let h = d.obj("HitDamageOnInterval");
        TickDef {
            heal_on: d.num("HealingOnInterval").unwrap_or(0.0),
            heal_every: d.num("HealingTickTime").unwrap_or(0.0),
            damage_tick: d.num("DamageTickTime"),
            decay: d.num("PercentDecayPerTick").unwrap_or(1.0),
            sfx_damage: data_str(w, d, "SFX_Damage"),
            pulse: Pulse {
                attack_id: pnum(h, "AttackID").unwrap_or(0.0),
                decay_type: pnum(h, "AttackDecayType").unwrap_or(0.0),
                base_kb: pnum(h, "BaseKnockBack").unwrap_or(0.0),
                additive: h.and_then(|h| h.get("AdditiveKBMode")).and_then(|v| data_enum(w, v)),
                scaling: pnum(h, "KnockBackScaling"),
                hit_stun: pnum(h, "HitStunMult"),
            },
            stack_rate: d.num("StackDecayRate"),
            stun: stun_duration(d),
            armor: buff_armor(d).map(|a| armor_type(w, a)),
        }
    }

    /// The fields of HitDamageOnInterval a taken-mods condition may read.
    fn packet(&self) -> HitPacket {
        HitPacket {
            base_knock_back: Some(self.pulse.base_kb),
            knock_back_scaling: self.pulse.scaling,
            hit_stun_mult: self.pulse.hit_stun,
            ..HitPacket::default()
        }
    }
}

/// `tickBuffs`: the buffs' frame (their timers, the classes' latent actions, the heal and
/// fire loops, the stack timer, the life timer), then the EndPlay of each that ended. `sweep`: only the ends of the
/// buffs already ended (frames 0), without counting a frame.
#[allow(clippy::too_many_lines, reason = "one pass over the buffs, in their own order")]
pub fn tick_buffs(w: &mut World, f: FighterId, sweep: bool) {
    if w.fighter(f).buffs.buffs.is_empty() {
        return;
    }
    let own = ActorRef::Fighter(f);
    let mut recompute = false;
    let mut walk = Walk::new(w, f);
    while let Some(r) = walk.next(w, f) {
        // A walked entry no longer in the list was swept (its EndPlay ran in an outer pass).
        let Some(b) = buff(w, r) else { continue };
        if b.swept {
            continue;
        }
        let d = TickDef::of(w, def_view(w, own, b.cls, b.own.as_ref()));
        let (cls, class) = (b.cls, b.class);
        let mut end = false;
        if sweep {
            if b.frames > 0.0 {
                continue;
            }
            end = true;
        } else {
            {
                let x = buff_mut(w, r).expect("found above");
                if x.frames > 0.0 {
                    if let Some(g) = x.gate {
                        let g = g - 1.0;
                        x.gate = if g <= 0.0 { None } else { Some(g) }; // TimedMinStackGate: the gate opens
                    }
                    if let Some(a) = x.add_age {
                        x.add_age = Some(a + 1.0);
                    }
                }
                if let Some(s) = x.rescue {
                    x.rescue = Some(s - 1.0);
                    if s - 1.0 <= 0.0 {
                        end = true;
                    }
                }
            }
            let live = |w: &World| buff(w, r).is_some_and(|x| x.frames > 0.0);
            if live(w) && w.fighter(f).holds.hold.as_ref().is_some_and(|h| h.cls == class) && hold_tick(w, f) {
                end = true; // the buff actor's tick
            }
            if live(w)
                && let Some(t) = kind_of(w, class).and_then(|e| e.hooks.tick)
                && t(w, f, r)
            {
                end = true; // a registered class's latent actions
            }
            // The buff actor's latent Delay runs in its tick, ahead of the frame's timers.
            if let Some(x) = buff_mut(w, r)
                && let Some(h) = x.heal
            {
                x.heal = Some(h - 1.0);
                if h - 1.0 <= 0.0 {
                    let amt = x.heal_amt.unwrap_or(d.heal_on);
                    apply_healing(w, f, amt);
                    if let Some(x) = buff_mut(w, r) {
                        x.heal = Some(latent_frames(d.heal_every));
                    }
                }
            }
            // Buff_Fire's looping DamageTickServer timer, ahead of the stack timer.
            // Unverified: popped by expire time, as the 4.26 timer heap; a tie (one stack: both 1.0) is not settled.
            if let Some(x) = buff_mut(w, r)
                && let Some(fi) = x.fire
            {
                x.fire = Some(fi - 1.0);
                if fi - 1.0 <= 0.0 {
                    fire_pulse(w, f, r, &d);
                    if let Some(x) = buff_mut(w, r) {
                        x.fire = Some(timer_ticks(d.damage_tick));
                    }
                }
            }
            // The rest of the turn writes the walked entry while it is in the list.
            // An entry a handler above swept from the list (its EndPlay run) is passed over here: the build runs one
            // EndPlay per actor (status update, stun pick, recompute, the class's end).
            if let Some(x) = buff_mut(w, r) {
                x.stack_frames -= 1.0;
                if x.stack_frames <= 0.0 {
                    x.stacks -= 1.0;
                    let s = x.stacks;
                    buff_status_update(w, f, class, s, false, false); // RemoveStack: StatusUpdateMulticast(stacks)
                    if s <= 0.0 {
                        end = true;
                    } else {
                        update_states(w, f); // RemoveStack @938-1009, ahead of the timer
                        if let Some(x) = buff_mut(w, r) {
                            x.stack_frames = timer_ticks(d.stack_rate); // set inside the timer tick: active at once
                        }
                        // TimedRemoveStack @5570 calls the class's RemoveStack.
                        if let Some(h) = kind_of(w, class).and_then(|e| e.hooks.stacks)
                            && h(w, f, r, false)
                        {
                            end = true;
                        }
                    }
                }
            }
            if let Some(x) = buff_mut(w, r) {
                x.frames -= 1.0;
                if x.frames <= 0.0 {
                    end = true;
                }
            }
        }
        if !end {
            continue;
        }
        let Some(x) = buff_mut(w, r) else { continue };
        x.frames = 0.0;
        x.swept = true;
        // A class whose EndPlay skips the parent's (BuffKind.endSkipsParent, Buff_Educated): its own part alone.
        let kind = kind_of(w, class);
        let parent = !kind.is_some_and(|k| k.end_skips_parent);
        parent_loop_end(w, f, r, parent); // EndPlay @4301: StopLoopSound
        if parent {
            buff_status_update(w, f, class, 0.0, false, true); // EndPlay: StatusUpdateMulticast(0, removed)
            recompute = true;
        }
        let picks = w.fighter(f).buffs.stun_picks;
        rescue_end(w, f, cls);
        if w.fighter(f).holds.hold.as_ref().is_some_and(|h| h.cls == class) {
            hold_end(w, f, true);
        }
        if let Some(e) = kind.and_then(|k| k.hooks.end) {
            e(w, f, r);
        }
        // BuffParentActor ReceiveEndPlay @4611 UpdateCurrentStates -> UpdatePlayerStatus(null) -> ApplyStunBuffs (@345).
        if parent && picks == w.fighter(f).buffs.stun_picks {
            apply_stun_buffs(w, f, None, d.stun > 0.0);
        }
        fire_loop_end(w, f, r);
        if let Some(t) = d.armor {
            remove_armor(w, f, t);
        }
        // Buff_Enlarge ReceiveEndPlay (ubergraph @322-1683), for an owner that IsMovingOnGround.
        if is_enlarge(cls) && w.fighter(f).body.on_floor {
            let id = w.fighter(f).id;
            w.fighter_mut(f).events.push(Event::EnlargeFx { fighter: id, begin: false });
        }
    }
    walk.done(w);
    filter_live(w, f);
    // The recompute for the buffs whose EndPlay is the parent's.
    if recompute {
        recalc_speed_mod(w, f, None); // EndPlay -> UpdateCurrentStates -> UpdatePlayerStatus(null)
        recalc_move_folds(w, f, None);
        recalc_ability_mods(w, f, None);
        recalc_misc_movement(w, f, None);
        recalc_charge_mod(w, f, None);
        recalc_size_mod(w, f, None);
        recalc_head_size(w, f, None);
    }
}

/// A UI state of the buff classes (`AddStateForPlayer`'s params with the parent's constants).
#[allow(clippy::too_many_arguments, reason = "the struct's fields")]
fn ui_add(
    id: Name,
    state_type: i32,
    display_type: i32,
    current: Real,
    maxv: Real,
    icon: Name,
    active: bool,
    pop: Name,
) -> UiStateOp {
    UiStateOp::Add(UiStateParams {
        id,
        state_type,
        display_type,
        current_value: current,
        max_value: maxv,
        timer: -1.0,
        icon_material: icon,
        active,
        pop_text: pop,
        auto_remove_time: -1.0,
        subicon: None,
    })
}

/// `buffStatusUpdate`: BuffParentActor StatusUpdateMulticast -> BPI After Status Update
/// Multicast(NewStackCount, BuffApplied, BuffRemoved, BuffRefreshed), the children's UI broker (the class's own hook,
/// Buff_Weakened's among them); Buff_SlowDown's here.
pub fn buff_status_update(w: &mut World, f: FighterId, class: BuffClass, stacks: Real, applied: bool, removed: bool) {
    if let Some(s) = kind_of(w, class).and_then(|e| e.hooks.status) {
        s(w, f, class, stacks, applied, removed);
    }
    let cls = class.registered_name();
    if cls.is_none() && matches!(class, BuffClass::Data(n) if n == name!("Buff_SlowDown_C")) {
        let ops = &mut w.fighter_mut(f).controls.ui_state_ops;
        if removed {
            ops.push(UiStateOp::Remove(name!("SlowDown")));
        } else if applied {
            ops.push(UiStateOp::Remove(name!("SlowDown")));
            ops.push(ui_add(name!("SlowDown"), 1, 0, -1.0, -1.0, name!("MI_State_Slow"), true, Name::NONE));
        }
    }
}

// ---- the damage mods

/// The float mods GetDamageMod scales per stack, in `MOD_FIELDS`' order.
const MOD_FIELDS: [&str; 7] = [
    "HorizontalKnockBackMod",
    "UpwardKnockBackMod",
    "DownwardKnockBackMod",
    "DamageMod",
    "KnockBackMod",
    "HitStunMod",
    "HitPauseMod",
];
const MOD_TYPES: [&str; 7] = [
    "HorizontalKnockBackModType",
    "UpwardKnockBackModType",
    "DownwardKnockBackModType",
    "DamageModType",
    "KnockBackModType",
    "HitStunModType",
    "HitPauseModType",
];
const H_KB: usize = 0;
const UP_KB: usize = 1;
const DOWN_KB: usize = 2;
const DAMAGE: usize = 3;
const KB: usize = 4;
const HIT_STUN: usize = 5;
const HIT_PAUSE: usize = 6;

/// A buff's damage mod struct as GetDamageMod returns it (`buffDamageMod`): the seven float mods (none: undefined)
/// and their types, the direction mod, the flags.
#[derive(Clone, Debug, Default)]
pub struct DamageMod {
    pub v: [Option<Real>; 7],
    pub t: [Real; 7],
    pub dir: (Option<Real>, Option<Real>),
    pub dir_t: Real,
    pub armor_break: bool,
    pub destroy_projectiles: bool,
    pub reflect_projectiles: bool,
    pub effect: Option<Name>,
}

/// `buffDamageMod`: BuffParentActor GetDamageMod: Dealt/TakenDamageMods, plus
/// GetStackModifier of the per-stack mods when CanStack (type 0: 0, 1: Value ^ stacks, 2/3: stacks x Value); the
/// KnockBDirMod of a stacking buff by its own rule (@1435-2281); a stacking buff's ReflectProjectiles is false
/// (@3780).
pub fn buff_damage_mod(w: &World, d: Def, taken: bool, n: Real) -> Option<DamageMod> {
    let base = d.obj(if taken { "TakenDamageMods" } else { "DealtDamageMods" })?;
    let b = Some(base);
    let dir = base.get("KnockBDirMod").and_then(Value::as_obj);
    let mut m = DamageMod {
        v: MOD_FIELDS.map(|k| pnum(b, k)),
        t: MOD_TYPES.map(|k| mod_type(base.get(k))),
        dir: (pnum(dir, "X"), pnum(dir, "Y")),
        dir_t: mod_type(base.get("KnockBDirModType")),
        armor_break: truthy(base.get("ArmorBreak")),
        destroy_projectiles: truthy(base.get("DestroyProjectiles")),
        reflect_projectiles: truthy(base.get("ReflectProjectiles")),
        effect: match base.get("HitEffectOverride") {
            Some(v) if truthy(Some(v)) => {
                // Unverified: the data holds the override as its object name; another kind of value is taken as none.
                v.as_str().and_then(|s| data_name(w, s))
            }
            _ => None,
        },
    };
    if !d.flag("CanStack") {
        return Some(m);
    }
    let per = d.obj(if taken { "TakenDamageModPerStack" } else { "DealtDamageModPerStack" });
    for (i, k) in MOD_FIELDS.iter().enumerate() {
        let v = pnum(per, k).unwrap_or(0.0);
        let t = mod_type(per.and_then(|p| p.get(MOD_TYPES[i])));
        let s = if t == 0.0 {
            0.0
        } else if t == 1.0 {
            ue::powf(v, n)
        } else {
            n * v
        };
        m.v[i] = Some(m.v[i].unwrap_or(0.0) + s);
    }
    let dt = mod_type(per.and_then(|p| p.get("KnockBDirModType")));
    if dt != 0.0 {
        let pd = per.and_then(|p| p.get("KnockBDirMod")).and_then(Value::as_obj);
        let px = pnum(pd, "X").unwrap_or(0.0) * n;
        let py = pnum(pd, "Y").unwrap_or(0.0) * n;
        let (sx, sy) = if dt == 1.0 { (ue::powf(px, n), ue::powf(py, n)) } else { (px, py) };
        m.dir = (Some(m.dir.0.unwrap_or(0.0) + sx), Some(m.dir.1.unwrap_or(0.0) + sy));
    }
    m.reflect_projectiles = false;
    Some(m)
}

/// The classes of the build with a TakenDamageModCondition of their own (`TAKEN_COND_OWN`), and the children that
/// inherit one with no graph of their own.
const TAKEN_COND_OWN: [&str; 24] = [
    "Buff_AttachToAlly_C",
    "Buff_C015AttachToDogpile_C",
    "Buff_VelmaAttachToAlly_C",
    "Buff_HeldByEnemy_C",
    "Buff_HeldByEnemyArya_C",
    "Buff_JakEat_C",
    "Buff_EatenByC015_C",
    "Buff_EatenByC015InAir_C",
    "Buff_HeldBy_C017_C",
    "Buff_Held_By_C021_C",
    "Buff_HeldByEnemyGarnet_C",
    "Buff_Light_C",
    "Buff_ProjectileBarrierDefense_C",
    "Buff_Weakened_C",
    "Buff_C018BubbleHandleIncomingDamage_C",
    "Buff_C021_ShockProjectile_C",
    "Buff_C021_SlowReflectProjectile_C",
    "Buff_HeldByChainsawActor_C",
    "Buff_GarnetGauntletsEquipped_C",
    "Buff_GarnetStopProjectile_C",
    "Buff_BackToBack_C",
    "Buff_DetonateFromMelee_C",
    "Buff_Garnet_Slow_C",
    "Buff_Perk_Tank_C",
];

/// `TAKEN_COND_SHARED`'s Buff_HeldByEnemy family (TakenDamageModCondition @0-129): Attacker.GetInstigator() != the
/// buff's Instigator, so the holder's own hits take none of the hold's mods.
const HELD_TAKEN: [&str; 8] = [
    "Buff_HeldByEnemy_C",
    "Buff_HeldByEnemyArya_C",
    "Buff_JakEat_C",
    "Buff_EatenByC015_C",
    "Buff_EatenByC015InAir_C",
    "Buff_HeldBy_C017_C",
    "Buff_Held_By_C021_C",
    "Buff_HeldByEnemyGarnet_C",
];

/// `takenCond`: the class's own condition (its entry's, or a shared one), else the parent's
/// true; a class of `TAKEN_COND_OWN` without one warns and takes no part.
fn taken_cond(
    w: &mut World,
    own: FighterId,
    (serial, class, cls): (u32, BuffClass, Name),
    attacker: Option<ActorRef>,
    boxed: bool,
    hit: &HitPacket,
    by: Option<FighterId>,
) -> bool {
    let r = BuffRef { fighter: own, serial };
    if let Some(c) = kind_of(w, class).and_then(|e| e.hooks.taken_cond) {
        return c(w, own, r, attacker, boxed, hit, by);
    }
    let cls = w.names.str(cls);
    if HELD_TAKEN.contains(&cls) {
        let h = w.fighter(own).holds.hold.as_ref();
        return !(by.is_some() && h.is_some_and(|h| Some(h.by) == by && h.cls == class));
    }
    if !TAKEN_COND_OWN.contains(&cls) {
        return true;
    }
    warn_once(w, name!("a buff's TakenDamageModCondition is not ported"));
    false
}

/// The fields of DamageAndLaunch's LocalHitInformation the taken pass changes: Damage, KnockBackScaling, HitStunMult,
/// HitPauseTime, KnockBackDirectionVector (in UE terms: X the horizontal axis Y of the world, Y up) and
/// OnHitEffectOverride.
#[derive(Clone, Copy, Debug)]
pub struct TakenHit {
    pub damage: Real,
    pub scaling: Real,
    pub hit_stun: Real,
    pub hit_pause: Real,
    pub dir: (Real, Real),
    pub effect: Option<Name>,
}

/// `applyTakenDamageMods` for a caller that reads Damage, KnockBackScaling and HitStunMult
/// alone ([`apply_taken_hit_mods`] with no hit pause, direction or effect of its own).
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn apply_taken_damage_mods(
    w: &mut World,
    own: FighterId,
    damage: Real,
    scaling: Real,
    hit_stun: Real,
    by: Option<FighterId>,
    boxed: bool,
    attacker: Option<ActorRef>,
    packet: Option<&HitPacket>,
) -> [Real; 3] {
    let x = TakenHit { damage, scaling, hit_stun, hit_pause: 0.0, dir: (0.0, 0.0), effect: None };
    let t = apply_taken_hit_mods(w, own, x, by, boxed, attacker, packet);
    [t.damage, t.scaling, t.hit_stun]
}

/// The four float fields of the taken pass, by `MOD_FIELDS` index, in [`TakenHit`]'s order.
const TAKEN: [usize; 4] = [DAMAGE, KB, HIT_STUN, HIT_PAUSE];

/// `applyTakenDamageMods`: Comp_State_Manager ApplyBuffDamageMods(DamageTaken) on the
/// victim (DamageAndLaunch @1878 and @8407): ApplyDamageBuffsOfType for BuffModType 1, 2 and 3 over its buff actors,
/// behind the class's TakenDamageModCondition (@901, the gate @1101-1244). Per field whose type is the pass's: Damage
/// @1375, KnockBackScaling @2646, KnockBackDirectionVector @3208, HitPauseTime @3770, HitStunMult @4332. Override (3)
/// writes the hit at once; Multiply (1) and Add (2) put (In x mod - In) or (mod) in AccumulatedMods, added after the
/// loop of passes 1 and 2 (@1641-2540; the direction per component). A valid HitEffectOverride of a taking-part buff
/// (@5082-5238, the last one wins) becomes OnHitEffectOverride after passes 1 and 2. Then ApplyElementalDamageMods
/// @318: every Buff_ElementalParent child's ApplyElementalDamageMod, which the parent and all seven overrides of the
/// build return as given: nothing.
/// ArmorBreak @4894, DestroyProjectiles @4988 and ReflectProjectiles @5276 of a taking-part buff are ORed into
/// LocalHitInformation too, but nothing reads them there: CalculateArmor @7560 has run, the projectile tests read the
/// dealer's own packet (pfgDoDamage @15158, @15491), and no HitByEnemy listener reads them.
/// Deviation: ArmorBreak, DestroyProjectiles and ReflectProjectiles of the taken pass are not carried into the hit.
/// One GetDamageMod per buff actor (@1254), its mods scaled by the actor's stack count (buffDamageMod).
/// GetDamageMod reads the actor's own TakenDamageMods: an instance whose graph has written the struct to zeroes
/// (every type 0, DoNothing: Buff_NullifyNextHit ProcBuff @3885) has `mods_off` and takes part in nothing here.
/// The condition's arguments (BuffHooks::taken_cond): `by` Attacker.GetInstigator() (the hold test's), `boxed`
/// IsValid(HitCollision), `attacker` the dealing actor, `packet` the hit (a stand-in of the three fields when the
/// caller has none). The condition is asked per pass ahead of the mod read (@901 before GetDamageMod @1254).
/// Deviation: the build adds the pass's accumulator to every field whatever took part; the port adds it only when a
/// buff of the pass changed a field (a zero sum turns only a -0 into +0).
/// Unverified: HitPauseMod's branch (@3770-4295) is ported from the bytecode alone, not checked against a capture.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn apply_taken_hit_mods(
    w: &mut World,
    own: FighterId,
    hit: TakenHit,
    by: Option<FighterId>,
    boxed: bool,
    attacker: Option<ActorRef>,
    packet: Option<&HitPacket>,
) -> TakenHit {
    let mut x = [hit.damage, hit.scaling, hit.hit_stun, hit.hit_pause];
    let mut dir = hit.dir;
    let mut effect = hit.effect;
    // `hp ??= { Damage, KnockBackScaling, HitStunMult }`: the call's own arguments, whenever it is made.
    let stand_in = HitPacket {
        damage: Some(hit.damage),
        knock_back_scaling: Some(hit.scaling),
        hit_stun_mult: Some(hit.hit_stun),
        ..HitPacket::default()
    };
    let hp = packet.unwrap_or(&stand_in);
    for t in [1.0, 2.0, 3.0] {
        let mut acc = [0.0; 4];
        let mut acc_dir = (0.0, 0.0);
        let mut changed = false;
        let mut a_effect: Option<Name> = None;
        // `for (const b of f.buffs)`: by index over the live list, as a class's condition may write the World.
        let mut i = 0;
        while i < w.fighter(own).buffs.buffs.len() {
            i += 1;
            let b = &w.fighter(own).buffs.buffs[i - 1];
            let cls = w.names.str(b.cls);
            if TAKEN_COND_OWN.contains(&cls) || kind_of(w, b.class).is_some_and(|e| e.hooks.taken_cond.is_some()) {
                let key = (b.serial, b.class, b.cls);
                if !taken_cond(w, own, key, attacker, boxed, hp, by) {
                    continue;
                }
            }
            let Some(b) = w.fighter(own).buffs.buffs.get(i - 1) else { continue };
            if b.mods_off == Some(true) {
                continue;
            }
            let d = def_view(w, ActorRef::Fighter(own), b.cls, b.own.as_ref());
            if !d.some() {
                continue;
            }
            let Some(m) = buff_damage_mod(w, d, true, b.stacks) else { continue };
            for (i, fi) in TAKEN.iter().enumerate() {
                if m.t[*fi] != t {
                    continue;
                }
                let v = m.v[*fi].unwrap_or(0.0);
                if t == 3.0 {
                    x[i] = v;
                } else {
                    changed = true;
                    let r = if t == 1.0 { x[i] * v } else { x[i] + v };
                    acc[i] += r - x[i];
                }
            }
            if m.dir_t == t {
                let (mx, my) = (m.dir.0.unwrap_or(0.0), m.dir.1.unwrap_or(0.0));
                if t == 3.0 {
                    dir = (mx, my);
                } else {
                    changed = true;
                    let rx = if t == 1.0 { dir.0 * mx } else { dir.0 + mx };
                    let ry = if t == 1.0 { dir.1 * my } else { dir.1 + my };
                    acc_dir.0 += rx - dir.0;
                    acc_dir.1 += ry - dir.1;
                }
            }
            if m.effect.is_some() {
                a_effect = m.effect;
            }
        }
        if t == 3.0 {
            break;
        }
        if changed {
            for i in 0..4 {
                x[i] += acc[i];
            }
            dir = (dir.0 + acc_dir.0, dir.1 + acc_dir.1);
        }
        if a_effect.is_some() {
            effect = a_effect;
        }
    }
    TakenHit { damage: x[0], scaling: x[1], hit_stun: x[2], hit_pause: x[3], dir, effect }
}

/// The classes of the build with a DealtDamageModCondition of their own (`DEALT_COND_OWN`), and the children that
/// inherit one with no graph of their own.
const DEALT_COND_OWN: [&str; 44] = [
    "Buff_AryaAllySetup_C",
    "Buff_AryaBackStab_C",
    "Buff_BackStab_C",
    "Buff_BatmanOnHitApplyBuff_C",
    "Buff_PassiveApplyBuffonHit_C",
    "Buff_C015Eating_C",
    "Buff_MeeseeksDamageWithHitstun_C",
    "Buff_C021_Shocked_C",
    "Buff_ClearHeadspace_C",
    "Buff_CullTheWeak_C",
    "Buff_DamageBoostOnDebuffHit_C",
    "Buff_Finn_GemOnCharge_C",
    "Buff_Garnet_Slow_C",
    "Buff_IceProjectile_C",
    "Buff_Inevitable_C",
    "Buff_InMemoriam_C",
    "Buff_LastStand_C",
    "Buff_PaintedTarget_C",
    "Buff_Perk_AirMeleeDamageBoost_Small_C",
    "Buff_Perk_AirMeleeKnockbackBoost_Small_C",
    "Buff_Perk_ChargeArmorBreak_C",
    "Buff_Perk_GroundMeleeDamageBoost_Small_C",
    "Buff_Perk_GroundMeleeKnockbackBoost_Small_C",
    "Buff_Perk_KBAngleDependent_C",
    "Buff_Perk_ProjectileDamageBoost_Small_C",
    "Buff_Perk_ProjectileKnockbackBoost_Small_C",
    "Buff_ProjectileApplyBuff_C",
    "Buff_ProjectileGrayHealth_C",
    "Buff_Sniper_C",
    "Buff_Superman_IceBreaker_C",
    "Buff_Perk_AirMeleeDamageBoost_Medium_C",
    "Buff_Perk_AirMeleeDamageBoost_Large_C",
    "Buff_Perk_AirMeleeKnockbackBoost_Medium_C",
    "Buff_Perk_AirMeleeKnockbackBoost_Large_C",
    "Buff_Perk_GroundMeleeDamageBoost_Medium_C",
    "Buff_Perk_GroundMeleeDamageBoost_Large_C",
    "Buff_Perk_GroundMeleeKnockbackBoost_Medium_C",
    "Buff_Perk_GroundMeleeKnockbackBoost_Large_C",
    "Buff_Perk_HorizontalDamageBoost_C",
    "Buff_Perk_UpwardDamageBoost_C",
    "Buff_ProjectileToMeleeFire_C",
    "Buff_Perk_ProjectileDamageBoost_Medium_C",
    "Buff_Perk_ProjectileDamageBoost_Large_C",
    "Buff_Perk_ProjectileKnockbackBoost_Large_C",
];

/// `applyDealtDamageMods`: Comp_State_Manager ApplyBuffDamageMods(hit, DamageTaken false) on
/// the dealer's State Manager (pfgDoDamage @5617-5904): the packet, or a copy with the fields a buff changed.
/// `hit_box`: IsValid(HitCollision). A class's condition may write the World (a buff's own Active, as
/// Buff_PassiveApplyBuffonHit's): it is called with the World in the pass's order, as the actor side's.
pub fn apply_dealt_damage_mods<'a>(
    w: &mut World,
    own: FighterId,
    victim: ActorRef,
    p: &'a HitPacket,
    hit_box: bool,
) -> Cow<'a, HitPacket> {
    let wr: &World = w;
    let bs = &wr.fighter(own).buffs.buffs;
    if bs.is_empty() {
        return Cow::Borrowed(p);
    }
    let list: Vec<DealtBuff> = bs
        .iter()
        .map(|b| {
            let d = def_view(wr, ActorRef::Fighter(own), b.cls, b.own.as_ref());
            let mods = if d.some() { buff_damage_mod(wr, d, false, b.stacks) } else { None };
            DealtBuff { serial: b.serial, cls: b.cls, class: b.class, mods: std::cell::RefCell::new(mods) }
        })
        .collect();
    // The condition's view of the hit once a buff changed a field, built in a packet of the World's pool.
    let mut buf = w.scratch.hit_packets.pop().unwrap_or_default();
    let out = dealt_mods_over_in(
        &list,
        |b| b.mods.borrow().clone(),
        |b, view| match kind_of(w, b.class).and_then(|e| e.hooks.dealt_cond) {
            Some(c) => {
                let r = c(w, victim, own, own, view.packet(), BuffRef { fighter: own, serial: b.serial }, hit_box);
                // The mods are read after the condition (`def(b)` follows `cond`): a condition that writes
                // the instance's mods (Buff_Superman_IceBreaker's DamageMod) is read as written.
                let wr: &World = w;
                if let Some(x) = wr.fighter(own).buffs.buffs.iter().find(|x| x.serial == b.serial) {
                    let d = def_view(wr, ActorRef::Fighter(own), x.cls, x.own.as_ref());
                    *b.mods.borrow_mut() = if d.some() { buff_damage_mod(wr, d, false, x.stacks) } else { None };
                }
                r
            }
            None => dealt_cond_of(w, w.names.str(b.cls)),
        },
        p,
        &mut buf,
    );
    if w.scratch.hit_packets.len() < 32 {
        w.scratch.hit_packets.push(buf);
    }
    out
}

/// A buff of the dealt pass: its place, its class and its mod (as at the pass's start, re-read after its condition).
struct DealtBuff {
    serial: u32,
    cls: Name,
    class: BuffClass,
    /// Re-read behind its condition (`apply_dealt_damage_mods`).
    mods: std::cell::RefCell<Option<DamageMod>>,
}

/// `dealtCondDefault`: a class's part in the dealt pass without a condition of its own:
/// BuffParentActor's true, unless the build gives the class one the port lacks: warned, and no part.
pub fn dealt_cond_default(w: &World, cls: Name) -> bool {
    dealt_cond_of(w, w.names.str(cls))
}

fn dealt_cond_of(w: &World, cls: &str) -> bool {
    if !DEALT_COND_OWN.contains(&cls) {
        return true;
    }
    warn_once(w, name!("a buff's DealtDamageModCondition is not ported"));
    false
}

/// The four float fields of ApplyDamageBuffsOfType: the mod and the packet field it changes.
const DEALT: [usize; 4] = [DAMAGE, KB, HIT_PAUSE, HIT_STUN];

/// `dealtModsOver`: ApplyDamageBuffsOfType's three passes over a State Manager's buff
/// children (`list`, in their order) with their mods (`mods`: GetDamageMod(false)) and the condition (`cond`: the
/// class's DealtDamageModCondition; its `view` the hit as the pass has it): the fighter's and a spawned actor's alike.
/// Override (3) writes the hit at once; Multiply (1) and Add (2) accumulate (In x mod - In) or (mod), added after the
/// loop of passes 1 and 2; ArmorBreak, DestroyProjectiles, HitEffectOverride and ReflectProjectiles of a taking-part
/// buff are ORed in. The packet comes back as it was when no buff changed a field.
/// Deviation: the order is the buff list's; the build's is Map_Keys of the Children map, which reuses the slot of a
/// destroyed actor. It matters between a Multiply and an Override of one field in one pass only, which no class of the
/// ported data has.
pub fn dealt_mods_over<'a, B>(
    list: &[B],
    mods: impl Fn(&B) -> Option<DamageMod>,
    cond: impl FnMut(&B, &mut DealtView) -> bool,
    p: &'a HitPacket,
) -> Cow<'a, HitPacket> {
    dealt_mods_over_in(list, mods, cond, p, &mut HitPacket::default())
}

/// The hit as a condition of the dealt pass reads it ([`DealtView::packet`]): the packet itself until a buff has changed
/// a field, then the packet with the changes, written into a buffer reused across the conditions' calls.
pub struct DealtView<'p, 'b> {
    p: &'p HitPacket,
    x: [Real; 7],
    set: [bool; 7],
    dir: (Real, Real),
    dir_set: bool,
    flags: (bool, bool, bool),
    effect: Option<Name>,
    buf: &'b mut HitPacket,
}

impl DealtView<'_, '_> {
    /// The hit as the pass has it now.
    pub fn packet(&mut self) -> &HitPacket {
        let changed = self.set.iter().any(|s| *s)
            || self.dir_set
            || self.effect.is_some()
            || self.flags.0
            || self.flags.1
            || self.flags.2;
        if !changed {
            return self.p;
        }
        HitPacket::clone_from(&mut *self.buf, self.p);
        self.write_changes_into_buf();
        self.buf
    }

    fn write_changes_into_buf(&mut self) {
        write_dealt_changes(self.buf, &self.x, &self.set, self.dir, self.dir_set, self.flags, self.effect);
    }
}

/// The fields the dealt pass changed, written into `o` (a copy of the packet).
fn write_dealt_changes(
    o: &mut HitPacket,
    x: &[Real; 7],
    set: &[bool; 7],
    dir: (Real, Real),
    dir_set: bool,
    fl: (bool, bool, bool),
    effect: Option<Name>,
) {
    for i in DEALT {
        if set[i] {
            let v = Some(x[i]);
            match i {
                DAMAGE => o.damage = v,
                KB => o.knock_back_scaling = v,
                HIT_PAUSE => o.hit_pause_time = v,
                _ => o.hit_stun_mult = v,
            }
        }
    }
    if dir_set {
        // The vector is `{ X, Y }`: no Z.
        o.knock_back_direction = Some(DataVec { x: Some(dir.0), y: Some(dir.1), z: None });
    }
    if effect.is_some() {
        o.on_hit_effect_override = effect;
    }
    if fl.0 {
        o.armor_pierce = Some(true);
    }
    if fl.1 {
        o.destroy_projectiles = Some(true);
    }
    if fl.2 {
        o.reflect_projectiles = Some(true);
    }
}

/// [`dealt_mods_over`] with the conditions' view built in `buf` (a packet of the World's pool).
pub fn dealt_mods_over_in<'a, B>(
    list: &[B],
    mods: impl Fn(&B) -> Option<DamageMod>,
    mut cond: impl FnMut(&B, &mut DealtView) -> bool,
    p: &'a HitPacket,
    buf: &mut HitPacket,
) -> Cow<'a, HitPacket> {
    let mut x = [0.0; 7];
    x[DAMAGE] = p.damage.unwrap_or(0.0);
    x[KB] = p.knock_back_scaling.unwrap_or(0.0);
    x[HIT_PAUSE] = p.hit_pause_time.unwrap_or(0.0);
    x[HIT_STUN] = p.hit_stun_mult.unwrap_or(1.0);
    let mut set = [false; 7];
    let kd = p.knock_back_direction.as_ref();
    let mut dir = (kd.and_then(|v| v.x).unwrap_or(1.0), kd.and_then(|v| v.y).unwrap_or(0.0));
    let mut dir_set = false;
    let (mut pierce, mut destroy, mut reflect) = (false, false, false);
    let mut effect: Option<Name> = None;
    for t in [1.0, 2.0, 3.0] {
        let mut acc = [0.0; 7];
        let mut hit = [false; 7];
        let (mut acc_x, mut acc_y, mut acc_dir) = (0.0, 0.0, false);
        let (mut a_pierce, mut a_destroy, mut a_reflect) = (false, false, false);
        let mut a_effect: Option<Name> = None;
        for b in list {
            let take = {
                let mut view =
                    DealtView { p, x, set, dir, dir_set, flags: (pierce, destroy, reflect), effect, buf: &mut *buf };
                cond(b, &mut view)
            };
            if !take {
                continue;
            }
            let Some(m) = mods(b) else { continue };
            for i in DEALT {
                if m.t[i] != t {
                    continue;
                }
                let v = m.v[i].unwrap_or(0.0);
                if t == 3.0 {
                    x[i] = v;
                    set[i] = true;
                } else {
                    let r = if t == 1.0 { x[i] * v } else { x[i] + v };
                    acc[i] += r - x[i];
                    hit[i] = true;
                }
            }
            if m.dir_t == t {
                let (mx, my) = (m.dir.0.unwrap_or(0.0), m.dir.1.unwrap_or(0.0));
                if t == 3.0 {
                    dir = (mx, my);
                    dir_set = true;
                } else {
                    let rx = if t == 1.0 { dir.0 * mx } else { dir.0 + mx };
                    let ry = if t == 1.0 { dir.1 * my } else { dir.1 + my };
                    acc_x += rx - dir.0;
                    acc_y += ry - dir.1;
                    acc_dir = true;
                }
            }
            a_pierce |= m.armor_break;
            a_destroy |= m.destroy_projectiles;
            if m.effect.is_some() {
                a_effect = m.effect;
            }
            a_reflect |= m.reflect_projectiles;
        }
        if t == 3.0 {
            break;
        }
        for i in DEALT {
            if hit[i] {
                x[i] += acc[i];
                set[i] = true;
            }
        }
        if acc_dir {
            dir = ((dir.0 + acc_x), (dir.1 + acc_y));
            dir_set = true;
        }
        if a_effect.is_some() {
            effect = a_effect;
        }
        pierce |= a_pierce;
        destroy |= a_destroy;
        reflect |= a_reflect;
    }
    if !(set.iter().any(|s| *s) || dir_set || effect.is_some() || pierce || destroy || reflect) {
        return Cow::Borrowed(p);
    }
    let mut o = p.clone();
    write_dealt_changes(&mut o, &x, &set, dir, dir_set, (pierce, destroy, reflect), effect);
    Cow::Owned(o)
}

/// `applyKnockBackMultipliers`: Comp_State_Manager ApplyKnockBackMultipliers over this
/// fighter's buff actors (GetDamageMod with DamageTaken): the three knockback axes of the struct alone (@957-2171; no
/// condition). In UE axes: horizontal = Y = -sim x, vertical = Z = sim y.
pub fn apply_knock_back_multipliers(w: &World, f: FighterId, vx: Real, vy: Real, taken: bool) -> (Real, Real) {
    let mut h = -vx;
    let mut v = vy;
    for b in &w.fighter(f).buffs.buffs {
        if taken && b.mods_off == Some(true) {
            continue;
        }
        let d = def_view(w, ActorRef::Fighter(f), b.cls, b.own.as_ref());
        if !d.some() {
            continue;
        }
        let Some(m) = buff_damage_mod(w, d, taken, b.stacks) else { continue };
        if h != 0.0 {
            h = apply_mod(h, m.v[H_KB], m.t[H_KB], false);
        }
        if v > 0.0 {
            v = apply_mod(v, m.v[UP_KB], m.t[UP_KB], false);
        }
        if v < 0.0 {
            v = apply_mod(v, m.v[DOWN_KB], m.t[DOWN_KB], true);
        }
    }
    (if h == 0.0 { 0.0 } else { -h }, v)
}

/// `modType`: BuffModType as ApplyDamageBuffsOfType switches on it: 0 DoNothing, 1 Multiply,
/// 2 Add, 3 Override (NewEnumerator3, 2, 1, 0); a number is taken as it is.
pub fn mod_type(t: Option<&Value>) -> Real {
    let s = match t {
        Some(Value::Num(n)) => return crate::math::narrow(*n),
        Some(Value::Str(s)) => s.as_str(),
        _ => return 0.0,
    };
    // The first `NewEnumerator\d+` of the text.
    const P: &str = "NewEnumerator";
    let mut rest = s;
    while let Some(i) = rest.find(P) {
        let tail = &rest[i + P.len()..];
        let n = tail.bytes().take_while(u8::is_ascii_digit).count();
        if n > 0 {
            return match &tail[..n] {
                "3" => 0.0,
                "2" => 1.0,
                "1" => 2.0,
                "0" => 3.0,
                _ => 0.0,
            };
        }
        rest = &rest[i + 1..];
    }
    0.0
}

/// `applyMod`: ApplyKnockBackMultipliers' switch: 0 keep, 1 multiply, 2 add (downward:
/// Mod - V), 3 set. An unset mod is NaN, as `undefined` is in JavaScript's arithmetic.
fn apply_mod(x: Real, m: Option<Real>, t: Real, down: bool) -> Real {
    let m = m.unwrap_or(Real::NAN);
    if t == 1.0 {
        x * m
    } else if t == 2.0 {
        if down { m - x } else { x + m }
    } else if t == 3.0 {
        m
    } else {
        x
    }
}

// ---- the stacks

/// `stackLifeBegin`: BeginPlay @1376-1431: StackDecayBuffer, or StackDecayRate when the
/// buffer is 0 (Infinity: no timer).
pub fn stack_life_begin(d: &Props) -> Real {
    stack_life_begin_of(Def::of(Some(d)))
}

fn stack_life_begin_of(d: Def) -> Real {
    timer_frames(or_num(d.num("StackDecayBuffer"), d.num("StackDecayRate")))
}

/// `stackLifeAdded`: AddStack @169-288: the same select; a rate of 0 leaves the running
/// timer (`cur`).
pub fn stack_life_added(d: &Props, cur: Real) -> Real {
    stack_life_added_of(Def::of(Some(d)), cur)
}

fn stack_life_added_of(d: Def, cur: Real) -> Real {
    match or_num(d.num("StackDecayBuffer"), d.num("StackDecayRate")) {
        Some(r) if r != 0.0 && !r.is_nan() => timer_frames(Some(r)),
        _ => cur,
    }
}

/// `stackLifeRemoved`: RemoveStack @1054-1173: StackDecayRate; 0 leaves the running timer.
pub fn stack_life_removed(d: &Props, cur: Real) -> Real {
    stack_life_removed_of(Def::of(Some(d)), cur)
}

fn stack_life_removed_of(d: Def, cur: Real) -> Real {
    match d.num("StackDecayRate") {
        Some(r) if r != 0.0 && !r.is_nan() => timer_frames(Some(r)),
        _ => cur,
    }
}

/// `stackLifeTimed`: the timer's own TimedRemoveStack @5570: the RemoveStack's re-arm made
/// inside the timer tick, active at once.
pub fn stack_life_timed(d: &Props) -> Real {
    timer_ticks(Def::of(Some(d)).num("StackDecayRate"))
}

/// `addStack`: BuffParentActor AddStack(StackCount) (ubergraph @3954). Below MaxStacks:
/// CurrentStackAmount = clamp(current + (n or 1), 0, MaxStacks) @428-620 (none while MinStackPreventedGate is
/// closed), `BPI On Max Stacks` at the cap @77-129, StatusUpdateMulticast @143; either way SetStackLifeTimer @169-288
/// and the owner's UpdateCurrentStates @311-382. A hook that destroys the actor leaves the rest of AddStack running on
/// the pending-kill actor with no Owner (UWorld::DestroyActor 142b3e8d0 calls SetOwner(null) 14280b7c0): the status
/// multicast's class event finds no BP_FighterCharacter owner (Buff_BubbleCount @43-128) and UpdateCurrentStates is
/// called on None, so neither does anything.
/// Deviation: a child's AddStack override behind the parent's (Buff_Fire's InitialVisuals @2844) is not run for a
/// destroyed actor; no class with such an override has a destroying hook.
fn add_stack(w: &mut World, f: FighterId, b: BuffRef, d: Def, n: Real) {
    let maxs = d.num("MaxStacks").unwrap_or(1.0);
    let Some(x) = buff_mut(w, b) else { return };
    if x.stacks < maxs {
        if x.gate.is_some() {
            return;
        }
        let k = if n != 0.0 && !n.is_nan() { n } else { 1.0 };
        x.stacks = min(max(x.stacks + k, 0.0), maxs);
        let (s, class) = (x.stacks, x.class);
        if s == maxs
            && let Some(h) = kind_of(w, class).and_then(|e| e.hooks.max_stacks)
            && h(w, f, b)
        {
            // The rest of AddStack, on the ownerless pending-kill actor, does nothing.
            end_buff(w, f, b);
            return;
        }
        let Some(x) = buff(w, b) else { return };
        if x.frames <= 0.0 {
            return;
        }
        let s = x.stacks;
        buff_status_update(w, f, class, s, false, false);
    }
    let Some(x) = buff(w, b) else { return };
    let sf = stack_life_added_of(d, x.stack_frames);
    buff_mut(w, b).expect("found above").stack_frames = sf;
    update_states(w, f);
    if buff(w, b).is_some_and(|x| x.fire.is_some()) {
        fire_visuals(w, f, b);
    }
}

/// `addStackGated`: AddStack as a class with NewStackApplyBuffer overrides it (Buff_Ice
/// @2476-2704): nothing unless the time since the last add >= NewStackApplyBuffer; then the parent's AddStack and the
/// class's own (BuffKind.stacks) on an actor that call left alive. The two game times are TimeSeconds (the world's
/// float sum of DeltaSeconds): now, and the tick of the last add, `add_age` ticks back.
fn add_stack_gated(w: &mut World, f: FighterId, b: BuffRef, d: &Props, n: Real) -> bool {
    let d = Def::of(Some(d));
    if let Some(gap) = d.num("NewStackApplyBuffer").or(d.get("NewStackApplyBuffer").map(|_| Real::NAN)) {
        let (now, tick) = (w.time_seconds, w.flow.tick);
        let Some(x) = buff_mut(w, b) else { return false };
        let then = time_seconds_at(tick.wrapping_sub(crate::math::to_int32(x.add_age.unwrap_or(0.0))));
        if !((now - then) >= gap) {
            return false;
        }
        x.add_age = Some(0.0);
    }
    add_stack(w, f, b, d, n);
    if let Some(x) = buff(w, b)
        && x.frames > 0.0
        && let Some(h) = kind_of(w, x.class).and_then(|e| e.hooks.stacks)
        && h(w, f, b, true)
    {
        end_buff(w, f, b);
    }
    true
}

/// UWorld TimeSeconds at world tick `tick` (what [`World::time_seconds`] held then): the float32 running sum of
/// DeltaSeconds, one per tick from the World's start (0 at or before it), in steps of a binade at a time. From 1 on,
/// DeltaSeconds rounded to the ulp of a sum in [2^k, 2^(k+1)) is never a tie, so every add whose result stays in the
/// binade adds the same step to the sum's bits; an add that crosses into the next binade, and every add under 1
/// (where a tie can round by the sum's parity), is made as the add itself. The test holds it to the running sum.
pub(crate) fn time_seconds_at(tick: i32) -> Real {
    let mut n = u32::try_from(tick).unwrap_or(0);
    let mut t: Real = 0.0;
    while n > 0 {
        let next = t + DELTA_SECONDS;
        let (bits, next_bits) = (t.to_bits(), next.to_bits());
        if t < 1.0 || next_bits >> 23 != bits >> 23 {
            t = next;
            n -= 1;
            continue;
        }
        let step = next_bits - bits;
        if step == 0 {
            break; // an add of DeltaSeconds no longer moves the sum
        }
        // The adds left in the binade: the sum's mantissa (with its leading bit) stays under 2^24.
        let room = (1u32 << 24) - ((bits & 0x7f_ffff) | 0x80_0000);
        let k = ((room - 1) / step).min(n);
        t = Real::from_bits(bits + k * step);
        n -= k;
    }
    t
}

/// `removeStack`: BuffParentActor RemoveStack(StackAmount) (ubergraph @652-1196):
/// CurrentStackAmount -= (amount, or 1 for 0) with no clamp, StatusUpdateMulticast @849, K2_DestroyActor at 0 or less
/// @923; else UpdateCurrentStates @1009 and SetStackLifeTimer(StackDecayRate) @1054-1173, then the class's own.
pub fn remove_stack(w: &mut World, f: FighterId, b: BuffRef, n: i32) {
    let Some(x) = buff_mut(w, b) else { return };
    if x.frames <= 0.0 {
        return;
    }
    x.stacks -= if n != 0 { crate::real(n) } else { 1.0 };
    let (s, class) = (x.stacks, x.class);
    buff_status_update(w, f, class, s, false, false);
    if s <= 0.0 {
        end_buff(w, f, b);
        return;
    }
    update_states(w, f);
    let Some(x) = buff(w, b) else { return };
    let sf = stack_life_removed_of(inst_view(w, b), x.stack_frames);
    buff_mut(w, b).expect("found above").stack_frames = sf;
    if let Some(h) = kind_of(w, class).and_then(|e| e.hooks.stacks)
        && h(w, f, b, false)
    {
        end_buff(w, f, b);
    }
}

/// `removeBuffStack`: Comp_State_Manager.RemoveBuffStack(class, n): RemoveStack(n) on every
/// child buff actor of the class or of a child class ([`is_buff_of_class`] @742).
pub fn remove_buff_stack(w: &mut World, f: FighterId, cls: Name, n: i32) {
    let list = refs(w, f);
    for &r in &list {
        let of = |b: &FighterBuff| b.cls == cls || is_buff_of_class(w, w.names.str(b.cls), w.names.str(cls));
        if buff(w, r).is_some_and(|b| of(b) && b.frames > 0.0) {
            remove_stack(w, f, r, n);
        }
    }
    put_refs(w, list);
}

/// `stackBuff`: BPI_AddStack on a live buff of this fighter from another graph:
/// AddStack(n) with the class's values (no CanStack test); nothing on a buff that has ended.
pub fn stack_buff(w: &mut World, f: FighterId, b: BuffRef, n: Real) {
    if !buff(w, b).is_some_and(|x| x.frames > 0.0) {
        return;
    }
    let Some(d) = inst_def_of(w, b).map(Cow::into_owned) else { return };
    add_stack_gated(w, f, b, &d, n);
}

/// `updateStates`: Comp_State_Manager.UpdateCurrentStates from a buff actor's AddStack /
/// RemoveStack (BuffParentActor @382, @1009) -> UpdatePlayerStatus(null) (@968).
pub fn update_states(w: &mut World, f: FighterId) {
    recalc_speed_mod(w, f, None);
    apply_stun_buffs(w, f, None, false);
    recalc_move_folds(w, f, None);
    recalc_ability_mods(w, f, None);
    recalc_misc_movement(w, f, None);
    recalc_charge_mod(w, f, None);
    recalc_size_mod(w, f, None);
    recalc_head_size(w, f, None);
}

// ---- Buff_MoveCompTo and its child Buff_MoveCompTo_WithStun

/// `startBuffMove`: Buff_MoveCompTo StartMove @241 -> @50: MoveComponentTo(ComponentToMove,
/// TargetRelativeLoc, ..., MoveDuration, Move (0)) @70, the latent action on the buff actor; its completion resumes
/// @35: K2_DestroyActor. Buff_MoveCompTo_WithStun's StartMove @590 adds SetCanSurviveBlastboxJumpOutArea(true) @548
/// ([`survives_blast_jump_out`]).
pub fn start_buff_move(w: &mut World, f: FighterId, b: BuffRef, tx: Real, ty: Real, seconds: Real) {
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let Some(e) = buff_mut(w, b) else { return };
    let BuffExt::MoveCompTo { mv } = &mut e.ext else {
        warn_once(w, name!("StartMove on a buff that is no Buff_MoveCompTo"));
        return;
    };
    *mv = ComponentMove::call(*mv, x, y, tx, ty, false, false, seconds, 0);
}

/// Buff_MoveCompTo_WithStun's SetCanSurviveBlastboxJumpOutArea on its fighter: true from its StartMove (@590 -> @548)
/// to its EndPlay (@10-321: false), so while the buff lives with its move started (the move's end destroys it). Read
/// by the blast box's exit (BlastBoxPlayerExit HandleBlastBoxExit @127-186).
pub fn survives_blast_jump_out(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| {
        b.frames > 0.0
            && b.class == BuffClass::BuffMoveCompToWithStun
            && matches!(b.ext, BuffExt::MoveCompTo { mv: Some(_) })
    })
}

/// `buffMoveTick`: the buff actors' latent MoveComponentTo actions of this frame, for one
/// fighter. True: the fighter's root was written (the world runs its overlap update).
pub fn buff_move_tick(w: &mut World, f: FighterId) -> bool {
    let mut moved = false;
    if w.fighter(f).life.dead > 0.0 {
        return false;
    }
    let list = refs(w, f);
    for &r in &list {
        let Some(e) = buff_mut(w, r) else { continue };
        if e.frames <= 0.0 {
            continue;
        }
        let BuffExt::MoveCompTo { mv: Some(m) } = &mut e.ext else { continue };
        let done = m.update(DELTA_SECONDS);
        let (mx, my) = (m.x, m.y);
        recall_move(w, f, mx, my);
        // A place inside a blocker is left there: the next CMC move starts in penetration and resolves it first.
        w.fighter_mut(f).capsule.cap_resized = true;
        moved = true;
        if done {
            if let Some(e) = buff_mut(w, r) {
                e.ext = BuffExt::MoveCompTo { mv: None };
            }
            end_buff(w, f, r); // @35 K2_DestroyActor
        }
    }
    put_refs(w, list);
    moved
}

// ---- the shared classes' pop texts

/// A graph's PopText of a constant text (`GraphPopText`): the text's key, its lifetime, colour, location, space,
/// whether it is queued or local only, its audio.
#[derive(Clone, Copy, Debug)]
pub struct BuffPop {
    pub key: Name,
    pub lifetime: Real,
    pub color: [Real; 4],
    pub location: [Real; 3],
    pub space: i32,
    pub queued: bool,
    pub local_only: bool,
}

/// `CLEANSE_POP`: Buff_Cleanse @209-718: "Cleanse!".
pub const CLEANSE_POP: BuffPop = BuffPop {
    key: name!("B52EE17D46FB1781FC8D468618DFB003"),
    lifetime: 1.0,
    color: [0.0, 1.0, 0.204562, 1.0],
    location: [0.0, 0.0, 250.0],
    space: 3,
    queued: true,
    local_only: false,
};
/// `SHARPEN_POP`: Buff_Sharpened @180-688: "Sharpen".
pub const SHARPEN_POP: BuffPop = BuffPop {
    key: name!("73CDF1DC451978B29AAEAB93FCD3BC2C"),
    lifetime: 1.0,
    color: [0.655304, 0.169569, 0.692708, 1.0],
    location: [0.0, 0.0, 250.0],
    space: 3,
    queued: false,
    local_only: false,
};
/// `SILENCED_POP`: C023A_Buff_Silenced @139-645: "Silenced!" at the owner's location @89 in
/// space 0, the class's Params Audio @547.
pub const SILENCED_POP: BuffPop = BuffPop {
    key: name!("4BF9E9EC4A893C4DE3AC0893643C9B21"),
    lifetime: 1.0,
    color: [1.0, 0.00687, 0.0, 1.0],
    location: [0.0, 0.0, 0.0],
    space: 0,
    queued: false,
    local_only: false,
};
/// `EDUCATED_TEXT`: Buff_Educated's Pop Text default ("+ Cooldown Reduction").
pub const EDUCATED_TEXT: &str = "EBAB0056453D91F25EBB1D8A51D45523";
/// `EDUCATED_POP`: Buff_Educated @3190-3660, LocalOnly (the owner's machine).
pub const EDUCATED_POP: BuffPop = BuffPop {
    key: name!("EBAB0056453D91F25EBB1D8A51D45523"),
    lifetime: 1.0,
    color: [0.183437, 1.0, 0.593957, 1.0],
    location: [0.0, 0.0, 350.0],
    space: 3,
    queued: false,
    local_only: true,
};

/// The pop text of one of the constants above, its text key and location as the caller has them, its audio.
/// Scale 1, centred, no delay, no arguments (the constants' own).
fn graph_pop(p: &BuffPop, key: Name, location: [Real; 3], audio: Option<Name>) -> GraphPopText {
    GraphPopText {
        text_key: key,
        source_string: None,
        localized_string: None,
        lifetime: p.lifetime,
        color: p.color,
        location,
        space: u8::try_from(p.space).expect("a space of 0 or 3"),
        scale: 1.0,
        justification: 1,
        delay: 0.0,
        audio,
        queued: p.queued,
        local_only: p.local_only,
        args: Vec::new(),
    }
}

/// `popTextOn` with one of the constants above.
fn pop(w: &mut World, f: FighterId, p: &BuffPop) {
    let g = graph_pop(p, p.key, p.location, None);
    pop_text_on(w, f, &g);
}

// ---- the registered classes

/// `Buff_JumpIncrease_C` begin: ReceiveBeginPlay @35 -> PopText @45, Delay(6.0) @59 -> K2_DestroyActor @15, the
/// Delay counted from the spawn frame.
fn jump_increase_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::JumpIncrease { life: max(latent_frames(6.0) - 1.0, 1.0) };
    }
}

fn jump_increase_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::JumpIncrease { life } = &mut x.ext else { return false };
    *life -= 1.0;
    *life <= 0.0
}

fn move_comp_to_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::MoveCompTo { mv: None };
    }
}

/// Buff_ProjectileImunity: ReceiveBeginPlay @205 sets the owner's Comp_Pawn_DamageManager.HasProjectileArmor,
/// ReceiveEndPlay @314 clears it (a plain write, not a count).
fn projectile_imunity_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    w.fighter_mut(f).invuln.projectile_armor = true;
}

fn projectile_imunity_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).invuln.projectile_armor = false;
}

/// Buff_Thorns ReceiveBeginPlay @2208: the owner's HitByEnemy -> ProcBuff (@2478), the reflected hit
/// ReflectedDamagePerStack x CurrentStackAmount @1219 on HitByEnemy's DamageDealer.
/// Unverified: a spawned actor's hit passes the actor (pfgDoDamage on a projectile or other actor, not ported:
/// nothing is reflected then).
fn thorns_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let per = inst_view(w, b).num("ReflectedDamagePerStack").unwrap_or(0.0);
    if let Some(x) = buff_mut(w, b) {
        x.src = from;
        x.ext = BuffExt::Thorns { per };
    }
}

/// Buff_Thorns BPI After Status Update Multicast @1347: removed or stacks <= 0: RemoveStateForPlayer('Thorns') @1692;
/// else AddStateForPlayer @2149.
fn thorns_status(w: &mut World, f: FighterId, class: BuffClass, stacks: Real, _applied: bool, removed: bool) {
    let op = if removed || stacks <= 0.0 {
        UiStateOp::Remove(name!("Thorns"))
    } else {
        let maxv = pnum(buff_def(w, ActorRef::Fighter(f), class_name(class)), "MaxStacks").unwrap_or(1.0);
        ui_add(name!("Thorns"), 2, 1, stacks, maxv, name!("MI_State_Thorn"), true, Name::NONE)
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

fn thorns_on(w: &mut World, f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    if sig == BuffSignal::HitByEnemy
        && let Some(d) = a.dealer
        && let Some(x) = buff(w, b)
        && let BuffExt::Thorns { per } = x.ext
    {
        let (src, dmg) = (x.src, (per * x.stacks));
        buff_actor_light_hit(w, f, b, d, src, dmg, false, false);
    }
    false
}

/// The name of a registered class (its `cls`).
fn class_name(c: BuffClass) -> Name {
    match c {
        BuffClass::Data(n) => n,
        BuffClass::BuffThorns => name!("Buff_Thorns_C"),
        BuffClass::BuffIce => name!("Buff_Ice_C"),
        BuffClass::BuffFearStack => name!("Buff_FearStack_C"),
        _ => Name::NONE,
    }
}

/// The classes whose graph overrides BPI On Cleanse (BuffParentActor's own and Buff_Tasty's alone).
const CLEANSE_OWN: [&str; 1] = ["Buff_Tasty_C"];

/// Buff_Cleanse ReceiveBeginPlay @834: CleanseDebuffs @874 (each child buff asked BPI On Cleanse @600, destroyed when
/// it says so @781), CleanseAttachedActors @15 (an attached SpawnedActorParent with DestroyOnCleanse? of another team
/// destroyed @1324), the pop text when either removed something @76-718, Delay(0.0) @764 and K2_DestroyActor @819.
/// Unverified: the children are taken in the port's buff order; the Delay is taken to end in the buff's first tick.
fn cleanse_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let mut cleansed = false;
    let list = refs(w, f);
    for &r in &list {
        let Some(x) = buff(w, r) else { continue };
        if r == b || x.frames <= 0.0 {
            continue;
        }
        let own = kind_of(w, x.class).and_then(|e| e.hooks.cleanse);
        if own.is_none() && CLEANSE_OWN.contains(&w.names.str(x.cls)) {
            warn_once(w, name!("a buff's BPI On Cleanse override is not ported (the parent's answer)"));
        }
        if let Some(c) = own {
            cleansed = true;
            if c(w, f, r) {
                end_buff(w, f, r);
            }
        } else if inst_view(w, r).flag("IsDebuff") {
            end_buff(w, f, r);
            cleansed = true;
        }
    }
    put_refs(w, list);
    let team = w.fighter(f).team.team;
    for i in 0..peers(w).len() {
        let o = peers(w)[i];
        if w.fighter(o).team.team == team {
            continue;
        }
        if cleanse_attached_of(w, o, f) {
            cleansed = true;
        }
    }
    if cleansed {
        pop(w, f, &CLEANSE_POP); // @718
    }
}

/// CleanseAttachedActors @0-1361 for the actors `o` spawned: a SpawnedActorParent attached to `f` with
/// DestroyOnCleanse? is destroyed @1324: it ends as a registered actor does (`dead`: its ReceiveDestroyed is the
/// actor kind's `end`; no detonation).
fn cleanse_attached_of(w: &mut World, o: FighterId, f: FighterId) -> bool {
    use crate::actors::world_actor::{actor_dead as spawned_dead, attached_to, destroy_on_cleanse, mark_spawned_dead};
    let mut any = false;
    for a in w.fighter(o).items.spawned_actors.clone() {
        if !spawned_dead(w, a) && attached_to(w, a) == Some(f) && destroy_on_cleanse(w, a) {
            mark_spawned_dead(w, a);
            any = true;
        }
    }
    any
}

fn cleanse_tick(_w: &mut World, _f: FighterId, _b: BuffRef) -> bool {
    true
}

// ---- Buff_Ice

/// The sound names of Buff_Ice's stacks.
const ICE_STACK: Name = name!("sfx_cha_superman_icebreath_stack");
const ICE_STACK_REMOVE: Name = name!("sfx_cha_superman_icebreath_stack_remove");
const ICE_FREEZE: Name = name!("sfx_cha_superman_icebreath_freeze");
const ICE_FREEZE_BREAK: Name = name!("sfx_cha_superman_icebreath_freeze_break");
const ICE_DEBUFF: Name = name!("sfx_cha_superman_icebreath_debuff");

/// `iceCheck`: Buff_Ice CheckStackLevel @2312-2471 -> @1394: IceLevel = how many of k x
/// IceIncrement (k = 1..5) CurrentStackAmount has reached; a new level: UpdatePlayerStatus(null) @1570, then at
/// MaxStacks Freeze once (@3781: StackDecayRate 0, SpeedMod 1, Stun = FreezeStun, CanStack false, UpdatePlayerStatus,
/// the four binds, FreezeVisuals), below it StackSoundMulticast @1760 and OldIceLevel = IceLevel @1792.
/// Unverified: the buff actor's location is taken as its owner's; the sounds at a location are posted with the frozen
/// fighter as the event's fighter, and the RTPC and the stack sounds on the owner's game object.
fn ice_check(w: &mut World, f: FighterId, b: BuffRef) {
    let d = inst_view(w, b);
    let inc = d.num("IceIncrement").unwrap_or(0.0);
    let maxs = d.num("MaxStacks").unwrap_or(1.0);
    let freeze = d.get("FreezeStun").cloned();
    let Some(x) = buff(w, b) else { return };
    let BuffExt::Ice { old, frozen, .. } = x.ext else { return };
    let stacks = x.stacks;
    let mut level = 0.0;
    while level < 5.0 && stacks >= (inc * (level + 1.0)) {
        level += 1.0;
    }
    if old == level {
        return;
    }
    update_states(w, f);
    if stacks == maxs {
        if frozen {
            return;
        }
        let Some(x) = buff_mut(w, b) else { return };
        if let BuffExt::Ice { frozen, .. } = &mut x.ext {
            *frozen = true;
        }
        x.ally_hit = true;
        let mut v = Props::new();
        v.insert("StackDecayRate".into(), Value::Num(0.0));
        v.insert("SpeedMod".into(), Value::Num(1.0));
        v.insert("Stun".into(), freeze.unwrap_or(Value::Null));
        v.insert("CanStack".into(), Value::Bool(false));
        set_buff_values(w, b, &v);
        update_states(w, f);
        // FreezeVisuals @982
        let (id, fx, fy) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
        w.fighter_mut(f).events.push(sound_at(id, ICE_FREEZE, Some(fx), Some(fy)));
        if let Some(s) = buff(w, b).and_then(|x| x.stun_by) {
            post_vo(w, s, name!("ability_icebreath_freeze"), None, 0.25);
        }
        return;
    }
    // StackSoundMulticast @1760
    let id = w.fighter(f).id;
    let ev = if level < old { ICE_STACK_REMOVE } else { ICE_STACK };
    let x = w.fighter_mut(f);
    x.events.push(Event::Rtpc { fighter: id, name: name!("cha_buff_ice_feedback"), value: level, interp_ms: 0.0 });
    x.events.push(sound_at(id, ev, None, None));
    if let Some(e) = buff_mut(w, b)
        && let BuffExt::Ice { old, .. } = &mut e.ext
    {
        *old = level;
    }
}

fn ice_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let old = inst_view(w, b).num("OldIceLevel").unwrap_or(-1.0);
    let Some(x) = buff_mut(w, b) else { return };
    x.ext = BuffExt::Ice { old, frozen: false, ally_breaks: 0.0 };
    // The stun is the actor's own from Freeze on: its SetMostRecentAttacker reads the Instigator all the same
    // (Comp_State_Manager ApplyStunBuffs @443-680).
    if from.is_some() {
        x.stun_by = from;
    }
    ice_check(w, f, b);
}

fn ice_stacks(w: &mut World, f: FighterId, b: BuffRef, _added: bool) -> bool {
    ice_check(w, f, b);
    false
}

/// Buff_Ice's binds from Freeze on: HitStunEnded -> FreezeBreak @4019, BuffStunEnded -> BuffStunEnd @4105, KnockedBack
/// -> HitBreakFreeze @4191, and HitByAlly -> AllyBreakFreeze @4277 -> @5297 (a packet with HitStunMult > 0 and
/// BaseKnockBack > 0 adds 1 to AllyBreakCount; at AllyBreakThreshold the actor is destroyed @5609).
fn ice_on(w: &mut World, _f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    let threshold = inst_view(w, b).num("AllyBreakThreshold").unwrap_or(1.0);
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::Ice { frozen, ally_breaks, .. } = &mut x.ext else { return false };
    if !*frozen {
        return false;
    }
    if matches!(sig, BuffSignal::HitStunEnded | BuffSignal::BuffStunEnded | BuffSignal::KnockedBack) {
        return true;
    }
    let Some(h) = a.hit.as_deref() else { return false };
    if sig != BuffSignal::HitByAlly {
        return false;
    }
    if !(h.hit_stun_mult.unwrap_or(1.0) > 0.0 && h.base_knock_back.unwrap_or(0.0) > 0.0) {
        return false;
    }
    *ally_breaks += 1.0;
    *ally_breaks >= threshold
}

/// Buff_Ice ReceiveEndPlay @2733 -> EndFreezeVisuals: the freeze break sound when it froze, else the debuff's.
fn ice_end(w: &mut World, f: FighterId, b: BuffRef) {
    let frozen = buff(w, b).is_some_and(|x| matches!(x.ext, BuffExt::Ice { frozen: true, .. }));
    let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.fighter_mut(f).events.push(sound_at(id, if frozen { ICE_FREEZE_BREAK } else { ICE_DEBUFF }, Some(x), Some(y)));
}

/// Buff_Ice BPI After Status Update Multicast @6173: RemoveStateForPlayer('IceStacks') @6510, else AddStateForPlayer
/// @7161 ('Frozen' at MaxStacks).
fn ice_status(w: &mut World, f: FighterId, class: BuffClass, stacks: Real, _applied: bool, removed: bool) {
    let op = if removed || stacks <= 0.0 {
        UiStateOp::Remove(name!("IceStacks"))
    } else {
        let maxv = pnum(buff_def(w, ActorRef::Fighter(f), class_name(class)), "MaxStacks").unwrap_or(1.0);
        let pop = if stacks == maxv { name!("Frozen") } else { Name::NONE };
        ui_add(name!("IceStacks"), 1, 0, stacks, maxv, name!("MI_State_ChillRadial"), true, pop)
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

// ---- the buffs of Blueprints/Abilities/Shield that C006 (Arya) gives

/// `caughtBegin`: Buff_Caught ReceiveBeginPlay @2608 -> @2411 -> @15: under hitstun,
/// RemoveHitstunFromKnockBack @322, LaunchCharacter(Normal(Velocity, 1e-4) x VelocityClamp) above the clamp @717, the
/// pop text "CAUGHT" @1446; @1508: RefreshesJumps off the ground: BPI_ResetJump @1704 and "Jumps Reset!" @2349. Then
/// K2_DestroyActor @2396.
fn caught_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let d = inst_view(w, b);
    let (clamp, refresh) = (d.num("VelocityClamp").unwrap_or(300.0), d.flag("RefreshesJumps"));
    let id = w.fighter(f).id;
    if crate::fighter::in_hitstun(w, f) {
        remove_hitstun_from_knock_back(w, f);
        let x = w.fighter_mut(f);
        let (vx, vy) = (x.body.vx, x.body.vy);
        let v = ue::size2d([vx, vy]);
        if v > clamp {
            let n = ue::normal([vx, vy, 0.0], ue::KINDA_SMALL_NUMBER);
            let (ix, iy) = (n[0] * clamp, n[1] * clamp);
            x.moves.impulse = Some(Impulse { vx: ix, vy: iy, xy: true, z: true, kind: 0, d: None });
            x.moves.hit_launch = true; // a direct LaunchCharacter, as CancelJump's
        }
        x.events.push(Event::PopText { fighter: id, kind: PopKind::Caught, stacks: 0.0, gain: None, params: None });
    }
    if refresh && !w.fighter(f).body.on_floor {
        reset_jump_counts(w, f);
        w.fighter_mut(f).events.push(Event::PopText {
            fighter: id,
            kind: PopKind::JumpsReset,
            stacks: 0.0,
            gain: None,
            params: None,
        });
    }
    end_buff(w, f, b);
}

/// Buff_TurnEnemyAwayOnHit ReceiveBeginPlay @833 -> SharpenVisuals @1008: the pop text "Turn Attack".
/// Unverified: behind HasAuthority, so whether a client of an online match sees the text is a live-game question.
fn turn_enemy_away_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: id,
        kind: PopKind::TurnAttack,
        stacks: 0.0,
        gain: None,
        params: None,
    });
}

/// ApplyBuff @633 (the owner's HitEnemyPlayer): Buff_Reverse_C on the hit player, the owner its Instigator, stacks 0,
/// 4.5 s @734, then K2_DestroyActor @818.
fn turn_enemy_away_on(w: &mut World, f: FighterId, _b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(v) = a.victim.filter(|_| sig == BuffSignal::HitEnemy) else { return false };
    spawn_buff(w, v, name!("Buff_Reverse_C"), 4.5, Some(0), Some(f), true, None);
    true
}

/// Buff_Reverse ReceiveBeginPlay @10: under hitstun (CheckHitstunActive @91), BPI_SetFacingLogic(the Instigator's
/// facing; 1 for none) @394 and K2_DestroyActor @440.
fn reverse_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if !crate::fighter::in_hitstun(w, f) {
        return;
    }
    let facing = from.map_or(1.0, |o| w.fighter(o).body.facing_dir);
    set_facing(w, f, facing);
    end_buff(w, f, b);
}

/// Buff_Sharpened ReceiveBeginPlay @15: the pop text @180-688 (SHARPEN_POP).
fn sharpened_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    pop(w, f, &SHARPEN_POP);
}

const FEARED: Name = name!("Feared");

/// The Feared state's UI operation.
fn fear_state(current: Real, maxv: Real, active: bool, pop: Name) -> UiStateOp {
    ui_add(FEARED, 1, 0, current, maxv, name!("MI_State_Fear"), active, pop)
}

/// `fearStackUI`: Buff_FearStack UpdateUI(Stack) @1142: below 0 RemoveStateForPlayer('Feared')
/// @1454, else AddStateForPlayer @1973 (MaxValue MaxStacks - 1, Active false).
fn fear_stack_ui(w: &mut World, f: FighterId, maxs: Real, stack: Real) {
    let op = if stack < 0.0 { UiStateOp::Remove(FEARED) } else { fear_state(stack, maxs - 1.0, false, Name::NONE) };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

/// Buff_Fear ReceiveBeginPlay @1258: DestroyAndBreak bound to the owner's HitByAlly @1416-1510; SetMovementMode(5
/// Flying) @1213 and StopMovementImmediately @995.
/// The mode write is on every path (no stun test); the fighter stays Flying until a write that leaves it (the True
/// Hitstun Timer's end, a knockback, a move command's hitstun branch, the stun suspension's end, the respawn reset).
fn fear_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ally_hit = true;
    }
    set_flying_mode(w, f, true);
    let x = w.fighter_mut(f);
    x.body.vx = 0.0;
    x.body.vy = 0.0;
}

fn fear_on(_w: &mut World, _f: FighterId, _b: BuffRef, sig: BuffSignal, _a: &BuffSignalArg) -> bool {
    sig == BuffSignal::HitByAlly
}

/// Buff_Fear BPI After Status Update Multicast @1561: applied: sfx_cha_arya_special_n_fear at the owner @1706; removed:
/// RemoveStateForPlayer('Feared') @295, else AddStateForPlayer @786 ('Stunned!').
fn fear_status(w: &mut World, f: FighterId, _class: BuffClass, _stacks: Real, applied: bool, removed: bool) {
    if applied {
        let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
        w.fighter_mut(f).events.push(sound_at(id, name!("sfx_cha_arya_special_n_fear"), Some(x), Some(y)));
    }
    let op = if removed { UiStateOp::Remove(FEARED) } else { fear_state(-1.0, -1.0, true, name!("Stunned!")) };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

/// Buff_FearStack ReceiveBeginPlay @767: UpdateUI(0) @811, OnOwnerHit bound to HitByEnemy @1015-1038; the
/// Instigator kept in `ext.by`.
fn fear_stack_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    let maxs = inst_view(w, b).num("MaxStacks").unwrap_or(1.0);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::FearStack { by: from };
    }
    fear_stack_ui(w, f, maxs, 0.0);
}

/// OnOwnerHit @2075: Hit.Damage > 1.0 and Hit.BaseKnockBack x Hit.HitStunMult > 0: AddStack(1 for the Instigator's
/// hit, else 2) @2373.
fn fear_stack_on(w: &mut World, f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    if sig != BuffSignal::HitByEnemy {
        return false;
    }
    let Some(h) = a.hit.as_deref() else { return false };
    if !(h.damage.unwrap_or(0.0) > 1.0 && (h.base_knock_back.unwrap_or(0.0) * h.hit_stun_mult.unwrap_or(0.0)) > 0.0) {
        return false;
    }
    let by = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::FearStack { by }) => *by,
        _ => None,
    };
    let n = if a.dealer.is_some() && a.dealer == by { 1.0 } else { 2.0 };
    stack_buff(w, f, b, n);
    false
}

/// Buff_FearStack AddStack @298: at MaxStacks @351 the owner's SpawnBuffActor(Buff_Fear_C, 3.0 s) @608 (whose
/// BreakBuffs destroys this one); below it UpdateUI(CurrentStackAmount - 1) @739; RemoveStack @1084 the same.
fn fear_stack_stacks(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    let maxs = inst_view(w, b).num("MaxStacks").unwrap_or(1.0);
    let Some(x) = buff(w, b) else { return false };
    let (stacks, by) = (x.stacks, if let BuffExt::FearStack { by } = x.ext { by } else { None });
    if added && stacks == maxs {
        spawn_buff(w, f, name!("Buff_Fear_C"), 3.0, Some(0), by, true, None);
    } else {
        fear_stack_ui(w, f, maxs, stacks - 1.0);
    }
    false
}

/// Buff_FearStack ReceiveEndPlay @2032: UpdateUI(-1).
fn fear_stack_end(w: &mut World, f: FighterId, _b: BuffRef) {
    fear_stack_ui(w, f, 1.0, -1.0);
}

/// Buff_PieHeal ReceiveBeginPlay @619 -> @30: HealVFXMulticast @60 (sfx_cha_arya_pie_heal at the owner @922),
/// ApplyDamage(owner, FMax(-CurrentDamage, -HealAmount)) @469, Delay(2.0) @564 -> @15 K2_DestroyActor.
/// Unverified: the Delay counts from the buff's first tick, as the heal buffs' Delay does.
fn pie_heal_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let amount = inst_view(w, b).num("HealAmount").unwrap_or(0.0);
    let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.fighter_mut(f).events.push(sound_at(id, name!("sfx_cha_arya_pie_heal"), Some(x), Some(y))); // @922
    apply_healing(w, f, amount);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::PieHeal { left: latent_frames(2.0) };
    }
}

fn pie_heal_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::PieHeal { left } = &mut x.ext else { return false };
    *left -= 1.0;
    *left <= 0.0
}

// ---- C023A_Buff_Silenced

/// The Silenced state's name.
pub const SILENCED: Name = name!("Silenced");

/// `silencedBegin`: C023A_Buff_Silenced ReceiveBeginPlay @705: the owner's
/// Comp_Pawn_AbilityManager.AdjustBlockCooldowns(true) @786, MulticastUI(true, true, BuffDuration) @44 -> @1300
/// (AddStateForPlayer, Timer = Duration), and the PopText at the owner's location @69-645.
pub fn silenced_begin(w: &mut World, f: FighterId, b: BuffRef, duration: Option<Real>) {
    let d = inst_view(w, b);
    let timer = duration.unwrap_or_else(|| d.num("BuffDuration").unwrap_or(0.0));
    let audio = data_str(w, d, "Params Audio");
    adjust_block_cooldowns(w, f, true);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: SILENCED,
        state_type: 1,
        display_type: 3,
        current_value: -1.0,
        max_value: -1.0,
        timer,
        icon_material: name!("MI_State_Silenced"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: Some(name!("MI_State_Block")),
    }));
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let g = silenced_pop(audio, x, y);
    pop_text_on(w, f, &g);
}

/// `silencedPop`: SILENCED_POP at the sim point (x, y) of its owner, with the class's Params
/// Audio.
pub fn silenced_pop(audio: Option<Name>, x: Real, y: Real) -> GraphPopText {
    graph_pop(&SILENCED_POP, SILENCED_POP.key, [0.0, -x, y], audio)
}

/// `silencedEnd`: ReceiveEndPlay @828: MulticastUI(false, ...) @847 (RemoveStateForPlayer
/// 'Silenced' @2062; again @1240, sent once here), AdjustBlockCooldowns(false) @939.
pub fn silenced_end(w: &mut World, f: FighterId) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(SILENCED));
    adjust_block_cooldowns(w, f, false);
}

fn silenced_begin_hook(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, dur: Option<Real>) {
    silenced_begin(w, f, b, dur);
}

fn silenced_end_hook(w: &mut World, f: FighterId, _b: BuffRef) {
    silenced_end(w, f);
}

// ---- Buff_Educated

/// `EDUCATED.begin`: Buff_Educated ReceiveBeginPlay @2387 -> CreateBuffVisuals @2661 -> @15:
/// with ShouldCleanse the owner's SpawnBuffActor(Buff_Cleanse_C, ..., the Instigator, 0, 0.0) @49;
/// CooldownReductionPopText @155; sfx_cha_velma_educated_cast @2766.
/// Unverified: the Cleanse spawned inside the BeginPlay runs its UpdatePlayerStatus with this buff already in the
/// owner's list.
pub fn educated_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if inst_view(w, b).flag("ShouldCleanse") {
        spawn_buff(w, f, name!("Buff_Cleanse_C"), 0.0, Some(0), from, true, None);
    }
    let pop = inst_view(w, b).obj("Pop Text");
    let part = |k: &str| pop.as_ref().and_then(|t| t.get(k)).and_then(Value::as_str).map(str::to_owned);
    let (text, source, localized) = (part("Key"), part("SourceString"), part("LocalizedString"));
    // The class's key is a Game.locres key the build script collects from the data: an uncollected one is the default.
    match text.map(|k| w.names.get(&k).unwrap_or(EDUCATED_POP.key)) {
        Some(k) => {
            let mut g = graph_pop(&EDUCATED_POP, k, EDUCATED_POP.location, None);
            // The class's Text is its whole data object: its SourceString and LocalizedString ride along.
            g.source_string = source.and_then(|s| w.names.get(&s));
            g.localized_string = localized.and_then(|s| w.names.get(&s));
            pop_text_on(w, f, &g); // @155
        }
        None => warn_once(w, name!("Buff_Educated: no Pop Text in the class data")),
    }
    sfx(w, f, Some(name!("sfx_cha_velma_educated_cast")));
}

/// `EDUCATED.end`: ReceiveEndPlay @2866 -> RemoveBuffVisuals @542: sfx_cha_velma_educated_end
/// at its location @1313. The override never calls the parent's ReceiveEndPlay (endSkipsParent).
pub fn educated_end(w: &mut World, f: FighterId, _b: BuffRef) {
    let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.fighter_mut(f).events.push(sound_at(id, name!("sfx_cha_velma_educated_end"), Some(x), Some(y)));
}

/// `EDUCATED.actor.end`: on a spawned actor the EndPlay's RemoveBuffVisuals @1200-1313 posts
/// sfx_cha_velma_educated_end at the buff's location (the actor's place).
pub fn educated_actor_end(w: &mut World, a: ActorRef, _b: u32) {
    let s = crate::fighter::common::actor_sound_from(w, a);
    // `at.x ?? 0`, `at.y ?? 0`.
    let (x, y) = crate::actors::world_actor::actor_place(w, a).map_or((0.0, 0.0), |(x, y, _)| (x, y));
    let fighter = s.fighter;
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_velma_educated_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: s.pooled,
        key: None,
    });
}

/// Buff_Weakened's UI state (BPI After Status Update Multicast).
fn weakened_status(w: &mut World, f: FighterId, _class: BuffClass, stacks: Real, applied: bool, removed: bool) {
    let op = if removed || stacks <= 0.0 {
        UiStateOp::Remove(name!("Weakened"))
    } else {
        let maxv = pnum(buff_def(w, ActorRef::Fighter(f), name!("Buff_Weakened_C")), "MaxStacks").unwrap_or(1.0);
        let pop = if applied { name!("Weakened") } else { Name::NONE };
        ui_add(name!("Weakened"), 1, 1, stacks, maxv, name!("MI_State_Weak"), true, pop)
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

/// Buff_Weakened's TakenDamageModCondition @0-84 on a fighter: the BuffOwner cast to BP_FighterCharacter holds.
fn weakened_taken(
    _w: &mut World,
    _own: FighterId,
    _b: BuffRef,
    _attacker: Option<ActorRef>,
    _boxed: bool,
    _hit: &HitPacket,
    _by: Option<FighterId>,
) -> bool {
    true
}

/// Buff_Weakened on a spawned actor: its TakenDamageModCondition's BuffOwner cast to BP_FighterCharacter fails.
fn weakened_actor_taken(_w: &World, _a: ActorRef, _b: u32, _f: FighterId) -> bool {
    false
}

// ---- the registration

/// A registry entry with no flags (`buffs({ cls: { ... } })`).
const fn entry(class: BuffClass, hooks: BuffHooks) -> BuffEntry {
    BuffEntry {
        class,
        end_skips_parent: false,
        block_fast_fall: false,
        thru_atk: None,
        active_attack: false,
        damage_hit_by_ally: false,
        hooks,
        actor: None,
    }
}

const NO_ACTOR_HOOKS: ActorBuffHooks = ActorBuffHooks {
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

const NO_HOOKS: BuffHooks = BuffHooks {
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

/// The `Buff_Educated_C` entry (`EDUCATED`), which Rick's child class takes too.
pub const EDUCATED: BuffEntry = BuffEntry {
    end_skips_parent: true,
    actor: Some(ActorBuffHooks { end: Some(educated_actor_end), ..NO_ACTOR_HOOKS }),
    ..entry(BuffClass::BuffEducated, BuffHooks { begin: Some(educated_begin), end: Some(educated_end), ..NO_HOOKS })
};

/// The shared buff classes (`buffs({...})`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    let list = [
        entry(
            C::BuffJumpIncrease,
            BuffHooks { begin: Some(jump_increase_begin), tick: Some(jump_increase_tick), ..NO_HOOKS },
        ),
        entry(C::BuffMoveCompTo, BuffHooks { begin: Some(move_comp_to_begin), ..NO_HOOKS }),
        entry(C::BuffMoveCompToWithStun, BuffHooks { begin: Some(move_comp_to_begin), ..NO_HOOKS }),
        entry(
            C::BuffProjectileImunity,
            BuffHooks { begin: Some(projectile_imunity_begin), end: Some(projectile_imunity_end), ..NO_HOOKS },
        ),
        entry(
            C::BuffThorns,
            BuffHooks { begin: Some(thorns_begin), status: Some(thorns_status), on: Some(thorns_on), ..NO_HOOKS },
        ),
        entry(C::BuffCleanse, BuffHooks { begin: Some(cleanse_begin), tick: Some(cleanse_tick), ..NO_HOOKS }),
        BuffEntry {
            // On a spawned actor every graph of the class stops at a cast to a fighter: an entry that shows and sounds
            // nothing.
            actor: Some(NO_ACTOR_HOOKS),
            ..entry(
                C::BuffIce,
                BuffHooks {
                    begin: Some(ice_begin),
                    stacks: Some(ice_stacks),
                    on: Some(ice_on),
                    end: Some(ice_end),
                    status: Some(ice_status),
                    ..NO_HOOKS
                },
            )
        },
        entry(
            C::BuffTurnEnemyAwayOnHit,
            BuffHooks { begin: Some(turn_enemy_away_begin), on: Some(turn_enemy_away_on), ..NO_HOOKS },
        ),
        entry(C::BuffReverse, BuffHooks { begin: Some(reverse_begin), ..NO_HOOKS }),
        entry(C::BuffCaught, BuffHooks { begin: Some(caught_begin), ..NO_HOOKS }),
        entry(C::BuffCaughtAndRefresh, BuffHooks { begin: Some(caught_begin), ..NO_HOOKS }),
        entry(C::BuffSharpened, BuffHooks { begin: Some(sharpened_begin), ..NO_HOOKS }),
        entry(
            C::BuffFear,
            BuffHooks { begin: Some(fear_begin), on: Some(fear_on), status: Some(fear_status), ..NO_HOOKS },
        ),
        entry(
            C::BuffFearStack,
            BuffHooks {
                begin: Some(fear_stack_begin),
                on: Some(fear_stack_on),
                stacks: Some(fear_stack_stacks),
                end: Some(fear_stack_end),
                ..NO_HOOKS
            },
        ),
        entry(C::BuffPieHeal, BuffHooks { begin: Some(pie_heal_begin), tick: Some(pie_heal_tick), ..NO_HOOKS }),
        // On a spawned actor: the PopText alone (`chars::gizmo::music::silenced_actor_begin`).
        BuffEntry {
            actor: Some(ActorBuffHooks {
                begin: Some(crate::chars::gizmo::music::silenced_actor_begin),
                ..NO_ACTOR_HOOKS
            }),
            ..entry(
                C::C023ABuffSilenced,
                BuffHooks { begin: Some(silenced_begin_hook), end: Some(silenced_end_hook), ..NO_HOOKS },
            )
        },
        EDUCATED,
        // Buff_Weakened: the UI state and the taken condition on a fighter; on a spawned actor no UI state and no damage mod.
        BuffEntry {
            actor: Some(ActorBuffHooks { taken_cond: Some(weakened_actor_taken), ..NO_ACTOR_HOOKS }),
            ..entry(
                C::BuffWeakened,
                BuffHooks { status: Some(weakened_status), taken_cond: Some(weakened_taken), ..NO_HOOKS },
            )
        },
        // Buff_ArmorPack on a spawned actor: the BeginPlay and EndPlay find no Comp_Armor on the actor and do nothing.
        BuffEntry { actor: Some(NO_ACTOR_HOOKS), ..entry(C::BuffArmorPack, NO_HOOKS) },
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}

/// `applyDealtDamageMods` over the typed packet: the dealer's buffs' rewrite of the hit, a
/// copy. A dealer with no buffs keeps the packet.
pub fn apply_dealt_damage_mods_hit(
    w: &mut World,
    own: FighterId,
    victim: ActorRef,
    p: &HitPacket,
    hit_box: bool,
) -> HitPacket {
    apply_dealt_damage_mods(w, own, victim, p, hit_box).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_seconds_at_is_the_running_sum() {
        assert_eq!(time_seconds_at(-5), 0.0);
        let mut t: Real = 0.0;
        for tick in 0..20_000_000i32 {
            if tick < 1_000_000 || tick % 9973 == 0 {
                assert_eq!(time_seconds_at(tick).to_bits(), t.to_bits(), "tick {tick}");
            }
            t += DELTA_SECONDS;
        }
    }

    fn s(v: &str) -> Value {
        Value::Str(v.to_owned())
    }

    #[test]
    fn mod_type_reads_the_enumerator() {
        assert_eq!(mod_type(Some(&s("BuffModType::NewEnumerator3"))), 0.0);
        assert_eq!(mod_type(Some(&s("BuffModType::NewEnumerator2"))), 1.0);
        assert_eq!(mod_type(Some(&s("BuffModType::NewEnumerator1"))), 2.0);
        assert_eq!(mod_type(Some(&s("NewEnumerator0"))), 3.0);
        assert_eq!(mod_type(Some(&s("NewEnumerator10"))), 0.0);
        assert_eq!(mod_type(Some(&Value::Num(2.0))), 2.0);
        assert_eq!(mod_type(None), 0.0);
        assert_eq!(mod_type(Some(&s("DoNothing"))), 0.0);
    }

    #[test]
    fn normalize_adds_the_percent_changes() {
        assert_eq!(normalize_multiplier_floats(1.0, 1.4), 1.4);
        assert_eq!(normalize_multiplier_floats(1.4, 1.4), (((100.0 + size_pct(1.4)) + size_pct(1.4)) / 100.0));
        assert_eq!(normalize_multiplier_floats(0.0, 1.4), 0.0);
        assert_eq!(normalize_multiplier_floats(0.2, 0.2), 0.0);
    }

    #[test]
    fn per_stack_mod_by_type() {
        let add = s("NewEnumerator1");
        // Buff_Ice's SpeedMod 0.85 with SpeedModPerStack -0.03 (Add) at 3 stacks.
        assert_eq!(per_stack_mod(0.85, 0.85, -0.03, Some(&add), 3.0), (0.85 + (3.0 * -0.03)));
        assert_eq!(per_stack_mod(0.85, 0.85, -0.03, None, 3.0), 0.85);
    }

    #[test]
    fn a_stacking_damage_mod_scales_per_stack() {
        let mut taken = Props::new();
        taken.insert("DamageMod".into(), Value::Num(1.0));
        taken.insert("DamageModType".into(), s("NewEnumerator2"));
        let mut per = Props::new();
        per.insert("DamageMod".into(), Value::Num(0.05));
        per.insert("DamageModType".into(), s("NewEnumerator1"));
        let mut d = Props::new();
        d.insert("TakenDamageMods".into(), Value::Obj(taken));
        d.insert("TakenDamageModPerStack".into(), Value::Obj(per));
        d.insert("CanStack".into(), Value::Bool(true));
        // Buff_Weakened at 5 stacks: 1 + 5 x 0.05 = 1.25.
        let b = Def::of(Some(&d));
        let base = b.obj("TakenDamageMods").expect("set above");
        let v = pnum(Some(base), "DamageMod").unwrap_or(0.0) + (5.0 * 0.05);
        assert_eq!(v, 1.25);
    }

    #[test]
    fn dealt_mods_leave_an_unchanged_packet_borrowed() {
        let p = HitPacket { damage: Some(10.0), ..HitPacket::default() };
        let list: [u8; 0] = [];
        let out = dealt_mods_over(&list, |_| None, |_, _| true, &p);
        assert!(matches!(out, Cow::Borrowed(_)));
        // One Add of 2 to the damage and one Multiply of 1.5: pass 1 then pass 2.
        let mul = DamageMod {
            v: [None, None, None, Some(1.5), None, None, None],
            t: [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            ..DamageMod::default()
        };
        let add = DamageMod {
            v: [None, None, None, Some(2.0), None, None, None],
            t: [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0],
            ..DamageMod::default()
        };
        let list = [add, mul];
        let out = dealt_mods_over(&list, |m| Some(m.clone()), |_, _| true, &p);
        assert_eq!(out.damage, Some(15.0 + 2.0));
    }

    #[test]
    fn a_condition_reads_the_packet_borrowed_until_a_field_changed() {
        let p = HitPacket { damage: Some(10.0), ..HitPacket::default() };
        let add = DamageMod {
            v: [None, None, None, Some(2.0), None, None, None],
            t: [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0],
            ..DamageMod::default()
        };
        let list = [add.clone(), add];
        let mut seen = Vec::new();
        let out = dealt_mods_over(
            &list,
            |m| Some(m.clone()),
            |_, view| {
                let v = view.packet();
                seen.push((std::ptr::eq(v, &p), v.damage));
                true
            },
            &p,
        );
        // Passes 1 and 2 see the packet itself (the Adds' sum is added behind pass 2); pass 3 sees the changed copy.
        assert_eq!(out.damage, Some(14.0));
        assert_eq!(seen.len(), 6);
        assert!(seen[..4].iter().all(|&(same, d)| same && d == Some(10.0)));
        assert!(seen[4..].iter().all(|&(same, d)| !same && d == Some(14.0)));
    }

    #[test]
    fn the_or_of_two_numbers() {
        assert_eq!(or_num(Some(0.0), None), None);
        assert_eq!(or_num(None, Some(0.0)), Some(0.0));
        assert_eq!(or_num(Some(4.0), Some(0.25)), Some(4.0));
        assert_eq!(or_num(Some(0.0), Some(0.25)), Some(0.25));
    }
}
