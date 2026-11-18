//! The loading screen of every map load: the build's native SLoadingScreen (exe
//! 1412164e0), which UFighterGameInstance gives the movie player on PreLoadMap (exe 1410ae0e0) with its
//! LoadingScreenSettings. The settings are the `ui` kind's loading entry (`LoadingData`); the throbber piece is the
//! engine's Slate brush file of the `image` kind.
//!
//! ```text
//! SOverlay
//!   slot Fill / Fill: SBorder (white brush, BackgroundColor) > SScaleBox (ImageStretch) > SImage (a random Images entry)
//!   slot Fill / Bottom: SBorder (white brush, TipBackgroundColor) > SSafeZone > SDPIScaler (the DPI curve at the
//!     widget's own size) > SHorizontalBox
//!       auto, padding (25, 0, 0, 0), VAlign Center: SCircularThrobber (default arguments)
//!       fill: SSpacer (1, 1)
//!       auto, padding 20, HAlign Right, VAlign Center: the tip STextBlock, or an SSpacer without tips (the build)
//! ```
//!
//! Not ported: the tip line (the entry holds the tips; the build draws one when it has any), the port's own load-error
//! line and download progress bar.
//! Deviation: a ScaleToFit image is scaled to fill, so the screen has no black bars at other aspect ratios than the
//! image's.
//! Deviation: the throbber is drawn at `THROBBER_SCALE` times the build's size, and the row grows with it.
//! Unverified: the entry's colours are LinearColors given to the element list as its tints are (not as sRGB bytes).

use std::f64::consts::TAU;

use serde::Deserialize;

use crate::bundle::Rgba;
use crate::element::{BoxEl, DrawEffects, Element, ElementList, PaintGeometry};
use crate::hud::dpi_scale;
use crate::{Error, Result};

/// SCircularThrobber's default arguments (exe 14120e240); the piece brush "Throbber.CircleChunk" is 8 x 8.
const PIECES: usize = 6;
const PERIOD: f64 = 0.75;
const RADIUS: f64 = 16.0;
const PIECE: f64 = 8.0;
const PAD_LEFT: f64 = 25.0;
const THROBBER_SCALE: f64 = 3.0;
const TIP_PAD: f64 = 20.0;
/// The screen's layer in the element list.
const LAYER: i32 = 0;

/// The loading entry (`LoadingValue`, mvs-import `kinds/ui/loading.rs`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    entry: String,
    images: Vec<Image>,
    image_stretch: String,
    background_color: Vec<f64>,
    tip_background_color: Vec<f64>,
    throbber_piece: String,
}

#[derive(Debug, Deserialize)]
struct Image {
    path: String,
}

/// The loading screen: its settings and the image it picked.
#[derive(Debug)]
pub struct LoadingScreen {
    data: Entry,
    /// The picked background (FMath::RandRange over Images: the build has one).
    image: Option<String>,
}

fn colour(c: &[f64]) -> Rgba {
    let at = |i: usize, d: f64| c.get(i).copied().unwrap_or(d);
    [at(0, 0.0), at(1, 0.0), at(2, 0.0), at(3, 1.0)]
}

/// A box at `at` of `size` (pixels), white without a texture.
fn boxed(list: &mut ElementList, at: [f64; 2], size: [f64; 2], texture: Option<&str>, tint: Rgba) {
    let geometry = PaintGeometry { size, transform: [1.0, 0.0, 0.0, 1.0, at[0], at[1]] };
    let el =
        BoxEl { geometry, texture: texture.map(str::to_owned), tint, uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() };
    list.push(LAYER, None, DrawEffects::NONE, Element::Box(el));
}

impl LoadingScreen {
    /// The `ui` kind's loading entry from the store; `pick` is a number in [0, 1) choosing the background.
    pub fn load(store: &mvs_store::Store, pick: f64) -> Result<LoadingScreen> {
        let pack = store.kind("ui")?;
        let path =
            pack.paths().find(|p| p.ends_with("/loading.loading")).map(str::to_string).ok_or_else(|| {
                Error::Format { what: "ui loading".into(), why: "the ui kind has no such entry".into() }
            })?;
        LoadingScreen::parse(&pack.bytes(&path)?, &path, pick)
    }

