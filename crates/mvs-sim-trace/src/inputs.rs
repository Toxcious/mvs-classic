//! The players a recorded match gives its inputs from (`matches.json`): a seeded masher, a seeded player that fights,
//! and listed inputs. Each is the same inputs for the same seed and the same World on every machine. None is part of
//! the match's state: the trace records the inputs each gives, so a run of the trace needs none of them.

use mvs_sim::InputState;
use mvs_sim::input::NO_INPUT;
use mvs_sim::rng::Rng;
use mvs_trace::{InputRecord, WorldRecord};

/// `masher(seed)`: a player mashing at random, the same inputs for a seed.
pub struct Masher {
    r: Rng,
    cur: InputState,
    until: usize,
    made: Vec<InputState>,
}

impl Masher {
    pub fn new(seed: i32) -> Masher {
        Masher { r: Rng::new(seed), cur: NO_INPUT, until: 0, made: Vec::new() }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "draws of a few values")]
    pub fn at(&mut self, tick: usize) -> InputState {
        while self.made.len() <= tick {
            if self.made.len() >= self.until {
                let r = &mut self.r;
                let long = r.int(4.0) != 0.0;
                self.until = self.made.len() + 1 + r.int(if long { 12.0 } else { 50.0 }) as usize;
                let x = r.int(3.0) - 1.0;
                let y = if r.int(4.0) != 0.0 { r.int(3.0) - 1.0 } else { 0.0 };
                let attack = r.int(3.0) == 0.0;
                let special = r.int(5.0) == 0.0;
                let jump = r.int(4.0) == 0.0;
                let dodge = r.int(7.0) == 0.0;
                let item = r.int(9.0) == 0.0;
                let taunt = if r.int(60.0) != 0.0 { None } else { Some(r.int(4.0) as u8) };
                self.cur = InputState { x, y, attack, special, jump, dodge, item, taunt, ..NO_INPUT };
            } else if self.cur.taunt.is_some() {
                self.cur.taunt = None;
            }
            self.made.push(self.cur);
        }
        self.made[tick]
    }
}

/// mulberry32: the fighting player's own generator (the sim's `Rng` is not used: the player is no part of the match).
struct Mulberry(u32);

impl Mulberry {
    /// A whole number below `n`.
    fn int(&mut self, n: u32) -> u32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        let u = t ^ (t >> 14);
        u32::try_from((u64::from(u) * u64::from(n)) >> 32).expect("below n")
    }
}

/// An input as the fighting player plans it: the stick and the four buttons, or the item button.
#[derive(Clone, Copy, Default)]
struct Plan {
    x: f64,
    y: f64,
    attack: bool,
    special: bool,
    jump: bool,
    dodge: bool,
    item: bool,
}

impl Plan {
    fn stick(x: f64, y: f64) -> Plan {
        Plan { x, y, ..Plan::default() }
    }
}

/// The stage's middle and edge are not read from the stage: the Lab's floor is at y 210, its ledges at |x| 2074.
const EDGE: f64 = 1950.0;
const NEAR: f64 = 170.0;

/// `combat(seed)`: a player that keeps to the other fighter and fights. It walks to it, jumps to it, uses grounded,
/// aerial and special attacks and dodges, picks up and throws the items near it, and recovers to the stage. It reads
/// the World after each step (its record) and plans the next ticks' inputs from it and its own generator.
pub struct Combat {
    me: usize,
    r: Mulberry,
    plan: std::collections::VecDeque<Plan>,
}

