//! The bots' behaviour trees as the build's assets hold them, for the whole roster: the
//! trees, the class defaults of their Blueprint node classes, the blackboard's keys (BotData) and the persona row
//! Default (PersonaDataTable), all property data of the build.
//!
//! The set: the closure of the trees the port's code names. Roots: the trees every bot runs by name and the ones a
//! character's minion runs (BOT_TREE_ROOTS, by key) and every tree a pawn's SetDynamicSubtree calls name
//! (BASE_FIGHTER_INJECTIONS, then each pawn's: BOT_INJECT); then every tree a BTTask_RunBehavior node of a loaded tree
//! runs (BehaviorAsset). The injection map is those calls, by character key, tags lower-cased (an FName match), a later
//! call winning.
//!
//! Shaping (the sim reads these names): a tree and a node class by their object names, a node by its export name
//! (`id`); a node's properties without TreeAsset, ParentNode, Children, Services, CachedDescription,
//! bShowPropertyDetails and NodeName; a reference as its object name, a BlackboardKeySelector as `{ key:
//! SelectedKeyName }`, a GameplayTag as its TagName, a map as `{ Key, Value }` entries, an Enum_InputDirection8Way value
//! as its byte (the enum asset's Names); other enums stay names.

use indexmap::IndexSet;
use mvs_data::fighter::Map;
use mvs_data::tables::{BlackboardKey, BtAuxDef, BtChildDef, BtNodeDef, BtTree};
use mvs_data::{BotTrees, Props, Value};
use mvs_protocol::roster::CHARACTERS;

use super::shape::{EnumNames, defaults_or_properties, enum_names, fail, prop, properties, table_rows};
use crate::kind::{Cx, KindError};

pub const BOT_DATA: &str = "/Game/Panda_Main/Blueprints/AI/BotData.BotData";
pub const PERSONA_TABLE: &str = "/Game/Panda_Main/Blueprints/AI/Persona/PersonaDataTable.PersonaDataTable";
const DIR8_ENUM: &str = "/Game/Panda_Main/Blueprints/Enums/Enum_InputDirection8Way.Enum_InputDirection8Way";
const BEHAVIOR_TREE: &str = "/Script/AIModule.BehaviorTree";
const RUN_BEHAVIOR: &str = "/Script/AIModule.BTTask_RunBehavior";

const SKIP: [&str; 7] =
    ["TreeAsset", "ParentNode", "Children", "Services", "CachedDescription", "bShowPropertyDetails", "NodeName"];

/// The name a build object path ends in: `/Game/A/B.B` -> `B`, `/Game/A/BT.BT:Node_0` -> `Node_0`,
/// `/Script/AIModule.BTComposite_Selector` -> `BTComposite_Selector`.
pub fn leaf_name(path: &str) -> &str {
    let s = &path[path.rfind('.').map_or(0, |i| i + 1)..];
    &s[s.rfind(':').map_or(0, |i| i + 1)..]
}

/// Every tree path the port's code names for a match of `keys`.
pub fn bot_tree_roots(keys: &[&str]) -> Vec<String> {
    let own = |k: &str| -> Vec<String> {
        BOT_TREE_ROOTS.iter().filter(|(r, _)| *r == k).map(|(_, p)| (*p).to_owned()).collect()
    };
    let mut out: IndexSet<String> = own("").into_iter().collect();
    out.extend(BASE_FIGHTER_INJECTIONS.iter().map(|(_, t)| (*t).to_owned()));
    for k in keys {
        out.extend(own(k));
        out.extend(injections(k).iter().map(|(_, t)| (*t).to_owned()));
    }
    out.into_iter().collect()
}

/// A key's own SetDynamicSubtree calls (BOT_INJECT), none for a key without an entry.
fn injections(key: &str) -> &'static [(&'static str, &'static str)] {
    BOT_INJECT.iter().find(|(k, _)| *k == key).map_or(&[], |(_, c)| c)
}

/// The JS `/^\/[A-Za-z0-9_]+\/\S*\.[^/]+$/` (a reference): a mount word between two slashes, then a `.` with no
/// whitespace before it and something without a `/` after it.
fn is_ref(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('/') else { return false };
    let Some(slash) = rest.find('/') else { return false };
    let mount = &rest[..slash];
    if mount.is_empty() || !mount.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return false;
    }
    let tail = &rest[slash + 1..];
    tail.match_indices('.').any(|(j, _)| {
        let after = &tail[j + 1..];
        !tail[..j].chars().any(char::is_whitespace) && !after.is_empty() && !after.contains('/')
    })
}

/// The trees' objects, read through the closure: the trees, their nodes, and the Blueprint
/// node classes (sorted) whose class defaults the table holds.
struct Closure {
    trees: Vec<String>,
    classes: Vec<String>,
}

/// The references a node holds to the nodes below it: its services, its children and their decorators.
fn node_refs(p: &Props) -> Vec<String> {
    let strs = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_arr).unwrap_or_default().iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()
    };
    let mut out = strs(p.get("Services"));
    for c in p.get("Children").and_then(Value::as_arr).unwrap_or_default() {
        let Some(c) = c.as_obj() else { continue };
        if let Some(n) = child_of(c) {
            out.push(n.to_owned());
        }
        out.extend(strs(c.get("Decorators")));
    }
    out
}

