//! The fighter data's skins and voices part: the default skin's VO rows and tags,
//! its meshes, every skin's look, the taunts and the taunt slot pool.
//!
//! The default skin: the CharacterData's Skins entry with IsDefaultSkin (Comp_Pawn_SkinManager GetDefaultSkin @465),
//! but where the port table's default_skin names another (Iron Giant's C017_S00. Deviation: a user decision). The
//! rows' event length is the AkAudioEventData's MaximumDuration (vo_rows event_length).

use std::collections::HashSet;

use indexmap::IndexMap;
use mvs_data::{Props, Value};

use super::Bind;
use super::bag::{arr_of, at, num, obj, s, some, str_of, truthy};
use super::body::{BASE_FIGHTER, body_sources_of, shape};
use super::montages::{MontageBind, montage_value};
use crate::fighter_port::{asset_name, asset_of, character_data_of, pkg_of, port_default_skin};
use crate::kind::{Cx, KindError};
use crate::kinds::gamedata::shape::{cdo_path, fail, properties, short_name};
use crate::objview::ObjView;

/// Where the build's EmoteData are found by class.
const EMOTE_FOLDER: &str = "/Game/Panda_Main";
const EMOTE_CLASS: &str = "/Script/MultiVersus.EmoteData";
const SKELETAL_MESH: &str = "/Script/Engine.SkeletalMesh";
const SCS_NODE: &str = "/Script/Engine.SCS_Node";
const TAUNT_MANAGER: &str = "Comp_Pawn_TauntManager_GEN_VARIABLE";

/// A non-empty string value.
fn text(v: Option<&Value>) -> Option<&str> {
    str_of(v).filter(|x| !x.is_empty())
}

/// The skins whose intro a pawn's graph sets where the skin has no IntroMontageOverride: (skin asset name, the
/// montage's object path). Pawn_Jake ExecuteUbergraph @199-308, after BeginPlay's Delay(0.5) and before ReadyForIntro
/// plays IntroAnimation: IntroAnimation = S01_Emote_Spawn_Montage when Comp_Pawn_SkinManager.AppliedSkin is Jake_S01.
pub(super) const PAWN_INTROS: [(&str, &str); 1] = [(
    "Jake_S01",
    "/Game/Panda_Main/Characters/Jake/Skins/S01/Animation/S01_Emote_Spawn_Montage.S01_Emote_Spawn_Montage",
)];

/// Skin `name`'s montage reference `prop` (its data `sp`): the skin's own, and for IntroMontageOverride a pawn
/// graph's where it has none (PAWN_INTROS).
pub(super) fn montage_ref<'a>(name: &str, sp: &'a Props, prop: &str) -> Option<&'a str> {
    text(sp.get(prop)).or_else(|| {
        (prop == "IntroMontageOverride")
            .then(|| PAWN_INTROS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, p)| *p))
            .flatten()
    })
}

/// A value as the data holds it, its UserDefinedEnum values by number (`v ?? []`).
fn shaped(b: &Bind<'_, '_>, v: Option<&Value>) -> Result<Value, KindError> {
    let v = some(v).cloned().unwrap_or(Value::Arr(Vec::new()));
    shape(b.cx, &b.key, &v)
}

/// An asset's own properties as the bag.
fn props_at(cx: &Cx<'_>, path: &str) -> Result<Props, KindError> {
    properties(&cx.object(path)?)
}

/// The inner object of chain `chain` exactly.
fn inner_exact<'a>(o: &ObjView<'a>, chain: &str) -> Result<Option<ObjView<'a>>, KindError> {
    Ok(o.inner()?.into_iter().find(|x| x.chain() == chain))
}

// ---- skins

/// What the skins' meshes are read against: the body's skeleton and the pawn's SCS sockets by variable name.
struct MeshCtx<'b, 'c, 'a> {
    b: &'b Bind<'c, 'a>,
    body_skeleton: String,
    scs_socket: IndexMap<String, String>,
}

