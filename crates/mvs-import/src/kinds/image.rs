//! The `image` kind: an image file of the paks that is no package (the engine's Slate brush `.png` files), as
//! the file's bytes, named `/<pak path without .png>.<file name>`.

use super::config::{file_items, file_of, png_name};
use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version.
pub const IMAGE_KIND_VERSION: u64 = 1;

/// A PNG file's signature.
const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// The `image` kind.
pub struct Image;

impl Kind for Image {
    fn id(&self) -> &'static str {
        "image"
    }

    fn version(&self) -> u64 {
        IMAGE_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "image/png"
    }

    /// Every `.png` of the paks.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(file_items(cx, png_name))
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = &item.path;
        let entry = file_of(path, ".png", png_name).and_then(|f| cx.paks.entry(&f).map(|e| e.path.clone()));
        let Some(entry) = entry else {
            return Err(KindError::NotFound(format!("image: the build has no {path} (.png)")));
        };
        let bytes = cx.paks.read(&entry).map_err(|e| KindError::Failed(e.to_string()))?;
        if !bytes.starts_with(&SIGNATURE) {
            return Err(KindError::Failed(format!("image: {path} is not a PNG file")));
        }
        Ok(bytes)
    }
}
