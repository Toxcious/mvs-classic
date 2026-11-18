//! A character's definition made from its fighter data, with charDef's writes
//! into that data, and what the definition keeps beside it (the maps altIdleDelay,
//! crouchRunOf, edgeIdleOf, runRateLandRun, knockBackShapes and defaultTerminal).

use std::collections::HashMap;

use mvs_data::def::{
    AbilityDef, AnimSet, AnimSetBlend, CharDef, CrouchRun, DefExtra, EdgeIdle, HoldOverride, Hover, NavSet, SetExtra,
    Sfx,
};
use mvs_data::fighter::{AttackEntry, CompositeSegment, Map, SlotGraphNode};
use mvs_data::value::from_value;
use mvs_data::{FighterData, Props, Value};

use super::fighter::bag::{at, some, str_of, truthy};
use crate::kind::KindError;

/// The walking capsule over its floor contact (memory: z 210.412 on top 18 +
/// half 190).
const FLOOR_REST_GAP: f64 = 2.412;

/// A failed TriggerAbilityCheck's sound, the class defaults' "SFX
/// Cooldown 0..2".
const SFX_ABILITY_COOLDOWN: &str = "ui_ability_cooldown";

/// Literals of a pawn class's own event graph (not in the CDO, so not in the data file), by character key: the crouch's
/// EvadeInvulnerabilityRechargeTime. Pawn_C019 ubergraph 1434-1457 / 1476-1503.
const PAWN_GRAPH_CROUCH_EVADE_RECHARGE: &[(&str, f64)] = &[("morty", 0.5)];

/// Clips named by a character AnimBP's own anim graph nodes (not in the base AnimBP child's CDO, so not in the data
/// file's animSet), by character key. C017_AnimBP's state machine HoverState (its BakedStateMachines[1]): HoverIn =
/// BlendSpacePlayer over C017_Hover_In_BlendSpace (bLoop false), HoverLoop = BlendSpacePlayer_1 over
/// C017_Hover_BlendSpace (bLoop true); both BlendSpace1D on LeanAngle -10..10 with the samples _F (-10), the plain clip
/// (0) and _B (10), X = HoverFallLean. The entries are the samples at 0.
/// Not ported: the lean, so the F / B samples are never shown.
const ANIM_GRAPH_HOVER: &[(&str, (&str, &str))] = &[("irongiant", ("C017_Hover_In", "C017_Hover"))];

/// The walking rest height above the capsule's floor contact, read in the game per character (standing actor Z less
/// the floor top and the capsule half height): Shaggy 210.412 = 18 + 190 + 2.412, Morty 200.426 = 18 + 180 + 2.426
/// (a game-memory capture). With Morty's value his fast knockback landings equal the capture.
/// Unverified: the rule behind the difference (FindFloor / AdjustFloorHeight; the two points fit
/// 2.15 + 0.00354 x the capsule radius) is not decoded, so another character takes Shaggy's until it is read.
const REST_GAP: &[(&str, f64)] = &[("shaggy", 2.412), ("morty", 2.426)];

/// The keys whose pawn hooks walk the AnimBP's slot graph (`sim/chars/*` CharHooks.slotInput): a machine that is a node
/// of that graph is run by the walk with its own entry, restart and first-update flags.
const SLOT_INPUT_KEYS: &[&str] =
    &["finn", "garnet", "gizmo", "jake", "marvin", "rick", "stripe", "taz", "tomandjerry", "velma"];

fn find<T: Copy>(t: &[(&str, T)], key: &str) -> Option<T> {
    t.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn bad(name: &str, what: impl std::fmt::Display) -> KindError {
    KindError::Failed(format!("{name}: {what}"))
}

/// A string value as `x as string` reads one (None: undefined or not a string).
fn opt_str(v: Option<&Value>) -> Option<String> {
    str_of(some(v)).map(str::to_owned)
}

fn need_num(name: &str, what: &str, v: Option<&Value>) -> Result<f64, KindError> {
    some(v).and_then(Value::as_f64).ok_or_else(|| bad(name, format!("{what} is not a number")))
}

fn num_or(v: Option<&Value>, d: f64) -> f64 {
    some(v).and_then(Value::as_f64).unwrap_or(d)
}

fn bool_or(v: Option<&Value>, d: bool) -> bool {
    some(v).and_then(Value::as_bool).unwrap_or(d)
}

// ---- the sub meshes

const MESH_ROLE_TAGS: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];

