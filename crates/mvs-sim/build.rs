//! Collects the code's own name literals: every `name!("...")` in `src/` becomes an entry of `CODE`, interned first
//! into each match's `Names`, so a literal's `Name` is known at compile time (`names::code_id`).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

const OPEN: &str = "name!(";

fn scan(dir: &Path, out: &mut BTreeSet<String>) {
    let mut entries: Vec<_> =
        std::fs::read_dir(dir).expect("src is readable").map(|e| e.expect("a dir entry").path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            scan(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&p).expect("a source file is readable");
            let mut rest = text.as_str();
            // rustfmt may put the literal on a line of its own: whitespace around it is allowed.
            while let Some(i) = rest.find(OPEN) {
                rest = rest[i + OPEN.len()..].trim_start();
                let Some(after) = rest.strip_prefix('"') else { continue };
                let end = after.find('"').unwrap_or_else(|| panic!("an unclosed name! literal in {}", p.display()));
                let lit = &after[..end];
                assert!(!lit.contains('\\'), "a name! literal holds no escape: {lit:?} in {}", p.display());
                rest = after[end + 1..].trim_start();
                assert!(rest.starts_with(')'), "name! takes one string literal: {lit:?} in {}", p.display());
                out.insert(lit.to_owned());
            }
        }
    }
}

fn main() {
    let src = Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir")).join("src");
    println!("cargo:rerun-if-changed={}", src.display());
    let mut lits = BTreeSet::new();
    scan(&src, &mut lits);
    lits.remove("");
    let mut code = String::from(
        "/// The code's name literals (`name!`), sorted; interned after `\"\"`, so entry `i` is `Name(i + 1)`.\n",
    );
    code.push_str("pub(crate) const CODE: &[&str] = &[\n");
    for l in &lits {
        writeln!(code, "    {l:?},").expect("a String takes writes");
    }
    code.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("code_names.rs");
    std::fs::write(out, code).expect("OUT_DIR is writable");
}
