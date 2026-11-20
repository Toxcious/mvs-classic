//! The release guard: no file of the build, no developer's key and no EOS secret in a release
//! zip, and nothing but names, sizes and hashes in the build list the exe embeds. It never prints a key or a secret.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mvs_release::{
    EOS_CREDENTIALS, EOS_DLL, GAME_EXE, LICENSE, LICENSES_DIR, ONLINE_JSON, RELEASE_FILES, SELF_CHECK_JSON, in_game,
};

use mvs_release::zip::{self, Entry};

/// Extensions of the build's data: paks and their containers, packages, Wwise sounds and banks, text and fonts, Bink
/// videos, shader code, converted meshes, sounds and anim files, the store's packs.
const GAME_EXT: &[&str] = &[
    ".pak",
    ".utoc",
    ".ucas",
    ".sig",
    ".uasset",
    ".uexp",
    ".ubulk",
    ".umap",
    ".wem",
    ".bnk",
    ".locres",
    ".ufont",
    ".bk2",
    ".bik",
    ".ushaderbytecode",
    ".mvt",
    ".mva",
    ".glb",
    ".ogg",
    ".pack",
];

/// The notices under `licenses/`.
pub const NOTICE_IMPORTER: &str = "NOTICE-importer.txt";
pub const LICENSE_CUE4PARSE: &str = "LICENSE-CUE4Parse.txt";

/// Which zip a guard reads: each has its own list of files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zip {
    /// The game's: `MVS-Classic-<version>.zip`.
    Game,
    /// The relay's: `MVS-Classic-Relay-<version>.zip`.
    Relay,
}

/// The files a zip of this kind may hold.
fn own_files(kind: Zip) -> Vec<String> {
    match kind {
        Zip::Game => vec![
            mvs_release::LAUNCHER_EXE.to_owned(),
            LICENSE.to_owned(),
            in_game(GAME_EXE),
            in_game(ONLINE_JSON),
            in_game(SELF_CHECK_JSON),
            in_game(RELEASE_FILES),
            in_game(&format!("{LICENSES_DIR}/{NOTICE_IMPORTER}")),
            in_game(&format!("{LICENSES_DIR}/{LICENSE_CUE4PARSE}")),
            in_game(EOS_DLL),
            in_game(EOS_CREDENTIALS),
        ],
        Zip::Relay => ["mvs-relay.exe", "mvs-relay", "README.md", LICENSE].map(str::to_owned).to_vec(),
    }
}

/// The developer's AES key, read from a `content/local.json` (`{ "key": "0x<64 hex digits>" }`).
pub fn read_key(file: &Path) -> Result<[u8; 32], String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("guard: {}: {e}", file.display()))?;
    // (no parser message: it quotes the text, which holds the key)
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| format!("guard: {} is not JSON", file.display()))?;
    let hex = json.get("key").and_then(|k| k.as_str()).unwrap_or("");
    let hex = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X")).unwrap_or(hex);
    let bad = || format!("guard: {} holds no key of 64 hex digits", file.display());
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let mut key = [0; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
    }
    Ok(key)
}

/// The keys a build list may hold: its id, the paks' and files' names, paths, sizes and hashes.
const BUILD_KEYS: &[&str] = &["id", "paks", "files", "tables", "name", "path", "size", "sha256"];

/// The build list the exe embeds (`crates/mvs-pak/build.json`): a finding when it does not read or holds a key
/// other than names, paths, sizes and hashes.
pub fn check_build_json(where_: &str, text: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![format!("{where_}: not JSON")];
    };
    let mut bad = std::collections::BTreeSet::new();
    let mut stack = vec![&json];
    while let Some(v) = stack.pop() {
        match v {
            serde_json::Value::Array(a) => stack.extend(a),
            serde_json::Value::Object(o) => {
                for (k, x) in o {
                    if !BUILD_KEYS.contains(&k.as_str()) {
                        bad.insert(k.clone());
                    }
                    stack.push(x);
                }
            }
            _ => {}
        }
    }
    if bad.is_empty() {
        Vec::new()
    } else {
        let keys: Vec<_> = bad.into_iter().collect();
        vec![format!("{where_}: keys other than names, sizes and hashes: {}", keys.join(", "))]
    }
}

