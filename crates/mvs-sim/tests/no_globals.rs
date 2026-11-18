//! The sim keeps every match state in the World: a thread-local or a mutable static would escape snapshots,
//! rollback and a second World in the same process. This test fails on any in `src/`.

use std::path::Path;

/// Banned anywhere.
const BANNED: &[&str] = &["thread_local!", "static mut "];
/// Banned in a `static`: interior mutability. A read-only static (the registry, a default) is fine.
const MUTABLE: &[&str] = &["Mutex<", "RwLock<", "RefCell<", "Cell<", "Atomic"];
/// Scratch that holds no match state (each use writes all of it before reading it), by file and the static's name:
/// the polygon sweep's hull buffers, one per thread so a sweep does not allocate; the bot log's lines (ai/log.rs), a
/// tool's output the sim only writes, off unless a tool turns it on.
const SCRATCH: &[(&str, &str)] = &[("solids.rs", "TRACER"), ("ai/log.rs", "SINK")];

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).expect("src is readable") {
        let p = e.expect("a dir entry").path();
        if p.is_dir() {
            scan(&p, hits);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let text = std::fs::read_to_string(&p).expect("a source file is readable");
            let lines: Vec<&str> = text.lines().collect();
            for (n, line) in lines.iter().enumerate() {
                // (a `thread_local!` whose next line declares an allowed scratch static, and that static's line)
                let scratch =
                    |l: &str| SCRATCH.iter().any(|(f, s)| p.ends_with(f) && l.contains(&format!("static {s}:")));
                if scratch(line) || (line.contains("thread_local!") && lines.get(n + 1).is_some_and(|l| scratch(l))) {
                    continue;
                }
                let code = line.split("//").next().unwrap_or("");
                let is_static = code.trim_start().starts_with("static ")
                    || code.contains("pub static ")
                    || code.contains("pub(crate) static ");
                for b in BANNED.iter().chain(MUTABLE.iter().filter(|_| is_static)) {
                    if code.contains(b) {
                        hits.push(format!("{}:{}: {}", p.display(), n + 1, line.trim()));
                    }
                }
            }
        }
    }
}

#[test]
fn no_state_outside_the_world() {
    let mut hits = Vec::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut hits);
    assert!(hits.is_empty(), "sim state outside the World:\n{}", hits.join("\n"));
}
