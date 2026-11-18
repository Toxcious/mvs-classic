/* FreeType 2.10.0's default options, less the compressed-stream and PostScript-name support the TrueType faces of
 * the font kind never reach (no zlib / LZW / bzip2 to build, no psnames module). The TrueType interpreter stays at
 * the default: version 40 (TT_CONFIG_OPTION_SUBPIXEL_HINTING 2). */

#include <freetype/config/ftoption.h>

#undef FT_CONFIG_OPTION_USE_LZW
#undef FT_CONFIG_OPTION_USE_ZLIB
#undef FT_CONFIG_OPTION_POSTSCRIPT_NAMES
#undef TT_CONFIG_OPTION_POSTSCRIPT_NAMES
