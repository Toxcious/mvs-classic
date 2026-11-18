//! FreeType 2.10.0 (the version the build's font cache links), vendored and built from source: a library with its
//! faces opened from memory, the size, advance and kerning queries and the glyph raster (fill, or the stroked outside
//! border) that the UI's text needs. The FFI goes through the crate's C shim (shim/mvs_ft.c), so no FreeType struct is
//! mirrored here.

use std::ffi::{c_int, c_long, c_uint, c_ulong, c_void};

/// FT_LOAD_NO_BITMAP.
pub const LOAD_NO_BITMAP: i32 = 0x8;
/// FT_LOAD_NO_HINTING.
pub const LOAD_NO_HINTING: i32 = 0x2;
/// FT_LOAD_FORCE_AUTOHINT.
pub const LOAD_FORCE_AUTOHINT: i32 = 0x20;
/// FT_LOAD_TARGET_LIGHT: hinted on the vertical axis alone, so stems sit on whole pixels and the advances keep
/// their design widths.
pub const LOAD_TARGET_LIGHT: i32 = 0x1_0000;
/// FT_LOAD_COLOR.
pub const LOAD_COLOR: i32 = 0x10_0000;

#[derive(Debug, thiserror::Error)]
#[error("FreeType {call}: error {code:#x}")]
pub struct Error {
    pub call: &'static str,
    pub code: i32,
}

pub type Result<T> = std::result::Result<T, Error>;

fn check(call: &'static str, code: c_int) -> Result<()> {
    if code == 0 { Ok(()) } else { Err(Error { call, code }) }
}

#[repr(C)]
#[derive(Default)]
struct RawFaceInfo {
    units_per_em: c_int,
    height: c_int,
    ascender: c_int,
    descender: c_int,
    bbox_y_min: c_int,
    bbox_y_max: c_int,
    scalable: c_int,
    kerning: c_int,
}

#[repr(C)]
struct RawBitmap {
    glyph: *mut c_void,
    buffer: *const u8,
    width: c_int,
    rows: c_int,
    pitch: c_int,
    left: c_int,
    top: c_int,
}

unsafe extern "C" {
    fn mvs_ft_init(lib: *mut *mut c_void) -> c_int;
    fn mvs_ft_done(lib: *mut c_void);
    fn mvs_ft_face(lib: *mut c_void, bytes: *const u8, len: c_long, face: *mut *mut c_void) -> c_int;
    fn mvs_ft_face_done(face: *mut c_void);
    fn mvs_ft_face_info(face: *mut c_void, out: *mut RawFaceInfo);
    fn mvs_ft_set_pixel_size(face: *mut c_void, ppem: c_uint, x_scale: *mut i64, y_scale: *mut i64) -> c_int;
    fn mvs_ft_char_index(face: *mut c_void, code: c_ulong) -> c_uint;
    fn mvs_ft_advance(face: *mut c_void, glyph: c_uint, flags: c_int, advance: *mut i64) -> c_int;
    fn mvs_ft_kerning(face: *mut c_void, left: c_uint, right: c_uint, x: *mut i64) -> c_int;
    fn mvs_ft_bitmap(
        lib: *mut c_void,
        face: *mut c_void,
        glyph: c_uint,
        flags: c_int,
        stroke: c_long,
        shift: c_long,
        out: *mut RawBitmap,
    ) -> c_int;
    fn mvs_ft_bitmap_done(b: *mut RawBitmap);
}

/// A face's design metrics (FT_FaceRec), font units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceInfo {
    pub units_per_em: i32,
    /// The line height (hhea ascender - descender + line gap for a TrueType face).
    pub height: i32,
    pub ascender: i32,
    pub descender: i32,
    /// The head table's bounding box, Y.
    pub bbox_y: [i32; 2],
    pub scalable: bool,
    /// FT_HAS_KERNING: the face has a kern table.
    pub kerning: bool,
}

/// A glyph's 8-bit coverage bitmap and its place from the pen (FT_BitmapGlyph left / top: the top is above the
/// baseline, Y up).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub rows: u32,
    pub left: i32,
    pub top: i32,
    /// `rows` rows of `width` bytes.
    pub pixels: Vec<u8>,
}

/// A face opened in a [`Library`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceId(pub usize);

struct Face {
    raw: *mut c_void,
    // FT_New_Memory_Face reads from the caller's bytes for the face's life.
    _bytes: Box<[u8]>,
}

/// An FT_Library and its faces. Not shared between threads (FreeType's objects are not).
pub struct Library {
    raw: *mut c_void,
    faces: Vec<Face>,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").field("faces", &self.faces.len()).finish()
    }
}

