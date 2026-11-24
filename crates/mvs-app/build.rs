//! The `mvs` and `mvs-launcher` exes' Windows resources: each its own icon and its version information, written as a `.res` file the MSVC
//! linker takes (no resource compiler needed); another target's build gets none, with a warning.

use std::path::PathBuf;

/// The product's name in the exe's version information.
const PRODUCT: &str = "MVS Classic";
/// Each program: its Cargo name, its icon file, its description (what Explorer and Task Manager name it) and its
/// file name in a release (mvs_release GAME_EXE, LAUNCHER_EXE).
const PROGRAMS: [(&str, &str, &str, &str); 2] = [
    ("mvs", "res/icon.ico", "MVS Classic", "MVS Classic.exe"),
    ("mvs-launcher", "res/launcher.ico", "MVSC Launcher", "MVSC Launcher.exe"),
];

/// The exes' manifest: the process heap is the segment heap (Windows 10 2004 on), which gives freed pages back to the
/// system where the older heap keeps them committed (0.5 GB more after a few dozen matches).
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <heapType xmlns="http://schemas.microsoft.com/SMI/2020/WindowsSettings">SegmentHeap</heapType>
    </windowsSettings>
  </application>
</assembly>
"#;

/// The resource types and the ids used.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;
/// The icon group's id: the first, so Explorer shows it; the window class loads it by this id (platform.rs).
const ICON_GROUP: u16 = 1;
/// English (United States), and the version block's code page (Unicode).
const LANGUAGE: u16 = 0x0409;
const CODE_PAGE: u16 = 1200;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for (_, ico, _, _) in PROGRAMS {
        println!("cargo:rerun-if-changed={ico}");
    }
    println!("cargo:rerun-if-env-changed=MVS_VERSION");
    // Linux: each program finds the libraries a release puts beside the game (`game/`, tools/linux-build.sh
    // release): the game in its own folder, the launcher in `game/` below its own. An RPATH, not a RUNPATH
    // (--disable-new-dtags), so it also serves the libraries' own loads (DXVK's of SDL3).
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux") {
        println!("cargo:rustc-link-arg-bin=mvs=-Wl,--disable-new-dtags,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg-bin=mvs-launcher=-Wl,--disable-new-dtags,-rpath,$ORIGIN/game");
    }
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.ends_with("windows-msvc") {
        println!("cargo:warning=mvs: no icon or version resource for the target {target} (MSVC only)");
        return;
    }
    // (the file and product version: `MVS_VERSION`, else the crate's, as `mvs_release::VERSION` takes it)
    let version = std::env::var("MVS_VERSION").unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap());
    let parts = version_parts(&version)
        .unwrap_or_else(|| panic!("MVS_VERSION {version} is not x.y.z with each part at most 65535"));
    // (one .res a program: each names its own file)
    for (bin, ico, description, file) in PROGRAMS {
        let ico = std::fs::read(ico).unwrap_or_else(|e| panic!("mvs-app/{ico}: {e}"));
        let mut res = Vec::new();
        // (a .res file starts with an empty entry)
        entry(&mut res, 0, 0, 0, &[]);
        icons(&mut res, &ico);
        entry(&mut res, RT_VERSION, 1, 0x0030, &version_info(bin, description, file, &version, parts));
        entry(&mut res, RT_MANIFEST, 1, 0x0030, MANIFEST.as_bytes());
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(format!("{bin}.res"));
        std::fs::write(&out, &res).expect("the .res file writes");
        println!("cargo:rustc-link-arg-bin={bin}={}", out.display());
    }
}

