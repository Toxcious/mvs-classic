//! The perks (`perks/`): Comp_Pawn_PerkManager's Add, the perk tables' lookups, the
//! general perks' own buff classes (the perk hooks and the classes) and what the tables reach.
//!
//! The tables are the bound data's: the general one is `GameData::perks`, a character's part its
//! `CharData::perks`. A fighter's part is looked up through its character key, so a buff's def comes from the parts
//! of the match's own fighters alone.

mod conds;
mod dodge;
mod elemental_projectile;
mod hooks;
mod ice_projectile;
mod marks;
mod misc;
pub mod reach;

pub use conds::has_debuff;
pub use hooks::{PerkDealer, armor_pack, check_for_blast_box_bounce, projectile_dealer};

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs::{Def, FighterBuff, inst_view, spawn_buff};
use crate::combat::{BuffClass, BuffExt, BuffRef};
use crate::fighter::FighterId;
use crate::fighter::common::{lit, peers, warn_text};
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use mvs_data::fighter::ActorDef;
use mvs_data::tables::{PerkEntry, PerkTable};
use mvs_data::value::{Props, Value};

// ---- the classes' own variables (the `ext` of the entry)

/// What a perk buff class keeps on its entry (`BuffExt::Perk`), a variant per class that has one.
#[derive(State, Copy, Debug)]
pub enum PerkExt {
    /// `Buff_InMemoriam_C` `MemoriamState`: Active and the EndActive timer's tick (-1: none).
    Memoriam { active: bool, at: Real },
    /// `Buff_Perk_ArmorOnDeath_C` `ArmorDeathState`: 0 the first Delay, 1 the armor's, 2 done.
    ArmorDeath { phase: u8, left: Real, applied: bool },
    /// `Buff_DodgeArmor_C` / `Buff_DodgeReflect_C` `DirState`: Direction and its Delay's ticks left.
    Dir { dir: Real, left: Real },
    /// `Buff_OnKnockbackProjectileBlock_C` `BlockState`.
    Block { stacks: Real, timer: Real, delay: Real },
    /// `Buff_LastStand_C` `LastStandState`: the Instigator's fighter id (-1: none).
    LastStand { by: i32, activated: bool },
    /// `Buff_Perk_JumpCount_C` `JumpCountState`: TripleJumpOn, MoreThan1Stack.
    JumpCount { on: bool, more: bool },
    /// `Buff_CollateralDamage_C`: its Instigator.
    Collateral { by: Option<FighterId> },
    /// `Buff_CollateralDamageTracker_C` `TrackerState`.
    Tracker { dmg: Real, buffer: bool, left: Real, by: Option<FighterId> },
    /// `Buff_DetonateFromMelee_C` `DetonateState`: DetonateOnHit.
    Detonate { latch: bool },
    /// `Buff_ProjectileApplyBuffAfterGrounded_C` `GroundState`: SecondsRemaining, IsCharged.
    Grounded { left: Real, charged: bool },
}

impl PerkExt {
    /// The extension a class's begin sets, at its begin's values (the trace loader's new entry); `None` for a
    /// class with none.
    pub fn for_class(c: BuffClass) -> Option<PerkExt> {
        use BuffClass as C;
        Some(match c {
            C::BuffInMemoriam => PerkExt::Memoriam { active: false, at: -1.0 },
            C::BuffPerkArmorOnDeath => PerkExt::ArmorDeath { phase: 2, left: 0.0, applied: false },
            C::BuffDodgeArmor | C::BuffDodgeReflect => PerkExt::Dir { dir: 5.0, left: 0.0 },
            C::BuffOnKnockbackProjectileBlock => PerkExt::Block { stacks: 0.0, timer: 0.0, delay: 0.0 },
            C::BuffLastStand => PerkExt::LastStand { by: -1, activated: false },
            C::BuffPerkJumpCount => PerkExt::JumpCount { on: false, more: false },
            C::BuffCollateralDamage => PerkExt::Collateral { by: None },
            C::BuffCollateralDamageTracker => PerkExt::Tracker { dmg: 0.0, buffer: false, left: 0.0, by: None },
            C::BuffDetonateFromMelee => PerkExt::Detonate { latch: false },
            C::BuffProjectileApplyBuffAfterGrounded => PerkExt::Grounded { left: 0.0, charged: false },
            _ => return None,
        })
    }
}

