//! Builds the vendored FreeType 2.10.0 (TrueType, sfnt, auto-hinter, smooth renderer, glyph and stroker APIs) as
//! a static library with the crate's own module list and options (config/).

fn main() {
    let root = std::path::Path::new("freetype");
    let src = root.join("src");
    let files = [
        "base/ftbase.c",
        "base/ftbitmap.c",
        "base/ftdebug.c",
        "base/ftglyph.c",
        "base/ftinit.c",
        "base/ftstroke.c",
        "base/ftsystem.c",
        "autofit/autofit.c",
        "sfnt/sfnt.c",
        "smooth/smooth.c",
        "truetype/truetype.c",
    ];
    let mut b = cc::Build::new();
    b.include("config")
        .include(root.join("include"))
        .define("FT2_BUILD_LIBRARY", None)
        .define("FT_CONFIG_OPTIONS_H", "<mvs_ftoption.h>")
        .define("FT_CONFIG_MODULES_H", "<mvs_ftmodule.h>")
        .warnings(false);
    for f in files {
        b.file(src.join(f));
    }
    b.file("shim/mvs_ft.c");
    b.compile("freetype");
    println!("cargo:rerun-if-changed=config");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=freetype");
}
