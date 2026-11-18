//! The perk tables: every PerkData of the build's perk folder by
//! asset name, with its AppliedBuffs classes in order and SpawnOnAllies, and the class defaults of every buff class
//! those lead to, merged over their Blueprint parents'. The classes those lead to: the reach tables below (what a
//! class's graph spawns or names, and the class default members that name more). The general table holds the perks
//! without an AssociatedCharacter and the defs they reach; a character's part (its signature perks and the defs only
//! they reach) is bound with that character's data.
//!
//! The set: the build finds its PerkData by class through its asset registry (GetAssetsByClass); here the listing of
//! the perk folder and its subfolders filtered by that class.

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use mvs_data::fighter::{ActorDef, Map};
use mvs_data::tables::PerkEntry;
use mvs_data::{PerkTable, Props, Value};

use super::actor_class::{ActorClass, ActorClasses, actor_passive_buffs};
use super::reach::{Reach, c};
use super::shape::{
    EnumTable, cdo_path, class_defaults, defaults_or_properties, enums_named, fail, load_enum_table, properties,
    short_name,
};
use crate::fighter_port::{character_data, character_data_of};
use crate::kind::{Cx, KindError};
use crate::objview::ObjView;

pub const PERK_DATA_FOLDER: &str = "/Game/Panda_Main/Blueprints/Perks/PerkData";
pub const PERK_DATA_CLASS: &str = "/Script/MultiVersus.PerkData";

/// The UserDefinedEnum assets whose values the perk tables' class defaults hold: a value `Enum_X::NewEnumerator<n>` is
/// the byte the asset's Names give it (n is the order of creation, not the value).
pub const PERK_ENUMS: [(&str, &str); 6] = [
    ("Enum_AttackDecayType", "/Game/Panda_Main/Blueprints/Enums/Enum_AttackDecayType"),
    ("Enum_ExclusiveDamageTypes", "/Game/Panda_Main/Blueprints/Enums/Enum_ExclusiveDamageTypes"),
    ("Enum_HitReaction", "/Game/Panda_Main/Blueprints/Enums/Enum_HitReaction"),
    ("Enum_InputButton", "/Game/Panda_Main/Blueprints/Enums/Enum_InputButton"),
    ("Enum_InputDirection8Way", "/Game/Panda_Main/Blueprints/Enums/Enum_InputDirection8Way"),
    ("Enum_ProjectileType", "/Game/Panda_Main/Blueprints/Enums/Enum_ProjectileType"),
];

// ---- reach

/// The class default objects reached, by class path (the build's case).
type Cdos<'a> = IndexMap<String, ObjView<'a>>;

/// The buff classes a class's defaults name (DEFAULTS_REACH), in member order.
fn defaults_reach(cls: &str, cdo: &ObjView<'_>) -> Result<Vec<String>, KindError> {
    let members = DEFAULTS_REACH.get_any_case(cls);
    if members.is_empty() {
        return Ok(Vec::new());
    }
    let d = Value::Obj(defaults_or_properties(cdo)?);
    let mut out = Vec::new();
    for m in members {
        let mut v = Some(&d);
        for k in m.split('.') {
            v = match v {
                Some(Value::Obj(o)) => o.get(k),
                _ => None,
            };
        }
        match v {
            Some(Value::Arr(l)) => out.extend(l.iter().filter_map(|x| x.as_str().map(str::to_owned))),
            Some(Value::Str(s)) => out.push(s.clone()),
            _ => {}
        }
    }
    Ok(out)
}