/// A child's node: its ChildComposite, else its ChildTask (`??`).
fn child_of(c: &Props) -> Option<&str> {
    match c.get("ChildComposite") {
        Some(Value::Null) | None => c.get("ChildTask").and_then(Value::as_str),
        Some(v) => v.as_str(),
    }
}

fn closure(cx: &Cx<'_>, roots: Vec<String>) -> Result<Closure, KindError> {
    let mut seen: IndexSet<String> = IndexSet::new();
    let mut classes: IndexSet<String> = IndexSet::new();
    let mut trees = roots;
    while !trees.is_empty() {
        let mut next = Vec::new();
        for t in &trees {
            if !seen.insert(t.clone()) {
                continue;
            }
            let bt = cx.object(t)?;
            if bt.class() != BEHAVIOR_TREE {
                return fail(format!("{t} is a {}, not a BehaviorTree", bt.class()));
            }
            let Value::Str(root) = prop(&properties(&bt)?, "RootNode", t)?.clone() else {
                return fail(format!("{t}: RootNode is no reference"));
            };
            let mut level = vec![root];
            while !level.is_empty() {
                let mut below = Vec::new();
                for n in &level {
                    let o = cx.object(n)?;
                    let p = properties(&o)?;
                    if !o.class().starts_with("/Script/") {
                        classes.insert(o.class().to_owned());
                    }
                    if o.class() == RUN_BEHAVIOR
                        && let Some(Value::Str(b)) = p.get("BehaviorAsset")
                    {
                        next.push(b.clone());
                    }
                    below.extend(node_refs(&p));
                }
                level = below;
            }
        }
        trees = next.into_iter().filter(|t| !seen.contains(t)).collect();
    }
    let mut classes: Vec<String> = classes.into_iter().collect();
    classes.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(Closure { trees: seen.into_iter().collect(), classes })
}

/// The shaping of a property value.
fn val(v: &Value, dir8: &EnumNames) -> Result<Value, KindError> {
    Ok(match v {
        Value::Arr(l) => {
            // a map: [key, value] pairs (no array property holds arrays otherwise), as { Key, Value } entries
            if !l.is_empty() && l.iter().all(|e| matches!(e, Value::Arr(p) if p.len() == 2)) {
                let mut out = Vec::new();
                for e in l {
                    let Value::Arr(p) = e else { unreachable!("checked above") };
                    let mut o = Props::new();
                    o.insert("Key".into(), val(&p[0], dir8)?);
                    o.insert("Value".into(), val(&p[1], dir8)?);
                    out.push(Value::Obj(o));
                }
                Value::Arr(out)
            } else {
                Value::Arr(l.iter().map(|x| val(x, dir8)).collect::<Result<_, _>>()?)
            }
        }
        Value::Obj(o) => {
            if let Some(k) = o.get("SelectedKeyName") {
                let mut out = Props::new();
                out.insert("key".into(), k.clone());
                Value::Obj(out)
            } else if let Some(t) = o.get("TagName") {
                t.clone()
            } else {
                Value::Obj(o.iter().map(|(k, x)| Ok((k.clone(), val(x, dir8)?))).collect::<Result<_, KindError>>()?)
            }
        }
        Value::Str(s) if is_ref(s) => Value::Str(leaf_name(s).to_owned()),
        Value::Str(s) if s.starts_with("Enum_InputDirection8Way::") => match dir8.get(s.as_str()) {
            Some(b) => Value::Num(*b),
            None => return fail(format!("{DIR8_ENUM}: no {s}")),
        },
        _ => v.clone(),
    })
}

fn props_of(o: &Props, dir8: &EnumNames, skip: &dyn Fn(&str) -> bool) -> Result<Props, KindError> {
    let mut out = Props::new();
    for (k, v) in o {
        if !skip(k) {
            out.insert(k.clone(), val(v, dir8)?);
        }
    }
    Ok(out)
}

fn skip(k: &str) -> bool {
    SKIP.contains(&k)
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_arr).unwrap_or_default().iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

fn aux(cx: &Cx<'_>, path: &str, dir8: &EnumNames) -> Result<BtAuxDef, KindError> {
    let o = cx.object(path)?;
    Ok(BtAuxDef {
        id: leaf_name(path).to_owned(),
        ty: leaf_name(o.class()).to_owned(),
        props: props_of(&properties(&o)?, dir8, &skip)?,
    })
}

fn node(cx: &Cx<'_>, path: &str, dir8: &EnumNames) -> Result<BtNodeDef, KindError> {
    let o = cx.object(path)?;
    let p = properties(&o)?;
    let a = aux(cx, path, dir8)?;
    let services = strs(p.get("Services"));
    let services = if services.is_empty() {
        None
    } else {
        Some(services.iter().map(|s| aux(cx, s, dir8)).collect::<Result<_, _>>()?)
    };
    let children = match p.get("Children") {
        None | Some(Value::Null) => None,
        Some(Value::Arr(l)) => {
            let mut out = Vec::new();
            for c in l {
                let Some(c) = c.as_obj() else { return fail(format!("{path}: a child is not a struct")) };
                let Some(n) = child_of(c) else { return fail(format!("{path}: a child with no node")) };
                let decs = strs(c.get("Decorators"));
                let decorators = if decs.is_empty() {
                    None
                } else {
                    Some(decs.iter().map(|d| aux(cx, d, dir8)).collect::<Result<_, _>>()?)
                };
                let ops = c.get("DecoratorOps").and_then(Value::as_arr).unwrap_or_default();
                let ops = if ops.is_empty() {
                    None
                } else {
                    let mut out = Vec::new();
                    for x in ops {
                        let x = x.as_obj();
                        let op = x.and_then(|x| x.get("Operation")).and_then(Value::as_str);
                        let num = x.and_then(|x| x.get("Number")).and_then(Value::as_f64);
                        let (Some(op), Some(num)) = (op, num) else {
                            return fail(format!("{path}: a DecoratorOp with no Operation or Number"));
                        };
                        out.push((op.replacen("EBTDecoratorLogic::", "", 1), num));
                    }
                    Some(out)
                };
                out.push(BtChildDef { node: node(cx, n, dir8)?, decorators, ops });
            }
            Some(out)
        }
        Some(_) => return fail(format!("{path}: Children is not a list")),
    };
    Ok(BtNodeDef { id: a.id, ty: a.ty, props: a.props, services, children })
}