fn tags_of(p: &Value) -> Option<Vec<String>> {
    match some(at(p, "ComponentTags")) {
        Some(Value::Arr(l)) => Some(l.iter().map(|t| str_of(Some(t)).unwrap_or_default().to_owned()).collect()),
        _ => None,
    }
}

fn sub_mesh_tag(tags: &[String]) -> Option<String> {
    tags.iter().find(|t| !MESH_ROLE_TAGS.contains(&t.as_str())).map(|t| t.to_lowercase())
}

/// Per skin SubMeshes entry: the Meshes key of the pawn's component it is applied to (its map key) and whether that
/// component has the PrimaryMesh tag; an entry whose key no component is filed under has
/// no tag.
fn skin_sub_meshes(d: &FighterData) -> Vec<(Option<String>, bool)> {
    let mut filed: HashMap<String, bool> = HashMap::new();
    for set in [&d.components.base, &d.components.pawn] {
        for c in set.values() {
            let Some(tags) = tags_of(&c.p) else { continue };
            if let Some(key) = sub_mesh_tag(&tags) {
                filed.insert(key, tags.iter().any(|t| t == "PrimaryMesh"));
            }
        }
    }
    d.skin
        .sub_meshes
        .iter()
        .map(|m| {
            let tag = m.key.to_lowercase();
            match filed.get(&tag) {
                Some(p) => (Some(tag), *p),
                None => (None, false),
            }
        })
        .collect()
}

/// The pawn's StaticMeshComponents and its SkeletalMeshComponents with a mesh of their
/// own but CharacterMesh0 (skeletalSubMeshes), each by Meshes key and whether it has the PrimaryMesh tag.
/// Unverified: the data file has no attach parents: every tagged mesh component of the pawn's SCS is taken to be under
/// the main mesh.
fn component_sub_meshes(d: &FighterData) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for skeletal in [false, true] {
        for (k, c) in &d.components.pawn {
            let take = if skeletal {
                c.ty == "SkeletalMeshComponent" && truthy(at(&c.p, "SkeletalMesh")) && k != "CharacterMesh0"
            } else {
                c.ty == "StaticMeshComponent" && truthy(Some(&c.p))
            };
            if !take {
                continue;
            }
            let tags = tags_of(&c.p).unwrap_or_default();
            let Some(tag) = sub_mesh_tag(&tags) else { continue };
            if tags.iter().any(|t| t == "UI") {
                continue;
            }
            out.push((tag, tags.iter().any(|t| t == "PrimaryMesh")));
        }
    }
    out
}

// ---- the anim sets

