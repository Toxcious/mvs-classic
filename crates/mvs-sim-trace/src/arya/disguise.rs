//! `Disguise` and its `CopyPlay` (`Fighter.disguise`): the fighter's `rest.disguise`.

use mvs_sim::Real;
use mvs_sim::combat::shapes::{Disguise, ForeignPlay};
use mvs_sim::fighter::{FighterId, MontageState};
use mvs_sim::names::Name;
use mvs_trace::Value;

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

/// A key that must be there.
fn need<T: Plain>(o: &mvs_trace::Obj, k: &str, t: &mut T, l: &mut Lx) {
    match o.get(k) {
        Some(v) => l.at(k, |l| t.load_value(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    }
}

/// `prev` (and an active hit's `copyPose`): `{anim, time}` or null.
pub(crate) fn put_pose(p: Option<(Name, Real)>, x: &Wx) -> Value {
    p.map_or(Value::Null, |(a, t)| obj(vec![("anim", a.to_value(x)), ("time", t.to_value(x))]))
}

pub(crate) fn load_pose(p: &mut Option<(Name, Real)>, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *p = None;
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    let mut at = p.unwrap_or((Name::NONE, 0.0));
    need(o, "anim", &mut at.0, l);
    need(o, "time", &mut at.1, l);
    check_keys(o, &["anim", "time"], l);
    *p = Some(at);
}

/// `wait`: `{neutral, ground}` or null.
fn put_wait(w: Option<(bool, bool)>) -> Value {
    w.map_or(Value::Null, |(n, g)| obj(vec![("neutral", Value::Bool(n)), ("ground", Value::Bool(g))]))
}

fn load_wait(w: &mut Option<(bool, bool)>, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *w = None;
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    let mut at = w.unwrap_or((false, false));
    need(o, "neutral", &mut at.0, l);
    need(o, "ground", &mut at.1, l);
    check_keys(o, &["neutral", "ground"], l);
    *w = Some(at);
}

const COPY_KEYS: &[&str] = &["m", "stopped", "blendOut", "prev", "neutral"];

impl Plain for ForeignPlay {
    fn to_value(&self, x: &Wx) -> Value {
        let mut o = vec![
            ("m", self.m.to_value(x)),
            ("stopped", Value::Bool(self.stopped)),
            ("blendOut", Value::Bool(self.blend_out)),
            ("prev", put_pose(self.prev, x)),
        ];
        // A Polymorph's play has no `neutral` key.
        if let Some(n) = self.neutral {
            o.push(("neutral", Value::Bool(n)));
        }
        obj(o)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        need(o, "m", &mut self.m, l);
        need(o, "stopped", &mut self.stopped, l);
        need(o, "blendOut", &mut self.blend_out, l);
        match o.get("prev") {
            Some(v) => l.at("prev", |l| load_pose(&mut self.prev, v, l)),
            None => l.miss("key prev absent"),
        }
        self.neutral = o.get("neutral").map(|v| {
            let mut n = false;
            l.at("neutral", |l| n.load_value(v, l));
            n
        });
        check_keys(o, COPY_KEYS, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let m = l.at("m", |l| MontageState::from_value(o.get("m")?, l))?;
        let mut p = ForeignPlay { m, stopped: false, blend_out: false, prev: None, neutral: None };
        p.load_value(v, l);
        Some(p)
    }
}

const KEYS: &[&str] = &[
    "from",
    "character",
    "remaining",
    "uiMax",
    "activated",
    "neutral",
    "taunt",
    "shown",
    "scaled",
    "ground",
    "air",
    "tauntGround",
    "tauntAir",
    "groundCd",
    "airCd",
    "ownerNum",
    "startNum",
    "seq",
    "copy",
    "wait",
    "destroyIn",
    "subHidden",
    "hurtOff",
    "aim",
];

impl Plain for Disguise {
    fn to_value(&self, x: &Wx) -> Value {
        let mut o: Vec<(String, Value)> = Vec::new();
        plain_keys!(put self, x, o;
            "from" = from, "character" = character, "remaining" = remaining, "uiMax" = ui_max,
            "activated" = activated, "neutral" = neutral, "taunt" = taunt, "shown" = shown, "scaled" = scaled,
            "ground" = ground, "air" = air, "tauntGround" = taunt_ground, "tauntAir" = taunt_air,
            "groundCd" = ground_cd, "airCd" = air_cd, "ownerNum" = owner_num, "startNum" = start_num, "seq" = seq,
            "copy" = copy, "destroyIn" = destroy_in, "subHidden" = sub_hidden, "hurtOff" = hurt_off, "aim" = aim,
        );
        o.push(("wait".to_owned(), put_wait(self.wait)));
        Value::Obj(mvs_trace::Obj::new(o))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = as_obj(v, l) else { return };
        plain_keys!(load self, ob, l;
            "from" = from, "character" = character, "remaining" = remaining, "uiMax" = ui_max,
            "activated" = activated, "neutral" = neutral, "taunt" = taunt, "shown" = shown, "scaled" = scaled,
            "ground" = ground, "air" = air, "tauntGround" = taunt_ground, "tauntAir" = taunt_air,
            "groundCd" = ground_cd, "airCd" = air_cd, "ownerNum" = owner_num, "startNum" = start_num, "seq" = seq,
            "copy" = copy, "destroyIn" = destroy_in, "subHidden" = sub_hidden, "hurtOff" = hurt_off, "aim" = aim,
        );
        match ob.get("wait") {
            Some(v) => l.at("wait", |l| load_wait(&mut self.wait, v, l)),
            None => l.miss("key wait absent"),
        }
        check_keys(ob, KEYS, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let from = l.at("from", |l| FighterId::from_value(o.get("from")?, l))?;
        let mut d = Disguise {
            from,
            character: Name::NONE,
            remaining: 0.0,
            ui_max: 0.0,
            activated: false,
            neutral: false,
            taunt: false,
            shown: false,
            scaled: false,
            ground: Name::NONE,
            air: Name::NONE,
            taunt_ground: Name::NONE,
            taunt_air: Name::NONE,
            ground_cd: false,
            air_cd: false,
            owner_num: 0.0,
            start_num: 0.0,
            seq: None,
            copy: None,
            wait: None,
            destroy_in: -1,
            sub_hidden: Default::default(),
            hurt_off: Default::default(),
            aim: 0.0,
        };
        d.load_value(v, l);
        Some(d)
    }
}

/// A BranchWindow of the copy's montage, filed on its owner (`ForeignWindow`): the Disguise's
/// copy is the one play that opens them.
impl Plain for mvs_sim::fighter::anim::ForeignWindow {
    fn to_value(&self, x: &Wx) -> Value {
        obj(vec![("n", self.n.to_value(x)), ("key", Value::Num(f64::from(self.key))), ("cur", self.cur.to_value(x))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        need(o, "n", &mut self.n, l);
        need(o, "key", &mut self.key, l);
        need(o, "cur", &mut self.cur, l);
        check_keys(o, &["n", "key", "cur"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let n = l.at("n", |l| mvs_sim::events::NotifyRef::from_value(o.get("n")?, l))?;
        let mut w = mvs_sim::fighter::anim::ForeignWindow { n, key: 0, cur: Name::NONE };
        w.load_value(v, l);
        Some(w)
    }
}