/// The bot trees of the whole roster: the trees and the node classes' defaults in code-unit order of their names (the
/// sim only looks them up by name, so the order of the loads does not matter).
pub fn bot_trees(cx: &Cx<'_>) -> Result<BotTrees, KindError> {
    let keys: &[&str] = &CHARACTERS;
    let c = closure(cx, bot_tree_roots(keys))?;
    let dir8 = enum_names(&cx.object(DIR8_ENUM)?)?;

    let mut trees = Map::new();
    for path in &c.trees {
        let name = leaf_name(path).to_owned();
        if trees.contains_key(&name) {
            return fail(format!("two trees named {name}"));
        }
        let p = properties(&cx.object(path)?)?;
        // (a root decorator would gate the whole tree: none in this build's trees, and the runtime has no place for
        // one)
        if p.get("RootDecorators").and_then(Value::as_arr).is_some_and(|l| !l.is_empty()) {
            return fail(format!("{path}: RootDecorators"));
        }
        let Some(root) = prop(&p, "RootNode", path)?.as_str() else {
            return fail(format!("{path}: RootNode is no reference"));
        };
        trees.insert(name, BtTree { root: node(cx, root, &dir8)? });
    }
    trees.sort_unstable_keys();

    // the Blueprint node classes' defaults (`<package>.Default__<class>`)
    let mut defaults = Map::new();
    for cls in &c.classes {
        let pkg = &cls[..cls.rfind('.').unwrap_or(0)];
        let o = cx.object(&format!("{pkg}.Default__{}", leaf_name(cls)))?;
        let d = props_of(&defaults_or_properties(&o)?, &dir8, &|k| skip(k) || k == "UberGraphFrame")?;
        defaults.insert(leaf_name(o.class()).to_owned(), d);
    }
    defaults.sort_unstable_keys();

    // the injection map: BaseFighter's calls, then the pawn's, a later one for a tag winning
    let loaded: IndexSet<&str> = c.trees.iter().map(String::as_str).collect();
    let mut inject = Map::new();
    for key in keys {
        let calls = injections(key);
        if calls.is_empty() {
            continue; // (a character without an entry: its bot fails every injected task)
        }
        let mut map = Map::new();
        for (tag, tree) in BASE_FIGHTER_INJECTIONS.iter().chain(calls) {
            if !loaded.contains(tree) {
                return fail(format!("{key}: injected tree {tree} not loaded"));
            }
            map.insert(tag.to_lowercase(), leaf_name(tree).to_owned());
        }
        inject.insert((*key).to_owned(), map);
    }

    let mut blackboard = Vec::new();
    let bd = properties(&cx.object(BOT_DATA)?)?;
    for k in prop(&bd, "Keys", BOT_DATA)?.as_arr().unwrap_or_default() {
        let Some(k) = k.as_obj() else { return fail(format!("{BOT_DATA}: a key is not a struct")) };
        let name = k.get("EntryName").and_then(Value::as_str).unwrap_or_default().to_owned();
        let Some(ty) = prop(k, "KeyType", BOT_DATA)?.as_str() else {
            return fail(format!("{BOT_DATA}: {name}: KeyType is no reference"));
        };
        let ty = leaf_name(ty);
        let ty = ty.strip_prefix("BlackboardKeyType_").unwrap_or(ty);
        // (the `_\d+$` suffix)
        let ty = match ty.rfind('_') {
            Some(i) if i + 1 < ty.len() && ty[i + 1..].bytes().all(|b| b.is_ascii_digit()) => &ty[..i],
            _ => ty,
        };
        blackboard.push(BlackboardKey { name, ty: ty.to_owned() });
    }

    let rows = table_rows(&cx.object(PERSONA_TABLE)?)?;
    let Some((_, Value::Obj(row))) = rows.iter().find(|(n, _)| n == "Default") else {
        return fail(format!("{PERSONA_TABLE}: no row Default"));
    };
    let mut persona = Map::new();
    for e in prop(row, "ActionMap", PERSONA_TABLE)?.as_arr().unwrap_or_default() {
        let Some([Value::Str(k), Value::Num(v)]) = e.as_arr() else {
            return fail(format!("{PERSONA_TABLE} Default: an ActionMap entry is not [name, number]"));
        };
        persona.insert(k.replacen("PersonaActionEnum::", "", 1), *v);
    }

    Ok(BotTrees { trees, inject, blackboard, persona, defaults })
}