/// One set's class defaults: a blend space's entry is its first sample (animAssets).
/// `base`: the set is the data's animSet (fallSpace and wallSpaces are its alone).
fn anim_set_from(
    d: &FighterData,
    st: &Props,
    base: bool,
    composites: &Map<Vec<CompositeSegment>>,
    hover: Option<Hover>,
    name: &str,
) -> Result<(AnimSet, SetExtra), KindError> {
    let get = |k: &str| st.get(k);
    let a = |k: &str| -> Option<String> {
        let v = str_of(some(get(k)))?;
        Some(
            d.anim_assets
                .get(v)
                .and_then(|x| x.samples.as_ref())
                .and_then(|s| s.first())
                .cloned()
                .unwrap_or_else(|| v.to_owned()),
        )
    };
    // OnlyLeanWhenFalling (class default false, no ported set sets it): its two rules (no lean while rising, FallLean
    // 0 at OnIsJumping @201-215) are not ported.
    if truthy(get("OnlyLeanWhenFalling")) {
        return Err(bad(name, "OnlyLeanWhenFalling is set (the sim's fall lean has no rule for it)"));
    }
    // UseFallBlendspace: the data's fallSpace is this set's FallBlendSpace (the bind writes it for the base set alone).
    let mut fall_space = None;
    if truthy(get("UseFallBlendspace")) {
        match &d.fall_space {
            Some(f) if base && d.anim_sets.is_none() && Some(f.name.as_str()) == str_of(get("FallBlendSpace")) => {
                fall_space = Some(f.clone())
            }
            _ => return Err(bad(name, "UseFallBlendspace without its FallBlendSpace in the data")),
        }
    }
    // UseWallBlendSpace (IsValid(BlendSpace_WallIdle)): the data's wallSpaces are this set's.
    let mut wall_spaces = None;
    if truthy(get("BlendSpace_WallIdle")) {
        match &d.wall_spaces {
            Some(w)
                if base
                    && d.anim_sets.is_none()
                    && Some(w.idle.name.as_str()) == str_of(get("BlendSpace_WallIdle"))
                    && Some(w.enter.name.as_str()) == str_of(get("BlendSpace_WallEnter")) =>
            {
                wall_spaces = Some(w.clone())
            }
            _ => return Err(bad(name, "BlendSpace_WallIdle without its wall blend spaces in the data")),
        }
    }
    // UseCrouchRun: no default clip for an unset Anim_CrouchRun.
    let mut crouch_run = None;
    if get("UseCrouchRun") == Some(&Value::Bool(true)) {
        let Some(anim) = a("Anim_CrouchRun").filter(|_| truthy(get("Anim_CrouchRun"))) else {
            return Err(bad(name, "UseCrouchRun without an Anim_CrouchRun"));
        };
        crouch_run = Some(CrouchRun {
            anim,
            base_speed: num_or(get("CrouchRunBaseSpeed"), 800.0),
            into: if truthy(get("AnimCrounchIntoCrouchRun")) { a("AnimCrounchIntoCrouchRun") } else { None },
            out_of: if truthy(get("AnimCrounchRunIntoCrouch")) { a("AnimCrounchRunIntoCrouch") } else { None },
        });
    }
    let mut looping = vec![
        a("Anim_IdleAltLoop"),
        a("Anim_Idle"),
        a("Anim_Run"),
        a("Anim_Fall_01"),
        a("Anim_Crouch"),
        a("Anim_WallIdle"),
        a("Anim_HitReact_FlyBack"),
        a("Anim_HitReact_FlyForward"),
    ];
    // Base_AnimBP's sequence players with bLoopAnimation, and the fall space's samples and CrouchRun's player where the
    // set can reach them.
    if let Some(f) = &fall_space {
        looping.extend(f.samples.iter().map(|x| Some(x.anim.clone())));
    }
    if let Some(c) = &crouch_run {
        looping.push(Some(c.anim.clone()));
    }
    let crouch_run_extra = crouch_run.clone();
    let set = AnimSet {
        idle: a("Anim_Idle"),
        idle_land: a("Anim_IdleLand"),
        alt_enter: a("Anim_IdleAltEnter"),
        alt_loop: a("Anim_IdleAltLoop"),
        crouch: a("Anim_Crouch"),
        crouch_land: a("Anim_CrouchLand"),
        run: a("Anim_Run"),
        run_stop: a("Anim_RunStop"),
        run_stop2: a("Anim_RunStop_2"),
        crouch_in: a("Anim_CrouchIn"),
        crouch_out: a("Anim_CrouchOut"),
        crouch_override: get("CrouchTransitionOverride") == Some(&Value::Bool(true)),
        crouch_in_from_run: get("UseCrouchInFromRun") == Some(&Value::Bool(true)),
        run_stop_blend_time: num_or(get("RunStopBlendTime"), 0.3),
        run_land: a("Anim_RunLand"),
        wall_enter: a("Anim_WallEnter"),
        wall_idle: a("Anim_WallIdle"),
        fall: a("Anim_Fall_01"),
        fast_fall: a("Anim_FastFall"),
        jump1: a("BlendSpace_Jump_01"),
        jump2: a("BlendSpace_Jump_02"),
        hr_front: a("Anim_HitReact_Front"),
        hr_back: a("Anim_HitReact_Back"),
        hr_n: vec![a("Anim_HitReact_N_01"), a("Anim_HitReact_N_02"), a("Anim_HitReact_N_03"), a("Anim_HitReact_N_04")],
        recover: a("Anim_HitReact_Recover"),
        fly_back: a("Anim_HitReact_FlyBack"),
        fly_forward: a("Anim_HitReact_FlyForward"),
        fly_back_intro: a("Anim_HitReact_FlyBack_Intro"),
        fly_forward_intro: a("Anim_HitReact_FlyForward_Intro"),
        rotate_90_on_hit: bool_or(get("Rotate90OnHit"), true),
        fly_back_orient: num_or(get("FlybackOrientMultiplier"), 1.0),
        fly_forward_orient: num_or(get("FlyforwardOrientMultiplier"), 1.0),
        fall_lean_amount: num_or(get("FallLeanAmount"), 0.0),
        fall_lean_speed: num_or(get("FallLeanSpeed"), 3.0),
        wall_bounce_back: a("Anim_HitReact_WallBounceBack"),
        wall_bounce_front: a("Anim_HitReact_WallBounceFront"),
        floor_bounce_back: a("Anim_HitReact_FloorBounceBack"),
        floor_bounce_front: a("Anim_HitReact_FloorBounceFront"),
        looping,
        fall_space,
        wall_spaces,
        composites: composites.clone(),
        hover,
        run_base_speed: num_or(get("RunBaseSpeed"), 1500.0),
    };
    // No default clip for an unset alternate idle: a set that can reach the state without one never enters it. A set
    // that names no Anim_Idle is read from an AnimBP that is no Base_AnimBP child (the police car's): never entered.
    let mut delay =
        if get("Anim_Idle").is_none() { f64::INFINITY } else { num_or(get("AlternateIdle_TimeDelay"), 3.0) };
    if delay < 1e12
        && (set.alt_enter.as_deref().is_none_or(str::is_empty) || set.alt_loop.as_deref().is_none_or(str::is_empty))
    {
        delay = f64::INFINITY;
    }
    // UseEdgeIdle: no default clip for an unset edge player.
    let mut edge_idle = None;
    if get("UseEdgeIdle") == Some(&Value::Bool(true)) {
        if !truthy(get("Anim_Edge_Idle")) || !truthy(get("Anim_Crouch_Edge")) || !truthy(get("Anim_Edge_Land")) {
            return Err(bad(name, "UseEdgeIdle without its Anim_Edge_Idle / Anim_Crouch_Edge / Anim_Edge_Land"));
        }
        edge_idle = Some(EdgeIdle {
            idle: a("Anim_Edge_Idle").unwrap_or_default(),
            crouch: a("Anim_Crouch_Edge").unwrap_or_default(),
            land: a("Anim_Edge_Land").unwrap_or_default(),
        });
    }
    let extra = SetExtra {
        alt_idle_delay: delay,
        crouch_run: crouch_run_extra,
        edge_idle,
        run_rate_land: get("RunRateEffectsLandRun") == Some(&Value::Bool(true)),
    };
    Ok((set, extra))
}

