//! The port's own HLSL in this crate, every (source, entry, target) `lighting::compile` is called with, and the
//! DXBC `D3DCompile` makes of each, committed in `shaders/` (Linux has no `D3DCompile`), with `shaders/keys.txt`
//! naming each blob's `mvs_d3d::dxbc_key`.

// On Windows only the test reads the list.
#![cfg_attr(all(windows, not(test)), allow(dead_code))]

use crate::flood::{FloodStyle, look_source as flood};
use crate::post::skip::src;
use crate::{capture, hole, lava, lighting, post, scenery, translucency, wind};

/// One compiled shader: its blob's name (`<module>.<const>.<entry>.<target>`), the source, entry and target it is
/// compiled with, and the committed blob.
pub(crate) struct Shader {
    pub name: &'static str,
    pub src: String,
    pub entry: &'static str,
    pub target: &'static str,
    pub dxbc: &'static [u8],
}

/// A `Shader` whose name is made of its module, constant, entry and target, and whose blob is that name's file.
macro_rules! shader {
    ($module:ident, $konst:ident, $entry:literal, $target:literal, $src:expr) => {{
        const NAME: &str = concat!(stringify!($module), ".", stringify!($konst), ".", $entry, ".", $target);
        Shader {
            name: NAME,
            src: String::from($src),
            entry: $entry,
            target: $target,
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
    }};
}

/// Every shader this crate compiles, one entry per `compile` call site.
pub(crate) fn shaders() -> Vec<Shader> {
    use post::pass::SCREEN_POSITION_VS;
    use post::skip::{COPY, DILATE, MARK, REDUCE};
    vec![
        shader!(lighting, SOURCE, "MainVS", "vs_5_0", lighting::SOURCE),
        shader!(lighting, SOURCE, "MainPS", "ps_5_0", lighting::SOURCE),
        shader!(capture, SOURCE, "MainVS", "vs_5_0", capture::SOURCE),
        shader!(capture, SOURCE, "MainPS", "ps_5_0", capture::SOURCE),
        shader!(lighting_capture, DEPTH_FILL, "MainVS", "vs_5_0", lighting::capture::DEPTH_FILL),
        shader!(lighting_capture, DEPTH_FILL, "MainPS", "ps_5_0", lighting::capture::DEPTH_FILL),
        shader!(translucency, COMPOSE, "MainVS", "vs_5_0", translucency::COMPOSE),
        shader!(translucency, COMPOSE, "MainPS", "ps_5_0", translucency::COMPOSE),
        shader!(translucency, COMPOSE, "CopyPS", "ps_5_0", translucency::COMPOSE),
        shader!(post_pass, SCREEN_POSITION_VS, "MainVS", "vs_5_0", SCREEN_POSITION_VS),
        shader!(scenery_widget, QUAD, "MainVS", "vs_5_0", scenery::widget::QUAD),
        shader!(scenery_widget, QUAD, "MainPS", "ps_5_0", scenery::widget::QUAD),
        shader!(post_skip, REDUCE, "Main", "cs_5_0", src(REDUCE)),
        shader!(post_skip, DILATE, "Main", "cs_5_0", src(DILATE)),
        shader!(post_skip, MARK, "Main", "vs_5_0", src(MARK)),
        shader!(post_skip, COPY, "Vertex", "vs_5_0", COPY),
        shader!(post_skip, COPY, "Pixel", "ps_5_0", COPY),
        // (the looks: each built source, `look::source` of its file)
        shader!(lava, SOURCE, "ShapeVS", "vs_5_0", lava::look_source()),
        shader!(lava, SOURCE, "ScreenVS", "vs_5_0", lava::look_source()),
        shader!(lava, SOURCE, "LookShape0", "ps_5_0", lava::look_source()),
        shader!(lava, SOURCE, "LookShape1", "ps_5_0", lava::look_source()),
        shader!(lava, SOURCE, "LookShape2", "ps_5_0", lava::look_source()),
        shader!(lava, SOURCE, "Heat", "ps_5_0", lava::look_source()),
        shader!(wind, SOURCE, "ShapeVS", "vs_5_0", wind::look_source()),
        shader!(wind, SOURCE, "ScreenVS", "vs_5_0", wind::look_source()),
        shader!(wind, SOURCE, "Wind", "ps_5_0", wind::look_source()),
        shader!(hole, SOURCE, "ShapeVS", "vs_5_0", hole::look_source()),
        shader!(hole, SOURCE, "ScreenVS", "vs_5_0", hole::look_source()),
        shader!(hole, SOURCE, "Lens", "ps_5_0", hole::look_source()),
        shader!(flood, TROPICAL, "ShapeVS", "vs_5_0", flood(FloodStyle::Tropical)),
        shader!(flood, TROPICAL, "ScreenVS", "vs_5_0", flood(FloodStyle::Tropical)),
        shader!(flood, TROPICAL, "Water", "ps_5_0", flood(FloodStyle::Tropical)),
        shader!(flood, STORM, "ShapeVS", "vs_5_0", flood(FloodStyle::Storm)),
        shader!(flood, STORM, "ScreenVS", "vs_5_0", flood(FloodStyle::Storm)),
        shader!(flood, STORM, "Storm", "ps_5_0", flood(FloodStyle::Storm)),
        shader!(flood, STORM, "Flood", "ps_5_0", flood(FloodStyle::Storm)),
        shader!(flood, STORM, "Rain", "ps_5_0", flood(FloodStyle::Storm)),
    ]
}

#[cfg(test)]
mod look_tests {
    use super::shaders;
    use crate::flood::FloodStyle;
    use crate::look::{LookFile, entries};

    /// Every entry each look compiles is listed with that look's source, so Linux finds its blob.
    #[test]
    fn every_look_entry_is_listed() {
        let looks: [(LookFile, String); 5] = [
            (crate::lava::look_file(), crate::lava::look_source()),
            (crate::wind::look_file(), crate::wind::look_source()),
            (crate::hole::look_file(), crate::hole::look_source()),
            (crate::flood::look_file(FloodStyle::Tropical), crate::flood::look_source(FloodStyle::Tropical)),
            (crate::flood::look_file(FloodStyle::Storm), crate::flood::look_source(FloodStyle::Storm)),
        ];
        let list = shaders();
        for (file, src) in &looks {
            for (entry, target) in entries(file) {
                assert!(
                    list.iter().any(|s| s.entry == entry && s.target == target && &s.src == src),
                    "{entry} {target} of a look is not listed in src/shaders.rs"
                );
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::shaders;
    use crate::lighting::compile;
    use mvs_d3d::dxbc_key;

    /// With `MVS_WRITE_DXBC=<dir>`, writes every blob and `keys.txt` to `<dir>` instead, and fails.
    #[test]
    fn the_committed_dxbc_is_what_d3dcompile_makes() {
        let list = shaders();
        let made = |s: &super::Shader| compile(&s.src, s.entry, s.target).unwrap_or_else(|e| panic!("{}: {e}", s.name));
        let line = |s: &super::Shader| format!("{} {:016x}", s.name, dxbc_key(&s.src, s.entry, s.target));
        if let Some(dir) = std::env::var_os("MVS_WRITE_DXBC") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("create the blob folder");
            let mut keys = String::new();
            for s in &list {
                std::fs::write(dir.join(format!("{}.dxbc", s.name)), made(s)).expect("write a blob");
                keys += &line(s);
                keys.push('\n');
            }
            std::fs::write(dir.join("keys.txt"), keys).expect("write keys.txt");
            panic!("written, run again without MVS_WRITE_DXBC");
        }
        let keys = include_str!("../shaders/keys.txt");
        for s in &list {
            assert!(made(s) == s.dxbc, "{}: the committed blob is not what D3DCompile makes", s.name);
            let line = line(s);
            assert!(keys.lines().any(|l| l == line), "keys.txt does not hold `{line}`");
        }
    }
}

#[cfg(all(test, not(windows)))]
mod linux_tests {
    use super::shaders;
    use mvs_d3d::dxbc_key;

    /// A source edited on Windows without its blobs made again fails here: every listed shader's current key is in
    /// `keys.txt` under its name, with a blob, and no blob in `shaders/` is unlisted.
    #[test]
    fn every_listed_shader_has_its_blob() {
        let list = shaders();
        let keys = include_str!("../shaders/keys.txt");
        for s in &list {
            let line = format!("{} {:016x}", s.name, dxbc_key(&s.src, s.entry, s.target));
            assert!(keys.lines().any(|l| l == line), "keys.txt does not hold `{line}`: regenerate the blobs");
            assert!(!s.dxbc.is_empty(), "{}: the blob is empty", s.name);
        }
        for l in keys.lines() {
            let name = l.split_once(' ').map_or(l, |(n, _)| n);
            assert!(list.iter().any(|s| s.name == name), "keys.txt names `{name}`, which src/shaders.rs does not list");
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        for entry in std::fs::read_dir(&dir).expect("read the blob folder") {
            let file = entry.expect("a blob folder entry").file_name().into_string().expect("a UTF-8 file name");
            if let Some(name) = file.strip_suffix(".dxbc") {
                assert!(list.iter().any(|s| s.name == name), "{file} is not listed in src/shaders.rs");
            }
        }
    }
}