/// `roots` and every class they lead to, in order of reaching (a queue: each class's graph's classes, then its
/// defaults'), each once (by path without regard to case: the build's names). Every
/// class's class default object is loaded into `cdos` (one the build lacks is an error).
fn reach_of<'a>(cx: &Cx<'a>, roots: &[String], cdos: &mut Cdos<'a>) -> Result<Vec<String>, KindError> {
    let mut order: Vec<String> = Vec::new();
    let mut seen: IndexSet<String> = IndexSet::new();
    let mut add = |c: &str, order: &mut Vec<String>| {
        if seen.insert(c.to_lowercase()) {
            order.push(c.to_owned());
        }
    };
    for r in roots {
        add(r, &mut order);
    }
    let mut i = 0;
    while i < order.len() {
        let cls = order[i].clone();
        if !cdos.contains_key(&cls) {
            cdos.insert(cls.clone(), cx.object(&cdo_path(&cls))?);
        }
        for b in GRAPH_REACH.get_any_case(&cls) {
            add(b, &mut order);
        }
        for b in defaults_reach(&cls, &cdos[&cls])? {
            add(&b, &mut order);
        }
        i += 1;
    }
    Ok(order)
}

/// The enum assets the class defaults of `objs` name.
fn enums_of<'o, 'a: 'o>(cx: &Cx<'_>, objs: impl Iterator<Item = &'o ObjView<'a>>) -> Result<EnumTable, KindError> {
    let mut names = Vec::new();
    for o in objs {
        enums_named(&Value::Obj(defaults_or_properties(o)?), &mut names);
    }
    load_enum_table(cx, &names, &PERK_ENUMS, "the perk tables' PERK_ENUMS")
}

// ---- the PerkData set

/// One PerkData of the set: its AppliedBuffs as class paths.
#[derive(Clone, Debug)]
struct PerkDataInfo {
    name: String,
    buffs: Vec<String>,
    spawn_on_allies: bool,
}

/// The set, bound once per run.
struct PerkSet {
    general: Arc<PerkTable>,
    /// The general table's classes, by path in lower case: a part leaves them out.
    general_classes: IndexSet<String>,
    /// Per AssociatedCharacter (`EFighter::X`), its perks in the set's order.
    by_fighter: IndexMap<String, Vec<PerkDataInfo>>,
}