/// The pawn class's SCS nodes' sockets (AttachToName, 'None' where unset) by InternalVariableName, in export order.
fn scs_sockets(cx: &Cx<'_>, pawn_class: &str) -> Result<IndexMap<String, String>, KindError> {
    let cls = cx.object(pawn_class)?;
    let mut out = IndexMap::new();
    for n in cls.inner()? {
        if n.class() != SCS_NODE {
            continue;
        }
        let p = properties(&n)?;
        if p.is_empty() {
            continue;
        }
        let var = match some(p.get("InternalVariableName")) {
            None => "null".to_owned(),
            Some(Value::Str(v)) => v.clone(),
            Some(v) => super::super::shape::show(v),
        };
        out.insert(var, text(p.get("AttachToName")).unwrap_or("None").to_owned());
    }
    Ok(out)
}

/// A skin's meshes (Comp_Pawn_SkinManager ApplySkinToPawn / ApplySubMeshes): MeshOverride, and the SubMeshes whose Mesh
/// is a SkeletalMesh (a null Mesh names none: the manager clears that component; a static one is not the body's) with
/// the tags the skin gives them. A skeletal SubMesh on a Skeleton other than the body's goes in `propMeshes` with its
/// pawn SCS node's socket (Pawn_Harley: JackInTheBox on fml_un_C_prop3; its UserConstructionScript @0-168 gives the
/// master pose to the body parts alone), written only when there are any. The skeleton is written as the dump spelt
/// the reference (`Skeleton'<name>'`). `with_paths` (the skin looks): each mesh's object path beside its name
/// (meshPath, `path`), the one the game loads it by: two skins may name meshes of one name in two packages (Shaggy's
/// C002_S02 and C002_S03, C002_S02_B_SkelMesh in Skins/S02 and Skins/S03).
fn skin_meshes(m: &MeshCtx<'_, '_, '_>, pkg: &str, with_paths: bool) -> Result<(Props, Props), KindError> {
    let cx = m.b.cx;
    let sp = props_at(cx, &asset_of(pkg))?;
    let subs: Vec<(String, Value)> = arr_of(sp.get("SubMeshes"))
        .iter()
        .filter_map(|e| match e.as_arr() {
            Some([k, v]) => Some((str_of(Some(k)).unwrap_or_default().to_owned(), v.clone())),
            _ => None,
        })
        .collect();
    let mesh_override = str_of(sp.get("MeshOverride")).unwrap_or_default().to_owned();
    let mut look = Props::new();
    look.insert("name".into(), s(asset_name(pkg)));
    look.insert("mesh".into(), s(asset_name(&mesh_override)));
    look.insert("subMeshes".into(), Value::Arr(Vec::new()));
    if with_paths {
        look.insert("meshPath".into(), s(cx.object(&mesh_override)?.path()));
    }
    let mut props: Vec<Props> = Vec::new();
    let mut body: Vec<Props> = Vec::new();
    for (key, v) in &subs {
        let Some(mp) = text(at(v, "Mesh")) else { continue };
        let mesh = cx.object(mp)?;
        if mesh.class() != SKELETAL_MESH {
            continue;
        }
        let mut entry = Props::new();
        entry.insert("key".into(), s(key.clone()));
        entry.insert("mesh".into(), s(asset_name(mp)));
        if with_paths {
            entry.insert("path".into(), s(mesh.path()));
        }
        entry.insert("tags".into(), shaped(m.b, at(v, "Tags"))?);
        let mprops = properties(&mesh)?;
        let skel = text(mprops.get("Skeleton")).map(short_name);
        // (asset names are without case: Morty's c019_ / C019_)
        if skel.unwrap_or_default().to_lowercase() != m.body_skeleton.to_lowercase() {
            entry.insert("skeleton".into(), skel.map_or(Value::Null, |k| s(format!("Skeleton'{k}'"))));
            entry.insert("socket".into(), s(m.scs_socket.get(key).map_or("None", String::as_str)));
            props.push(entry);
        } else {
            body.push(entry);
        }
    }
    let prop_keys: HashSet<String> =
        props.iter().map(|p| str_of(p.get("key")).unwrap_or_default().to_owned()).collect();
    let subs: Vec<Value> = body
        .into_iter()
        .filter(|b| !prop_keys.contains(str_of(b.get("key")).unwrap_or_default()))
        .map(Value::Obj)
        .collect();
    look.insert("subMeshes".into(), Value::Arr(subs));
    if !props.is_empty() {
        look.insert("propMeshes".into(), Value::Arr(props.into_iter().map(Value::Obj).collect()));
    }
    Ok((look, sp))
}