// ---- the navigation sets

fn hold_overrides_of(pack: Option<&Value>) -> Result<Vec<HoldOverride>, KindError> {
    let mut out = Vec::new();
    for o in super::fighter::bag::arr_of(some(pack).and_then(|p| at(p, "Overrides"))) {
        let att = at(o, "Attack");
        out.push(HoldOverride {
            air: truthy(at(o, "AerialMove")),
            dir: att.and_then(|x| at(x, "RequiredDirectionInput")).and_then(Value::as_f64).unwrap_or(0.0) as i32,
            montage: opt_str(att.and_then(|x| at(x, "AttackMontage"))),
        });
    }
    Ok(out)
}

fn evades_of(v: Option<&Value>) -> Result<Option<Vec<AttackEntry>>, KindError> {
    match some(v) {
        None => Ok(None),
        Some(v) => from_value(v.clone()).map(Some).map_err(|e| KindError::Failed(format!("Evades: {e}"))),
    }
}

/// LeBron's two navigation sets: SetHasBall's two values of the navigation
/// variables (@593-1163: the montages are literals of the graph, the evade lists the pawn's WithBallEvades /
/// WithoutBallEvades) and of Comp_ItemManager.HoldingItemOverride (@387 `Ball Holding Item Override`, @486 `No Ball
/// Holding Item Override`).
fn lebron_nav_sets(d: &FighterData) -> Result<Map<NavSet>, KindError> {
    let p = &d.pawn;
    let set = |ball: bool| -> Result<NavSet, KindError> {
        let pick = |a: &str, b: &str| Some(if ball { a } else { b }.to_owned());
        Ok(NavSet {
            dash: pick("C016_Nav_Run_Start_Montage", "C016_NoBall_Run_Start_Montage"),
            dash_turn: pick("C016_Nav_Run_Turn_Montage", "C016_NoBall_Run_Turn_Montage"),
            evades: evades_of(p.get(if ball { "WithBallEvades" } else { "WithoutBallEvades" }))?,
            fwd_evade: pick("C016_Nav_Evade_F_Montage", "C016_NoBall_Nav_Evade_F_Montage"),
            spot_dodge: pick("C016_Nav_Evade_N_Montage", "C016_NoBall_Nav_Evade_N_Montage"),
            wall_jump: pick("C016_Nav_Wall_Jump_Montage", "C016_NoBall_Nav_Wall_Jump_Montage"),
            hold: hold_overrides_of(p.get(if ball {
                "Ball Holding Item Override"
            } else {
                "No Ball Holding Item Override"
            }))?,
        })
    };
    let mut m = Map::new();
    m.insert("ball".to_owned(), set(true)?);
    m.insert("noBall".to_owned(), set(false)?);
    Ok(m)
}

