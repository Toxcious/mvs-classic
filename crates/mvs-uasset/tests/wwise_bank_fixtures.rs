//! `natives::wwise_bank::read_bank` against the oracle's reader's trees: the folder named by `MVS_BANK_FIXTURES` holds
//! `<n>.bnk` (a bank's bytes, an AkAssetData's bulk) and `<n>.json` (the oracle's `readBank` tree of it,
//! JSON.stringify'd). Each tree must equal the Rust one exactly, key order included; a float is equal when the JSON
//! number rounds to the same f32. The fixtures are the build's data, made on the developer's machine and never
//! committed: without the variable the test says so and passes.

use std::path::Path;

use mvs_uasset::natives::wwise_bank::read_bank;
use mvs_uasset::value::Value;

/// A JSON value with its object keys in order and its numbers as written.
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.s.get(self.i), Some(&c), "json: expected {} at {}", c as char, self.i);
        self.i += 1;
    }
    fn value(&mut self) -> J {
        self.ws();
        match self.s[self.i] {
            b'n' => {
                self.i += 4;
                J::Null
            }
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'"' => J::Str(self.string()),
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.s[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(v);
                }
                loop {
                    v.push(self.value());
                    self.ws();
                    self.i += 1;
                    if self.s[self.i - 1] == b']' {
                        return J::Arr(v);
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.s[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(v);
                }
                loop {
                    self.ws();
                    let k = self.string();
                    self.eat(b':');
                    v.push((k, self.value()));
                    self.ws();
                    self.i += 1;
                    if self.s[self.i - 1] == b'}' {
                        return J::Obj(v);
                    }
                }
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                J::Num(String::from_utf8(self.s[start..self.i].to_vec()).unwrap())
            }
        }
    }
    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut units: Vec<u16> = Vec::new();
        let mut out = String::new();
        let flush = |units: &mut Vec<u16>, out: &mut String| {
            out.push_str(&String::from_utf16_lossy(units));
            units.clear();
        };
        loop {
            let c = self.s[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    let ch = match e {
                        b'u' => {
                            let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
                            self.i += 4;
                            units.push(u16::from_str_radix(h, 16).unwrap());
                            continue;
                        }
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        other => other as char,
                    };
                    flush(&mut units, &mut out);
                    out.push(ch);
                }
                _ => {
                    flush(&mut units, &mut out);
                    // a UTF-8 sequence: copy its bytes whole
                    let start = self.i - 1;
                    let mut end = self.i;
                    while end < self.s.len() && (self.s[end] & 0xc0) == 0x80 {
                        end += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.s[start..end]).unwrap());
                    self.i = end;
                }
            }
        }
        flush(&mut units, &mut out);
        out
    }
}

fn non_finite(s: &str) -> Option<f64> {
    match s {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn same_float(got: f64, j: &J, as_f32: bool) -> bool {
    let want = match j {
        J::Num(t) => t.parse::<f64>().unwrap(),
        J::Str(s) => match non_finite(s) {
            Some(v) => v,
            None => return false,
        },
        _ => return false,
    };
    if want.is_nan() || got.is_nan() {
        return want.is_nan() && got.is_nan();
    }
    if as_f32 { want as f32 == got as f32 } else { want == got }
}

fn compare(v: &Value, j: &J, path: &str, out: &mut Vec<String>) {
    let ok = match (v, j) {
        (Value::Null, J::Null) => true,
        (Value::Bool(a), J::Bool(b)) => a == b,
        (Value::Int(a), J::Num(t)) => t.parse::<i64>().ok() == Some(*a),
        (Value::UInt(a), J::Num(t)) => t.parse::<u64>().ok() == Some(*a),
        (Value::Float(a), _) => same_float(f64::from(*a), j, true),
        (Value::Double(a), _) => same_float(*a, j, false),
        (Value::Str(a), J::Str(b)) => a == b,
        (Value::Array(a), J::Arr(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: {} items, the oracle has {}", a.len(), b.len()));
                return;
            }
            for (k, (x, y)) in a.iter().zip(b).enumerate() {
                compare(x, y, &format!("{path}[{k}]"), out);
            }
            true
        }
        (Value::Struct(a), J::Obj(b)) => {
            let ka: Vec<&str> = a.keys().map(String::as_str).collect();
            let kb: Vec<&str> = b.iter().map(|(k, _)| k.as_str()).collect();
            if ka != kb {
                out.push(format!("{path}: keys {ka:?}, the oracle has {kb:?}"));
                return;
            }
            for ((k, x), (_, y)) in a.iter().zip(b) {
                compare(x, y, &format!("{path}.{k}"), out);
            }
            true
        }
        _ => false,
    };
    if !ok {
        let shown = match j {
            J::Null => "null".to_owned(),
            J::Bool(b) => b.to_string(),
            J::Num(t) => t.clone(),
            J::Str(s) => format!("{s:?}"),
            J::Arr(_) => "[..]".to_owned(),
            J::Obj(_) => "{..}".to_owned(),
        };
        out.push(format!("{path}: {v:?}, the oracle has {shown}"));
    }
}

fn hirc_items(j: &J) -> usize {
    let J::Obj(o) = j else { return 0 };
    o.iter().find(|(k, _)| k == "Hierarchies").map_or(0, |(_, h)| if let J::Arr(a) = h { a.len() } else { 0 })
}

#[test]
fn banks_equal_the_ts_trees() {
    let Ok(dir) = std::env::var("MVS_BANK_FIXTURES") else {
        println!("skip wwise_bank_fixtures: MVS_BANK_FIXTURES names no folder of <n>.bnk + <n>.json");
        return;
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("MVS_BANK_FIXTURES: a folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "bnk"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "{dir}: no .bnk files");
    let (mut banks, mut items, mut bad) = (0usize, 0usize, Vec::new());
    for bnk in &names {
        let bytes = std::fs::read(bnk).unwrap();
        let text = std::fs::read(Path::new(bnk).with_extension("json")).unwrap();
        let want = Parser { s: &text, i: 0 }.value();
        let name = bnk.file_name().unwrap().to_string_lossy();
        match read_bank(&bytes, "bank") {
            Ok(got) => {
                let mut diffs = Vec::new();
                compare(&got, &want, "", &mut diffs);
                if let Some(d) = diffs.first() {
                    bad.push(format!("{name}: {} differences, first {d}", diffs.len()));
                }
            }
            Err(e) => bad.push(format!("{name}: {e}")),
        }
        banks += 1;
        items += hirc_items(&want);
    }
    println!("wwise_bank_fixtures: {banks} banks, {items} hierarchy items compared, {} differ", bad.len());
    assert!(bad.is_empty(), "{}", bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}