fn vec3(v: Option<&Value>, d: [f64; 3]) -> Value {
    match v {
        Some(v @ Value::Obj(_)) => {
            Value::Arr(["X", "Y", "Z"].iter().zip(d).map(|(a, d)| some(at(v, a)).cloned().unwrap_or(num(d))).collect())
        }
        _ => Value::Arr(d.iter().map(|x| num(*x)).collect()),
    }
}

/// A skin's SkinAudioOverride class's events (its class default's SkinAudioOverride array; Comp_Pawn_SkinManager @52 /
/// @179 LoadAssetClass keeps them loaded), by name in the class's order; none without the class.
pub fn override_events(cx: &Cx<'_>, sp: &Props) -> Result<Vec<String>, KindError> {
    let Some(cls) = text(sp.get("SkinAudioOverride")) else { return Ok(Vec::new()) };
    let p = props_at(cx, &cdo_path(cls))?;
    Ok(arr_of(p.get("SkinAudioOverride"))
        .iter()
        .filter_map(|x| text(Some(x)))
        .map(|x| short_name(x).to_owned())
        .collect())
}

/// Every skin a picker offers: the port's default, then the CharacterData's Skins in their order that the build holds
/// (a soft reference to a package it does not ship loads as null) and that are not IsUnavailableOffline (the `_Server`
/// skins the dedicated server applies), by asset name.
fn skin_looks(
    m: &MeshCtx<'_, '_, '_>,
    mb: &MontageBind<'_>,
    cd: Option<&ObjView<'_>>,
    def: &str,
    mons: &HashSet<String>,
) -> Result<Value, KindError> {
    let cx = m.b.cx;
    let mut pkgs = vec![def.to_owned()];
    let listed: Vec<String> = match cd {
        Some(cd) => arr_of(properties(cd)?.get("Skins"))
            .iter()
            .map(|x| pkg_of(str_of(Some(x)).unwrap_or_default()).to_owned())
            .collect(),
        None => Vec::new(),
    };
    for p in listed {
        if !cx.exists(&p) || p.to_lowercase() == def.to_lowercase() {
            continue;
        }
        if matches!(props_at(cx, &asset_of(&p))?.get("IsUnavailableOffline"), Some(Value::Bool(true))) {
            if !p.to_lowercase().ends_with("_server") {
                return fail(format!("skin {p} is IsUnavailableOffline and not a _Server skin"));
            }
            continue;
        }
        pkgs.push(p);
    }
    let mut out = Props::new();
    for pkg in pkgs {
        let (mut o, sp) = skin_meshes(m, &pkg, true)?;
        let name = str_of(o.get("name")).unwrap_or_default().to_owned();
        let Some(slug) = sp.get("Slug") else { return fail(format!("skin {name} has no Slug")) };
        o.insert("slug".into(), slug.clone());
        // (a soft reference to a package the build lacks, BugsBunny_S03's intro, is left out: nothing of it can play.
        // Unverified: what the build plays then rests on LoadAsset of a missing package returning null)
        for (field, prop) in [
            ("introMontage", "IntroMontageOverride"),
            ("victoryMontage", "VictoryMontageOverride"),
            ("defeatMontage", "DefeatMontageOverride"),
        ] {
            if let Some(r) = montage_ref(&name, &sp, prop)
                && cx.exists(pkg_of(r))
            {
                o.insert(field.into(), s(short_name(r)));
            }
        }
        // The skin's intro as the match view draws it, and its podium montages with their notifies (the preview
        // actor runs them: C019_S02's defeat hides EyepatchMesh): one the character's montages lack is written whole.
        // The podium montages are written with the notifies of a skin's own victory or defeat montage.
        let mut ms = Props::new();
        for (field, prop) in [
            ("introMontage", "IntroMontageOverride"),
            ("victoryMontage", "VictoryMontageOverride"),
            ("defeatMontage", "DefeatMontageOverride"),
        ] {
            if let Some(im) = str_of(o.get(field)).map(str::to_owned)
                && !mons.contains(&im)
                && !ms.contains_key(&im)
            {
                let path = montage_ref(&name, &sp, prop).unwrap_or_default();
                ms.insert(im, Value::Obj(montage_value(mb, path)?));
            }
        }
        if !ms.is_empty() {
            o.insert("montages".into(), Value::Obj(ms));
        }
        for (field, prop) in [("previewIdle", "PreviewIdle"), ("homescreenIdle", "HomescreenIdle")] {
            if let Some(r) = text(sp.get(prop)) {
                o.insert(field.into(), s(short_name(r)));
            }
        }
        if let Some(Value::Arr(scale)) = sp.get("ScaleOverrides")
            && !scale.is_empty()
        {
            let mut so = Props::new();
            for e in scale {
                let Some([k, v]) = e.as_arr() else { continue };
                let r = at(v, "Rotation");
                let rot = ["X", "Y", "Z", "W"]
                    .iter()
                    .map(|a| some(r.and_then(|r| at(r, a))).cloned().unwrap_or(num(if *a == "W" { 1.0 } else { 0.0 })))
                    .collect();
                so.insert(
                    str_of(Some(k)).unwrap_or_default().to_owned(),
                    obj([
                        ("loc", vec3(at(v, "Translation"), [0.0; 3])),
                        ("rot", Value::Arr(rot)),
                        ("scale", vec3(at(v, "Scale3D"), [1.0; 3])),
                    ]),
                );
            }
            o.insert("scale".into(), Value::Obj(so));
        }
        if let Some(cs @ Value::Obj(_)) = sp.get("CharacterScale") {
            o.insert("characterScale".into(), vec3(Some(cs), [1.0; 3]));
        }
        if let Some(l) = text(sp.get("CharacterLighting")) {
            o.insert("lighting".into(), s(short_name(l)));
        }
        let events = override_events(cx, &sp)?.into_iter().map(Value::Str).collect();
        o.insert(
            "audio".into(),
            obj([
                ("audioName", some(sp.get("AudioName")).cloned().unwrap_or(s(""))),
                ("voTable", s(text(sp.get("VODataTable")).map_or("", short_name))),
                ("overrideEvents", Value::Arr(events)),
            ]),
        );
        out.insert(name, Value::Obj(o));
    }
    Ok(Value::Obj(out))
}