// ---- the roots (the port's code: the bot runner, its injection, the character modules)

/// The behaviour trees the port's code runs by name, by build object path, under the character key whose match needs
/// them ('' for every bot: the Lab's bots and the tutorial's bags, TutorialRecoverOnly_AIController;
/// 'steven': the watermelon's minion tree; 'velma': the police's).
pub const BOT_TREE_ROOTS: [(&str, &str); 6] = [
    ("", concat!("/Game/Panda_Main/Blueprints/AI/", "RecoverOnly_BehaviorTree.RecoverOnly_BehaviorTree")),
    ("", concat!("/Game/Panda_Main/Blueprints/AI/", "Base_BehaviorTree.Base_BehaviorTree")),
    ("", concat!("/Game/Panda_Main/Blueprints/AI/", "Patrolling_BehaviorTree.Patrolling_BehaviorTree")),
    (
        "",
        concat!("/Game/Panda_Main/Blueprints/AI/", "TutorialRecoverOnly_BehaviorTree.TutorialRecoverOnly_BehaviorTree"),
    ),
    (
        "steven",
        "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WatermelonSteven/WatermelonSteven_BehaviorTree.WatermelonSteven_BehaviorTree",
    ),
    ("velma", "/Game/Panda_Main/Characters/VelmaPrototype/AI/VelmaPolice_BehaviorTree.VelmaPolice_BehaviorTree"),
];

/// BaseFighter's SetDynamicSubtree calls (AssignDefaultBehaviors, ExecuteUbergraph_BaseFighter @176643-177435): the
/// trees every pawn's bot runs by tag before its own pawn's calls replace some. [the gameplay tag as the graph writes
/// it, the tree's build object path], in the graph's order. The tag is matched without regard to case (an FName).
pub const BASE_FIGHTER_INJECTIONS: &[(&str, &str)] = &[
    (
        "AI.CustomMoveToTargetLogic",
        "/Game/Panda_Main/Blueprints/AI/MoveToTargetSubtrees/Close_MoveToTargetAI.Close_MoveToTargetAI",
    ), // @176643
    ("AI.CustomRecovery", "/Game/Panda_Main/Blueprints/AI/RecoverySubtrees/DefaultRecovery_AI.DefaultRecovery_AI"), // @176715
    ("AI.CustomMeleeCombo", "/Game/Panda_Main/Blueprints/AI/MeleeComboSubtree.MeleeComboSubtree"), // @176787
    ("AI.CustomDodge", "/Game/Panda_Main/Blueprints/AI/DodgeSubtree.DodgeSubtree"),                // @176859
    ("Ai.CustomHitSafe", "/Game/Panda_Main/Blueprints/AI/HitSafeSubtree.HitSafeSubtree"),          // @176931
    ("Ai.CustomTargetAboveOnPlatform", "/Game/Panda_Main/Blueprints/AI/AboveOnPlatformSubtree.AboveOnPlatformSubtree"), // @177003
    ("Ai.CustomVictory", "/Game/Panda_Main/Blueprints/AI/DefaultVictorySubtree.DefaultVictorySubtree"), // @177075
    ("Ai.CustomAirMelee", "/Game/Panda_Main/Blueprints/AI/AirMeleeSubtree.AirMeleeSubtree"),            // @177147
    ("Ai.CustomDownSpike", "/Game/Panda_Main/Blueprints/AI/DefaultDownSpike.DefaultDownSpike"),         // @177219
    ("Ai.CustomWallSlide", "/Game/Panda_Main/Blueprints/AI/DefaultWallSlide.DefaultWallSlide"),         // @177291
    (
        "Ai.CustomRecoveryNeutralSpecial",
        "/Game/Panda_Main/Blueprints/AI/DefaultRecoveryNeutralSpecial.DefaultRecoveryNeutralSpecial",
    ), // @177363
    ("Ai.CustomRecoveryJumping", "/Game/Panda_Main/Blueprints/AI/DefaultRecoveryJumping.DefaultRecoveryJumping"), // @177435
];

