//! The port's own HLSL in this crate, every (source, entry, target) `lighting::compile` is called with, and the
//! DXBC `D3DCompile` makes of each, committed in `shaders/` (Linux has no `D3DCompile`), with `shaders/keys.txt`
//! naming each blob's `mvs_d3d::dxbc_key`.

// On Windows only the test reads the list.
#![cfg_attr(all(windows, not(test)), allow(dead_code))]

use crate::post::skip::src;
use crate::{capture, lighting, post, scenery, translucency};

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
    ]
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
