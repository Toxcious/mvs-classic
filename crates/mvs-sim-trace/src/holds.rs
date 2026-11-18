//! `Hold`: the state of one live hold buff, `fighters[].rest.hold` as the oracle trace writes it.
//!
//! An object of `cls` (the buff's class name), `by` (the instigator, a fighter reference), `delay`, `snap`,
//! `attached`, `bound`, `applied`, and `offset` / `socket` / `grip` / `consume` / `own` left out while they are
//! undefined. `consume` is `true` or `"all"`.

use mvs_sim::combat::BuffClass;
use mvs_sim::combat::holds::{Hold, HoldConsume, HoldOwn};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, Unset, as_obj, check_keys, plain_keys, text};
use crate::cx::{Lx, Wx};
use crate::parts::buff_class;

impl Unset for HoldConsume {
    fn unset(&self) -> bool {
        *self == HoldConsume::None
    }
    fn clear(&mut self) {
        *self = HoldConsume::None;
    }
}

impl Plain for HoldConsume {
    fn to_value(&self, _: &Wx) -> Value {
        match self {
            HoldConsume::None => Value::Null,
            HoldConsume::Attached => Value::Bool(true),
            HoldConsume::All => Value::Str("all".to_owned()),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        *self = match v {
            Value::Bool(true) => HoldConsume::Attached,
            Value::Str(s) if s == "all" => HoldConsume::All,
            v => {
                l.miss(format_args!("{v:?} is not a hold's consume (true or \"all\")"));
                return;
            }
        };
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut c = HoldConsume::None;
        c.load_value(v, l);
        Some(c)
    }
}

// A child class's own variables (`own`): left out while none (before its kind's begin, or a class with none).
// Buff_JakEat's is the one class with any, so a written `own` loads as its.
impl Unset for HoldOwn {
    fn unset(&self) -> bool {
        matches!(self, HoldOwn::None)
    }
    fn clear(&mut self) {
        *self = HoldOwn::None;
    }
}

impl Plain for HoldOwn {
    fn to_value(&self, x: &Wx) -> Value {
        match self {
            HoldOwn::None => Value::Null,
            HoldOwn::JakEat(o) => o.to_value(x),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let mut o = match self {
            HoldOwn::JakEat(o) => *o,
            HoldOwn::None => Default::default(),
        };
        o.load_value(v, l);
        *self = HoldOwn::JakEat(o);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut o = HoldOwn::None;
        o.load_value(v, l);
        Some(o)
    }
}

/// The keys of a hold but its `cls`.
macro_rules! hold_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "by" = by, "delay" = delay, "snap" = snap, "attached" = attached, "bound" = bound, "applied" = applied,
            "offset" ~ offset, "socket" ~ socket, "grip" ~ grip, "consume" ~ consume, "own" ~ own,
        )
    };
}

impl Plain for Hold {
    fn to_value(&self, x: &Wx) -> Value {
        let class = match self.cls {
            BuffClass::Data(n) => x.str(n),
            c => c.registered_name().unwrap_or_default(),
        };
        let mut o: Vec<(String, Value)> = vec![("cls".to_owned(), Value::Str(class.to_owned()))];
        hold_keys!(plain_keys put self, x, o;);
        Value::Obj(Obj::new(o))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = as_obj(v, l) else { return };
        match ob.get("cls") {
            Some(c) => l.at("cls", |l| {
                if let Some(c) = text(c, l).and_then(|s| buff_class(s, l)) {
                    self.cls = c;
                }
            }),
            None => l.miss("key cls absent"),
        }
        hold_keys!(plain_keys load self, ob, l;);
        let mut known: Vec<&'static str> = vec!["cls"];
        known.extend(hold_keys!(plain_keys names));
        check_keys(ob, &known, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        // (the class and the instigator are made first, the rest loaded over them)
        let ob = as_obj(v, l)?;
        let cls = l.at("cls", |l| ob.get("cls").and_then(|c| text(c, l)).and_then(|s| buff_class(s, l)))?;
        let by = l.at("by", |l| ob.get("by").and_then(|b| mvs_sim::fighter::FighterId::from_value(b, l)))?;
        let mut h = Hold {
            cls,
            by,
            delay: 0,
            snap: 0,
            attached: false,
            bound: false,
            applied: false,
            offset: None,
            socket: None,
            grip: None,
            consume: HoldConsume::None,
            own: HoldOwn::None,
        };
        h.load_value(v, l);
        Some(h)
    }
}