/// Each pawn's SetDynamicSubtree calls (its ReadyForAI / AssignDefaultBehaviors), by character key, in the graph's
/// order: they run after BaseFighter's and a later call for a tag wins. A roster character without an entry has no
/// injection map.
pub const BOT_INJECT: &[(&str, &[(&str, &str)])] = &[
    (
        "arya",
        &[
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Characters/Arya/AI/Arya_MeleeComboSubtree.Arya_MeleeComboSubtree",
            ), // @14560
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/Arya/AI/Arya_StealFaceAI.Arya_StealFaceAI"), // @14632
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/Arya/AI/Arya_SharpenAI.Arya_SharpenAI"), // @14704
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/Arya/AI/Arya_TeleportAI.Arya_TeleportAI"), // @14776
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/Arya/AI/Arya_AirMeleeSubtree.Arya_AirMeleeSubtree"), // @14848
        ],
    ),
    (
        "batman",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_HookshotAI.Batman_HookshotAI",
            ), // @8242
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_BatarangAI.Batman_BatarangAI",
            ), // @8314
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_RecoveryAI.Batman_RecoveryAI",
            ), // @8386
            (
                "AI.CustomMoveToTargetLogic",
                "/Game/Panda_Main/Blueprints/AI/MoveToTargetSubtrees/BL_Batman/Batman_MoveToTargetAI.Batman_MoveToTargetAI",
            ), // @8458
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_SmokebombAI.Batman_SmokebombAI",
            ), // @8530
            (
                "AI.CustomProjectileLedgeGuard",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_BatarangAI.Batman_BatarangAI",
            ), // @8602
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_BombEquipAI.Batman_BombEquipAI",
            ), // @8674
            (
                "AI.CustomSpecialLogic5",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_DownSpecialAI.Batman_DownSpecialAI",
            ), // @8746
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Batman/Batman_MeleeComboTree.Batman_MeleeComboTree",
            ), // @8818
        ],
    ),
    (
        "blackadam",
        &[
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C021/AI/C021_MeleeCombos.C021_MeleeCombos"), // @855
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C021/AI/C021_AirMeele.C021_AirMeele"),         // @927
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/C021/AI/C021_Recovery.C021_Recovery"),         // @999
            (
                "Ai.CustomHandleIncomingProjectile",
                "/Game/Panda_Main/Characters/C021/AI/C021_HandleProjectiles.C021_HandleProjectiles",
            ), // @1071
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C021/AI/C021_LightningGrab.C021_LightningGrab"), // @1143
            (
                "Ai.CustomTargetAboveOnPlatform",
                "/Game/Panda_Main/Characters/C021/AI/C021_AboveOnPlatform.C021_AboveOnPlatform",
            ), // @1215
        ],
    ),
    (
        "bugs",
        &[
            ("AI.CustomMeleeLogic", "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_KissAI.Bugs_KissAI"), // @4373
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_TunnelAI.Bugs_TunnelAI",
            ), // @4445
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_ThrowPieAI.Bugs_ThrowPieAI",
            ), // @4517
            (
                "AI.CustomProjectileLedgeGuard",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_ThrowPieAI.Bugs_ThrowPieAI",
            ), // @4589
            (
                "AI.CustomMoveToTargetLogic",
                "/Game/Panda_Main/Blueprints/AI/MoveToTargetSubtrees/BL_Bugs/Bugs_MoveToTargetAI.Bugs_MoveToTargetAI",
            ), // @4661
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_DropSafeAI.Bugs_DropSafeAI",
            ), // @4733
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_AirMeleeSubtree.Bugs_AirMeleeSubtree",
            ), // @4805
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_MeleeComboSubtreeAI.Bugs_MeleeComboSubtreeAI",
            ), // @4877
            (
                "Ai.CustomHitSafe",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_HitSafeAI.Bugs_HitSafeAI",
            ), // @4949
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_UpRocketAI.Bugs_UpRocketAI",
            ), // @5021
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Bugs/Bugs_RecoveryAI.Bugs_RecoveryAI",
            ), // @5093
        ],
    ),
    (
        "creature",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_FirewallAI.Reindog_FirewallAI",
            ), // @2669
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_CrystalAI.Reindog_CrystalAI",
            ), // @2741
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_ChargedProjectileAI.Reindog_ChargedProjectileAI",
            ), // @2813
            (
                "Ai.CustomPrioritySpecial1",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_TetherAI.Reindog_TetherAI",
            ), // @2885
            (
                "AI.CustomSpecialLogic5",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_AllyItemAI.Reindog_AllyItemAI",
            ), // @2957
            (
                "AI.CustomProjectileLedgeGuard",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_FirewallAI.Reindog_FirewallAI",
            ), // @3029
            (
                "AI.CustomProjectileLedgeGuard2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_ChargedProjectileAI.Reindog_ChargedProjectileAI",
            ), // @3101
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_MeleeComboTree.Reindog_MeleeComboTree",
            ), // @3173
            (
                "Ai.CustomTargetAboveOnPlatform",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_AboveOnPlatformAI.Reindog_AboveOnPlatformAI",
            ), // @3245
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_RecoveryAI.Reindog_RecoveryAI",
            ), // @3317
            (
                "Ai.CustomVictory",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/Reindog_VictoryAI.Reindog_VictoryAI",
            ), // @3389
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Reindog/ReindogAirMeleeSubtree.ReindogAirMeleeSubtree",
            ), // @3461
        ],
    ),
    (
        "finn",
        &[
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/Finn/AI/Finn_MeleeComboTree.Finn_MeleeComboTree"), // @8065
            (
                "AI.CustomMoveToTargetLogic",
                "/Game/Panda_Main/Characters/Finn/AI/Finn_MoveToTargetAI.Finn_MoveToTargetAI",
            ), // @8137
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/Finn/AI/Finn_BagShopAI.Finn_BagShopAI"), // @8209
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/Finn/AI/Finn_RushAttackAI.Finn_RushAttackAI"), // @8281
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/Finn/AI/Finn_HighFiveAI.Finn_HighFiveAI"), // @8353
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/Finn/AI/Finn_AirMeleeSubtree.Finn_AirMeleeSubtree"), // @8425
            (
                "Ai.CustomTargetAboveOnPlatform",
                "/Game/Panda_Main/Characters/Finn/AI/Finn_AboveOnPlatformAI.Finn_AboveOnPlatformAI",
            ), // @8497
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/Finn/AI/Finn_RecoveryAI.Finn_RecoveryAI"), // @8569
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Characters/Finn/AI/Finn_DashToTargetingGemAI.Finn_DashToTargetingGemAI",
            ), // @8641
            ("AI.CustomSpecialLogic5", "/Game/Panda_Main/Characters/Finn/AI/Finn_BagBaitAI.Finn_BagBaitAI"), // @8713
        ],
    ),
    (
        "garnet",
        &[
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Characters/Garnet/AI/Garnet_ComboSubtreeAI.Garnet_ComboSubtreeAI",
            ), // @20132
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Characters/Garnet/AI/Garnet_AirMeleeSubtreeAI.Garnet_AirMeleeSubtreeAI",
            ), // @20204
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/Garnet/AI/Garnet_RecoveryAI.Garnet_RecoveryAI"), // @20276
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/Garnet/AI/Garnet_GauntletAI.Garnet_GauntletAI"), // @20348
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/Garnet/AI/Garnet_ElectricAI.Garnet_ElectricAI"), // @20420
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Characters/Garnet/AI/Garnet_HeartMarkerAI.Garnet_HeartMarkerAI",
            ), // @20492
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/Garnet/AI/Garnet_SingAI.Garnet_SingAI"), // @20564
            (
                "Ai.CustomRecoveryNeutralSpecial",
                "/Game/Panda_Main/Characters/Garnet/AI/Garnet_RecoveryNeutralSpecialAI.Garnet_RecoveryNeutralSpecialAI",
            ), // @20636
        ],
    ),
    (
        "gizmo",
        &[
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C023A/AI/C023A_MeleeCombo.C023A_MeleeCombo"), // @3205
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C023A/AI/C023A_AirMelee.C023A_AirMelee"),       // @3277
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/C023A/AI/C023A_RecoveryAI.C023A_RecoveryAI"),   // @3349
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C023A/AI/C023A_CarDashAI.C023A_CarDashAI"), // @3421
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/C023A/AI/C023A_Sing.C023A_Sing"),          // @3493
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/C023A/AI/C023A_Arrow.C023A_Arrow"),        // @3565
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/C023A/AI/C023A_Popcorn.C023A_Popcorn"),    // @3637
        ],
    ),
    (
        "harley",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_JackInBoxAI.Harley_JackInBoxAI",
            ), // @1833
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_BatBombAI.Harley_BatBombAI"), // @1905
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_MeleeComboTree.Harley_MeleeComboTree",
            ), // @1977
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Characters/HarleyQuinn/AI/HarleyAirMeleeSubtree.HarleyAirMeleeSubtree",
            ), // @2049
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_UpShotAI.Harley_UpShotAI"), // @2121
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_DashShootAI.Harley_DashShootAI",
            ), // @2193
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_RecoveryAI.Harley_RecoveryAI"), // @2265
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Characters/HarleyQuinn/AI/Harley_GroundSlideAI.Harley_GroundSlideAI",
            ), // @2337
        ],
    ),
    (
        "irongiant",
        &[
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/C017/AI/C017_RecoveryAI.C017_RecoveryAI"), // @14545
            ("Ai.CustomWallSlide", "/Game/Panda_Main/Characters/C017/AI/C017_WallSide.C017_WallSide"),    // @14617
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C017/AI/C017_MeleeCombos.C017_MeleeCombos"), // @14689
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C017/AI/C017_AirMeleeSubtree.C017_AirMeleeSubtree"), // @14761
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C017/AI/C017_Throw.C017_Throw"), // @14833
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/C017/AI/C017_PlantArt.C017_PlantArt"), // @14905
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/C017/AI/C017_Transform.C017_Transform"), // @14977
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/C017/AI/C017_Cannonball.C017_Cannonball"), // @15049
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/C017/AI/C017_Grab.C017_Grab"),  // @15121
            ("AI.CustomSpecialLogic5", "/Game/Panda_Main/Characters/C017/AI/C017_Bolt.C017_Bolt"),  // @15193
            (
                "Ai.CustomRecoveryJumping",
                "/Game/Panda_Main/Characters/C017/AI/C017_RecoveryJumping.C017_RecoveryJumping",
            ), // @15265
        ],
    ),
    (
        "jake",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_StretchPunchAI.Jake_StretchPunchAI",
            ), // @3093
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_HorseDashAI.Jake_HorseDashAI",
            ), // @3165
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_DropShapeshiftAI.Jake_DropShapeshiftAI",
            ), // @3237
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_Skateboard3HitAI.Jake_Skateboard3HitAI",
            ), // @3309
            (
                "AI.CustomSpecialLogic5",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_EatAI.Jake_EatAI",
            ), // @3381
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/RecoverySubtrees/BL_Jake/Jake_RecoveryAI.Jake_RecoveryAI",
            ), // @3453
            (
                "Ai.CustomHandleIncomingProjectile",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_RepelProjectileAI.Jake_RepelProjectileAI",
            ), // @3525
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_MeleeComboTree.Jake_MeleeComboTree",
            ), // @3597
            (
                "Ai.CustomTargetAboveOnPlatform",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_AboveOnPlatformAI.Jake_AboveOnPlatformAI",
            ), // @3669
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Jake/Jake_AirMeleeSubtree.Jake_AirMeleeSubtree",
            ), // @3741
        ],
    ),
    (
        "lebron",
        &[
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/c016/AI/C016_Recovery.C016_Recovery"), // @3231
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/c016/AI/C016_MeleeCombos.C016_MeleeCombos"), // @3303
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/c016/AI/C016_AirMeleeSubtree.C016_AirMeleeSubtree"), // @3375
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/c016/AI/C016_Ball.C016_Ball"), // @3447
        ],
    ),
    (
        "marvin",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Characters/C018/AI/C018_SideSpecialCombos.C018_SideSpecialCombos",
            ), // @4426
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C018/AI/C018_MeleeCombos.C018_MeleeCombos"), // @4498
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Characters/C018/AI/C018_ReverseProjectiles.C018_ReverseProjectiles",
            ), // @4570
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/C018/AI/C018_Recovery.C018_Recovery"),         // @4642
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C018/AI/C018_AirMeleeCombos.C018_AirMeleeCombos"), // @4714
        ],
    ),
    (
        "morty",
        &[
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C019/AI/C019_AirMeleeSubtree.C019_AirMeleeSubtree"), // @1730
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Characters/C019/AI/C019_MeleeComboSubtree.C019_MeleeComboSubtree",
            ), // @1802
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C019/AI/C019_RushAttackAI.C019_RushAttackAI"), // @1874
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/C019/AI/C019_CallUFO.C019_CallUFO"), // @1946
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/C019/AI/C019_GrenadeAI.C019_GrenadeAI"), // @2018
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/C019/AI/C019_RewindAI.C019_RewindAI"), // @2090
            (
                "Ai.CustomHandleIncomingProjectile",
                "/Game/Panda_Main/Characters/C019/AI/C019_IncomingProjectileAI.C019_IncomingProjectileAI",
            ), // @2162
        ],
    ),
    (
        "rick",
        &[
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C020/AI/C020_ShootAI.C020_ShootAI"), // @1744
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/C020/AI/C020_Meeseek.C020_Meeseek"), // @1816
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/C020/AI/C020_PortalAI.C020_PortalAI"), // @1888
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C020/AI/C020_MeleeComboTree.C020_MeleeComboTree"), // @1960
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C020/AI/C020_AirMeleeSubtree.C020_AirMeleeSubtree"), // @2032
            ("AI.CustomRecovery", "/Game/Panda_Main/Characters/C020/AI/C020_RecoveryAI.C020_RecoveryAI"), // @2104
        ],
    ),
    (
        "shaggy",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_RageAI.Shaggy_RageAI",
            ), // @3494
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_MeleeComboTree.Shaggy_MeleeComboTree",
            ), // @3566
            (
                "AI.CustomMoveToTargetLogic",
                "/Game/Panda_Main/Blueprints/AI/MoveToTargetSubtrees/BL_Shaggy/Shaggy_MoveToTargetAI.Shaggy_MoveToTargetAI",
            ), // @3638
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_SandwichAI.Shaggy_SandwichAI",
            ), // @3710
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_UppercutAI.Shaggy_UppercutAI",
            ), // @3782
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_RecoveryAI.Shaggy_RecoveryAI",
            ), // @3854
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_AirMeleeSubtree.Shaggy_AirMeleeSubtree",
            ), // @3926
            (
                "Ai.CustomDownSpike",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_DownSpike.Shaggy_DownSpike",
            ), // @3998
            (
                "Ai.CustomOffPlatformSpecial1",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_RageBaitAI.Shaggy_RageBaitAI",
            ), // @4070
            (
                "Ai.CustomRecoveryNeutralSpecial",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Shaggy/Shaggy_RecoveryNeutralSpecialAI.Shaggy_RecoveryNeutralSpecialAI",
            ), // @4142
        ],
    ),
    (
        "steven",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_BarrierThrow.Steven_BarrierThrow",
            ), // @9504
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_Shield.Steven_Shield",
            ), // @9576
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_SummonWatermelon.Steven_SummonWatermelon",
            ), // @9648
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/StevenRecovery_AI.StevenRecovery_AI",
            ), // @9720
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_TetherThrow.Steven_TetherThrow",
            ), // @9792
            (
                "AI.CustomSpecialLogic5",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_AirShieldDown.Steven_AirShieldDown",
            ), // @9864
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_AirMeleeSubtree.Steven_AirMeleeSubtree",
            ), // @9936
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Steven/Steven_MeleeComboSubtree.Steven_MeleeComboSubtree",
            ), // @10008
        ],
    ),
    (
        "stripe",
        &[
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C023B/AI/C023B_AirMelee.C023B_AirMelee"), // @827
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C023B/AI/C023B_MeleeCombo.C023B_MeleeCombo"), // @899
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C023B/AI/C023B_Saw.C023B_Saw"),       // @971
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/C023B/AI/C023B_Skateboard.C023B_Skateboard"), // @1043
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/C023B/AI/C023B_AttachToEnemy.C023B_AttachToEnemy"), // @1115
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/C023B/AI/C023B_Chainsaw.C023B_Chainsaw"), // @1187
            ("AI.CustomSpecialLogic5", "/Game/Panda_Main/Characters/C023B/AI/C023B_Bomb.C023B_Bomb"),         // @1259
            ("Ai.CustomSpecialLogic6", "/Game/Panda_Main/Characters/C023B/AI/C023B_Gun.C023B_Gun"),           // @1331
        ],
    ),
    (
        "superman",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/Superman_FlyGrabAI.Superman_FlyGrabAI",
            ), // @2706
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/Superman_AirLaserAI.Superman_AirLaserAI",
            ), // @2778
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/SupermanRecovery_AI.SupermanRecovery_AI",
            ), // @2850
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/Superman_FreezeAI.Superman_FreezeAI",
            ), // @2922
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/Superman_SuperpunchAI.Superman_SuperpunchAI",
            ), // @2994
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/Superman_LeapAttackAI.Superman_LeapAttackAI",
            ), // @3066
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_Superman/Superman_MeleeComboTree.Superman_MeleeComboTree",
            ), // @3138
        ],
    ),
    (
        "taz",
        &[
            ("AI.CustomMeleeCombo", "/Game/Panda_Main/Characters/C015/AI/C015_MeleeCombo_AI.C015_MeleeCombo_AI"), // @18720
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/C015/AI/C015_Sandwich_AI.C015_Sandwich_AI"), // @18792
            ("Ai.CustomAirMelee", "/Game/Panda_Main/Characters/C015/AI/C015_AirMeleeSubtree.C015_AirMeleeSubtree"), // @18864
        ],
    ),
    (
        "tomandjerry",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_TomAndJerry/TomAndJerry_PlaceTrap.TomAndJerry_PlaceTrap",
            ), // @6814
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_TomAndJerry/TomAndJerry_Rocket.TomAndJerry_Rocket",
            ), // @6886
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_TomAndJerry/TomAndJerry_SlingShot_Tennis.TomAndJerry_SlingShot_Tennis",
            ), // @6958
            (
                "Ai.CustomSpecialLogic6",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_TomAndJerry/TomAndJerry_Paddleball.TomAndJerry_Paddleball",
            ), // @7030
            (
                "None",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_TomAndJerry/TomAndJerry_DynomateTennis_AI.TomAndJerry_DynomateTennis_AI",
            ), // @7102
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_TomAndJerry/TomAndJerry_MeleeComboTree.TomAndJerry_MeleeComboTree",
            ), // @7174
        ],
    ),
    (
        "velma",
        &[
            ("AI.CustomSpecialLogic", "/Game/Panda_Main/Characters/Velma/AI/Velma_VialAI.Velma_VialAI"), // @7325
            ("AI.CustomSpecialLogic2", "/Game/Panda_Main/Characters/Velma/AI/Velma_ThrowAI.Velma_ThrowAI"), // @7397
            ("AI.CustomSpecialLogic3", "/Game/Panda_Main/Characters/Velma/AI/Velma_BeamAI.Velma_BeamAI"), // @7469
            ("AI.CustomSpecialLogic4", "/Game/Panda_Main/Characters/Velma/AI/Velma_CarryAI.Velma_CarryAI"), // @7541
            ("AI.CustomSpecialLogic5", "/Game/Panda_Main/Characters/Velma/AI/Velma_SassAI.Velma_SassAI"), // @7613
            ("AI.CustomMeleeLogic", "/Game/Panda_Main/Characters/Velma/AI/Velma_MeleeComboTree.Velma_MeleeComboTree"), // @7685
            (
                "Ai.CustomSpecialLogic6",
                "/Game/Panda_Main/Characters/Velma/AI/Velma_ProjectileFromVialAI.Velma_ProjectileFromVialAI",
            ), // @7757
        ],
    ),
    (
        "wonderwoman",
        &[
            (
                "AI.CustomSpecialLogic",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_LassoAI.WonderWoman_LassoAI",
            ), // @3629
            (
                "AI.CustomSpecialLogic2",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_SideShieldAI.WonderWoman_SideShieldAI",
            ), // @3701
            (
                "AI.CustomSpecialLogic3",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_UpShieldAI.WonderWoman_UpShieldAI",
            ), // @3773
            (
                "AI.CustomSpecialLogic4",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_BuffAI.WonderWoman_BuffAI",
            ), // @3845
            (
                "AI.CustomSpecialLogic5",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_ShieldAI.WonderWoman_ShieldAI",
            ), // @3917
            (
                "Ai.CustomAirMelee",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_AirMeleeSubtree.WonderWoman_AirMeleeSubtree",
            ), // @3989
            (
                "AI.CustomMeleeCombo",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_MeleeComboTree.WonderWoman_MeleeComboTree",
            ), // @4061
            (
                "Ai.CustomHandleIncomingProjectile",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_HandleProjectileAI.WonderWoman_HandleProjectileAI",
            ), // @4133
            (
                "AI.CustomRecovery",
                "/Game/Panda_Main/Blueprints/AI/CharacterSubtrees/BL_WonderWoman/WonderWoman_Recovery_AI.WonderWoman_Recovery_AI",
            ), // @4205
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::{is_ref, leaf_name};

    #[test]
    fn names_and_refs() {
        assert_eq!(leaf_name("/Game/A/B.B"), "B");
        assert_eq!(leaf_name("/Game/A/BT.BT:Node_0"), "Node_0");
        assert_eq!(leaf_name("/Script/AIModule.BTComposite_Selector"), "BTComposite_Selector");
        assert!(is_ref("/Game/A/B.B"));
        assert!(is_ref("/Script/AIModule.BTComposite_Selector"));
        assert!(is_ref("/Game/A/B.B:C D"));
        assert!(is_ref("/Game/A/B.c."));
        assert!(!is_ref("/Game/A B/C.C"));
        assert!(!is_ref("/Game/A/B."));
        assert!(!is_ref("/Game/A/B.C/D"));
        assert!(!is_ref("Game/A/B.B"));
        assert!(!is_ref("//A.B"));
    }
}