/// What a perk buff class keeps on an actor's buff (`ElemExt::Perk`).
#[derive(State, Copy, PartialEq, Debug)]
pub enum PerkActorExt {
    /// `Buff_ProjectileGrayHealth_C` `GrayState`.
    Gray { bound: bool, active: bool },
    /// `Buff_Elemental_Electric_C` `ElectricActorState`: the Instigator's fighter id and Team Index (-1: none).
    Electric { by: i32, team: i32 },
}

// ---- the entries' reads the classes share

/// The live entry `b` (not the entry itself): `None` once it left the list.
pub(crate) fn entry(w: &World, b: BuffRef) -> Option<&FighterBuff> {
    w.fighter(b.fighter).buffs.buffs.iter().find(|x| x.serial == b.serial)
}

pub(crate) fn entry_mut(w: &mut World, b: BuffRef) -> Option<&mut FighterBuff> {
    w.fighter_mut(b.fighter).buffs.buffs.iter_mut().find(|x| x.serial == b.serial)
}

/// `b.stacks` (0 for an entry gone).
pub(crate) fn stacks(w: &World, b: BuffRef) -> Real {
    entry(w, b).map_or(0.0, |x| x.stacks)
}

/// The class's own variables, where its begin set them.
pub(crate) fn ext(w: &World, b: BuffRef) -> Option<&PerkExt> {
    match &entry(w, b)?.ext {
        BuffExt::Perk(e) => Some(e),
        _ => None,
    }
}

pub(crate) fn ext_mut(w: &mut World, b: BuffRef) -> Option<&mut PerkExt> {
    match &mut entry_mut(w, b)?.ext {
        BuffExt::Perk(e) => Some(e),
        _ => None,
    }
}

pub(crate) fn set_ext(w: &mut World, b: BuffRef, e: PerkExt) {
    if let Some(x) = entry_mut(w, b) {
        x.ext = BuffExt::Perk(e);
    }
}

/// `instDef(f, b)`: the entry's values over its class's defaults (none for an entry gone).
pub(crate) fn def(w: &World, b: BuffRef) -> Def<'_> {
    inst_view(w, b)
}

/// `instDef(f, b)` of an entry `b` of `f`'s list, read from the entry itself.
pub(crate) fn values_of<'a>(w: &'a World, f: FighterId, b: &'a FighterBuff) -> Def<'a> {
    Def {
        base: crate::fighter::common::buff_def(w, ActorRef::Fighter(f), b.cls),
        own: b.own.as_ref().map(|o| &o.values),
    }
}

/// What a class's values are read through: a buff's values (`Def`) or an object of the data.
pub trait Vals: Copy {
    fn val(&self, k: &str) -> Option<&Value>;
}

impl Vals for Def<'_> {
    fn val(&self, k: &str) -> Option<&Value> {
        self.get(k)
    }
}

impl Vals for Option<&Props> {
    fn val(&self, k: &str) -> Option<&Value> {
        self.and_then(|d| d.get(k))
    }
}

/// `d.<k> ?? 0`.
pub(crate) fn num(d: impl Vals, k: &str) -> Real {
    crate::math::narrow(d.val(k).and_then(Value::as_f64).unwrap_or(0.0))
}

/// `!!d.<k>`.
pub(crate) fn flag(d: impl Vals, k: &str) -> bool {
    match d.val(k) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// The fighter of an id among `peers`.
pub(crate) fn peer_by_id(w: &World, id: i32) -> Option<FighterId> {
    if id < 0 {
        return None;
    }
    peers(w).iter().copied().find(|&o| w.fighter(o).id == id)
}

// ---- the perk data

/// A character's part of the perk tables (`byData.get(own.data)`): the part its character's data carries.
fn part_of(w: &World, f: FighterId) -> Option<&PerkTable> {
    let ch = w.fighter(f).ch;
    w.data.chars.get(w.names.str(ch))?.perks.as_ref()
}

/// `generalPerks`: the general table, bound with the game data.
pub fn general_perks(w: &World) -> Option<&PerkTable> {
    Some(&w.data.perks)
}

/// `perkActorDef`: an actor class of the general table's "actors".
pub fn perk_actor_def<'a>(w: &'a World, cls: &str) -> Option<&'a ActorDef> {
    let a = w.data.perks.actors.as_ref().and_then(|m| m.get(cls));
    if a.is_none() {
        warn_text(w, &format!("{cls}: not in the general perk table actors"));
    }
    a
}

