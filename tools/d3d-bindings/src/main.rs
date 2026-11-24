//! Writes mvs-d3d's Linux bindings: windows-bindgen over the names in `crates/mvs-d3d/names.txt`, then the changes
//! DXVK-native needs (its `wchar_t` WCHAR, no Windows string types), then rustfmt with the repo's settings.
//!   cargo run --manifest-path tools/d3d-bindings/Cargo.toml             write crates/mvs-d3d/src/linux/bindings.rs
//!   cargo run --manifest-path tools/d3d-bindings/Cargo.toml -- --check  exit 1 if the committed file differs

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use windows_metadata::Type;
use windows_metadata::reader::Index;

/// What the generated file starts with, ahead of windows-bindgen's own lines.
const HEADER: &str =
    "// The D3D11, DXGI and Direct3D items mvs-d3d gives on Linux, for DXVK-native: `windows-bindgen` output over
// `names.txt`, with WCHAR arrays as 4-byte `wchar_t` and the local `PCSTR` and `PSTR`. Written by
// `tools/d3d-bindings`; do not edit by hand, run it again.
";

/// The narrow string pointers `windows_core` defines only on Windows, and the local types that stand in for them
/// (same layout: a `char` is one byte under DXVK-native too).
const NARROW: &[(&str, &str)] =
    &[("windows_core::PCSTR", "crate::linux::strs::PCSTR"), ("windows_core::PSTR", "crate::linux::strs::PSTR")];

/// What must not be left in the file: Windows-only string types (DXVK-native's `wchar_t` is 4 bytes, so a 2-byte
/// wide pointer would be wrong), `Free` impls (the trait is Windows-only) and links to libraries DXVK does not give.
const REFUSED: &[&str] = &[
    "PCWSTR",
    "PWSTR",
    "windows_core::PSTR",
    "HSTRING",
    "windows_core::PCSTR",
    "windows_core::Free",
    "windows_core::w!",
    "windows_core::s!",
    "*const u16",
    "*mut u16",
    "\"d3dcompiler",
    "\"kernel32",
];

