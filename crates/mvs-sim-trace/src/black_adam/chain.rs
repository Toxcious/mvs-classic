//! `Chain` (C021_ElectricChain_C and C021_ElectricChain_Starter_C, a projectile's `ext`):
//! the keys a chain adds to its projectile's `rest`.
//!
//! `stunF` is the whole buff entry the oracle keeps (`FighterBuff`): written from the caught fighter's list through the
//! reference, read back as a reference to a marker serial that [`resolve_stuns`] (after the World has its fighters'
//! buffs) turns into the entry of the class the record names.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::chars::black_adam::chain::Chain;
use mvs_sim::combat::BuffRef;
use mvs_sim::world::World;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

/// The serial a loaded `stunF` waits with, by the class it names.
const PENDING_VICTIM: u32 = u32::MAX - 3;
const PENDING_GAUNTLETS: u32 = u32::MAX - 4;
/// The serial of a stun buff the fighter's list no longer holds (swept): its entry is kept whole for the writer.
const GONE_STUN: u32 = u32::MAX - 9;
const VICTIM: &str = "C021_Buff_ElectricChainVictim_C";
const GAUNTLETS: &str = "Buff_AllyElectricGauntlets_C";

fn stun_f_put(p: &Chain, x: &Wx) -> Value {
    buff_entry_put(p.stun_f, x)
}

/// A buff the oracle holds as the whole entry (`FighterBuff`), written from its fighter's list through the reference
/// (a chain's stun buff, Iron Giant's DamageReflect thorns).
/// One gone from the list is written as it was last seen, ended (`Book.held_buffs`).
// Unverified: the fields an end changes besides frames and swept (none for the classes held so far) are not followed.
pub(crate) fn buff_entry_put(r: Option<BuffRef>, x: &Wx) -> Value {
    let Some(r) = r else { return Value::Null };
    let Some(b) = x.w.fighter(r.fighter).buffs.buffs.iter().find(|b| b.serial == r.serial) else {
        let held = x.book.held_buffs.borrow();
        let Some(Value::Obj(o)) = held.get(&(r.fighter, r.serial)) else {
            x.unmapped("a held buff no longer in its fighter's list");
            return Value::Null;
        };
        let mut o = o.clone();
        o.insert("frames".to_owned(), Value::Num(0.0));
        o.insert("swept".to_owned(), Value::Bool(true));
        return Value::Obj(o);
    };
    let v = buff_entry_live(b, x);
    x.book.held_buffs.borrow_mut().insert((r.fighter, r.serial), v.clone());
    v
}

/// A held buff entry of its fighter's list.
fn buff_entry_live(b: &mvs_sim::combat::FighterBuff, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = vec![
        ("cls".to_owned(), Value::Str(x.str(b.cls).to_owned())),
        ("frames".to_owned(), Value::Num(f64::from(b.frames))),
        ("stacks".to_owned(), Value::Num(f64::from(b.stacks))),
        ("stackFrames".to_owned(), Value::Num(f64::from(b.stack_frames))),
    ];
    // (an entry with no extension has no `ext` key in the oracle's object)
    let ext = crate::parts::ext_put(&b.ext, x);
    if !matches!(ext, Value::Null) {
        o.push(("ext".to_owned(), ext));
    }
    let mut opt = |k: &str, v: Option<f64>| {
        if let Some(v) = v {
            o.push((k.to_owned(), Value::Num(v)));
        }
    };
    opt("heal", b.heal.map(f64::from));
    opt("healAmt", b.heal_amt.map(f64::from));
    opt("born", b.born.map(f64::from));
    opt("rescue", b.rescue.map(f64::from));
    opt("by", b.by.map(f64::from));
    opt("fire", b.fire.map(f64::from));
    opt("fireDmg", b.fire_dmg.map(f64::from));
    opt("gate", b.gate.map(f64::from));
    opt("addAge", b.add_age.map(f64::from));
    if let Some(v) = b.pulled {
        o.push(("pulled".to_owned(), Value::Bool(v)));
    }
    if let Some(v) = b.mods_off {
        o.push(("modsOff".to_owned(), Value::Bool(v)));
    }
    if b.stun {
        o.push(("stun".to_owned(), Value::Bool(true)));
    }
    if b.ally_hit {
        o.push(("allyHit".to_owned(), Value::Bool(true)));
    }
    if b.swept {
        o.push(("swept".to_owned(), Value::Bool(true)));
    }
    if b.src.is_some() {
        o.push(("src".to_owned(), b.src.to_value(x)));
    }
    if b.stun_by.is_some() {
        o.push(("stunBy".to_owned(), b.stun_by.to_value(x)));
    }
    if b.notify.is_some() {
        o.push(("notify".to_owned(), b.notify.to_value(x)));
    }
    if b.own.is_some() {
        o.push(("own".to_owned(), crate::parts::own_put(&b.own, x)));
    }
    Value::Obj(Obj::new(o))
}