/// `perkPartActor`: an actor class of a character part's "actors", the owner's part
/// first, else the first of its peers' whose part has it. No warning.
pub fn perk_part_actor<'a>(
    w: &'a World,
    own: FighterId,
    peers: Option<&[FighterId]>,
    cls: &str,
) -> Option<&'a ActorDef> {
    let of = |f: FighterId| part_of(w, f).and_then(|t| t.actors.as_ref()).and_then(|m| m.get(cls));
    let a = of(own);
    if a.is_some() {
        return a;
    }
    peers?.iter().find_map(|&o| of(o))
}

/// `perkInfo`: a PerkData asset's entry from the general table, else a loaded character's
/// part. Here the loaded characters are the match's data.
pub fn perk_info<'a>(w: &'a World, id: &str) -> Option<&'a PerkEntry> {
    let p = w
        .data
        .perks
        .perks
        .get(id)
        .or_else(|| w.data.chars.values().find_map(|c| c.perks.as_ref().and_then(|t| t.perks.get(id))));
    if p.is_none() {
        warn_text(w, &format!("perk {id}: in no loaded perk table (data/perks/)"));
    }
    p
}

/// `matchPerkInfo`: the general table's, else the owner's part, else the first of the
/// match's fighters (`peers`) whose part has it. An id none of them has warns and gives `None`.
pub fn match_perk_info<'a>(w: &'a World, own: FighterId, peers: &[FighterId], id: &str) -> Option<&'a PerkEntry> {
    let p = w
        .data
        .perks
        .perks
        .get(id)
        .or_else(|| part_of(w, own).and_then(|t| t.perks.get(id)))
        .or_else(|| peers.iter().find_map(|&o| part_of(w, o).and_then(|t| t.perks.get(id))));
    if p.is_none() {
        warn_text(
            w,
            &format!("perk {id}: in neither the general table nor the match's characters' parts (data/perks/)"),
        );
    }
    p
}

/// `perkBuffDef`: a buff class's defaults from the perk tables: the general table's,
/// else the owner's part, else the first of `peers` whose part has it.
pub fn perk_buff_def<'a>(w: &'a World, own: FighterId, peers: Option<&[FighterId]>, cls: &str) -> Option<&'a Props> {
    let g = w.data.perks.buffs.get(cls).or_else(|| part_of(w, own).and_then(|t| t.buffs.get(cls)));
    if g.is_some() {
        return g;
    }
    peers?.iter().find_map(|&o| part_of(w, o).and_then(|t| t.buffs.get(cls)))
}

/// `perkBuffClasses`: the classes `perk_buff_def` finds for them (the cheat menu's list).
pub fn perk_buff_classes<'a>(w: &'a World, own: FighterId, peers: Option<&[FighterId]>) -> Vec<&'a str> {
    let mut out: Vec<&str> = w.data.perks.buffs.keys().map(String::as_str).collect();
    let rest = std::iter::once(own).chain(peers.unwrap_or(&[]).iter().copied());
    for o in rest {
        if let Some(t) = part_of(w, o) {
            out.extend(t.buffs.keys().map(String::as_str));
        }
    }
    out
}

// ---- the perks

