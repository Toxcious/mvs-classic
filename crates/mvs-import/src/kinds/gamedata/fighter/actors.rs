//! The fighter data's actors part: `buffs`, the buff classes a match of the
//! character spawns, each its class defaults; `actors`, the actor classes it spawns, each as `actor_class.rs` binds
//! it; `actorMontages`, the montages those actors' own graphs play.
//!
//! What reaches them: the montages' notifies (a buff, a spawner's class, a hit's on-hit buffs), the sequences'
//! spawners (seqNotifies: the AnimBP's sequences), the game mode's buffs, the pawn's passive buffs and components,
//! then each class's own properties and graphs (`reach.rs`), in the worklist's order: the order of reaching numbers
//! the data's objects.
//!
//! The montages, pawn and components it reads are the earlier parts' (shaped: class names); the object paths of the
//! names come from the raw objects they were bound from (the character's montages, the pawn's and BaseFighter's class
//! and default object), each name resolved to the one path those name it by.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::fighter::{ActorDef, Montage};
use mvs_data::{Props, Value};

use super::Bind;
use super::bag::{arr_of, at, str_of};
use super::body::{BASE_FIGHTER, pawn_class_of};
use super::montages::{MontageBind, asset_of, bind_montage, montage_packages, sequence_packages};
use crate::kind::KindError;
use crate::kinds::gamedata::actor_class::{ActorClass, ActorClasses, actor_passive_buffs};
use crate::kinds::gamedata::reach::{
    BUFF_SPAWN_MEMBERS, GRAPH_BUFFS, GRAPH_MONTAGES, GRAPH_SPAWNS, MODE_BUFFS_FROM, NOTIFY_ACTOR_UNSPAWNED,
    PASSIVE_SPAWNS, Reach,
};
use crate::kinds::gamedata::shape::{cdo_path, defaults_or_properties, fail, properties, short_name};
use crate::objview::ObjView;

/// An actor's class default variables of the on-hit struct its graph hands to pfgDoDamage: HitBoxHandler OnHitBuffs
/// (Shaggy_AllyTeleportBurst: Buff_Weakened_C x 5); PolymorphGrenade @539 "On Hit Buffs Param"
/// (Buff_C020Polymorph_C); VelmaHealBeam / VelmaChildBeam's "On Hit Buffs Array" (the final blast's) and "On Hit
/// Charging Buffs Array" (the charging hits').
pub const ON_HIT_MEMBERS: [&str; 4] =
    ["On Hit Buffs", "On Hit Buffs Param", "On Hit Buffs Array", "On Hit Charging Buffs Array"];

/// The on-hit buffs of a Struct_ApplyBuffs value (its OnHitBuffs entries' AppliedBuffs; `Null` for an entry that is
/// no struct or has none, `undefined`).
pub fn applied_buffs(v: &Value) -> Vec<Value> {
    match v.as_obj().and_then(|o| o.get("OnHitBuffs")) {
        Some(Value::Arr(l)) => {
            l.iter().map(|e| e.as_obj().and_then(|e| e.get("AppliedBuffs")).cloned().unwrap_or(Value::Null)).collect()
        }
        _ => Vec::new(),
    }
}

/// The class names of shaped values, each to the one object path the raw values name it by (`None`: two paths).
#[derive(Default)]
struct Names {
    paths: HashMap<String, Option<String>>,
}

impl Names {
    fn add(&mut self, v: &Value) {
        match v {
            Value::Str(p) => {
                if !p.starts_with("/Game/") {
                    return;
                }
                let n = short_name(p).to_owned();
                match self.paths.get(&n) {
                    None => {
                        self.paths.insert(n, Some(p.clone()));
                    }
                    Some(Some(was)) if was.to_lowercase() != p.to_lowercase() => {
                        self.paths.insert(n, None);
                    }
                    Some(_) => {}
                }
            }
            Value::Arr(a) => a.iter().for_each(|x| self.add(x)),
            Value::Obj(o) => o.values().for_each(|x| self.add(x)),
            _ => {}
        }
    }

    fn add_props(&mut self, p: &Props) {
        p.values().for_each(|x| self.add(x));
    }

    fn add_object(&mut self, o: &ObjView<'_>) -> Result<(), KindError> {
        self.add_props(&properties(o)?);
        if let Some(d) = crate::kinds::gamedata::shape::defaults(o)? {
            self.add_props(&d);
        }
        for x in o.inner()? {
            self.add_props(&properties(&x)?);
        }
        Ok(())
    }

    fn path(&self, name: &str, where_: &str) -> Result<String, KindError> {
        match self.paths.get(name) {
            None => fail(format!("{where_}: no object path for {name}")),
            Some(None) => fail(format!("{where_}: {name} names two objects")),
            Some(Some(p)) => Ok(p.clone()),
        }
    }
}

