//! The input codec against the oracle's golden vectors (`encode-input.json`): the same
//! bytes both ways, and `decode(encode(i))` equal to `canon_input(i)` (and to the oracle's own decode, bit for bit on
//! the sticks). The vectors are the oracle's at 22 bytes: the port's 4 more (the right stick's Y) are zero with no Y
//! (the oracle's fixture widener).

use mvs_net::codec::{decode, encode, same_input};
use mvs_protocol::vectors;
use mvs_sim::InputState;
use mvs_sim::input::{AttackBtn, Btn, Dir, Order, canon_input};
use serde_json::Value;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex")).collect()
}

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_be_bytes(unhex(s).try_into().expect("8 bytes")))
}

fn flag(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn press(v: &Value) -> AttackBtn {
    if v == "attack" { AttackBtn::Attack } else { AttackBtn::Special }
}

fn btn(v: &Value) -> Btn {
    match v.as_str().expect("a button") {
        "attack" => Btn::Attack,
        "special" => Btn::Special,
        "jump" => Btn::Jump,
        "dodge" => Btn::Dodge,
        _ => Btn::Item,
    }
}

const DIRS: [Dir; 10] = [
    Dir::Up,
    Dir::Down,
    Dir::Fwd,
    Dir::Back,
    Dir::Neutral,
    Dir::Default,
    Dir::FwdUp,
    Dir::FwdDown,
    Dir::BackUp,
    Dir::BackDown,
];

/// The vector's input (sticks from their f64 bits, since JSON loses -0).
fn input_of(v: &Value) -> InputState {
    let b = &v["bits"];
    let u8_of = |k: &str| v.get(k).map(|n| u8::try_from(n.as_u64().expect("a number")).expect("a byte"));
    InputState {
        x: mvs_sim::math::narrow(bits(b["x"].as_str().expect("x bits"))),
        y: mvs_sim::math::narrow(bits(b["y"].as_str().expect("y bits"))),
        attack: flag(v, "attack"),
        special: flag(v, "special"),
        jump: flag(v, "jump"),
        dodge: flag(v, "dodge"),
        jump_local: flag(v, "jumpLocal"),
        item: flag(v, "item"),
        order: v.get("order").map(|o| {
            let list = o.as_array().expect("an order");
            let mut order = Order { len: u8::try_from(list.len()).expect("a length"), ..Order::default() };
            for (n, k) in list.iter().enumerate().take(5) {
                order.btns[n] = Some(btn(k));
            }
            order.len = order.len.min(5);
            order
        }),
        down_btn: flag(v, "downBtn"),
        atk_dir: u8_of("atkDir").map(|d| DIRS[usize::from(d)]),
        force_dir: u8_of("forceDir"),
        force_press: v.get("forcePress").map(press),
        force_release: v.get("forceRelease").map(press),
        force: flag(v, "force"),
        neutral_evade: flag(v, "neutralEvade"),
        taunt: u8_of("taunt"),
        taunt_held: flag(v, "tauntHeld"),
        rsx: b.get("rsx").map(|s| mvs_sim::math::narrow(bits(s.as_str().expect("rsx bits")))),
        rsy: None,
        axis: flag(v, "axis"),
        call_x: flag(v, "callX"),
        call_y: flag(v, "callY"),
    }
}

#[test]
fn encode_input_vectors() {
    let Some(v) = vectors::read("encode-input.json") else { return };
    let cases = v["cases"].as_array().expect("cases");
    for (n, c) in cases.iter().enumerate() {
        let i = input_of(&c["input"]);
        let wire = encode(&i);
        assert_eq!(wire.to_vec(), unhex(c["bytes"].as_str().expect("bytes")), "case {n}: the bytes");
        let back = decode(&wire);
        let canon = canon_input(&i);
        assert_eq!(back, canon, "case {n}: decode(encode(i)) is canon_input(i)");
        // The sticks as the oracle decoded them, to the bit (-0 and float32 rounding).
        let want = &c["decoded"]["bits"];
        assert_eq!(
            f64::from(back.x).to_bits(),
            u64::from_str_radix(want["x"].as_str().expect("x"), 16).expect("hex"),
            "case {n}: x bits"
        );
        assert_eq!(
            f64::from(back.y).to_bits(),
            u64::from_str_radix(want["y"].as_str().expect("y"), 16).expect("hex"),
            "case {n}: y bits"
        );
        if let Some(r) = want.get("rsx") {
            assert_eq!(
                back.rsx.map(|r| f64::from(r).to_bits()),
                Some(u64::from_str_radix(r.as_str().expect("rsx"), 16).expect("hex")),
                "case {n}: rsx bits"
            );
        }
        assert!(same_input(&i, &canon), "case {n}: an input and its canon are the same on the wire");
    }
    eprintln!("encode-input: {} cases", cases.len());
}
