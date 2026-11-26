//! The port's own HLSL in this crate, every (source, entry, target) the menu blur's and the launcher's `compile`
//! are called with, and the DXBC `D3DCompile` makes of each, committed in `shaders/` (Linux has no `D3DCompile`),
//! with `shaders/keys.txt` naming each blob's `mvs_d3d::dxbc_key`.

// On Windows only the test reads the list; elsewhere only the test reads each module's `compile`.
#![cfg_attr(not(test), allow(dead_code))]

use crate::Result;
use crate::launcher::paint;
use crate::menu::blur;

/// The same file `menu::blur::compile` includes as its source.
const BLUR: &str = include_str!("menu/blur.hlsl");

/// One compiled shader: its blob's name (`<module>.<const>.<entry>.<target>`), the source, entry and target it is
/// compiled with, the module's `compile` (each holds its own source), and the committed blob.
pub(crate) struct Shader {
    pub name: &'static str,
    pub src: &'static str,
    pub entry: &'static str,
    pub target: &'static str,
    pub compile: fn(&str, &str) -> Result<Vec<u8>>,
    pub dxbc: &'static [u8],
}

/// A `Shader` whose name is made of its module, constant, entry and target, and whose blob is that name's file.
macro_rules! shader {
    ($module:ident, $konst:ident, $entry:literal, $target:literal, $src:expr, $compile:expr) => {
        Shader {
            name: concat!(stringify!($module), ".", stringify!($konst), ".", $entry, ".", $target),
            src: $src,
            entry: $entry,
            target: $target,
            compile: $compile,
            dxbc: include_bytes!(concat!(
                "../shaders/",
                stringify!($module),
                ".",
                stringify!($konst),
                ".",
                $entry,
                ".",
                $target,
                ".dxbc"
            )),
        }
    };
}

/// Every shader this crate compiles, one entry per `compile` call.
pub(crate) const SHADERS: &[Shader] = &[
    shader!(menu_blur, SOURCE, "MainVS", "vs_5_0", BLUR, blur::compile),
    shader!(menu_blur, SOURCE, "DownPS", "ps_5_0", BLUR, blur::compile),
    shader!(menu_blur, SOURCE, "BlurPS", "ps_5_0", BLUR, blur::compile),
    shader!(menu_blur, SOURCE, "UpPS", "ps_5_0", BLUR, blur::compile),
    shader!(launcher_paint, SOURCE, "MainVS", "vs_5_0", paint::SOURCE, paint::compile),
    shader!(launcher_paint, SOURCE, "MainPS", "ps_5_0", paint::SOURCE, paint::compile),
];

/// The committed blob `shaders/keys.txt` names for `src`, `entry` and `target`'s `mvs_d3d::dxbc_key`: each module's
/// `compile` on Linux, `caller` its caller.
#[cfg(not(windows))]
pub(crate) fn committed(src: &str, entry: &str, target: &str, caller: &std::panic::Location<'_>) -> Result<Vec<u8>> {
    let key = format!("{:016x}", mvs_d3d::dxbc_key(src, entry, target));
    let name = include_str!("../shaders/keys.txt")
        .lines()
        .find_map(|l| l.split_once(' ').filter(|(_, k)| *k == key).map(|(n, _)| n));
    // (the listed entry of that name, when it is still the same source, entry and target)
    let blob = name
        .and_then(|n| SHADERS.iter().find(|s| s.name == n && s.entry == entry && s.target == target && s.src == src));
    blob.map(|s| s.dxbc.to_vec()).ok_or_else(|| crate::Error::Format {
        what: format!("shader {entry} {target} at {caller}"),
        why: format!("no compiled blob has key {key}: add it to src/shaders.rs and regenerate the blobs"),
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::{SHADERS, Shader};
    use mvs_d3d::dxbc_key;

    /// With `MVS_WRITE_DXBC=<dir>`, writes every blob and `keys.txt` to `<dir>` instead, and fails.
    #[test]
    fn the_committed_dxbc_is_what_d3dcompile_makes() {
        let made = |s: &Shader| (s.compile)(s.entry, s.target).unwrap_or_else(|e| panic!("{}: {e}", s.name));
        let line = |s: &Shader| format!("{} {:016x}", s.name, dxbc_key(s.src, s.entry, s.target));
        if let Some(dir) = std::env::var_os("MVS_WRITE_DXBC") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("create the blob folder");
            let mut keys = String::new();
            for s in SHADERS {
                std::fs::write(dir.join(format!("{}.dxbc", s.name)), made(s)).expect("write a blob");
                keys += &line(s);
                keys.push('\n');
            }
            std::fs::write(dir.join("keys.txt"), keys).expect("write keys.txt");
            panic!("written, run again without MVS_WRITE_DXBC");
        }
        let keys = include_str!("../shaders/keys.txt");
        for s in SHADERS {
            assert!(made(s) == s.dxbc, "{}: the committed blob is not what D3DCompile makes", s.name);
            let line = line(s);
            assert!(keys.lines().any(|l| l == line), "keys.txt does not hold `{line}`");
        }
    }
}

#[cfg(all(test, not(windows)))]
mod linux_tests {
    use super::SHADERS;
    use mvs_d3d::dxbc_key;

    /// A source edited on Windows without its blobs made again fails here: every listed shader's current key is in
    /// `keys.txt` under its name, with a blob, and no blob in `shaders/` is unlisted.
    #[test]
    fn every_listed_shader_has_its_blob() {
        let keys = include_str!("../shaders/keys.txt");
        for s in SHADERS {
            let line = format!("{} {:016x}", s.name, dxbc_key(s.src, s.entry, s.target));
            assert!(keys.lines().any(|l| l == line), "keys.txt does not hold `{line}`: regenerate the blobs");
            assert!(!s.dxbc.is_empty(), "{}: the blob is empty", s.name);
            let blob = (s.compile)(s.entry, s.target).unwrap_or_else(|e| panic!("{}: {e}", s.name));
            assert!(blob == s.dxbc, "{}: compile does not find its own blob", s.name);
        }
        for l in keys.lines() {
            let name = l.split_once(' ').map_or(l, |(n, _)| n);
            assert!(
                SHADERS.iter().any(|s| s.name == name),
                "keys.txt names `{name}`, which src/shaders.rs does not list"
            );
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        for entry in std::fs::read_dir(&dir).expect("read the blob folder") {
            let file = entry.expect("a blob folder entry").file_name().into_string().expect("a UTF-8 file name");
            if let Some(name) = file.strip_suffix(".dxbc") {
                assert!(SHADERS.iter().any(|s| s.name == name), "{file} is not listed in src/shaders.rs");
            }
        }
    }
}