fn u16s(out: &mut Vec<u8>, v: &[u16]) {
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

fn pad4(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// One resource of a `.res` file: its header (type and name by id, memory flags, language), its data, padded.
fn entry(out: &mut Vec<u8>, ty: u16, name: u16, flags: u16, data: &[u8]) {
    let size = u32::try_from(data.len()).expect("a resource under 4 GB");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    u16s(out, &[0xffff, ty, 0xffff, name]);
    out.extend_from_slice(&0u32.to_le_bytes());
    let lang = if ty == 0 { 0 } else { LANGUAGE };
    u16s(out, &[flags, lang]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    pad4(out);
}

/// The `.ico` file's images as RT_ICON resources (ids from 1) and their RT_GROUP_ICON directory.
fn icons(out: &mut Vec<u8>, ico: &[u8]) {
    let word = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let dword = |at: usize| u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]);
    assert!(word(0) == 0 && word(2) == 1, "an icon of res/ is not an icon file");
    let count = word(4);
    let mut group = Vec::new();
    u16s(&mut group, &[0, 1, count]);
    for i in 0..count {
        let e = 6 + 16 * usize::from(i);
        let (bit_count, size, offset) = (word(e + 6), dword(e + 8), dword(e + 12));
        let image = &ico[offset as usize..(offset + size) as usize];
        let id = i + 1;
        entry(out, RT_ICON, id, 0x1010, image);
        // (width, height, colours, reserved as in the file; one plane)
        group.extend_from_slice(&ico[e..e + 4]);
        u16s(&mut group, &[1, bit_count]);
        group.extend_from_slice(&size.to_le_bytes());
        u16s(&mut group, &[id]);
    }
    entry(out, RT_GROUP_ICON, ICON_GROUP, 0x1030, &group);
}

/// A version block: its length, its value's length (`value_len`: bytes for binary, words for text), its type, its
/// key, its value and its children, each on a 4-byte boundary.
fn block(key: &str, value: &[u8], value_len: usize, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0; 6];
    u16s(&mut b, &key.encode_utf16().chain([0]).collect::<Vec<_>>());
    pad4(&mut b);
    b.extend_from_slice(value);
    for c in children {
        pad4(&mut b);
        b.extend_from_slice(c);
    }
    let len = u16::try_from(b.len()).expect("a version block under 64 KB");
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b[2..4].copy_from_slice(&u16::try_from(value_len).unwrap().to_le_bytes());
    b[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
    b
}

/// A string of the string table.
fn string(key: &str, value: &str) -> Vec<u8> {
    let v: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let mut bytes = Vec::new();
    u16s(&mut bytes, &v);
    block(key, &bytes, v.len(), true, &[])
}

/// `x.y.z` as three numbers that each fit the version resource's 16 bits; None for anything else.
fn version_parts(version: &str) -> Option<[u16; 3]> {
    let mut it = version.split('.');
    let mut out = [0; 3];
    for o in &mut out {
        *o = it.next()?.parse().ok()?;
    }
    it.next().is_none().then_some(out)
}

/// VS_VERSIONINFO of program `bin` (its `description` and release `file` name) for `version` (its text) and its
/// `parts`.
fn version_info(bin: &str, description: &str, file: &str, version: &str, [major, minor, patch]: [u16; 3]) -> Vec<u8> {
    let ms = (u32::from(major) << 16) | u32::from(minor);
    let ls = u32::from(patch) << 16;
    let mut fixed = Vec::new();
    // (signature, structure version, file and product version, flags mask, flags, NT Win32, an app, no subtype, no
    // date)
    for v in [0xfeef_04bd, 0x0001_0000, ms, ls, ms, ls, 0x3f, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend_from_slice(&u32::to_le_bytes(v));
    }
    let table = block(
        &format!("{LANGUAGE:04X}{CODE_PAGE:04X}"),
        &[],
        0,
        true,
        &[
            string("FileDescription", description),
            string("FileVersion", version),
            string("InternalName", bin),
            string("OriginalFilename", file),
            string("ProductName", PRODUCT),
            string("ProductVersion", version),
        ],
    );
    let strings = block("StringFileInfo", &[], 0, true, &[table]);
    let mut translation = Vec::new();
    u16s(&mut translation, &[LANGUAGE, CODE_PAGE]);
    let vars = block("VarFileInfo", &[], 0, true, &[block("Translation", &translation, 4, false, &[])]);
    block("VS_VERSION_INFO", &fixed, fixed.len(), false, &[strings, vars])
}