/// One form of the key the search looks for.
struct Form {
    name: &'static str,
    bytes: Vec<u8>,
    /// searched in the file's bytes with ASCII letters lowered (hex: any case)
    any_case: bool,
}

/// Every form of the key: hex (any case), its 32 raw bytes, and base64 / base64url at each of the three byte
/// alignments a key inside longer encoded data can have (only the characters its own bytes decide: the first
/// group's that mix in earlier bytes and the last partial group's are left out).
fn key_forms(key: &[u8; 32]) -> Vec<Form> {
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let mut forms = vec![
        Form { name: "hex", bytes: hex.into_bytes(), any_case: true },
        Form { name: "raw bytes", bytes: key.to_vec(), any_case: false },
    ];
    for a in 0..3 {
        let mut enc = vec![0; a];
        enc.extend_from_slice(key);
        let whole = enc.len() / 3 * 4;
        let b64 = STANDARD.encode(&enc)[[0, 2, 3][a]..whole].to_owned();
        let url = b64.replace('+', "-").replace('/', "_");
        forms.push(Form { name: "base64", bytes: b64.into_bytes(), any_case: false });
        forms.push(Form { name: "base64url", bytes: url.into_bytes(), any_case: false });
    }
    forms
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// The EOS client id and secret of an `eos.json`: the strings that must be in no other file. None when it does not
/// read or holds no id and secret of 16 characters or more.
pub fn eos_secrets(json: &[u8]) -> Option<Vec<Vec<u8>>> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let secrets: Vec<Vec<u8>> = ["clientSecret", "clientId"]
        .iter()
        .filter_map(|k| v.get(k)?.as_str().filter(|s| s.len() >= 16).map(|s| s.as_bytes().to_vec()))
        .collect();
    (secrets.len() == 2).then_some(secrets)
}

/// What a guard found: one line per finding (no key, no secret in them) and what it read.
pub struct Report {
    pub findings: Vec<String>,
    /// The EOS credentials it searched for (those given and the zip's own `resources/eos.json`'s), for the next
    /// zip's check.
    pub secrets: Vec<Vec<u8>>,
    pub files: usize,
    pub bytes: u64,
}

/// The guard over a zip's files. `key` is the developer's key (None: that search is skipped); `secrets` are EOS
/// credentials known from elsewhere (the pack step's), searched in every file but this zip's own `resources/eos.json`.
pub fn check_files(where_: &str, entries: &[Entry], kind: Zip, key: Option<&[u8; 32]>, secrets: &[Vec<u8>]) -> Report {
    let own = own_files(kind);
    let mut findings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut secrets = secrets.to_vec();
    if kind == Zip::Game
        && let Some(e) = entries.iter().find(|e| e.name == in_game(EOS_CREDENTIALS))
    {
        match eos_secrets(&e.data) {
            Some(s) => secrets.extend(s),
            None => findings.push(format!("{where_}: {}: no client id and secret", e.name)),
        }
    }
    let forms = key.map(key_forms).unwrap_or_default();
    let mut bytes = 0;
    for e in entries {
        let name = &e.name;
        let lower = name.to_ascii_lowercase();
        let file = lower.rsplit('/').next().unwrap_or("");
        if let Some(dot) = file.rfind('.')
            && GAME_EXT.contains(&&file[dot..])
        {
            findings.push(format!("{where_}: {name}: a game data file ({})", &file[dot..]));
        }
        if name.ends_with('/') {
            if !own.iter().any(|o| o.starts_with(name.as_str())) {
                findings.push(format!("{where_}: {name}: a folder that is not one of the release's own"));
            }
        } else if !own.contains(name) {
            findings.push(format!("{where_}: {name}: not one of the release's own files"));
        }
        if !seen.insert(name.as_str()) {
            findings.push(format!("{where_}: {name}: in the zip twice"));
        }
        bytes += e.data.len() as u64;
        if !forms.is_empty() {
            let lowered = e.data.to_ascii_lowercase();
            for f in &forms {
                if contains(if f.any_case { &lowered } else { &e.data }, &f.bytes) {
                    findings.push(format!("{where_}: {name}: the developer's key (content/local.json), as {}", f.name));
                }
            }
        }
        if lower.ends_with(".exe") {
            for dll in vc_runtime_imports(&e.data) {
                findings.push(format!(
                    "{where_}: {name}: needs {dll}, which Windows does not hold (link the C runtime statically)"
                ));
            }
        }
        let theirs = kind == Zip::Game && *name == in_game(EOS_CREDENTIALS);
        if !theirs && secrets.iter().any(|s| contains(&e.data, s)) {
            findings
                .push(format!("{where_}: {name}: the EOS client credentials, outside {}", in_game(EOS_CREDENTIALS)));
        }
    }
    Report { findings, secrets, files: entries.len(), bytes }
}