// ---- the definition

const SUB: &str = "AnimNotify_SetSubmeshVisibility_C";

/// The key's definition from its fighter data, with charDef's writes into the data: the EvilMorty slot's notifies moved
/// off its montages' `notifies` into their `skin_slot_notifies` (C019_AnimBP plays that slot for the skin C019_S02
/// alone), a SetSubmeshVisibility notify's `p.key`, the montages' `keepsNav` and `holdsSource`.
pub fn char_def(d: &mut FighterData, name: &str) -> Result<(CharDef, DefExtra), KindError> {
    // A slot track's sequence notifies are dispatched only while its slot node is relevant
    // (IsSlotNodeRelevantForNotifies). C019_AnimBP plays the EvilMorty slot for the skin C019_S02 only
    // (IsEvilMortySkin, set in OnSkinApplied), so with the default skin that track's notifies never fire.
    // They are kept for the view of that skin (mvs-view chars/morty.rs).
    for m in d.montages.values_mut() {
        if m.notifies.iter().any(|n| n.slot.as_deref() == Some("EvilMorty")) {
            let (own, kept): (Vec<_>, Vec<_>) =
                std::mem::take(&mut m.notifies).into_iter().partition(|n| n.slot.as_deref() != Some("EvilMorty"));
            m.notifies = own;
            m.skin_slot_notifies = kept;
        }
    }
    // A SetSubmeshVisibility notify's identity (the notify object keys BlendOutDependentNotifies): its asset and name.
    for m in d.montages.values_mut() {
        let mname = m.name.clone();
        for n in &mut m.notifies {
            if n.cls == SUB {
                let key = if n.seq.as_deref().is_some_and(|x| !x.is_empty()) {
                    n.inst.clone().map_or(Value::Null, Value::Str)
                } else {
                    Value::Str(format!("{mname}:{}", n.inst.as_deref().unwrap_or("null")))
                };
                n.p.insert("key".into(), key);
            }
        }
    }
    // Base_AnimBP OnMontageStartFunction @16-616: IsMontagePlaying is raised unless the montage has a track in a slot
    // named in DontStopNavMontageSlots, a variable of the linked base layer's class. Base_AnimBP's own default is
    // [Additive].
    // Unverified: that the handler runs on the linked layer instance for a montage of the main instance is not read in
    // the exe.
    let keep_nav = |st: &Props| -> String {
        match some(st.get("DontStopNavMontageSlots")) {
            Some(Value::Arr(l)) => l.iter().map(|x| str_of(Some(x)).unwrap_or_default()).collect::<Vec<_>>().join("|"),
            _ => "Additive".to_owned(),
        }
    };
    let base_nav = keep_nav(&d.anim_set);
    for st in d.anim_sets.iter().flat_map(|m| m.values()) {
        if keep_nav(st) != base_nav {
            return Err(bad(name, "an anim set with its own DontStopNavMontageSlots is not ported"));
        }
    }
    let nav_slots: Vec<&str> = base_nav.split('|').collect();
    for m in d.montages.values_mut() {
        if m.slots.iter().any(|s| nav_slots.contains(&s.slot.as_str())) {
            m.keeps_nav = Some(true);
        }
    }
    // FAnimNode_Slot::Update_AnyThread (exe 1424945f0): a Slot node updates its source only with
    // bAlwaysUpdateSourcePose or a source weight above 0.00001. A montage with a non-empty, non-additive track in every
    // slot of a set of slotHolds.stages holds the locomotion while it is at full weight.
    // A montage with only some slots of a set holds it by the blend inputs at the time: the sim walks the slot graph
    // for it (mvs-sim fighter/anim/update.rs holds_source).
    let stages = d.slot_holds.stages.clone();
    for m in d.montages.values_mut() {
        let holds = stages.iter().any(|st| {
            st.iter()
                .all(|n| m.slots.iter().any(|s| s.slot == *n && !s.segments.is_empty() && s.additive != Some(true)))
        });
        if holds {
            m.holds_source = Some(true);
        }
    }
    // The restart after a missed update is written for these flags and entry states of the base layer's machines; any
    // other value is refused.
    {
        let l = &d.slot_holds.layer;
        let want = [
            ("MovementAndHitReact", "Movement"),
            ("Movement", "Ground"),
            ("Ground", "ProxyEntryState"),
            ("IdleSubState", "CrouchCheck"),
            ("Wall", "Wall Enter"),
            ("AirState", "Fall"),
        ];
        for (mach, entry) in want {
            let ok = l.get(mach).is_some_and(|v| v.entry == entry && (mach == "AirState" || v.reinit));
            if !ok {
                return Err(bad(
                    name,
                    format!(
                        "Base_AnimBP's machine {mach} is not the one the restart is written for ({:?})",
                        l.get(mach)
                    ),
                ));
            }
        }
        // The AnimBP's own machines: each one with a ported restart (Iron Giant's hover, Batman's GlideAdditive,
        // Steven's SlowFallState, Jake's EatOverride / OriginsEatOverride, the police car's Navigation), or a node of
        // the slot graph the character's hooks walk.
        let cls = d.slot_holds.class.as_str();
        let jake_abp = cls == "Jake_AnimBP_C";
        let graph_walked = name.is_empty() || SLOT_INPUT_KEYS.contains(&name);
        let walked = |mach: &str, entry: &str| -> bool {
            graph_walked
                && d.slot_holds.graph.nodes.iter().any(|n| match n {
                    SlotGraphNode::Machine(m) => {
                        m.machine == mach
                            && usize::try_from(m.entry)
                                .ok()
                                .and_then(|i| m.states.get(i))
                                .is_some_and(|s| s.name == entry)
                    }
                    _ => false,
                })
        };
        for (mach, v) in &d.slot_holds.machines {
            if walked(mach, &v.entry) {
                continue;
            }
            let entry = match mach.as_str() {
                "MovementAndHover" => Some("Movement"),
                "HoverState" => Some("HoverIn"),
                "GlideAdditive" if cls == "Batman_AnimBP_C" => Some("GlideEnter"),
                "SlowFallState" if cls == "Steven_AnimBP_C" => Some("Into"),
                "EatOverride" | "OriginsEatOverride" if jake_abp => Some("Idle"),
                "Navigation" if cls == "Velma_PoliceCar_AnimBP_C" => Some("Stop"),
                _ => None,
            };
            if entry != Some(v.entry.as_str()) || !v.reinit || !v.skip_first {
                return Err(bad(name, format!("its AnimBP's machine {mach} has no ported restart ({v:?})")));
            }
        }
    }
    if let Some(seqs) = &mut d.seq_notifies {
        for (k, l) in seqs.iter_mut() {
            for n in l {
                if n.cls == SUB {
                    n.p.insert("key".into(), Value::Str(format!("{k}:{}", n.inst.as_deref().unwrap_or("null"))));
                }
            }
        }
    }
    let d = &*d;
    let mut subs: Vec<(String, bool)> = skin_sub_meshes(d).into_iter().filter_map(|(t, p)| t.map(|t| (t, p))).collect();
    for m in component_sub_meshes(d) {
        if !subs.iter().any(|s| s.0 == m.0) {
            subs.push(m);
        }
    }
    let mut composites = Map::new();
    for (k, v) in &d.anim_assets {
        if let Some(seg) = &v.segments {
            composites.insert(k.clone(), seg.clone());
        }
    }
    let hover = find(ANIM_GRAPH_HOVER, name).map(|(i, l)| Hover { enter: i.to_owned(), looping: l.to_owned() });
    let (anim, anim_extra) = anim_set_from(d, &d.anim_set, true, &composites, hover.clone(), name)?;
    // The further sets (LeBron alone).
    let mut anim_sets = None;
    let mut anim_set_blend = None;
    let mut sets_extra = None;
    if let (Some(sets), Some(b)) = (&d.anim_sets, &d.anim_set_blend) {
        // The one form the machines model: a plain linear blend, children not reset.
        if b.blend_type != "Linear" || b.transition_type != "StandardBlend" || b.reset_child_on_activation {
            return Err(bad(
                name,
                format!(
                    "animSetBlend {} / {} / reset {} is not ported",
                    b.blend_type, b.transition_type, b.reset_child_on_activation
                ),
            ));
        }
        let (mut m, mut x) = (Map::new(), Map::new());
        for (k, st) in sets {
            let (set, extra) = anim_set_from(d, st, false, &composites, hover.clone(), name)?;
            m.insert(k.clone(), set);
            x.insert(k.clone(), extra);
        }
        anim_sets = Some(m);
        sets_extra = Some(x);
        anim_set_blend = Some(AnimSetBlend {
            on_true: b.on_true.clone(),
            on_false: b.on_false.clone(),
            blend_time: b.blend_time.clone(),
            blend_type: b.blend_type.clone(),
            reset_child_on_activation: b.reset_child_on_activation,
        });
    }
    let comp = |c: &str, k: &str| -> Option<&Value> {
        some(d.components.pawn.get(c).and_then(|x| at(&x.p, k)))
            .or_else(|| some(d.components.base.get(c).and_then(|x| at(&x.p, k))))
    };
    let cdo = |k: &str| some(d.pawn.get(k)).or_else(|| some(d.base.get(k)));
    let audio = |k: &str| opt_str(comp("Comp_Actor_Audio_Container_GEN_VARIABLE", k));
    let pickup = |air: bool| -> String {
        super::fighter::bag::arr_of(
            comp("Comp_ItemManager_GEN_VARIABLE", "PickUpOverride").and_then(|p| at(p, "Overrides")),
        )
        .iter()
        .find(|o| at(o, "AerialMove") == Some(&Value::Bool(air)))
        .and_then(|o| opt_str(at(o, "Attack").and_then(|a| at(a, "AttackMontage"))))
        .unwrap_or_default()
    };
    // The pawn data is the whole class default object, so the fields are read from it alone.
    let nav = NavSet {
        dash: opt_str(d.pawn.get("DashAnim")),
        dash_turn: opt_str(d.pawn.get("DashTurnAnim")),
        evades: evades_of(d.pawn.get("Evades"))?,
        fwd_evade: opt_str(d.pawn.get("FWD_Evade")),
        spot_dodge: opt_str(d.pawn.get("SpotDodgeAnim")),
        wall_jump: opt_str(d.pawn.get("WallJumpAnim")),
        hold: hold_overrides_of(comp("Comp_ItemManager_GEN_VARIABLE", "HoldingItemOverride"))?,
    };
    let nav_sets = if name == "lebron" { lebron_nav_sets(d)? } else { Map::new() };
    let ability_mgr = d
        .components
        .pawn
        .get("Comp_Pawn_AbilityManager_GEN_VARIABLE")
        .or_else(|| d.components.base.get("Comp_Pawn_AbilityManager_GEN_VARIABLE"));
    let abilities: Vec<AbilityDef> = match ability_mgr.and_then(|c| some(at(&c.p, "Abilities"))) {
        Some(v) => from_value(v.clone()).map_err(|e| bad(name, format!("Abilities: {e}")))?,
        None => Vec::new(),
    };
    let sfx = Sfx {
        jump: audio("SFX_Jump").unwrap_or_default(),
        jump_double: audio("SFX_Jump_Double").unwrap_or_default(),
        land: audio("SFX_Land").unwrap_or_default(),
        wall_attach: audio("SFX_Wall_Attach").unwrap_or_default(),
        wall_exit: audio("SFX_Wall_Exit").unwrap_or_default(),
        wall_jump: audio("SFX_Wall_Jump").unwrap_or_default(),
        crouch_enter: audio("SFX_Crouch_Enter").unwrap_or_default(),
        crouch_exit: audio("SFX_Crouch_Exit").unwrap_or_default(),
        vo_jump: audio("VO_Jump"),
        vo_land: audio("VO_Land"),
        vo_evade: audio("VO_Evade"),
        vo_death: audio("VO_Death"),
    };
    let extra = def_extra(d, anim_extra, sets_extra, name)?;
    let def = CharDef {
        anim,
        anim_sets,
        anim_set_blend,
        max_walk: need_num(name, "MaxWalkSpeed", comp("CharMoveComp", "MaxWalkSpeed"))?,
        max_walk_crouched: need_num(name, "MaxWalkSpeedCrouched", comp("CharMoveComp", "MaxWalkSpeedCrouched"))?,
        crouched_half: num_or(comp("CharMoveComp", "CrouchedHalfHeight"), 40.0),
        max_custom_speed: need_num(name, "MaxCustomMovementSpeed", comp("CharMoveComp", "MaxCustomMovementSpeed"))?,
        max_fly_speed: need_num(name, "MaxFlySpeed", comp("CharMoveComp", "MaxFlySpeed"))?,
        jump_velocity: need_num(name, "JumpVelocity", cdo("JumpVelocity"))?,
        max_jump_height: need_num(name, "MaxJumpHeight", cdo("MaxJumpHeight"))?,
        max_double_jump_height: need_num(name, "MaxDoubleJumpHeight", cdo("MaxDoubleJumpHeight"))?,
        max_wall_jump_height: need_num(name, "MaxWallJumpHeight", cdo("MaxWallJumpHeight"))?,
        fast_fall_speed: need_num(name, "VelocityFromFastFall", cdo("VelocityFromFastFall"))?,
        key: name.to_owned(),
        max_jumps: need_num(name, "MaxJumpNumber", cdo("MaxJumpNumber"))?,
        max_hover_speed: some(d.pawn.get("MaxHoverSpeed")).and_then(Value::as_f64),
        hover_speed_mult: num_or(d.pawn.get("HoverSpeedMult"), 1.0),
        cap_half: need_num(name, "CapsuleHalfHeight", comp("CollisionCylinder", "CapsuleHalfHeight"))?,
        rest_gap: find(REST_GAP, name).unwrap_or(FLOOR_REST_GAP),
        cap_radius: need_num(name, "CapsuleRadius", comp("CollisionCylinder", "CapsuleRadius"))?,
        inner_cap_radius: need_num(name, "Capsule radius", comp("Capsule_GEN_VARIABLE", "CapsuleRadius"))?,
        inner_cap_half: need_num(name, "Capsule half height", comp("Capsule_GEN_VARIABLE", "CapsuleHalfHeight"))?,
        mesh_scale: need_num(name, "mesh scale", comp("CharacterMesh0", "RelativeScale3D").and_then(|v| at(v, "X")))?,
        mesh_x: need_num(name, "mesh location", comp("CharacterMesh0", "RelativeLocation").and_then(|v| at(v, "X")))?,
        sfx,
        vo_tag_enemy: d.vo_tags.enemy.clone(),
        vo_tag_ally: d.vo_tags.ally.clone(),
        pickup_ground: pickup(false),
        pickup_air: pickup(true),
        nav,
        nav_sets,
        throw_speed: num_or(comp("Comp_ItemManager_GEN_VARIABLE", "ThrowSpeed"), 2000.0),
        taunts: d.taunts.clone(),
        rock: d.components.pawn.contains_key("SM_DebrisBallProjectile_GEN_VARIABLE"),
        use_2d: truthy(d.pawn.get("Use 2D Hitboxes")),
        crouch_evade_recharge: find(PAWN_GRAPH_CROUCH_EVADE_RECHARGE, name),
        sub_meshes: subs.iter().map(|m| m.0.clone()).collect(),
        sub_hidden: subs.iter().filter(|m| !m.1).map(|m| m.0.clone()).collect(),
        seq_notifies: d.seq_notifies.clone().unwrap_or_default(),
        abilities,
        ability_cooldown_vo: (1..=3)
            .map(|n| {
                opt_str(comp("Comp_Pawn_AbilityManager_GEN_VARIABLE", &format!("VO Cooldown Ability {n}")))
                    .unwrap_or_default()
            })
            .collect(),
        ability_cooldown_sfx: (0..=2)
            .map(|n| {
                opt_str(comp("Comp_Pawn_AbilityManager_GEN_VARIABLE", &format!("SFX Cooldown {n}")))
                    .unwrap_or_else(|| SFX_ABILITY_COOLDOWN.to_owned())
            })
            .collect(),
    };
    Ok((def, extra))
}