fn has(r: &Reach, key: &str) -> Result<bool, KindError> {
    Ok(r.entries()?.iter().any(|(k, _)| *k == key))
}

/// What the part leaves for the fighter data: buffs (bags), actors and actorMontages (typed).
pub struct ActorsOut {
    pub buffs: Props,
    pub actors: IndexMap<String, ActorDef>,
    pub actor_montages: Option<IndexMap<String, Montage>>,
}

/// The worklist of one bind.
struct Work<'c, 'a> {
    classes: &'c ActorClasses<'a>,
    buffs: Props,
    buff_path: HashMap<String, String>,
    actors: IndexMap<String, Arc<ActorClass>>,
}

impl Work<'_, '_> {
    fn add_buff(&mut self, name: &str, path: &str) -> Result<(), KindError> {
        if self.buffs.contains_key(name) {
            return Ok(());
        }
        self.buff_path.insert(name.to_owned(), path.to_owned());
        let d = self.classes.class_defaults(path)?;
        self.buffs.insert(name.to_owned(), Value::Obj(d));
        Ok(())
    }

    fn take(&mut self, list: &mut Vec<Option<String>>) -> Result<(), KindError> {
        while let Some(a) = list.pop() {
            let Some(a) = a else { continue };
            let name = short_name(&a).to_owned();
            if self.actors.contains_key(&name) {
                continue;
            }
            let c = self.classes.actor(&a)?;
            self.actors.insert(name.clone(), c.clone());
            let rp = &c.raw_p;
            list.push(str_of(rp.get("ActorClassSpawnedAtDeath")).map(str::to_owned));
            list.push(str_of(rp.get("Spiltter")).map(str::to_owned)); // C019_SplitterProjectile's follow-up class
            list.extend(GRAPH_SPAWNS.get(&name)?.iter().map(|x| Some((*x).to_owned())));
            for b in GRAPH_BUFFS.get(&name)? {
                self.add_buff(short_name(b), b)?;
            }
            if let Some(Value::Arr(pb)) = rp.get("PassiveBuffs") {
                for b in pb {
                    if let Value::Str(b) = b {
                        self.add_buff(short_name(b), b)?;
                    }
                }
            }
            for k in ON_HIT_MEMBERS {
                for b in rp.get(k).map(applied_buffs).unwrap_or_default() {
                    if let Value::Str(b) = b {
                        self.add_buff(short_name(&b), &b)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// What a buff's graph spawns: BUFF_SPAWN_MEMBERS' values, then its graph literals.
    fn buff_spawns(&self, name: &str) -> Result<Vec<String>, KindError> {
        let mut out: Vec<String> = Vec::new();
        if has(&BUFF_SPAWN_MEMBERS, name)? {
            let d = self.classes.raw_defaults(self.buff_path.get(name).map_or("", String::as_str))?;
            for m in BUFF_SPAWN_MEMBERS.get(name)? {
                if let Some(Value::Str(x)) = d.get(*m) {
                    out.push(x.clone());
                }
            }
        }
        for b in GRAPH_BUFFS.get(name)? {
            if !out.iter().any(|x| short_name(x) == short_name(b)) {
                out.push((*b).to_owned());
            }
        }
        Ok(out)
    }

    fn spread(&mut self, mut more: Vec<String>) -> Result<(), KindError> {
        while let Some(b) = more.pop() {
            let n = short_name(&b).to_owned();
            if self.buffs.contains_key(&n) {
                continue;
            }
            self.add_buff(&n, &b)?;
            let mut sp = self.buff_spawns(&n)?;
            sp.reverse();
            more.extend(sp);
        }
        Ok(())
    }

    fn spawns_of_all(&self, only: &dyn Fn(&str) -> bool) -> Result<Vec<String>, KindError> {
        let mut out = Vec::new();
        for b in self.buffs.keys() {
            for x in self.buff_spawns(b)? {
                if only(&x) {
                    out.push(x);
                }
            }
        }
        Ok(out)
    }
}

/// The actors part: buffs, actors and actorMontages, from the montages, seqNotifies, pawn and components the earlier
/// parts wrote.
pub fn bind_actors(b: &Bind<'_, '_>, mb: &MontageBind<'_>) -> Result<ActorsOut, KindError> {
    let cx = b.cx;
    let empty = Value::Obj(Props::new());
    let montages = b.out.get("montages").unwrap_or(&empty);
    let seq_notifies = b.out.get("seqNotifies").unwrap_or(&empty);
    let unspawned = |cls: &str| NOTIFY_ACTOR_UNSPAWNED.get(cls);
    let spawns = |n: &Value| -> Result<bool, KindError> {
        let cls = str_of(at(n, "cls")).unwrap_or_default();
        let actor = str_of(at(n, "p").and_then(|p| at(p, "ActorClass")));
        Ok(cls.contains("Spawner") && !unspawned(cls)?.iter().any(|x| Some(*x) == actor))
    };
    let comps = b.out.get("components");
    let comp_list = |side: &str| -> Vec<&Value> {
        comps.and_then(|c| at(c, side)).and_then(Value::as_obj).map(|o| o.values().collect()).unwrap_or_default()
    };
    let pawn = pawn_class_of(cx, b.port, b.character_data)?;
    let pawn_name = short_name(&pawn).to_owned();
    // (by lower case: a package may differ in case from the montage it holds, c016_Special_Dunk_DownSlam_Montage)
    let mut names = Names::default();
    for p in [pawn.clone(), cdo_path(&pawn), BASE_FIGHTER.to_owned(), cdo_path(BASE_FIGHTER)] {
        names.add_object(&cx.object(&p)?)?;
    }
    // the montages part's montages, raw: the notifies' class names resolve through them
    for p in montage_packages(cx, b.port) {
        names.add_object(&cx.object(&asset_of(&p))?)?;
    }
    // and the sequences whose notifies spawn an actor (the AnimBP plays them: Pawn_C017's C017_Wall_In)
    for p in sequence_packages(cx, b.port) {
        let n = &p[p.rfind('/').map_or(0, |i| i + 1)..];
        let mut any = false;
        for x in arr_of(at(seq_notifies, n)) {
            any = any || spawns(x)?;
        }
        if any {
            names.add_object(&cx.object(&asset_of(&p))?)?;
        }
    }
    let classes = ActorClasses::new(*cx, &b.key);
    let mut w = Work { classes: &classes, buffs: Props::new(), buff_path: HashMap::new(), actors: IndexMap::new() };

    // ---- buffs: the notifies', the game mode's, the pawn's passive ones
    let add_named = |w: &mut Work<'_, '_>, v: Option<&Value>, where_: &dyn Fn() -> String| -> Result<(), KindError> {
        if let Some(Value::Str(v)) = v
            && !w.buffs.contains_key(v)
        {
            let p = names.path(v, &where_())?;
            w.add_buff(v, &p)?;
        }
        Ok(())
    };
    for (mn, m) in montages.as_obj().into_iter().flatten() {
        for n in arr_of(at(m, "notifies")) {
            let cls = str_of(at(n, "cls")).unwrap_or("null");
            let p = at(n, "p");
            add_named(&mut w, p.and_then(|p| at(p, "Buff")), &|| format!("montage {mn} {cls}"))?;
            for x in p.and_then(|p| at(p, "OnHitBuffs")).map(applied_buffs).unwrap_or_default() {
                add_named(&mut w, Some(&x), &|| format!("montage {mn} {cls} OnHitBuffs"))?;
            }
        }
    }
    // FFAGameMode SpawnAndInitPlayer @1454-1799: RespawnCatchUpBuffs' keys, in the map's order
    let mode = defaults_or_properties(&cx.object(MODE_BUFFS_FROM.cdo)?)?;
    for e in arr_of(mode.get(MODE_BUFFS_FROM.member)) {
        if let Some(Value::Str(x)) = e.as_arr().and_then(|p| p.first()) {
            w.add_buff(short_name(x), x)?;
        }
    }
    let pm_of =
        |side: &str| comps.and_then(|c| at(c, side)).and_then(|c| at(c, "Comp_Pawn_PassiveBuffManager_GEN_VARIABLE"));
    let pm = pm_of("pawn").filter(|v| !v.is_null()).or_else(|| pm_of("base"));
    for e in arr_of(pm.and_then(|p| at(p, "p")).and_then(|p| at(p, "Buffs"))) {
        if let Some(Value::Str(buff)) = at(e, "Buff")
            && has(&PASSIVE_SPAWNS, buff)?
        {
            add_named(&mut w, Some(&Value::Str(buff.clone())), &|| {
                format!("{pawn_name} Comp_Pawn_PassiveBuffManager")
            })?;
            for x in PASSIVE_SPAWNS.get(buff)? {
                w.add_buff(short_name(x), x)?;
            }
        }
    }

    // ---- actors
    let all_comps: Vec<&Value> = comp_list("base").into_iter().chain(comp_list("pawn")).collect();
    let comp_types: Vec<String> =
        all_comps.iter().map(|c| str_of(at(c, "type")).unwrap_or_default().to_owned()).collect();
    let mut todo: Vec<Option<String>> = Vec::new();
    for t in comp_types.iter().chain(std::iter::once(&pawn_name)) {
        todo.extend(GRAPH_SPAWNS.get(t)?.iter().map(|x| Some((*x).to_owned())));
    }
    todo.reverse();
    // a pawn's Comp_Pawn_SpawnObjectRandomArea's SpawnedActor (its graph hands it to the pawn's CompSpawnActor @535 /
    // @1324: Pawn_Arya's AryaKillPie_C), behind the notifies' classes too
    let mut random: Vec<Option<String>> = Vec::new();
    for c in &all_comps {
        if str_of(at(c, "type")) != Some("Comp_Pawn_SpawnObjectRandomArea_C") {
            continue;
        }
        random.push(match at(c, "p").and_then(|p| at(p, "SpawnedActor")) {
            Some(Value::Str(sa)) => Some(names.path(sa, &format!("{pawn_name} SpawnedActor"))?),
            _ => None,
        });
    }
    random.reverse();
    // the sequences' spawners' classes, behind everything else of the first pass (the list is taken from its end)
    let mut seq_spawns: Vec<Option<String>> = Vec::new();
    for (sn, l) in seq_notifies.as_obj().into_iter().flatten() {
        for n in arr_of(Some(l)) {
            if spawns(n)? {
                seq_spawns.push(match at(n, "p").and_then(|p| at(p, "ActorClass")) {
                    Some(Value::Str(a)) => {
                        Some(names.path(a, &format!("sequence {sn} {}", str_of(at(n, "cls")).unwrap_or("null")))?)
                    }
                    _ => None,
                });
            }
        }
    }
    seq_spawns.reverse();
    let mut list: Vec<Option<String>> = seq_spawns.into_iter().chain(random).chain(todo).collect();
    for (mn, m) in montages.as_obj().into_iter().flatten() {
        for n in arr_of(at(m, "notifies")) {
            if spawns(n)? {
                list.push(match at(n, "p").and_then(|p| at(p, "ActorClass")) {
                    Some(Value::Str(a)) => {
                        Some(names.path(a, &format!("montage {mn} {}", str_of(at(n, "cls")).unwrap_or("null")))?)
                    }
                    _ => None,
                });
            }
        }
    }
    w.take(&mut list)?;
    // the component classes' and the pawn's own graphs
    for t in comp_types.iter().chain(std::iter::once(&pawn_name)) {
        for x in GRAPH_BUFFS.get(t)? {
            w.add_buff(short_name(x), x)?;
        }
    }
    // a spawned actor's own Comp_Pawn_PassiveBuffManager, in the actors' order
    let acts: Vec<Arc<ActorClass>> = w.actors.values().cloned().collect();
    for c in &acts {
        for x in actor_passive_buffs(c) {
            w.add_buff(short_name(&x), &x)?;
        }
    }
    let mut first = w.spawns_of_all(&|_| true)?;
    first.reverse();
    w.spread(first)?;
    // a buff's own graph that spawns an actor (Finn's Bmo_Chop_Buff @207 BMO_FallowActor_C): after everything else
    let mut late: Vec<Option<String>> = Vec::new();
    for bn in w.buffs.keys() {
        for x in GRAPH_SPAWNS.get(bn)? {
            if !w.actors.contains_key(short_name(x)) {
                late.push(Some((*x).to_owned()));
            }
        }
    }
    if !late.is_empty() {
        let n = w.actors.len();
        late.reverse();
        w.take(&mut late)?;
        let mut more: Vec<String> = w.actors.values().skip(n).flat_map(|c| actor_passive_buffs(c)).collect();
        more.reverse();
        let held: Vec<String> = w.buffs.keys().cloned().collect();
        let mut extra = w.spawns_of_all(&|x| !held.iter().any(|h| h == short_name(x)))?;
        extra.reverse();
        more.extend(extra);
        w.spread(more)?;
    }

    // ---- actorMontages: the montages the actors' own graphs play (GRAPH_MONTAGES), by name, when there are any
    let mut amons: IndexMap<String, Montage> = IndexMap::new();
    for (n, c) in &w.actors {
        let supers = c.def.supers.clone().unwrap_or_default();
        let keys = std::iter::once(n.as_str()).chain(supers.split(' ').filter(|x| !x.is_empty()));
        for k in keys {
            for path in GRAPH_MONTAGES.get(k)? {
                let mn = short_name(path);
                if amons.contains_key(mn) {
                    continue;
                }
                amons.insert(mn.to_owned(), bind_montage(mb, path)?);
            }
        }
    }
    let actors = w.actors.iter().map(|(n, c)| (n.clone(), c.def.clone())).collect();
    Ok(ActorsOut { buffs: w.buffs, actors, actor_montages: (!amons.is_empty()).then_some(amons) })
}
