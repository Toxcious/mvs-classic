//! The loading screen's entry: the build's native loading screen (SLoadingScreen, exe 1412164e0), set up by
//! UFighterGameInstance from its LoadingScreenSettings (FLoadingScreenDescription; PandaGameInstance's class defaults
//! over the constructor's, exe 14120e930).

use serde::Serialize;

use super::widget::{Obj, truthy};
use super::{TEXTURE, pkg_of};
use crate::game::exported;
use crate::json::Json;
use crate::kind::{Cx, KindError};

/// FCoreStyle "Throbber.CircleChunk": IMAGE_BRUSH("Common/Throbber_Piece", FVector2D(8, 8)).
const THROBBER_PIECE: &str = "Common/Throbber_Piece";
const STRETCH: [&str; 8] =
    ["None", "Fill", "ScaleToFit", "ScaleToFitX", "ScaleToFitY", "ScaleToFill", "ScaleBySafeZone", "UserSpecified"];

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

#[derive(Serialize)]
struct Image {
    path: String,
}

#[derive(Serialize)]
struct TipsFont {
    face: String,
    size: Json,
}

/// The loading entry (`LoadingValue`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Loading {
    entry: &'static str,
    images: Vec<Image>,
    image_stretch: String,
    background_color: Vec<Json>,
    tip_background_color: Vec<Json>,
    tips: Vec<String>,
    tips_font: TipsFont,
    throbber_piece: String,
}

/// A colour as `[R, G, B, A]`, `d` for none (`color`).
fn color(c: Option<&Json>, d: &[f64; 4]) -> Vec<Json> {
    match c {
        None => d.iter().map(|&n| Json::Num(n)).collect(),
        Some(x) => ["R", "G", "B", "A"].iter().map(|k| x.get(k).cloned().unwrap_or(Json::Null)).collect(),
    }
}

/// A text value's source string (`source`).
fn source(t: &Json) -> String {
    match t {
        Json::Obj(_) => t.get("source").and_then(Json::as_str).unwrap_or("").to_owned(),
        _ => String::new(),
    }
}

/// The loading entry's JSON.
pub fn bind_loading(cx: &Cx<'_>) -> Result<Vec<u8>, KindError> {
    let cdo =
        cx.object("/Game/Panda_Main/Blueprints/PandaGameInstance/PandaGameInstance.Default__PandaGameInstance_C")?;
    let props = match cdo.defaults()? {
        Some(d) => crate::json::props_json(cdo.cache(), &d),
        None => crate::json::props_json(cdo.cache(), cdo.props()),
    };
    let Some(set) = props.get("LoadingScreenSettings") else {
        return failed("ui: loading: no LoadingScreenSettings".into());
    };
    let field = |k: &str| set.get(k);
    // (BackgroundColor's default is a global of the exe that the class defaults always override here)
    if !truthy(field("BackgroundColor")) {
        return failed("ui: loading: BackgroundColor: constructor default not read".into());
    }
    let stretch = match field("ImageStretch") {
        Some(s) if truthy(Some(s)) => match s.as_str() {
            Some(s) => s.rfind("::").map_or(s, |i| &s[i + 2..]).to_owned(),
            None => "undefined".to_owned(),
        },
        _ => "ScaleToFit".to_owned(),
    };
    if !STRETCH.contains(&stretch.as_str()) {
        return failed(format!("ui: loading: ImageStretch {stretch}"));
    }
    if field("MoviePaths").and_then(Json::as_array).is_some_and(|m| !m.is_empty()) {
        return failed("ui: loading: MoviePaths not handled".into());
    }
    let mut images = Vec::new();
    for p in field("Images").and_then(Json::as_array).unwrap_or(&[]) {
        let path = p.as_str().unwrap_or("undefined");
        if cx.object(path)?.class() != TEXTURE {
            return failed(format!("ui: loading image {path}: not a Texture2D"));
        }
        images.push(Image { path: path.to_owned() });
    }
    // LoadingTipsFont: the PFG composite's typeface -> the font face's package
    let Some(font) = field("LoadingTipsFont") else { return failed("ui: loading: no LoadingTipsFont".into()) };
    let Some(font_object) = font.get("FontObject").and_then(Json::as_str) else {
        return failed("ui: loading: LoadingTipsFont has no FontObject".into());
    };
    let pfg = Obj::load(cx, font_object)?;
    let fonts = pfg
        .props
        .get("CompositeFont")
        .and_then(|c| c.get("DefaultTypeface"))
        .and_then(|d| d.get("Fonts"))
        .and_then(Json::as_array)
        .unwrap_or(&[]);
    let wanted = font.get("TypefaceFontName");
    let Some(entry) = fonts.iter().find(|f| f.get("Name") == wanted && wanted.is_some()) else {
        return failed(format!("ui: loading: typeface {}", wanted.and_then(Json::as_str).unwrap_or("undefined")));
    };
    let face = entry.get("Font").and_then(|f| f.get("LocalFontFaceAsset")).and_then(Json::as_str).unwrap_or("");
    let tips = field("LoadingTips").and_then(Json::as_array).unwrap_or(&[]).iter().map(source).collect();
    let v = Loading {
        entry: "loading",
        images,
        image_stretch: stretch,
        background_color: color(field("BackgroundColor"), &[0.0; 4]),
        tip_background_color: color(field("TipBackgroundColor"), &[0.0, 0.0, 0.0, 0.75]),
        tips,
        tips_font: TipsFont { face: pkg_of(face).to_owned(), size: font.get("Size").cloned().unwrap_or(Json::Null) },
        throbber_piece: exported(&format!("{}{THROBBER_PIECE}", super::tables::SLATE_FOLDER)),
    };
    Ok(crate::json::to_vec(&v)?)
}