/// A held entry loaded as gone (`swept`): a reference no list entry has (`serial`), its value kept for the writer.
pub(crate) fn buff_entry_gone_load(v: &Value, fighter: mvs_sim::fighter::FighterId, serial: u32, l: &Lx) -> BuffRef {
    l.book.held_buffs.borrow_mut().insert((fighter, serial), v.clone());
    BuffRef { fighter, serial }
}

/// The keys a chain adds to its projectile's `rest`.
pub fn put(p: &Chain, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "launched" = launched, "srcX" = src_x, "srcY" = src_y, "tetherLength" = tether_length,
        "stopDone" = stop_done, "attemptDone" = attempt_done, "listenDone" = listen_done, "caught" = caught,
        "chainAt" = chain_at, "throwAt" = throw_at, "tailAt" = tail_at, "conds" = conds, "listen" = listen,
        "hitAnyOff" = hit_any_off, "launchVx" = launch_vx, "launchVy" = launch_vy, "starter" = starter,);
    o.push(("stunF".to_owned(), stun_f_put(p, x)));
    if p.stun_a.is_some() {
        x.unmapped("a chain's stun buff on a projectile (no rule to write it)");
    }
    o.push(("stunA".to_owned(), Value::Null));
}

/// [`put`]'s inverse.
pub fn load(p: &mut Chain, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "launched" = launched, "srcX" = src_x, "srcY" = src_y, "tetherLength" = tether_length,
        "stopDone" = stop_done, "attemptDone" = attempt_done, "listenDone" = listen_done, "caught" = caught,
        "chainAt" = chain_at, "throwAt" = throw_at, "tailAt" = tail_at, "conds" = conds, "listen" = listen,
        "hitAnyOff" = hit_any_off, "launchVx" = launch_vx, "launchVy" = launch_vy, "starter" = starter,);
    known.extend(plain_keys!(names
        "launched" = launched, "srcX" = src_x, "srcY" = src_y, "tetherLength" = tether_length,
        "stopDone" = stop_done, "attemptDone" = attempt_done, "listenDone" = listen_done, "caught" = caught,
        "chainAt" = chain_at, "throwAt" = throw_at, "tailAt" = tail_at, "conds" = conds, "listen" = listen,
        "hitAnyOff" = hit_any_off, "launchVx" = launch_vx, "launchVy" = launch_vy, "starter" = starter,));
    known.extend(["stunF", "stunA"]);
    p.stun_a = None;
    if !matches!(ob.get("stunA"), None | Some(Value::Null)) {
        l.miss("a chain's stun buff on a projectile (no rule to load it)");
    }
    p.stun_f = match ob.get("stunF") {
        None | Some(Value::Null) => None,
        Some(v @ Value::Obj(b)) => match (p.caught, b.get("cls")) {
            (Some(ActorRef::Fighter(fighter)), Some(Value::Str(c)))
                if (c == VICTIM || c == GAUNTLETS) && matches!(b.get("swept"), Some(Value::Bool(true))) =>
            {
                Some(buff_entry_gone_load(v, fighter, GONE_STUN, l))
            }
            (Some(ActorRef::Fighter(fighter)), Some(Value::Str(c))) if c == VICTIM || c == GAUNTLETS => {
                let serial = if c == VICTIM { PENDING_VICTIM } else { PENDING_GAUNTLETS };
                Some(BuffRef { fighter, serial })
            }
            _ => {
                l.miss("a chain's stun buff of another class, or with no fighter caught");
                None
            }
        },
        Some(_) => {
            l.miss("a chain's stun buff that is no object");
            None
        }
    };
}

/// Turns the markers [`load`] left into the entries of the caught fighters' lists (the first of the class).
pub fn resolve_stuns(w: &mut World, l: &mut Lx) {
    let mut fix: Vec<(mvs_sim::actors::ProjectileId, BuffRef)> = Vec::new();
    for (id, p) in w.actors.projectiles.iter() {
        if let ProjectileExt::Chain(c) = &p.ext
            && let Some(r) = c.stun_f
            && (r.serial == PENDING_VICTIM || r.serial == PENDING_GAUNTLETS)
        {
            let want = if r.serial == PENDING_VICTIM { VICTIM } else { GAUNTLETS };
            match w.fighter(r.fighter).buffs.buffs.iter().find(|b| w.names.str(b.cls) == want) {
                Some(b) => fix.push((id, BuffRef { fighter: r.fighter, serial: b.serial })),
                None => l.miss(format_args!("a chain's stun buff {want} is in no list of its fighter")),
            }
        }
    }
    for (id, r) in fix {
        if let Some(p) = w.actors.projectiles.get_mut(id)
            && let ProjectileExt::Chain(c) = &mut p.ext
        {
            c.stun_f = Some(r);
        }
    }
}
