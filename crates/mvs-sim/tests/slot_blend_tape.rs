//! The slot graph walk (`fighter::anim::slot_blend`) against a tape of the oracle's: per
//! character, a scripted run of `slot_blend_step` and `slot_graph_parts` answering the walk's questions as the oracle's
//! run did, the state, the parts and the players' reads compared bit for bit after every step, and the questions asked
//! compared with the oracle's. The tape is `.scratch/slot_tape.json` of the tree (written by an oracle probe, never
//! committed); without it, or without the dev store, the test says so and passes.

use mvs_data::GameData;
use mvs_data::fighter::SlotGraphNode;
use mvs_sim::fighter::anim::slot_blend::{
    Crossfade, SeqInfo, SlotBlend, SlotGraphIn, SlotPart, SlotPlayers, SlotScratch, has_players, has_timed_lists,
    slot_blend_step, slot_graph_parts, slot_machine_state, slot_player_time, slot_player_time_from_end,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn hx(v: f64) -> String {
    format!("{:x}", v.to_bits())
}

fn from_hx(s: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(s.as_str().expect("a hex number"), 16).expect("hex"))
}

/// The answers the oracle's walk gave in one step, and the questions the Rust walk asked.
struct Answers<'a> {
    who: String,
    ans: &'a Value,
    data: &'a mvs_data::CharData,
    asked: RefCell<BTreeMap<String, usize>>,
}

impl Answers<'_> {
    fn get(&self, group: &str, key: &str) -> &Value {
        self.asked.borrow_mut().entry(format!("{group} {key}")).or_default();
        self.ans[group].get(key).unwrap_or_else(|| panic!("{}: the tape has no answer for {group} {key}", self.who))
    }
}

impl SlotGraphIn for Answers<'_> {
    fn who(&self) -> &str {
        &self.who
    }
    fn warn(&self, msg: &str) {
        eprintln!("{msg}");
    }
    fn input(&self, name: &str) -> mvs_sim::Real {
        mvs_sim::math::narrow(from_hx(self.get("input", name)))
    }
    fn layered(&self, node: &str, _kind: &str) -> bool {
        self.get("layered", node).as_bool().expect("a bool")
    }
    fn source(&self, node: &str, _kind: &str) -> bool {
        self.get("source", node).as_bool().expect("a bool")
    }
    fn layer(&self, node: &str) -> Option<mvs_sim::Real> {
        self.get("layer", node).as_f64().map(mvs_sim::math::narrow)
    }
    fn anim(&self, anim: &str) -> Option<SeqInfo> {
        self.data.boxes.anims.get(anim).map(|a| SeqInfo {
            length: mvs_sim::math::narrow(a.length),
            rate_scale: mvs_sim::math::narrow(a.rate_scale),
        })
    }
}

fn ints(v: &Value) -> Vec<i64> {
    v.as_array().expect("an array").iter().map(|x| x.as_i64().expect("an int")).collect()
}

fn hexes(v: &Value) -> Vec<String> {
    v.as_array().expect("an array").iter().map(|x| x.as_str().expect("hex").to_owned()).collect()
}

fn check_blend(who: &str, k: usize, s: &SlotBlend, t: &Value) {
    let last: Vec<i64> = s.last.iter().map(|&x| i64::from(x)).collect();
    assert_eq!(last, ints(&t["last"]), "{who} step {k}: SlotBlend.last");
    for (name, v) in [("w", &s.w), ("from", &s.from), ("rem", &s.rem), ("time", &s.time)] {
        assert_eq!(
            v.iter().map(|&x| hx(f64::from(x))).collect::<Vec<_>>(),
            hexes(&t[name]),
            "{who} step {k}: SlotBlend.{name}"
        );
    }
}

fn check_players(who: &str, k: usize, p: &SlotPlayers, t: &Value) {
    assert_eq!(i64::from(p.frame), t["frame"].as_i64().unwrap(), "{who} step {k}: frame");
    assert_eq!(p.time.iter().map(|&x| hx(f64::from(x))).collect::<Vec<_>>(), hexes(&t["time"]), "{who} step {k}: time");
    assert_eq!(p.w.iter().map(|&x| hx(f64::from(x))).collect::<Vec<_>>(), hexes(&t["w"]), "{who} step {k}: w");
    for (name, v) in [("seen", &p.seen), ("last", &p.last), ("cur", &p.cur)] {
        assert_eq!(v.iter().map(|&x| i64::from(x)).collect::<Vec<_>>(), ints(&t[name]), "{who} step {k}: {name}");
    }
    let first: Vec<bool> = t["first"].as_array().unwrap().iter().map(|x| x.as_bool().unwrap()).collect();
    assert_eq!(p.first, first, "{who} step {k}: first");
    let act: Vec<Vec<String>> = p
        .act
        .iter()
        .map(|a| {
            a.iter()
                .flat_map(|c: &Crossfade| {
                    let opt = match c.opt {
                        mvs_sim::fighter::anim::slot_blend::BlendOpt::Linear => 0.0,
                        mvs_sim::fighter::anim::slot_blend::BlendOpt::HermiteCubic => 1.0,
                    };
                    [f64::from(c.from), f64::from(c.to), f64::from(c.dur), f64::from(c.el), f64::from(c.alpha), opt]
                        .map(hx)
                })
                .collect()
        })
        .collect();
    let want: Vec<Vec<String>> = t["act"].as_array().unwrap().iter().map(hexes).collect();
    assert_eq!(act, want, "{who} step {k}: act");
}