// ---- taunts

/// Every EmoteData of the build in lower-cased path order.
fn emote_assets(cx: &Cx<'_>) -> Vec<String> {
    let mut all = cx.catalog.list_packages(EMOTE_FOLDER, true, Some(EMOTE_CLASS));
    let key = |p: &String| format!("{}.json", p.to_lowercase()).encode_utf16().collect::<Vec<u16>>();
    all.sort_by_key(key);
    all
}

/// Whether an asset is offered: not EnabledForDebugging.
fn asset_offered(p: &Props) -> bool {
    !matches!(p.get("EnabledForDebugging"), Some(Value::Bool(true)))
}

/// The assets a Comp_Pawn_TauntManager slot can hold, by asset name: the CharacterData's AvailableTauntData
/// (GetCharacterTaunts' CharacterTaunts) and every EmoteData (its Emotes, the stickers), each offered. A TauntData's
/// Montage, Conditions, Priority (the class default 0, UTauntData ctor 14129ff60); an EmoteData's ParticleEffect and
/// Scale (UEmoteData ctor 1412488d0: (1, 1, 1)), and its Montage where set (none in the build: the pawn's EmoteMontage
/// plays). Slug: the asset's.
fn taunt_table(b: &Bind<'_, '_>, taunts: &[String], mons: &HashSet<String>) -> Result<Value, KindError> {
    let cx = b.cx;
    // (an emote's montage is the pawn's Comp_Pawn_TauntManager EmoteMontage, the pawn's template over BaseFighter's:
    // one of the character's montages)
    let Some(comps) = b.out.get("components") else { return fail("no components (the body part runs first)") };
    let tm = at(comps, "pawn")
        .and_then(|c| at(c, TAUNT_MANAGER))
        .filter(|v| !v.is_null())
        .or_else(|| at(comps, "base").and_then(|c| at(c, TAUNT_MANAGER)));
    if let Some(emote) = text(tm.and_then(|t| at(t, "p")).and_then(|p| at(p, "EmoteMontage")))
        && !mons.contains(emote)
    {
        return fail(format!("Comp_Pawn_TauntManager EmoteMontage {emote} is not among the montages"));
    }
    let mut out = Props::new();
    for r in taunts {
        let p = props_at(cx, r)?;
        let name = asset_name(r);
        if !asset_offered(&p) {
            // (the default slot is AvailableTauntData[0] unfiltered, @4908)
            if out.is_empty() {
                return fail(format!(
                    "AvailableTauntData[0] {name} is not offered: the default slot needs its own field"
                ));
            }
            continue;
        }
        let mon = short_name(str_of(p.get("Montage")).unwrap_or_default()).to_owned();
        if !mons.contains(&mon) {
            return fail(format!("taunt {name}: montage {mon} is not among the montages"));
        }
        out.insert(
            name.to_owned(),
            obj([
                ("slug", some(p.get("Slug")).cloned().unwrap_or(s(""))),
                ("montage", s(mon)),
                ("conds", shaped(b, p.get("Conditions"))?),
                ("priority", some(p.get("Priority")).cloned().unwrap_or(num(0.0))),
            ]),
        );
    }
    // the stickers: cosmetic-rules stickerOffered, the Collection's rule less its tile picture test
    let mut emotes: Vec<(String, Props)> = Vec::new();
    for pkg in emote_assets(cx) {
        let p = props_at(cx, &asset_of(&pkg))?;
        emotes.push((pkg, p));
    }
    let mut systems: Vec<String> = Vec::new();
    for (_, p) in &emotes {
        if let Some(ps) = text(p.get("ParticleEffect")) {
            let k = pkg_of(ps).to_owned();
            if !systems.contains(&k) {
                systems.push(k);
            }
        }
    }
    let held: HashSet<String> = systems.into_iter().filter(|p| cx.exists(p)).collect();
    for (pkg, p) in &emotes {
        let ps = p.get("ParticleEffect");
        let offered = asset_offered(p) && (!truthy(ps) || str_of(ps).is_some_and(|x| held.contains(pkg_of(x))));
        if !offered {
            continue;
        }
        let psn = text(ps);
        let scale = match p.get("Scale") {
            Some(sc) if !sc.is_null() => {
                Value::Arr(["X", "Y", "Z"].iter().map(|a| at(sc, a).cloned().unwrap_or(Value::Null)).collect())
            }
            _ => Value::Arr(vec![num(1.0), num(1.0), num(1.0)]),
        };
        let mut ent = Props::new();
        ent.insert("slug".into(), some(p.get("Slug")).cloned().unwrap_or(s("")));
        ent.insert("conds".into(), shaped(b, p.get("Conditions"))?);
        ent.insert("priority".into(), some(p.get("Priority")).cloned().unwrap_or(num(0.0)));
        ent.insert("sticker".into(), obj([("ps", s(psn.map_or("", short_name))), ("scale", scale)]));
        if let Some(mon) = text(p.get("Montage")) {
            if !mons.contains(short_name(mon)) {
                return fail(format!(
                    "emote {}: montage {} is not among the montages",
                    asset_name(pkg),
                    short_name(mon)
                ));
            }
            ent.insert("montage".into(), s(short_name(mon)));
        }
        out.insert(asset_name(pkg).to_owned(), Value::Obj(ent));
    }
    Ok(Value::Obj(out))
}