impl Library {
    pub fn new() -> Result<Library> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: FT_Init_FreeType writes the new library's handle into `raw`.
        check("FT_Init_FreeType", unsafe { mvs_ft_init(&mut raw) })?;
        Ok(Library { raw, faces: Vec::new() })
    }

    /// Opens face 0 of a font file's bytes (kept with the face).
    pub fn open(&mut self, bytes: Vec<u8>) -> Result<FaceId> {
        let bytes = bytes.into_boxed_slice();
        let len = c_long::try_from(bytes.len()).map_err(|_| Error { call: "FT_New_Memory_Face", code: -1 })?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: the library is live; the bytes outlive the face (both dropped in Library::drop, the face first).
        check("FT_New_Memory_Face", unsafe { mvs_ft_face(self.raw, bytes.as_ptr(), len, &mut raw) })?;
        self.faces.push(Face { raw, _bytes: bytes });
        Ok(FaceId(self.faces.len() - 1))
    }

    fn face(&self, f: FaceId) -> *mut c_void {
        self.faces[f.0].raw
    }

    pub fn info(&self, f: FaceId) -> FaceInfo {
        let mut r = RawFaceInfo::default();
        // SAFETY: a live face; the shim only reads it.
        unsafe { mvs_ft_face_info(self.face(f), &mut r) };
        FaceInfo {
            units_per_em: r.units_per_em,
            height: r.height,
            ascender: r.ascender,
            descender: r.descender,
            bbox_y: [r.bbox_y_min, r.bbox_y_max],
            scalable: r.scalable != 0,
            kerning: r.kerning != 0,
        }
    }

    /// FT_Set_Pixel_Sizes(ppem, ppem): the size's x and y scales (16.16).
    pub fn set_pixel_size(&mut self, f: FaceId, ppem: u32) -> Result<[i64; 2]> {
        let (mut x, mut y) = (0, 0);
        // SAFETY: a live face, owned by this library (`&mut self`: no other call on it meanwhile).
        check("FT_Set_Pixel_Sizes", unsafe { mvs_ft_set_pixel_size(self.face(f), ppem, &mut x, &mut y) })?;
        Ok([x, y])
    }

    /// FT_Get_Char_Index: 0 for a character the face lacks.
    pub fn char_index(&self, f: FaceId, c: char) -> u32 {
        // SAFETY: a live face; a cmap lookup.
        unsafe { mvs_ft_char_index(self.face(f), c_ulong::from(c)) }
    }

    /// FT_Get_Advance at the face's current size: 16.16 pixels.
    pub fn advance(&mut self, f: FaceId, glyph: u32, flags: i32) -> Result<i64> {
        let mut a = 0;
        // SAFETY: a live face (FT_Get_Advance may load the glyph into the face's slot: `&mut self`).
        check("FT_Get_Advance", unsafe { mvs_ft_advance(self.face(f), glyph, flags, &mut a) })?;
        Ok(a)
    }

    /// FT_Get_Kerning in FT_KERNING_DEFAULT mode at the face's current size: 26.6 pixels, rounded.
    pub fn kerning(&self, f: FaceId, left: u32, right: u32) -> Result<i64> {
        let mut x = 0;
        // SAFETY: a live face; reads its kern table and size.
        check("FT_Get_Kerning", unsafe { mvs_ft_kerning(self.face(f), left, right, &mut x) })?;
        Ok(x)
    }

    /// The glyph at the face's current size loaded with `flags` and rendered in the normal mode; with `stroke` (26.6)
    /// above 0, its outside border stroked with round caps and joins first (FT_Glyph_StrokeBorder, inside 0).
    pub fn bitmap(&mut self, f: FaceId, glyph: u32, flags: i32, stroke: i64) -> Result<Bitmap> {
        self.bitmap_at(f, glyph, flags, stroke, 0)
    }

    /// As `bitmap`, the glyph moved right by `shift` (26.6, a part of a pixel) before it is rendered: a glyph drawn
    /// at a pen position between pixels.
    pub fn bitmap_at(&mut self, f: FaceId, glyph: u32, flags: i32, stroke: i64, shift: i64) -> Result<Bitmap> {
        let shift = c_long::try_from(shift).map_err(|_| Error { call: "FT_Glyph_To_Bitmap", code: -1 })?;
        let stroke = c_long::try_from(stroke).map_err(|_| Error { call: "FT_Stroker_Set", code: -1 })?;
        let mut r = RawBitmap {
            glyph: std::ptr::null_mut(),
            buffer: std::ptr::null(),
            width: 0,
            rows: 0,
            pitch: 0,
            left: 0,
            top: 0,
        };
        // SAFETY: a live library and face; on success `r` holds a glyph the shim owns until mvs_ft_bitmap_done.
        check("FT_Glyph_To_Bitmap", unsafe {
            mvs_ft_bitmap(self.raw, self.face(f), glyph, flags, stroke, shift, &mut r)
        })?;
        let width = r.width.max(0).cast_unsigned();
        // A zero-width bitmap may have rows and no buffer (FreeType allocates nothing): no pixels.
        let rows = if r.pitch < 0 || width == 0 || r.buffer.is_null() { 0 } else { r.rows.max(0).cast_unsigned() };
        let mut pixels = Vec::with_capacity((width * rows) as usize);
        for y in 0..rows as i32 {
            // SAFETY: row y of a gray bitmap of `rows` rows of `pitch` bytes (the smooth renderer writes top-down,
            // pitch >= width), `width` bytes used per row; read before the glyph is freed below.
            let row = unsafe {
                let start = r.buffer.offset((y * r.pitch) as isize);
                std::slice::from_raw_parts(start, width as usize)
            };
            pixels.extend_from_slice(row);
        }
        // SAFETY: frees the glyph mvs_ft_bitmap made; `pixels` holds a copy.
        unsafe { mvs_ft_bitmap_done(&mut r) };
        Ok(Bitmap { width, rows, left: r.left, top: r.top, pixels })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        for f in self.faces.drain(..) {
            // SAFETY: each face is done once, before its bytes and before the library.
            unsafe { mvs_ft_face_done(f.raw) };
        }
        // SAFETY: the library's faces are done; it is not used after.
        unsafe { mvs_ft_done(self.raw) };
    }
}