/// The DLLs of the Visual C++ Redistributable a PE file imports, plainly or delay-loaded (VCRUNTIME140.dll, MSVCP140.dll
/// and the like). Windows 10 and 11 hold the Universal CRT (`api-ms-win-crt-*`, `ucrtbase.dll`) but not these, so a
/// release exe that imports one does not start on a PC without the Redistributable. Data that is not a PE file
/// imports nothing.
fn vc_runtime_imports(pe: &[u8]) -> Vec<String> {
    let u16_at = |o: usize| pe.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| pe.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let names = || -> Option<Vec<String>> {
        if pe.get(..2)? != b"MZ" {
            return None;
        }
        let head = u32_at(0x3c)?;
        if pe.get(head..head + 4)? != b"PE\0\0" {
            return None;
        }
        let sections = u16_at(head + 6)? as usize;
        let opt = head + 24;
        let dirs = opt + if u16_at(opt)? == 0x20b { 112 } else { 96 };
        let table = opt + u16_at(head + 20)? as usize;
        let file_at = |rva: usize| {
            (0..sections).find_map(|i| {
                let s = table + i * 40;
                let (size, va, raw) = (u32_at(s + 8)?, u32_at(s + 12)?, u32_at(s + 20)?);
                (rva >= va && rva < va + size).then(|| rva - va + raw)
            })
        };
        let text = |rva: usize| {
            let at = file_at(rva)?;
            let len = pe.get(at..)?.iter().position(|&b| b == 0)?;
            Some(String::from_utf8_lossy(&pe[at..at + len]).into_owned())
        };
        let mut out = Vec::new();
        // the import table (20-byte descriptors, the name's RVA at +12) and the delay-load table (32-byte ones, at +4)
        for (dir, size, name_at) in [(1, 20, 12), (13, 32, 4)] {
            let rva = u32_at(dirs + dir * 8)?;
            if rva == 0 {
                continue;
            }
            let mut at = file_at(rva)?;
            while let Some(name) = u32_at(at + name_at).filter(|&n| n != 0) {
                out.push(text(name)?);
                at += size;
            }
        }
        Some(out)
    };
    names()
        .unwrap_or_default()
        .into_iter()
        .filter(|n| {
            let n = n.to_ascii_lowercase();
            n.starts_with("vcruntime") || n.starts_with("msvcp") || n.starts_with("concrt") || n == "ucrtbased.dll"
        })
        .collect()
}