/// Comp_Pawn_TauntManager GetDanceMontage (BaseFighter BPI_GetSleepMontage): the DanceTaunt TauntData's Montage
/// (GameState TauntMontageCache[DanceTaunt.Slug], filled at BeginPlay by LoadTauntMontageIntoMap @6258), null when the
/// pawn's template sets no DanceTaunt (the component's and BaseFighter's leave it null). The asset is read whether
/// offered or not: GetDanceMontage tests neither the offer nor AvailableTauntData (Superman's default taunt is in
/// neither list).
fn dance_montage(cx: &Cx<'_>, pawn: &str, mons: &HashSet<String>) -> Result<Value, KindError> {
    let mut r: Option<Value> = None;
    for c in [cx.object(pawn)?, cx.object(BASE_FIGHTER)?] {
        if let Some(t) = inner_exact(&c, TAUNT_MANAGER)?
            && let Some(v) = properties(&t)?.get("DanceTaunt")
        {
            r = Some(v.clone());
            break;
        }
    }
    let Some(path) = text(r.as_ref()) else { return Ok(Value::Null) };
    let p = props_at(cx, &asset_of(path))?;
    let Some(mon) = text(p.get("Montage")) else { return fail(format!("DanceTaunt {}: no Montage", asset_name(path))) };
    if !mons.contains(short_name(mon)) {
        return fail(format!("DanceTaunt {}: montage {} is not among the montages", asset_name(path), short_name(mon)));
    }
    Ok(s(short_name(mon)))
}

