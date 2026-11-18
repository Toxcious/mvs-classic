/* A flat C face over the FreeType calls mvs-freetype makes, so the Rust side mirrors no FreeType struct (FT_Long is
 * 32 bits on Windows and 64 elsewhere; FT_FaceRec and FT_GlyphSlotRec are long). */

#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_GLYPH_H
#include FT_STROKER_H
#include FT_ADVANCES_H

typedef struct {
  int units_per_em;
  int height;
  int ascender;
  int descender;
  int bbox_y_min;
  int bbox_y_max;
  int scalable;
  int kerning;
} MvsFaceInfo;

typedef struct {
  void *glyph;
  const unsigned char *buffer;
  int width;
  int rows;
  int pitch;
  int left;
  int top;
} MvsBitmap;

int mvs_ft_init(FT_Library *lib) { return FT_Init_FreeType(lib); }

void mvs_ft_done(FT_Library lib) { FT_Done_FreeType(lib); }

int mvs_ft_face(FT_Library lib, const unsigned char *bytes, long len, FT_Face *face) {
  return FT_New_Memory_Face(lib, bytes, (FT_Long)len, 0, face);
}

void mvs_ft_face_done(FT_Face face) { FT_Done_Face(face); }

void mvs_ft_face_info(FT_Face face, MvsFaceInfo *out) {
  out->units_per_em = face->units_per_EM;
  out->height = face->height;
  out->ascender = face->ascender;
  out->descender = face->descender;
  out->bbox_y_min = (int)face->bbox.yMin;
  out->bbox_y_max = (int)face->bbox.yMax;
  out->scalable = FT_IS_SCALABLE(face) ? 1 : 0;
  out->kerning = FT_HAS_KERNING(face) ? 1 : 0;
}

/* FT_Set_Pixel_Sizes(ppem, ppem); the size's 16.16 scales. */
int mvs_ft_set_pixel_size(FT_Face face, unsigned int ppem, long long *x_scale, long long *y_scale) {
  FT_Error e = FT_Set_Pixel_Sizes(face, ppem, ppem);
  if (e) return e;
  *x_scale = face->size->metrics.x_scale;
  *y_scale = face->size->metrics.y_scale;
  return 0;
}

unsigned int mvs_ft_char_index(FT_Face face, unsigned long code) { return FT_Get_Char_Index(face, code); }

int mvs_ft_advance(FT_Face face, unsigned int glyph, int flags, long long *advance) {
  FT_Fixed a = 0;
  FT_Error e = FT_Get_Advance(face, glyph, flags, &a);
  *advance = a;
  return e;
}

int mvs_ft_kerning(FT_Face face, unsigned int left, unsigned int right, long long *x) {
  FT_Vector v = {0, 0};
  FT_Error e = FT_Get_Kerning(face, left, right, FT_KERNING_DEFAULT, &v);
  *x = v.x;
  return e;
}

/* The glyph loaded with `flags`, its outside border stroked at `stroke` (26.6, round caps and joins) when it is
 * above 0, moved right by `shift` (26.6, a part of a pixel) and rendered in the normal (8-bit coverage) mode. The
 * bitmap lives until mvs_ft_bitmap_done. */
int mvs_ft_bitmap(FT_Library lib, FT_Face face, unsigned int glyph, int flags, long stroke, long shift,
                  MvsBitmap *out) {
  FT_Vector origin;
  FT_Glyph g = NULL;
  FT_Error e = FT_Load_Glyph(face, glyph, flags);
  if (e) return e;
  e = FT_Get_Glyph(face->glyph, &g);
  if (e) return e;
  if (stroke > 0 && g->format == FT_GLYPH_FORMAT_OUTLINE) {
    FT_Stroker s = NULL;
    e = FT_Stroker_New(lib, &s);
    if (!e) {
      FT_Stroker_Set(s, stroke, FT_STROKER_LINECAP_ROUND, FT_STROKER_LINEJOIN_ROUND, 0);
      e = FT_Glyph_StrokeBorder(&g, s, 0, 1);
      FT_Stroker_Done(s);
    }
    if (e) {
      FT_Done_Glyph(g);
      return e;
    }
  }
  origin.x = shift;
  origin.y = 0;
  e = FT_Glyph_To_Bitmap(&g, FT_RENDER_MODE_NORMAL, &origin, 1);
  if (e) {
    FT_Done_Glyph(g);
    return e;
  }
  {
    FT_BitmapGlyph b = (FT_BitmapGlyph)g;
    if (b->bitmap.pixel_mode != FT_PIXEL_MODE_GRAY && b->bitmap.rows > 0) {
      FT_Done_Glyph(g);
      return -1;
    }
    out->glyph = g;
    out->buffer = b->bitmap.buffer;
    out->width = (int)b->bitmap.width;
    out->rows = (int)b->bitmap.rows;
    out->pitch = b->bitmap.pitch;
    out->left = b->left;
    out->top = b->top;
  }
  return 0;
}

void mvs_ft_bitmap_done(MvsBitmap *b) {
  if (b->glyph) FT_Done_Glyph((FT_Glyph)b->glyph);
  b->glyph = NULL;
}