    /// The entry from its JSON bytes; `what` names it in errors.
    pub fn parse(bytes: &[u8], what: &str, pick: f64) -> Result<LoadingScreen> {
        let data: Entry =
            serde_json::from_slice(bytes).map_err(|e| Error::Format { what: what.to_string(), why: e.to_string() })?;
        if data.entry != "loading" {
            return Err(Error::Format { what: what.to_string(), why: format!("a {} entry", data.entry) });
        }
        let n = data.images.len();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let image = (n > 0).then(|| data.images[((pick.clamp(0.0, 1.0) * n as f64) as usize).min(n - 1)].path.clone());
        Ok(LoadingScreen { data, image })
    }

    /// The background's texture path (its size is wanted by `paint`).
    pub fn image(&self) -> Option<&str> {
        self.image.as_deref()
    }

    /// The frame at `time` seconds after the screen came up, on a target of `size` pixels; `image` is the
    /// background's pixel size.
    #[allow(clippy::cast_precision_loss)]
    pub fn paint(&self, size: [u32; 2], image: Option<[f64; 2]>, time: f64, list: &mut ElementList) {
        let [w, h] = size.map(f64::from);
        let d = &self.data;
        // the window is black behind the widget
        boxed(list, [0.0, 0.0], [w, h], None, [0.0, 0.0, 0.0, 1.0]);
        boxed(list, [0.0, 0.0], [w, h], None, colour(&d.background_color));
        if let (Some(path), Some([iw, ih])) = (&self.image, image) {
            let (sx, sy) = (w / iw, h / ih);
            // SScaleBox: ScaleToFit takes the smaller axis scale, ScaleToFill the larger, the content centred.
            let stretch = if d.image_stretch == "ScaleToFit" { "ScaleToFill" } else { d.image_stretch.as_str() };
            let (fx, fy) = match stretch {
                "Fill" => (sx, sy),
                "ScaleToFill" => (sx.max(sy), sx.max(sy)),
                "ScaleToFitX" => (sx, sx),
                "ScaleToFitY" => (sy, sy),
                "None" => (1.0, 1.0),
                _ => (sx.min(sy), sx.min(sy)),
            };
            boxed(list, [(w - iw * fx) / 2.0, (h - ih * fy) / 2.0], [iw * fx, ih * fy], Some(path), [1.0; 4]);
        }

        // The bottom row, in the SDPIScaler's units. Without tips its height is the tip slot's padding.
        let s = dpi_scale(w, h);
        let row = (2.0 * RADIUS * THROBBER_SCALE).max(2.0 * TIP_PAD);
        let top = h - row * s;
        boxed(list, [0.0, top], [w, row * s], None, colour(&d.tip_background_color));

        // SCircularThrobber::OnPaint (Unverified: 4.26 source): piece i at angle 2 pi / N x i + phase, scaled
        // (i + 1) / N, placed at LocalOffset + LocalOffset x (sin, cos) with LocalOffset = (size - piece) / 2.
        let throbber = 2.0 * RADIUS * THROBBER_SCALE;
        let piece = PIECE * THROBBER_SCALE;
        let (x0, y0) = (PAD_LEFT * s, top + ((row - throbber) / 2.0) * s);
        let off = (throbber - piece) / 2.0;
        let phase = ((time / PERIOD) % 1.0) * TAU;
        for i in 0..PIECES {
            let a = (TAU / PIECES as f64) * i as f64 + phase;
            let k = (i + 1) as f64 / PIECES as f64;
            let side = piece * k * s;
            boxed(
                list,
                [x0 + (off + off * a.sin()) * s, y0 + (off + off * a.cos()) * s],
                [side, side],
                Some(&d.throbber_piece),
                [1.0; 4],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = r#"{"entry":"loading","images":[{"path":"a"},{"path":"b"}],"imageStretch":"ScaleToFit",
        "backgroundColor":[0,0,0,1],"tipBackgroundColor":[0,0,0,0.75],"tips":[],
        "tipsFont":{"face":"f","size":18},"throbberPiece":"p"}"#;

    #[test]
    fn picks_an_image_and_paints_the_rows() {
        let s = LoadingScreen::parse(ENTRY.as_bytes(), "t", 0.99).unwrap();
        assert_eq!(s.image(), Some("b"));
        let mut list = ElementList::default();
        s.paint([1920, 1080], Some([1920.0, 1080.0]), 0.0, &mut list);
        // black, background colour, image, tip bar, six throbber pieces
        assert_eq!(list.elements.len(), 4 + PIECES);
    }

    #[test]
    fn rejects_another_entry() {
        assert!(LoadingScreen::parse(ENTRY.replace("\"loading\"", "\"hud\"").as_bytes(), "t", 0.0).is_err());
    }
}
