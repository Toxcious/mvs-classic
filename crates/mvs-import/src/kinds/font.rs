//! The `font` kind: a UFontFace's font file (`<package>.ufont`, the TrueType or OpenType file itself).

use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version.
pub const FONT_KIND_VERSION: u64 = 1;

/// The class of a font face.
const FONT_FACE: &str = "/Script/Engine.FontFace";

/// The `font` kind.
pub struct Font;

impl Kind for Font {
    fn id(&self) -> &'static str {
        "font"
    }

    fn version(&self) -> u64 {
        FONT_KIND_VERSION
    }

    /// `font/otf` for an OpenType file with CFF outlines (`OTTO`), else `font/ttf`.
    fn content_type(&self, bytes: &[u8]) -> &'static str {
        if bytes.starts_with(b"OTTO") { "font/otf" } else { "font/ttf" }
    }

    /// Every UFontFace of the build.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx.catalog.paths_of(&[FONT_FACE]).into_iter().map(Item::new).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = &item.path;
        let (pkg, i) = cx.read(path)?;
        let name = cx.class_of(&pkg, i)?;
        if name != FONT_FACE {
            return Err(KindError::NotFound(format!("font: {path} is a {name}, not a {FONT_FACE}")));
        }
        let ufont = format!("{}.ufont", pkg.path);
        if cx.paks.entry(&ufont).is_none() {
            return Err(KindError::Failed(format!(
                "font: {path} has no {ufont} (a face with its data inline is not read)"
            )));
        }
        cx.paks.read(&ufont).map_err(|e| KindError::Failed(e.to_string()))
    }
}
