//! The `text` kind: one localization target's strings for one culture of [`TEXT_CULTURES`], as JSON
//! namespace -> key -> string in the file's order (`mvs_uasset::locres`), named by the .locres's path under its
//! mount without the extension.

use std::sync::Arc;

use mvs_uasset::locres::{LocresTable, read_locres};

use crate::json;
use crate::kind::{Cx, Item, Kind, KindError, Memo};

/// The kind's version.
pub const TEXT_KIND_VERSION: u64 = 1;

/// The cultures the game draws in.
pub const TEXT_CULTURES: &[&str] = &["en"];

/// The culture the game's text is read in.
pub const GAME_CULTURE: &str = "en";

/// The game target's entry of a culture:
/// `/Game/Localization/Game/<culture>/Game.Game`.
pub fn game_text_path(culture: &str) -> String {
    format!("/Game/Localization/Game/{culture}/Game.Game")
}

/// The run's tables, by entry path.
#[derive(Default)]
struct Tables(Memo<Result<Arc<LocresTable>, KindError>>);

/// The game target's strings of `culture` (namespace -> key -> string), read from the paks once per run as the
/// kind's entry holds them: what a game kind read of the text kind (`scope.load('text', gameTextPath(c))`).
pub fn table(cx: &Cx<'_>, culture: &str) -> Result<Arc<LocresTable>, KindError> {
    let path = game_text_path(culture);
    cx.shared.get_or_init(Tables::default).0.get(&path, || read_table(cx, &path).map(Arc::new))
}

/// The table of the text entry at `path`.
fn read_table(cx: &Cx<'_>, path: &str) -> Result<LocresTable, KindError> {
    let file = cx.paths.package_file(path).and_then(|b| cx.paks.entry(&format!("{b}.locres")).map(|e| e.path.clone()));
    let Some(file) = file else {
        return Err(KindError::NotFound(format!("text: the build has no {path} (.locres)")));
    };
    let bytes = cx.paks.read(&file).map_err(|e| KindError::Failed(e.to_string()))?;
    Ok(read_locres(&bytes, &file)?)
}

/// `(directory, name)` of a `<dir>/Localization/<Target>/<culture>/<Target>.locres` of a culture the kind covers
/// (the regex `^(.*\/Localization\/([^/]+)\/([^/]+)\/)([^/]+)\.locres$`, `i`; the target and the file's name equal
/// as written): the part before the name, and the name.
fn locres_name(file: &str) -> Option<(&str, &str)> {
    let cut = file.len().checked_sub(".locres".len())?;
    if !file.is_char_boundary(cut) || !file[cut..].eq_ignore_ascii_case(".locres") {
        return None;
    }
    let stem = &file[..cut];
    let segments: Vec<&str> = stem.split('/').collect();
    // `.*` and its slash take at least one segment
    let [.., localization, target, culture, name] = segments[..] else { return None };
    if segments.len() < 5
        || !localization.eq_ignore_ascii_case("Localization")
        || [target, culture, name].iter().any(|s| s.is_empty())
    {
        return None;
    }
    (target == name && TEXT_CULTURES.contains(&culture)).then(|| (&stem[..stem.len() - name.len()], name))
}

/// The `text` kind.
pub struct Text;

impl Kind for Text {
    fn id(&self) -> &'static str {
        "text"
    }

    fn version(&self) -> u64 {
        TEXT_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// Every target's file of the cultures of [`TEXT_CULTURES`].
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx
            .paks
            .paths()
            .filter_map(|f| {
                let (dir, name) = locres_name(f)?;
                let p = cx.paths.package_path(&format!("{dir}{name}"))?;
                Some(Item::new(format!("{p}.{name}")))
            })
            .collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        Ok(json::to_vec(&read_table(cx, &item.path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let n = locres_name("MultiVersus/Content/Localization/Game/en/Game.locres");
        assert_eq!(n, Some(("MultiVersus/Content/Localization/Game/en/", "Game")));
        assert_eq!(locres_name("MultiVersus/Content/Localization/Game/fr/Game.locres"), None);
        assert_eq!(locres_name("MultiVersus/Content/Localization/Game/en/Other.locres"), None);
        assert_eq!(locres_name("Localization/Game/en/Game.locres"), None);
    }
}