fn main() -> ExitCode {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(other) => {
            eprintln!("unknown argument `{other}` (only --check)");
            return ExitCode::from(2);
        }
    };
    match run(check) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("d3d-bindings: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(check: bool) -> Result<ExitCode, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().map_err(|e| e.to_string())?;
    // rustfmt reads its settings from the folder it runs in: the repo's own, so the file is as `cargo fmt` leaves it.
    std::env::set_current_dir(&root).map_err(|e| e.to_string())?;
    let crate_dir = root.join("crates/mvs-d3d");
    let names_file = crate_dir.join("names.txt");
    let out_file = crate_dir.join("src/linux/bindings.rs");

    let names = read_names(&names_file)?;
    let (raw, skipped) = generate(&names)?;
    let wchar = wchar_fields(&bindgen_winmd()?)?;
    let (text, rewritten) = post_process(&raw, &wchar)?;
    let text = rustfmt(&format!("{HEADER}{text}"))?;

    if check {
        let old = std::fs::read_to_string(&out_file).map_err(|e| format!("{}: {e}", out_file.display()))?;
        if old != text {
            eprintln!(
                "{} is not what the generator writes from names.txt: run `cargo run --manifest-path \
                 tools/d3d-bindings/Cargo.toml`",
                out_file.display()
            );
            return Ok(ExitCode::FAILURE);
        }
        println!("{} is up to date", out_file.display());
    } else {
        std::fs::write(&out_file, &text).map_err(|e| format!("{}: {e}", out_file.display()))?;
        println!("wrote {} ({} lines)", out_file.display(), text.lines().count());
    }
    println!("{} methods left out (they take a type names.txt does not name):", skipped.len());
    for method in &skipped {
        println!("  {method}");
    }
    println!("structs with WCHAR arrays rewritten to u32 (wchar_t):");
    for (name, fields) in &rewritten {
        println!("  {name}: {}", fields.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

/// The names, one a line; `#` starts a comment.
fn read_names(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let names: Vec<String> = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if names.is_empty() {
        return Err(format!("{}: no names", path.display()));
    }
    Ok(names)
}

/// windows-bindgen's module tree (`Windows::Win32::...`) for the names, read back as text, and the methods it left
/// out.
fn generate(names: &[String]) -> Result<(String, Vec<String>), String> {
    let tmp = std::env::temp_dir().join(format!("d3d-bindings-{}.rs", std::process::id()));
    let mut args =
        vec!["--out".to_string(), tmp.to_string_lossy().into_owned(), "--rustfmt".into(), "edition=2024".into()];
    args.push("--filter".into());
    args.extend(names.iter().cloned());
    let warnings = windows_bindgen::bindgen(&args);
    let text = std::fs::read_to_string(&tmp).map_err(|e| format!("{}: {e}", tmp.display()));
    let _ = std::fs::remove_file(&tmp);
    // A method that takes a type not named is left out (its vtable slot stays, as a `usize`): that is how the file
    // stays filtered to what the code uses. Any other warning is an error.
    let mut skipped = Vec::new();
    for w in warnings.iter() {
        match w.strip_prefix("skipping `").and_then(|r| r.split_once('`')) {
            Some((method, _)) => skipped.push(method.to_string()),
            None => return Err(format!("windows-bindgen warned: {w}")),
        }
    }
    Ok((text?, skipped))
}

/// The Win32 metadata file windows-bindgen generates from (it ships it in its package's `default/` folder).
fn bindgen_winmd() -> Result<PathBuf, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--locked", "--manifest-path"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err("cargo metadata failed".into());
    }
    let json = String::from_utf8_lossy(&out.stdout);
    let key = "\"manifest_path\":\"";
    for part in json.split(key).skip(1) {
        let path = PathBuf::from(&part[..part.find('"').unwrap_or(0)]);
        let dir = path.parent().unwrap_or(Path::new(""));
        if dir.file_name().is_some_and(|n| n.to_string_lossy().starts_with("windows-bindgen-")) {
            return Ok(dir.join("default/Windows.Win32.winmd"));
        }
    }
    Err("windows-bindgen's package is not in cargo metadata".into())
}

/// Every struct or union field in the Win32 metadata whose type is an array of fixed size, by `(type, field)`:
/// true for a WCHAR (`char`) array, false for any other.
fn wchar_fields(winmd: &Path) -> Result<BTreeMap<(String, String), bool>, String> {
    let index = Index::read(winmd).ok_or_else(|| format!("{}: not readable metadata", winmd.display()))?;
    let mut fields = BTreeMap::new();
    for (_, name, def) in index.iter() {
        for f in def.fields() {
            if let Type::ArrayFixed(elem, _) = f.ty() {
                let wide = *elem == Type::Char;
                let entry = fields.entry((name.to_string(), f.name().to_string())).or_insert(false);
                *entry |= wide;
            }
        }
    }
    Ok(fields)
}

/// The Linux changes, line by line over windows-bindgen's formatted output. Returns the text and, per struct, the
/// fields it rewrote from `[u16; N]` to `[u32; N]`.
fn post_process(
    raw: &str,
    wchar: &BTreeMap<(String, String), bool>,
) -> Result<(String, BTreeMap<String, Vec<String>>), String> {
    let mut out = String::with_capacity(raw.len());
    let mut rewritten: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut errors = BTreeSet::new();
    // The struct or union being read, with the indent of its closing brace.
    let mut aggregate: Option<(String, usize)> = None;
    let mut lines = raw.lines();
    while let Some(line) = lines.next() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        // `impl windows_core::Free for X { ... }`: the trait exists only on Windows; DXVK's handles are not closed
        // through it.
        if trimmed.starts_with("impl windows_core::Free for ") {
            if !trimmed.ends_with('}') {
                skip_block(&mut lines, indent)?;
            }
            continue;
        }
        // A string constant built with `w!` or `s!`: the macros are Windows-only.
        if trimmed.starts_with("pub const ")
            && (trimmed.contains("windows_core::w!") || trimmed.contains("windows_core::s!"))
        {
            let mut statement = trimmed.to_string();
            while !statement.ends_with(';') {
                statement = lines.next().ok_or("unterminated constant")?.trim().to_string();
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("pub struct ").or_else(|| trimmed.strip_prefix("pub union ")) {
            if let Some(name) = rest.strip_suffix(" {") {
                aggregate = Some((name.to_string(), indent));
            }
        } else if trimmed == "}" && aggregate.as_ref().is_some_and(|(_, i)| *i == indent) {
            aggregate = None;
        }

        let mut line = line.to_string();
        for (from, to) in NARROW {
            line = line.replace(from, to);
        }
        if let (Some((name, _)), Some((field, ty))) =
            (&aggregate, trimmed.strip_prefix("pub ").and_then(|f| f.split_once(": ")))
            && let Some(len) = ty.strip_prefix("[u16; ").and_then(|t| t.strip_suffix("],"))
        {
            match wchar.get(&(name.clone(), field.to_string())) {
                Some(true) => {
                    line = line.replace(&format!("[u16; {len}]"), &format!("[u32; {len}]"));
                    rewritten.entry(name.clone()).or_default().push(field.to_string());
                }
                Some(false) => {}
                None => {
                    errors.insert(format!("{name}.{field}: a [u16; {len}] field the metadata does not name"));
                }
            }
        }
        out.push_str(&line);
        out.push('\n');
    }

    // Every WCHAR array of every struct in the file is rewritten: a struct the file holds whose metadata has a
    // WCHAR field left as it was is an error (a parse the loop above missed).
    let defined = struct_names(&out);
    for ((name, field), wide) in wchar {
        if *wide && defined.contains(name.as_str()) && !rewritten.get(name).is_some_and(|f| f.contains(field)) {
            errors.insert(format!("{name}.{field}: a WCHAR array left as [u16; N]"));
        }
    }
    for refused in REFUSED {
        for (n, l) in out.lines().enumerate() {
            if l.contains(refused) {
                errors.insert(format!("line {}: `{refused}` left in the output: {}", n + 1, l.trim()));
            }
        }
    }
    if !errors.is_empty() {
        let list: Vec<_> = errors.into_iter().collect();
        return Err(format!("the bindings cannot be made for DXVK-native:\n  {}", list.join("\n  ")));
    }
    Ok((out, rewritten))
}

/// Skips the lines of a block whose opening line has been read, up to its closing brace at `indent`.
fn skip_block<'a>(lines: &mut impl Iterator<Item = &'a str>, indent: usize) -> Result<(), String> {
    for line in lines {
        if line.trim() == "}" && line.len() - line.trim_start().len() == indent {
            return Ok(());
        }
    }
    Err("an `impl windows_core::Free` block with no end".into())
}

/// The names of the structs and unions the text defines.
fn struct_names(text: &str) -> BTreeSet<&str> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("pub struct ").or_else(|| t.strip_prefix("pub union "))
        })
        .map(|rest| rest.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or(""))
        .collect()
}

/// The text through rustfmt, with the repo's settings (read from the current folder) and edition.
fn rustfmt(text: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("rustfmt: {e}"))?;
    child.stdin.take().ok_or("rustfmt: no stdin")?.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("rustfmt failed".into());
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}