/// The PerkData of the set, in the tables' order: by package path, ordinal (the order numbers the tables' perk
/// entries), not EnabledForDebugging (a debugging asset is off in the shipping build: HelperFunctionLibrary
/// IsAssetEnabledInCurrentHydraEnvironment? @124-198).
fn perk_datas(cx: &Cx<'_>) -> Result<Vec<(PerkDataInfo, Option<String>)>, KindError> {
    let mut paths = cx.catalog.list_packages(PERK_DATA_FOLDER, true, Some(PERK_DATA_CLASS));
    paths.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let mut out = Vec::new();
    for path in paths {
        let o = cx.object(&path)?;
        if o.class() != PERK_DATA_CLASS {
            continue;
        }
        let p = properties(&o)?;
        if p.get("EnabledForDebugging") == Some(&Value::Bool(true)) {
            continue;
        }
        let buffs = match p.get("AppliedBuffs") {
            None => Vec::new(),
            Some(Value::Arr(l)) => match l.iter().map(|b| b.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>() {
                Some(b) => b,
                None => return fail(format!("{path}: AppliedBuffs is not a list of classes")),
            },
            Some(_) => return fail(format!("{path}: AppliedBuffs is not a list of classes")),
        };
        // (SpawnOnAllies: absent is the C++ constructor's true)
        let spawn_on_allies = p.get("SpawnOnAllies") != Some(&Value::Bool(false));
        let info = PerkDataInfo { name: short_name(o.path()).to_owned(), buffs, spawn_on_allies };
        let fighter = match p.get("AssociatedCharacter") {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        };
        out.push((info, fighter));
    }
    Ok(out)
}

fn entry_of(i: &PerkDataInfo) -> PerkEntry {
    PerkEntry { buffs: i.buffs.iter().map(|b| short_name(b).to_owned()).collect(), spawn_on_allies: i.spawn_on_allies }
}

/// The class defaults of `classes` by class name (a later class of the same name keeps the first's place).
fn buffs_of(classes: &[String], cdos: &Cdos<'_>, enums: &EnumTable) -> Result<Map<Props>, KindError> {
    let mut out = Map::new();
    for c in classes {
        out.insert(short_name(c).to_owned(), class_defaults(&cdos[c], enums)?);
    }
    Ok(out)
}

fn bind_set(cx: &Cx<'_>) -> Result<PerkSet, KindError> {
    let all = perk_datas(cx)?;
    let mut by_fighter: IndexMap<String, Vec<PerkDataInfo>> = IndexMap::new();
    let mut perks = Map::new();
    let mut roots = Vec::new();
    for (info, fighter) in all {
        match fighter {
            None => {
                roots.extend(info.buffs.iter().cloned());
                perks.insert(info.name.clone(), entry_of(&info));
            }
            Some(f) => by_fighter.entry(f).or_default().push(info),
        }
    }
    let mut cdos = Cdos::new();
    let classes = reach_of(cx, &roots, &mut cdos)?;
    let mut actor_cdos = Vec::new();
    for a in GENERAL_ACTORS {
        actor_cdos.push(((*a).to_owned(), cx.object(&cdo_path(a))?));
    }
    let enums = enums_of(cx, cdos.values().chain(actor_cdos.iter().map(|(_, o)| o)))?;
    let buffs = buffs_of(&classes, &cdos, &enums)?;
    let mut actors = Map::new();
    for (a, o) in &actor_cdos {
        let p = class_defaults(o, &enums)?;
        actors.insert(short_name(a).to_owned(), ActorDef { p, components: Props::new(), supers: None, ifaces: None });
    }
    Ok(PerkSet {
        general: Arc::new(PerkTable { perks, buffs, actors: Some(actors) }),
        general_classes: classes.iter().map(|c| c.to_lowercase()).collect(),
        by_fighter,
    })
}

/// The run's set, bound once.
struct Set(Result<Arc<PerkSet>, String>);

fn perk_set(cx: &Cx<'_>) -> Result<Arc<PerkSet>, KindError> {
    let s = cx.shared.get_or_init(|| Set(bind_set(cx).map(Arc::new).map_err(|e| e.to_string())));
    s.0.clone().map_err(KindError::Failed)
}

/// The general perk table: the perks without an AssociatedCharacter and the defs they reach, bound
/// once per run.
pub fn general(cx: &Cx<'_>) -> Result<Arc<PerkTable>, KindError> {
    Ok(perk_set(cx)?.general.clone())
}

// ---- a character's part

/// A roster character's own perk part: its signature perks (the PerkData whose
/// AssociatedCharacter is its CharacterData's), the defs only they reach (not the general table's), and the actors
/// their buffs spawn (PART_ACTORS) with the buffs those actors' passive buff managers name and what they reach.
pub fn char_perks(cx: &Cx<'_>, key: &str) -> Result<PerkTable, KindError> {
    let set = perk_set(cx)?;
    let Some(cd) = character_data_of(cx, character_data(key))? else {
        return fail(format!("perks {key}: no CharacterData"));
    };
    let Some(Value::Str(ac)) = properties(&cd)?.get("AssociatedCharacter").cloned() else {
        return fail(format!("{}: no AssociatedCharacter", cd.path()));
    };
    let own = set.by_fighter.get(&ac).map_or(&[][..], Vec::as_slice);
    let roots: Vec<String> = own.iter().flat_map(|i| i.buffs.iter().cloned()).collect();
    let mut cdos = Cdos::new();
    let not_general = |c: &str| !set.general_classes.contains(&c.to_lowercase());
    let mut classes: Vec<String> = reach_of(cx, &roots, &mut cdos)?.into_iter().filter(|c| not_general(c)).collect();
    let mut actors = Map::new();
    let mut bound: IndexMap<String, Arc<ActorClass>> = IndexMap::new();
    let actor_classes = ActorClasses::new(*cx, &format!("perks {key}"));
    for b in &classes {
        for a in PART_ACTORS.get_any_case(b) {
            let n = short_name(a).to_owned();
            if bound.contains_key(&n) {
                continue;
            }
            let c = actor_classes.actor(a)?;
            actors.insert(n.clone(), c.def.clone());
            bound.insert(n, c);
        }
    }
    // (the buffs those actors bring: their passive buff managers'; their own graphs spawn none)
    let more: Vec<String> = bound.values().flat_map(|a| actor_passive_buffs(a)).collect();
    if !more.is_empty() {
        let mut in_part: IndexSet<String> = classes.iter().map(|c| c.to_lowercase()).collect();
        for c in reach_of(cx, &more, &mut cdos)? {
            if not_general(&c) && in_part.insert(c.to_lowercase()) {
                classes.push(c);
            }
        }
    }
    let enums = enums_of(cx, cdos.values())?;
    let perks = own.iter().map(|i| (i.name.clone(), entry_of(i))).collect();
    let buffs = buffs_of(&classes, &cdos, &enums)?;
    Ok(PerkTable { perks, buffs, actors: (!actors.is_empty()).then_some(actors) })
}

// ---- the reach tables

/// By class: the classes its graphs (its own and its Blueprint parents') spawn or name as a class literal, in the
/// order the tables reach them. "spawn": a SpawnBuffActor's class; "names": a literal that only names the class (a
/// CheckIfHasBuff, a cast, a struct's member type), which the tables take in all the same, so the sim finds its def.
/// "the struct": an empty Struct_ElementalBuffs (pfgDoDamage's / HitBoxesOn's argument) whose ElementalBuffs member
/// is typed TSubclassOf<Buff_ElementalParent>.
pub const GRAPH_REACH: Reach = Reach::of(&[
    (c!("Blueprints/Perks/Buffs/Arya", "Buff_Arya_Trophy"), &[c!("Blueprints/Abilities/Shield", "Buff_Disguise")]), // spawn @2274
    (
        c!("Blueprints/Perks/Buffs/C015", "Buff_C015_TeamFightCloud"),
        &[c!("Blueprints/Buffs/BL_C015", "Buff_C015AttachToDogpile")],
    ), // spawn @8593
    (
        c!("Blueprints/Perks/Buffs/C016", "Buff_C016_DistanceShot"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent")],
    ), // the struct @699
    // (no graph of its own names it: BasketBallProjectile @7215 CheckHasBuff(Buff_C016_HotHands), @7404 / @7531 picks
    // Buff_IgnitedBasketballPass for its owner's pass)
    (
        c!("Blueprints/Perks/Buffs/C016", "Buff_C016_HotHands"),
        &[c!("Characters/c016/Blueprints", "Buff_IgnitedBasketballPass")],
    ),
    (
        c!("Blueprints/Perks/Buffs/C018", "Buff_C018_Redirect"),
        &[c!("Characters/C018/Blueprints", "Buff_C018_PerkProjectileSpeed")],
    ), // spawn @561
    (
        c!("Blueprints/Perks/Buffs/C020", "Buff_C020_Meeseeks"),
        &[c!("Characters/C020/Blueprints", "Buff_MeeseeksDamageWithHitstun")],
    ), // spawn @195
    // spawn @745, @1275, @1745
    (
        c!("Blueprints/Perks/Buffs/C020", "Buff_C020_Portal"),
        &[
            c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire"),
            c!("Blueprints/Abilities/Shield", "Buff_FireGauntlets"),
        ],
    ),
    (
        c!("Blueprints/Perks/Buffs/C021", "Buff_C021_Shocked"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric")],
    ), // names @79 (BPI_CheckIfHasBuff)
    (
        c!("Blueprints/Perks/Buffs/C023A", "Buff_C023A_Backpack"),
        &[c!("Characters/C023A/BluePrints", "C023A_Buff_Perk_ChargeSpeed")],
    ), // spawn @913
    (
        c!("Blueprints/Perks/Buffs/C023A", "Buff_C023A_Dance"),
        &[c!("Characters/C023A/BluePrints", "C023A_DanceCount_Debuff")],
    ), // @677 into AppliedBuffs
    (
        c!("Blueprints/Perks/Buffs/C023B", "Buff_C023B_Leap"),
        &[c!("Characters/C023B/BluePrints", "C023B_DashProjectileBlock")],
    ), // names @736 (a cast)
    (
        c!("Blueprints/Perks/Buffs/C023B", "Buff_C023B_PerkAttach"),
        &[c!("Blueprints/Abilities/Shield", "Buff_TransferDebuffs")],
    ), // spawn @195
    (
        c!("Blueprints/Perks/Buffs/Bruiser", "Buff_CollateralDamageTracker"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent")],
    ), // the struct @332
    (
        c!("Blueprints/Perks/Buffs/Bruiser", "Buff_CollateralDamage"),
        &[c!("Blueprints/Perks/Buffs/Bruiser", "Buff_CollateralDamageTracker")],
    ), // spawn @473
    (
        c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_NoArmorDamage"),
        ],
    ), // the struct @1408 (MakeStruct), names @1388
    (
        c!("Blueprints/Perks/Buffs/Ranged", "Buff_DetonateFromMelee"),
        &[c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire")],
    ), // spawn @176
    (
        c!("Blueprints/Perks/Buffs/Tank", "Buff_DodgeReflect"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent")],
    ), // the struct @1115
    // spawn @1325 / @2723, spawn @2508; names @1082 / @1728
    (
        c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Electric"),
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Electric"),
            c!("Blueprints/Buffs/Elemental", "Buff_Shield_Electric"),
        ],
    ),
    // spawn @1647 / @9062, spawn @6784; names @2606, @5843, @9551 / @744
    (
        c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire"),
        &[
            c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire"),
            c!("Blueprints/Abilities/Shield", "Buff_Ice"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Ice"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fireproof"),
        ],
    ),
    // spawn @2245 / @8761, spawn @7118; names @3720, @6173, @8974 / @507
    (
        c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Ice"),
        &[
            c!("Blueprints/Abilities/Shield", "Buff_Ice"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Ice"),
            c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Iceproof"),
        ],
    ),
    (
        c!("Blueprints/Abilities/Shield", "Buff_EvidenceCount"),
        &[c!("Characters/Velma/BluePrints", "Buff_PoliceEvidence")],
    ), // spawn @1473
    (
        c!("Blueprints/Abilities/DamageOverTime", "Buff_Fire"),
        &[
            c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent"),
            c!("Blueprints/Buffs/Elemental", "Buff_Elemental_NoArmorDamage"),
        ],
    ), // the struct @2471 / @2527, names @2451
    // spawn @458 (names @152), names @914
    (
        c!("Blueprints/Perks/Buffs/Garnet", "Buff_Garnet_ElectricGroove"),
        &[
            c!("Characters/Garnet/Blueprints", "Buff_GarnetStackingSinging"),
            c!("Blueprints/Buffs/Elemental", "Buff_Damage_Electric"),
        ],
    ),
    (c!("Blueprints/Perks/Buffs/Ranged", "Buff_IceProjectile"), &[c!("Blueprints/Abilities/Shield", "Buff_Ice")]), // names @143 (BPI_CheckIfHasBuff)
    (
        c!("Characters/c016/Blueprints", "Buff_IgnitedBasketballPass"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fire")],
    ), // spawn @551 / @635
    (
        c!("Blueprints/Perks/Buffs/Jake/FloatyBounce", "Buff_Jake_FloatyBounce"),
        &[c!("Blueprints/Abilities/Shield", "Buff_Float")],
    ), // spawn @15
    (
        c!("Blueprints/Perks/Buffs/Jake/SafetyNet", "Buff_Jake_SafetyNet"),
        &[c!("Blueprints/Abilities/Shield", "Buff_AirSuperiority")],
    ), // spawn @542
    (
        c!("Blueprints/Perks/Buffs/Jake/WeaponMaster", "Buff_Jake_WeaponMaster"),
        &[c!("Blueprints/Abilities/Shield/BL_Jake", "Buff_Jake_WeaponMasterAlly")],
    ), // spawn @2504
    (
        c!("Blueprints/Perks/Buffs/Bruiser", "Buff_LastStand"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent")],
    ), // the struct, in OnHitByEnemy's hit info @303
    (
        c!("Blueprints/Perks/Buffs/Tank", "Buff_OnKnockbackProjectileBlock"),
        &[c!("Blueprints/Abilities/Shield", "Buff_ProjectileBlock")],
    ), // spawn @106
    (
        c!("Blueprints/Perks/Buffs/WonderWoman", "Buff_Perk_WonderWoman_FastFallSlam"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent")],
    ), // the struct @1196 (HitBoxesOn)
    (
        c!("Blueprints/Perks/Buffs/Ranged", "Buff_ProjectileApplyBuffAfterGrounded"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Electric")],
    ), // spawn @366
    (
        c!("Blueprints/Buffs/Elemental", "Buff_Shield_Electric"),
        &[c!("Blueprints/Buffs/Elemental", "Buff_ElementalParent")],
    ), // the struct @1904
    (c!("Blueprints/Perks/Buffs/Velma", "Buff_Studied"), &[c!("Blueprints/Abilities/Shield", "Buff_EvidenceCount")]), // names @495, @706 (a cast)
    (
        c!("Blueprints/Perks/Buffs/Superman/IceBreaker", "Buff_Superman_IceBreaker"),
        &[c!("Blueprints/Abilities/Shield", "Buff_Ice")],
    ), // names @224
    (
        c!("Characters/C023A/BluePrints", "C023A_DanceCount_Debuff"),
        &[c!("Characters/C023A/BluePrints", "C023A_ForceDance_Debuff")],
    ), // spawn @161
]);

/// By class: the members of its class defaults (a dotted path into a struct) that hold buff classes, which the
/// tables take after its graph's (the build's own values, read at load). StateInfo.BreakBuffs: the buffs whose
/// arrival ends this one (BuffParentActor's state rules).
pub const DEFAULTS_REACH: Reach = Reach::of(&[
    (c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Fireproof"), &["StateInfo.BreakBuffs"]),
    (c!("Blueprints/Buffs/Elemental", "Buff_Elemental_Iceproof"), &["StateInfo.BreakBuffs"]),
    // Buff_SupermanAllyIceGauntlet's graph spawns BuffToApplyEnemy on the enemy its holder hits (its child's value)
    (c!("Blueprints/Abilities/Shield", "Buff_FireGauntlets"), &["BuffToApplyEnemy"]),
    // its parent's graph spawns Buff (Buff_DetonateFromMelee) on a melee hit
    (c!("Blueprints/Perks/Buffs/Ranged", "Buff_ProjectileToMeleeFire"), &["Buff"]),
    (c!("Characters/C023A/BluePrints", "C023A_ForceDance_Debuff"), &["StateInfo.BreakBuffs"]),
]);

/// Actors a character perk's buff spawns by its own graph that the character's data does not hold, by the buff
/// class: their whole class data goes into the part that buff lands in, and the buffs
/// their Comp_Pawn_PassiveBuffManager names join that part's buffs.
pub const PART_ACTORS: Reach = Reach::of(&[
    // Buff_Superman_FireSlam SpawnFireWall @387 CompSpawnActor(FireWallHazard_C)
    (
        c!("Blueprints/Perks/Buffs/Superman/FireSlam", "Buff_Superman_FireSlam"),
        &[c!("Blueprints/Projectiles", "FireWallHazard")],
    ),
    // Buff_Perk_WonderWoman_ProjectileBlock OnEvadeTriggered @948 CompSpawnActor(WW_EvadeProjectileBlock_C)
    (
        c!("Blueprints/Perks/Buffs/WonderWoman", "Buff_Perk_WonderWoman_ProjectileBlock"),
        &[c!("Blueprints/Projectiles", "WW_EvadeProjectileBlock")],
    ),
    // Buff_C017_ScorchedEarth SpawnFirewall @1840 CompSpawnActor(FireWallHazard_C)
    (c!("Blueprints/Perks/Buffs/C017", "Buff_C017_ScorchedEarth"), &[c!("Blueprints/Projectiles", "FireWallHazard")]),
    // Buff_Creature_FireStorm @553 sets a Firebolt's ActorClassSpawnedAtDeath = FireStormHazard_C
    (
        c!("Blueprints/Perks/Buffs/Creature/Firestorm", "BUff_Creature_Firestorm", "Buff_Creature_FireStorm_C"),
        &[c!("Blueprints/Projectiles", "FireStormHazard")],
    ),
    // Buff_Creature_WreckingBall spawns Creature_WreckingBall_C
    (
        c!("Blueprints/Perks/Buffs/Creature/WreckingBall", "Buff_Creature_WreckingBall"),
        &[c!("Blueprints/Projectiles", "Creature_WreckingBall")],
    ),
    // Buff_TunnelTeleport's Shockwave branch @1708 CompSpawnActor(TunnelShockwave_C) under Buff_BugsBunny_Shockwave
    (
        c!("Blueprints/Perks/Buffs/BugsBunny/Shockwave", "Buff_BugsBunny_Shockwave"),
        &[c!("Blueprints/Projectiles/BugsBunny", "TunnelShockwave")],
    ),
    // Buff_C015_EatProjectiles' anvil
    (c!("Blueprints/Perks/Buffs/C015", "Buff_C015_EatProjectiles"), &[c!("Characters/C015/Blueprints", "C015Anvil")]),
]);

///  Actors a general perk's graph spawns that no character's data holds: their class defaults go into the general
///  table's "actors". BlastBoxPlayerExit CheckForBlastBoxBounce (Buff_KnockOutBounce's).
pub const GENERAL_ACTORS: &[&str] = &[c!("Blueprints/Projectiles", "BlastBoxExitKnockbackExplosion")];

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use mvs_data::value::{decode_entry, encode};
    use mvs_pak::{BuildPaths, pak_files_in, setup};
    use mvs_store::Store;
    use mvs_uasset::PackageCache;
    use mvs_uasset::package::FileSource;

    use super::char_perks;
    use crate::catalog::scan_catalog;
    use crate::kind::{Cx, Shared};
    use crate::reads::StoreReads;

    fn env(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")))
    }

    /// Each `Char/<k>` entry's `perks` member of the oracle's store (`MVS_GAMEDATA_ORACLE`, the store's folder)
    /// against `char_perks` over the developer's paks (`MVS_PAKS`), byte for byte as encoded. (The Char entries are
    /// not built yet; once they are, the gamedata compare covers this.)
    #[test]
    #[ignore = "needs the paks and the oracle store"]
    fn char_perks_equal_oracle() {
        let dir = env("MVS_PAKS");
        let key = setup::key_from_exe(&setup::exe_beside_paks(&dir)).unwrap();
        let set = Arc::new(mvs_pak::PakSet::open(&pak_files_in(&dir).unwrap(), &key).unwrap());
        let paths = Arc::new(BuildPaths::from_paks(&set).unwrap());
        let cache = PackageCache::new(set.clone() as Arc<dyn FileSource>, paths.clone(), 1 << 30);
        let (catalog, _) = scan_catalog(&set, &paths, &|_| {});
        let shared = Shared::default();
        let reads = StoreReads::none();
        let cx = Cx { paks: &set, paths: &paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };
        let oracle = Store::open(env("MVS_GAMEDATA_ORACLE")).unwrap().kind("gamedata").unwrap();
        let mut bad = Vec::new();
        let mut n = 0;
        for p in oracle.paths().filter(|p| p.starts_with("/GameData/Char/")) {
            let k = &p["/GameData/Char/".len()..p.rfind('.').unwrap()];
            let v = decode_entry(&oracle.bytes(p).unwrap()).unwrap();
            // (a data-only key has no part: its entry's perks is null)
            let Some(want) = v.as_obj().and_then(|o| o.get("perks")).filter(|v| !v.is_null()) else { continue };
            let want = Some(encode(want).unwrap());
            let got = match char_perks(&cx, k) {
                Ok(t) => Some(encode(&t).unwrap()),
                Err(e) => {
                    bad.push(format!("{k}: {e}"));
                    continue;
                }
            };
            n += 1;
            if want != got {
                bad.push(format!("{k}: differs"));
            }
        }
        eprintln!("{n} parts, {} differ", bad.len());
        for b in &bad {
            eprintln!("{b}");
        }
        assert!(bad.is_empty());
    }
}