/// `addPerks`: Comp_Pawn_PerkManager Add, run at the pawn's first possession
/// (BaseFighter @136510 / @141183 -> PerkManager.Initialize: Remove Perks of an empty list, then Add) and at each
/// reset (ResetFighterLogic @859 -> PerkManager.ResetFighter -> Add, with no removal: every perk buff died in the
/// defeat tail, DestroyBuffWhenOwnerDefeated).
/// - Own perks in array order: each valid perk's AppliedBuffs through SpawnBuffActor with stacks 1, duration 0 (no
///   life timer), BreakIfBlocked true, the owner its own instigator (Comp_Pawn_PerkManager:327 / 92-115). An id
///   outside the tables warns and is skipped, as Add skips a perk that is not valid. No slot, category or level
///   test.
/// - Then every other player of the owner's team in player order (:137-205): each of its valid perks with
///   SpawnOnAllies gives its buffs to the owner the same way, the instigator that player's pawn when it has one,
///   else the owner. This recipient-driven pass is the teammate grant (SpawnBuffActor's own ally spread stays off).
///
/// `first`: the match's first spawn (World.reset), where the pawns are made in player order and a team-mate made
/// later has no pawn yet when this one is possessed.
// Unverified: possession is taken as following the pawn's own spawn, before the next player's pawn is made.
pub fn add_perks(w: &mut World, f: FighterId, first: bool) {
    let (fid, team) = (w.fighter(f).id, w.fighter(f).team.team);
    for id in perks_of(w, fid) {
        give_perk(w, f, id, f, false);
    }
    // `for (const o of w.fighters)`: by place, as a perk's begin may write the World.
    for i in 0..w.players.len() {
        let o = w.players[i];
        let (oid, oteam) = (w.fighter(o).id, w.fighter(o).team.team);
        if o == f || oteam != team {
            continue;
        }
        let inst = if first && oid > fid { f } else { o };
        for id in perks_of(w, oid) {
            give_perk(w, f, id, inst, true);
        }
    }
}

/// The perks of set-up player `player` given to a mode's pawn that stands for it (the port's own Waves mode: its
/// player's pawn), as its possession's Comp_Pawn_PerkManager.Initialize -> Add gives a player's pawn its own: no ally
/// takes a share (the mode's player has none).
pub fn add_player_perks(w: &mut World, f: FighterId, player: usize) {
    let ids = w.fixed.perks.get(player).cloned().unwrap_or_default();
    for id in ids {
        give_perk(w, f, id, f, false);
    }
}

/// `w.perks[id] ?? []`.
fn perks_of(w: &World, id: i32) -> Vec<Name> {
    usize::try_from(id).ok().and_then(|i| w.fixed.perks.get(i)).cloned().unwrap_or_default()
}

/// `givePerk`: the peers are the match's players (`w.fighters`).
fn give_perk(w: &mut World, f: FighterId, id: Name, instigator: FighterId, ally: bool) {
    let Some(p) = match_perk_info(w, f, &w.players, w.names.str(id)) else { return };
    if ally && !p.spawn_on_allies {
        return;
    }
    // A class the bind did not intern is in no table: Name::NONE, which spawn_buff refuses.
    let classes: Vec<Name> = p.buffs.iter().map(|c| lit(w, c)).collect();
    for cls in classes {
        spawn_buff(w, f, cls, 0.0, Some(1), Some(instigator), true, None);
    }
}

// ---- the registry's entries

/// A `BUFFS` entry of `class` with these hooks and no flags.
pub(crate) const fn kind(class: BuffClass, hooks: BuffHooks, actor: Option<ActorBuffHooks>) -> BuffEntry {
    BuffEntry {
        class,
        end_skips_parent: false,
        block_fast_fall: false,
        thru_atk: None,
        active_attack: false,
        damage_hit_by_ally: false,
        hooks,
        actor,
    }
}

/// Enters `es` in `BUFFS` under their classes' names.
pub(crate) fn enter(r: &mut Registry, es: &[BuffEntry]) -> Result<(), RegistryError> {
    for e in es {
        let n = e.class.registered_name().expect("a registered buff class");
        r.buffs.enter(n, *e)?;
    }
    Ok(())
}

/// The perks' entries, (the perk hooks, then the classes).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    hooks::register(r)?;
    conds::register(r)?;
    marks::register(r)?;
    dodge::register(r)?;
    misc::register(r)?;
    elemental_projectile::register(r)?;
    ice_projectile::register(r)?;
    Ok(())
}
