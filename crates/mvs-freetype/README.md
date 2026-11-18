# mvs-freetype

FreeType built from source for the UI's text: the build's font cache links FreeType 2.10, and its TrueType hinter
and smooth rasteriser decide the glyphs' pixels, so the port uses the same library rather than another rasteriser.

## The vendored source

`freetype/` is FreeType 2.10.0, unmodified, from the FreeType project's release
`https://downloads.sourceforge.net/project/freetype/freetype2/2.10.0/freetype-2.10.0.tar.gz`
(sha256 `955e17244e9b38adb0c98df66abb50467312e6bb70eac07e49ce6bd1a20e809a`). Only what the build script compiles
is kept: `include/`, `src/base`, `src/truetype`, `src/sfnt`, `src/smooth`, `src/autofit`, the README and the licence
texts (`docs/LICENSE.TXT`, `docs/FTL.TXT`).

`config/` holds the crate's own module list (`mvs_ftmodule.h`: TrueType, sfnt, the auto-hinter, the smooth renderers)
and options (`mvs_ftoption.h`: the release's defaults without zlib, LZW and PostScript glyph names), passed to the
build as `FT_CONFIG_MODULES_H` and `FT_CONFIG_OPTIONS_H`, so the vendored tree stays as released. `shim/mvs_ft.c` is a
flat C face over the calls the Rust side makes.

## Licence

FreeType is dual-licensed (FreeType License or GPLv2, `freetype/docs/LICENSE.TXT`). This project takes it under the
**FreeType License** (`freetype/docs/FTL.TXT`), whose clause 3 asks a product's documentation to credit it:

> Portions of this software are copyright © 2019 The FreeType Project (www.freetype.org). All rights reserved.

The rest of this crate (`build.rs`, `config/`, `shim/`, `src/`) is under the workspace's licence.