// ---- the part

/// The skin part: writes vo, voTags, skin, skins, taunts, tauntTable and danceMontage.
pub fn bind_skin(b: &mut Bind<'_, '_>, mb: &MontageBind<'_>) -> Result<(), KindError> {
    let cx = b.cx;
    let Some(Value::Obj(montages)) = b.out.get("montages") else {
        return fail("no montages (the montages part runs first)");
    };
    let mons: HashSet<String> = montages.keys().cloned().collect();
    let cd = character_data_of(cx, b.character_data)?;
    let def = port_default_skin(cx, &b.key, b.port, cd.as_ref())?;
    let sp = props_at(cx, &asset_of(&def))?;
    let mut out = Props::new();

    // Comp_Actor_VO.InitVOData: the applied skin's VODataTable in row order, and its tags.
    let Some(table) = text(sp.get("VODataTable")) else { return fail(format!("{def}: no VODataTable")) };
    let rows = crate::kinds::gamedata::vo_rows::vo_rows(cx, table, &|_| true)?;
    let vo = rows
        .into_iter()
        .map(|(a, t, p, e, l)| {
            Value::Arr(vec![s(a), s(t), num(p), e.map_or(Value::Null, s), l.map_or(Value::Null, num)])
        })
        .collect();
    out.insert("vo".into(), Value::Arr(vo));
    out.insert(
        "voTags".into(),
        obj([
            ("enemy", some(sp.get("TagForEnemies")).cloned().unwrap_or(Value::Null)),
            ("ally", some(sp.get("TagForAllies")).cloned().unwrap_or(Value::Null)),
        ]),
    );

    // The default skin's meshes, its intro (BaseFighter ReadyForIntro @84838: IntroMontageOverride when the soft
    // reference is valid; one of the character's own montages).
    // (the pawn class and its skeleton by the body's chain, as the body part reads them)
    let src = body_sources_of(b)?;
    let m = MeshCtx { b, body_skeleton: short_name(&src.skeleton).to_owned(), scs_socket: scs_sockets(cx, &src.pawn)? };
    let (mut skin, _) = skin_meshes(&m, &def, false)?;
    if let Some(intro) = text(sp.get("IntroMontageOverride")) {
        if !mons.contains(short_name(intro)) {
            return fail(format!("{def}: IntroMontageOverride {} is not among the montages", short_name(intro)));
        }
        skin.insert("introMontage".into(), s(short_name(intro)));
    }
    out.insert("skin".into(), Value::Obj(skin));
    out.insert("skins".into(), skin_looks(&m, mb, cd.as_ref(), &def, &mons)?);

    // Comp_Pawn_TauntManager offline (TauntDataMap empty @4202, no DefaultTaunt): slot 0 is AvailableTauntData[0]
    // (@4489-5089: its Montage and Conditions), and the slot pool. A key without AvailableTauntData (velmapolice) has
    // none.
    let taunts: Vec<String> = match &cd {
        Some(cd) => arr_of(properties(cd)?.get("AvailableTauntData"))
            .iter()
            .filter(|t| truthy(Some(t)))
            .map(|t| str_of(Some(t)).unwrap_or_default().to_owned())
            .collect(),
        None => Vec::new(),
    };
    if taunts.is_empty() {
        out.insert("taunts".into(), Value::Arr(Vec::new()));
        out.insert("tauntTable".into(), Value::Obj(Props::new()));
    } else {
        let td = props_at(cx, &taunts[0])?;
        let first = obj([
            ("montage", s(short_name(str_of(td.get("Montage")).unwrap_or_default()))),
            ("conds", shaped(b, td.get("Conditions"))?),
        ]);
        out.insert("taunts".into(), Value::Arr(vec![first]));
        out.insert("tauntTable".into(), taunt_table(b, &taunts, &mons)?);
    }
    out.insert("danceMontage".into(), dance_montage(cx, &src.pawn, &mons)?);
    // (the announcer's rows, PandaGameState's own Comp_Actor_VO, are no fighter's)
    drop(m);
    for (k, v) in out {
        b.out.insert(k, v);
    }
    Ok(())
}