#[test]
fn the_slot_graph_walk_matches_the_ts_tape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(text) = std::fs::read_to_string(root.join(".scratch/slot_tape.json")) else {
        eprintln!("no .scratch/slot_tape.json: skipped");
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let tape: Value = serde_json::from_str(&text).expect("the tape's json");
    let mut checked = 0;
    for (key, steps) in tape.as_object().expect("an object") {
        let data = GameData::load(&store, &[key.as_str()], "trainingroomsmall").expect("the gamedata kind loads");
        let cd = data.chars.get(key.as_str()).expect("the character's data");
        let g = &cd.fighter.slot_holds.graph;
        let mut s = has_timed_lists(g).then(SlotBlend::default);
        let mut p = has_players(g).then(SlotPlayers::default);
        let mut sc = SlotScratch::default();
        for (k, st) in steps.as_array().expect("steps").iter().enumerate() {
            let gi = Answers { who: key.clone(), ans: &st["ans"], data: cd, asked: RefCell::default() };
            let dt = mvs_sim::math::narrow(from_hx(&st["dt"]));
            let mut src = |node: &str, _slot: &str| gi.get("src", node).as_bool().expect("a bool");
            slot_blend_step(g, s.as_mut(), dt, &gi, &mut src, p.as_mut(), &mut sc);
            let mut parts = Vec::new();
            let mut track = |slot: &str| gi.get("track", slot).as_bool().expect("a bool");
            slot_graph_parts(g, s.as_ref(), &gi, &mut track, &mut parts, p.as_ref());
            // every question the oracle's walk asked, the Rust walk asked
            for (group, m) in st["ans"].as_object().unwrap() {
                for q in m.as_object().unwrap().keys() {
                    assert!(
                        gi.asked.borrow().contains_key(&format!("{group} {q}")),
                        "{key} step {k}: the Rust walk did not ask {group} {q}"
                    );
                }
            }
            match (&s, st["s"].is_null()) {
                (Some(s), false) => check_blend(key, k, s, &st["s"]),
                (None, true) => {}
                _ => panic!("{key}: a SlotBlend on one side only"),
            }
            match (&p, st["p"].is_null()) {
                (Some(p), false) => check_players(key, k, p, &st["p"]),
                (None, true) => {}
                _ => panic!("{key}: SlotPlayers on one side only"),
            }
            let got: Vec<(String, String)> = parts
                .iter()
                .map(|(part, w)| {
                    let n = match part {
                        SlotPart::Slot(s) => (*s).to_owned(),
                        SlotPart::Loco => "LOCO".to_owned(),
                        SlotPart::Seq(i) => format!("#{i}"),
                    };
                    (n, hx(f64::from(*w)))
                })
                .collect();
            let want: Vec<(String, String)> = st["parts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (x[0].as_str().unwrap().to_owned(), x[1].as_str().unwrap().to_owned()))
                .collect();
            assert_eq!(got, want, "{key} step {k}: parts");
            let len = |a: &str| cd.boxes.anims.get(a).map_or(0.0, |x| mvs_sim::math::narrow(x.length));
            for n in &g.nodes {
                match n {
                    SlotGraphNode::Machine(m) => {
                        let got = slot_machine_state(g, p.as_ref(), &m.machine).unwrap_or("null");
                        assert_eq!(got, st["extra"][format!("state {}", m.machine)], "{key} step {k}: machine state");
                    }
                    SlotGraphNode::Seq { player, .. } => {
                        assert_eq!(
                            hx(f64::from(slot_player_time(g, p.as_ref(), *player))),
                            st["extra"][format!("t {player}")],
                            "{key} step {k}: player time"
                        );
                        assert_eq!(
                            hx(f64::from(slot_player_time_from_end(g, p.as_ref(), *player, &len))),
                            st["extra"][format!("te {player}")],
                            "{key} step {k}: player time from end"
                        );
                    }
                    _ => {}
                }
            }
            checked += 1;
        }
    }
    eprintln!("{checked} steps checked");
}