/// `Math.sign(v) || 1`: the sign, 1 for a zero.
fn sign_or_one(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

impl Combat {
    pub fn new(me: usize, seed: u32) -> Combat {
        Combat { me, r: Mulberry(seed), plan: std::collections::VecDeque::new() }
    }

    fn set(&mut self, plan: Vec<Plan>) -> Plan {
        self.plan = plan.into();
        self.plan.pop_front().unwrap_or_default()
    }

    fn repeat(n: u32, p: Plan) -> Vec<Plan> {
        vec![p; n as usize]
    }

    /// The next input, from the World as `w` records it.
    pub fn next(&mut self, w: &WorldRecord) -> InputState {
        let p = self.plan_next(w);
        #[allow(clippy::cast_possible_truncation, reason = "the stick's -1, 0 and 1")]
        let (x, y) = (p.x as mvs_sim::Real, p.y as mvs_sim::Real);
        InputState {
            x,
            y,
            attack: p.attack,
            special: p.special,
            jump: p.jump,
            dodge: p.dodge,
            item: p.item,
            ..NO_INPUT
        }
    }

    #[allow(clippy::too_many_lines, reason = "one decision tree, kept in one place")]
    fn plan_next(&mut self, w: &WorldRecord) -> Plan {
        if let Some(p) = self.plan.pop_front() {
            return p;
        }
        let Some(f) = w.fighters.get(self.me) else { return Plan::default() };
        let other = w.fighters.iter().enumerate().find(|&(i, _)| i != self.me).map(|(_, g)| g);
        let Some(o) = other.filter(|_| f.dead <= 0.0) else { return Plan::default() };
        let (dx, dy) = (o.x - f.x, o.y - f.y);
        let toward = sign_or_one(dx);
        let r = &mut self.r;
        // off the stage: back to the middle, the jumps and the up special
        if f.x.abs() > EDGE || f.y < 150.0 {
            // (toward the middle; from the middle itself, right)
            let back = if f.x > 0.0 { -1.0 } else { 1.0 };
            let plan = match r.int(4) {
                0 => {
                    vec![Plan { jump: true, ..Plan::stick(back, 0.0) }, Plan::stick(back, 0.0), Plan::stick(back, 0.0)]
                }
                1 => {
                    let mut p = vec![Plan { special: true, ..Plan::stick(back, 1.0) }];
                    p.extend(Self::repeat(8, Plan::stick(back, 0.0)));
                    p
                }
                _ => Self::repeat(4, Plan::stick(back, 0.0)),
            };
            return self.set(plan);
        }
        // an item near and none held: pick it up (walk to one lying further off); one held and the other near: throw it
        let items = w.actors.iter().filter(|a| a.family == "items");
        let lying: Vec<(f64, f64)> = items
            .clone()
            .filter(|it| !it.dead && it.holder.is_some_and(|h| h < 0.0))
            .filter_map(|it| Some((it.x?, it.y?)))
            .filter(|&(x, y)| (x - f.x).abs() < 700.0 && (y - f.y).abs() < 200.0)
            .collect();
        let item = lying.iter().any(|&(x, _)| (x - f.x).abs() < 120.0);
        let holding = items.clone().any(|it| it.holder == Some(f.id));
        let pick = (item && !holding && r.int(2) == 0) || (holding && dx.abs() < 600.0 && r.int(3) == 0);
        if pick {
            let x = if holding { toward } else { 0.0 };
            return self.set(vec![Plan { item: true, ..Plan::stick(x, 0.0) }, Plan::default(), Plan::default()]);
        }
        if let Some(&(lx, _)) = lying.first()
            && !holding
            && !item
            && f.grounded
            && r.int(2) == 0
        {
            return self.set(Self::repeat(4, Plan::stick(sign_or_one(lx - f.x), 0.0)));
        }
        // above them: down to them (fast fall); below them: wait under them
        if dx.abs() <= NEAR * 2.0 && dy.abs() > 160.0 {
            let n = 2 + r.int(4);
            let x = if dx.abs() > 40.0 { toward } else { 0.0 };
            let y = if dy < 0.0 && !f.grounded { -1.0 } else { 0.0 };
            return self.set(Self::repeat(n, Plan::stick(x, y)));
        }
        if dx.abs() > NEAR {
            // walk or jump to them
            let plan = if dy > 300.0 && f.grounded && r.int(3) == 0 {
                let mut p = vec![Plan { jump: true, ..Plan::stick(toward, 0.0) }];
                p.extend(Self::repeat(6, Plan::stick(toward, 0.0)));
                p
            } else {
                Self::repeat(2 + r.int(6), Plan::stick(toward, 0.0))
            };
            return self.set(plan);
        }
        // near: an attack, a special, an aerial, a dodge or a pause
        let k = r.int(13);
        let dir = |r: &mut Mulberry| -> (f64, f64) {
            match r.int(6) {
                0 => (0.0, 1.0),
                1 => (0.0, -1.0),
                5 => (0.0, 0.0),
                _ => (toward, 0.0),
            }
        };
        // (a tick of the stick toward them first: the attack faces them)
        let hold = |n: u32, i: Plan| -> Vec<Plan> {
            let mut p = vec![Plan::stick(toward, 0.0), i];
            p.extend(Self::repeat(n, Plan::stick(i.x, i.y)));
            p
        };
        let plan = if k < 7 {
            let (x, y) = dir(r);
            let n = 2 + r.int(5);
            hold(n, Plan { attack: true, ..Plan::stick(x, y) })
        } else if k < 9 {
            let (x, y) = dir(r);
            // (the neutral special held: Shaggy's rage builder, its buff and the rage specials' armor after it)
            if x == 0.0 && y == 0.0 {
                Self::repeat(20 + r.int(60), Plan { special: true, ..Plan::default() })
            } else {
                let n = 3 + r.int(8);
                hold(n, Plan { special: true, ..Plan::stick(x, y) })
            }
        } else if k < 10 {
            let (x, y) = dir(r);
            let mut p = vec![Plan { jump: true, ..Plan::stick(toward, 0.0) }];
            p.extend(Self::repeat(3 + r.int(8), Plan::stick(toward, 0.0)));
            let n = 3 + r.int(6);
            let attack = r.int(3) != 0;
            let special = r.int(3) == 0;
            p.extend(hold(n, Plan { attack, special, ..Plan::stick(x, y) }));
            p
        } else if k < 11 {
            let (x, y) = dir(r);
            let n = 4 + r.int(10);
            hold(n, Plan { dodge: true, ..Plan::stick(x, y) })
        } else if k < 12 {
            // a charged attack: the button held
            let (x, y) = dir(r);
            Self::repeat(10 + r.int(30), Plan { attack: true, ..Plan::stick(x, y) })
        } else {
            let n = 1 + r.int(3);
            (0..n).map(|_| Plan::stick(if r.int(2) == 0 { 0.0 } else { toward }, 0.0)).collect()
        };
        self.set(plan)
    }
}

/// An input held from tick `from` until the next entry's (the entries in tick order, the first from 0).
pub struct Listed {
    list: Vec<(usize, InputRecord)>,
    k: usize,
}

impl Listed {
    /// The entries as `matches.json` lists them: `{from, input}`, the input in the record's keys.
    pub fn new(v: &serde_json::Value) -> Result<Listed, String> {
        let list = v
            .as_array()
            .ok_or("listed: not a list")?
            .iter()
            .map(|e| {
                let from = e["from"].as_u64().ok_or("listed: an entry with no `from`")?;
                Ok((usize::try_from(from).map_err(|_| "listed: `from` too large")?, input_record(&e["input"])?))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if list.first().is_none_or(|e| e.0 != 0) || list.windows(2).any(|p| p[1].0 <= p[0].0) {
            return Err("listed: inputs not in tick order from 0".to_owned());
        }
        Ok(Listed { list, k: 0 })
    }

    pub fn at(&mut self, tick: usize) -> &InputRecord {
        while self.k + 1 < self.list.len() && self.list[self.k + 1].0 <= tick {
            self.k += 1;
        }
        &self.list[self.k].1
    }
}

/// A listed input: the stick, the four buttons, and the item, jumpLocal, downBtn, order and taunt keys where given.
fn input_record(v: &serde_json::Value) -> Result<InputRecord, String> {
    let num = |k: &str| v[k].as_f64().ok_or(format!("listed input: `{k}` is not a number"));
    let flag = |k: &str| v[k].as_bool().ok_or(format!("listed input: `{k}` is not true or false"));
    let opt = |k: &str| v.get(k).and_then(serde_json::Value::as_bool);
    let order = match v.get("order") {
        Some(o) => Some(
            o.as_array()
                .ok_or("listed input: `order` is not a list")?
                .iter()
                .map(|s| s.as_str().map(str::to_owned).ok_or("listed input: an `order` entry is not a name"))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        None => None,
    };
    Ok(InputRecord {
        x: num("x")?,
        y: num("y")?,
        attack: flag("attack")?,
        special: flag("special")?,
        jump: flag("jump")?,
        dodge: flag("dodge")?,
        item: opt("item"),
        jump_local: opt("jumpLocal"),
        down_btn: opt("downBtn"),
        order,
        taunt: v.get("taunt").and_then(serde_json::Value::as_f64),
        ..InputRecord::default()
    })
}