/// What the definition keeps beside it: per anim set its altIdleDelay, crouchRun,
/// edgeIdle and runRateLand; the pawn's KnockBack shapes; DefaultTerminalVelocity.
fn def_extra(
    d: &FighterData,
    anim: SetExtra,
    anim_sets: Option<Map<SetExtra>>,
    name: &str,
) -> Result<DefExtra, KindError> {
    // The names of the pawn's hit shapes on the collision profile KnockBack: the
    // components of BaseFighter and the pawn, the pawn's template over the base's.
    let mut merged: Map<&Value> = Map::new();
    for set in [&d.components.base, &d.components.pawn] {
        for (k, c) in set {
            merged.insert(k.clone(), &c.p);
        }
    }
    let knock_back_shapes = merged
        .iter()
        .filter(|(_, p)| str_of(at(p, "BodyInstance").and_then(|b| at(b, "CollisionProfileName"))) == Some("KnockBack"))
        .map(|(k, _)| k.replacen("_GEN_VARIABLE", "", 1))
        .collect();
    let default_terminal = need_num(
        name,
        "DefaultTerminalVelocity",
        some(d.pawn.get("DefaultTerminalVelocity")).or_else(|| some(d.base.get("DefaultTerminalVelocity"))),
    )?;
    Ok(DefExtra { anim, anim_sets, knock_back_shapes, default_terminal })
}