/// The guard over a zip file.
pub fn check_zip(path: &Path, kind: Zip, key: Option<&[u8; 32]>, secrets: &[Vec<u8>]) -> Result<Report, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("guard: {}: {e}", path.display()))?;
    let entries = zip::read(&bytes).map_err(|e| format!("guard: {}: {e}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(check_files(&name, &entries, kind, key, secrets))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, data: &[u8]) -> Entry {
        Entry { name: name.to_owned(), data: data.to_vec(), executable: false }
    }

    const KEY: [u8; 32] = [
        0x3a, 0xf1, 0x07, 0xc4, 0x9e, 0x22, 0x5b, 0x80, 0x11, 0xd9, 0x6c, 0x4f, 0xe0, 0x73, 0x2a, 0xb5, 0x08, 0x91,
        0xfe, 0x46, 0x3d, 0xc2, 0x57, 0x6a, 0x19, 0xe8, 0xb4, 0x02, 0x7f, 0xcd, 0x55, 0x9b,
    ];
    const SECRET: &str = "FAKE-client-secret-0123456789abcdef";
    const CLIENT: &str = "FAKE-client-id-0123456789";

    fn clean() -> Vec<Entry> {
        let eos = format!(
            r#"{{"productId":"p","sandboxId":"s","deploymentId":"d","clientId":"{CLIENT}","clientSecret":"{SECRET}"}}"#
        );
        vec![
            entry(&in_game(GAME_EXE), b"MZ the program"),
            entry(mvs_release::LAUNCHER_EXE, b"MZ the launcher"),
            entry(LICENSE, b"MIT"),
            entry("game/licenses/NOTICE-importer.txt", b"notice"),
            entry("game/licenses/LICENSE-CUE4Parse.txt", b"licence"),
            entry(&in_game(EOS_DLL), b"MZ fake dll"),
            entry(&in_game(EOS_CREDENTIALS), eos.as_bytes()),
        ]
    }

    fn findings(entries: &[Entry]) -> Vec<String> {
        check_files("t.zip", entries, Zip::Game, Some(&KEY), &[]).findings
    }

    /// The release with `data` planted in the game exe's bytes.
    fn planted(data: &[u8]) -> Vec<String> {
        let mut e = clean();
        e[0].data.extend_from_slice(b"..");
        e[0].data.extend_from_slice(data);
        e[0].data.extend_from_slice(b"..");
        findings(&e)
    }

    #[test]
    fn a_clean_release_passes() {
        assert_eq!(findings(&clean()), Vec::<String>::new());
        let relay = [entry("mvs-relay.exe", b"MZ"), entry("README.md", b"x"), entry(LICENSE, b"MIT")];
        let secrets = vec![SECRET.as_bytes().to_vec()];
        assert!(check_files("r.zip", &relay, Zip::Relay, Some(&KEY), &secrets).findings.is_empty());
    }

    #[test]
    fn the_key_is_found_in_every_form() {
        let hex: String = KEY.iter().map(|b| format!("{b:02X}")).collect();
        let found = planted(hex.as_bytes());
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("as hex"));
        // the finding never quotes the key
        assert!(!found[0].to_ascii_lowercase().contains(&hex.to_ascii_lowercase()[..16]));
        assert!(planted(&KEY)[0].contains("raw bytes"));
        // the key inside longer encoded data, at each alignment
        for lead in 0..3 {
            let mut data = vec![0x55; lead];
            data.extend_from_slice(&KEY);
            data.extend_from_slice(&[0xaa; 5]);
            let b64 = STANDARD.encode(&data);
            assert!(planted(b64.as_bytes()).iter().any(|f| f.ends_with("as base64")), "base64 at {lead}");
            let url = b64.replace('+', "-").replace('/', "_");
            assert!(planted(url.as_bytes()).iter().any(|f| f.contains("base64")), "base64url at {lead}");
        }
    }

    #[test]
    fn the_eos_secret_is_found_outside_its_file() {
        let found = planted(SECRET.as_bytes());
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("game/MVS Classic.exe: the EOS client credentials"));
        assert!(!found[0].contains(SECRET));
        // in the relay's zip, known from the pack step
        let relay = [entry("mvs-relay.exe", SECRET.as_bytes())];
        let secrets = vec![SECRET.as_bytes().to_vec()];
        assert_eq!(check_files("r.zip", &relay, Zip::Relay, None, &secrets).findings.len(), 1);
        let mut e = clean();
        e.iter_mut().find(|f| f.name == in_game(EOS_CREDENTIALS)).expect("the credentials").data = b"{}".to_vec();
        assert!(findings(&e)[0].contains("no client id and secret"));
    }

    #[test]
    fn game_data_and_strange_files_fail() {
        let mut e = clean();
        e.push(entry("pakchunk0-WindowsNoEditor.pak", b"x"));
        e.push(entry("data/store/x.bin", b"x"));
        e.push(entry("../evil.txt", b"x"));
        e.push(entry(&in_game(GAME_EXE), b"MZ"));
        let found = findings(&e);
        assert!(found.iter().any(|f| f.contains("a game data file (.pak)")));
        assert!(found.iter().any(|f| f.starts_with("t.zip: data/store/x.bin: not one")));
        assert!(found.iter().any(|f| f.starts_with("t.zip: ../evil.txt: not one")));
        assert!(found.iter().any(|f| f.contains("in the zip twice")));
        // a relay file is not the game's
        assert!(findings(&[entry("mvs-relay.exe", b"MZ")])[0].contains("not one of the release's own"));
    }

    #[test]
    fn the_build_list_holds_names_sizes_and_hashes_only() {
        let ok = r#"{"id":"a","paks":[{"name":"p.pak","size":1,"sha256":"00"}],"files":[{"path":"x","size":2}]}"#;
        assert!(check_build_json("b", ok).is_empty());
        let found = check_build_json("b", &ok.replace(r#""size":2"#, r#""size":2,"key":"k","aes":{"x":1}"#));
        assert_eq!(found, vec!["b: keys other than names, sizes and hashes: aes, key, x".to_owned()]);
        assert_eq!(check_build_json("b", "{"), vec!["b: not JSON".to_owned()]);
    }

    #[test]
    fn the_key_file_reads() {
        let dir = std::env::temp_dir().join(format!("mvs-dist-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("local.json");
        let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&file, format!(r#"{{"key":"0x{hex}"}}"#)).unwrap();
        assert_eq!(read_key(&file).unwrap(), KEY);
        std::fs::write(&file, format!(r#"{{"key":"0x{}"}}"#, &hex[2..])).unwrap();
        let err = read_key(&file).unwrap_err();
        assert!(!err.contains(&hex[2..20]));
        std::fs::write(&file, format!("{{\"key\":\"{hex}\"")).unwrap();
        assert!(!read_key(&file).unwrap_err().contains(&hex[..16]));
        // a sign or a multibyte character among 64 bytes is refused, not parsed or panicked on
        std::fs::write(&file, format!(r#"{{"key":"+f{}"}}"#, &hex[2..])).unwrap();
        assert!(read_key(&file).is_err());
        std::fs::write(&file, format!(r#"{{"key":"é{}"}}"#, &hex[2..])).unwrap();
        assert!(read_key(&file).is_err());
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    /// A 64-bit PE file of one section (RVA 0x1000 at file offset 0x200) importing `plain` and delay-loading `delayed`.
    fn pe(plain: &[&str], delayed: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x1200];
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        b[..2].copy_from_slice(b"MZ");
        put(&mut b, 0x3c, 0x40);
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        b[0x46] = 1; // one section
        b[0x54] = 240; // the optional header's size
        b[0x58..0x5a].copy_from_slice(&0x20b_u16.to_le_bytes());
        let dirs = 0x58 + 112;
        put(&mut b, dirs + 8, 0x1000);
        put(&mut b, dirs + 13 * 8, 0x1080);
        let section = 0x58 + 240;
        put(&mut b, section + 8, 0x1000);
        put(&mut b, section + 12, 0x1000);
        put(&mut b, section + 20, 0x200);
        for (i, name) in plain.iter().chain(delayed).enumerate() {
            let rva = 0x1100 + i * 32;
            b[rva - 0xe00..rva - 0xe00 + name.len()].copy_from_slice(name.as_bytes());
            let desc = if i < plain.len() { 0x200 + i * 20 + 12 } else { 0x280 + (i - plain.len()) * 32 + 4 };
            put(&mut b, desc, rva as u32);
        }
        b
    }

    #[test]
    fn an_exe_needing_the_vc_runtime_fails() {
        let ucrt = ["KERNEL32.dll", "api-ms-win-crt-runtime-l1-1-0.dll", "ucrtbase.dll"];
        assert!(vc_runtime_imports(&pe(&ucrt, &["d3d11.dll"])).is_empty());
        assert_eq!(vc_runtime_imports(&pe(&["KERNEL32.dll", "VCRUNTIME140.dll"], &[])), ["VCRUNTIME140.dll"]);
        assert_eq!(vc_runtime_imports(&pe(&["KERNEL32.dll"], &["msvcp140.dll"])), ["msvcp140.dll"]);
        // data that is not a PE file imports nothing
        assert!(vc_runtime_imports(b"MZ the program").is_empty());
        let mut e = clean();
        e[0].data = pe(&["KERNEL32.dll", "VCRUNTIME140.dll"], &[]);
        let found = findings(&e);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("MVS Classic.exe: needs VCRUNTIME140.dll"));
    }
}
