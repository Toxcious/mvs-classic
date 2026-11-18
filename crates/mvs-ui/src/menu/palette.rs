//! UI_Functions GetPaletteEntry and PaletteForIndex, and PreferencesManager "Apply UI Material Colors": the
//! CentralPalette row a PaletteEntryType names (through the UI preferences' colour bindings) and its colours.

use indexmap::IndexMap;
use mvs_settings::UiSettings;

use crate::bundle::Rgba;

/// PaletteEntryType UI-White 5, UI-MainButton 6, UI-Interactive1 7: GetPaletteEntry takes the row of the type's
/// display name (@5-198); GLOBAL-Ally 0 .. GLOBAL-Team4 4 name no row and resolve through Get-UI-Color Binding
/// (@203).
const UI_TYPES: [&str; 3] = ["UI-White", "UI-MainButton", "UI-Interactive1"];

/// The UI preferences' colour binding of a GLOBAL type: 0 colorAlly, 1 colorEnemy,
/// 2 colorMe, 3 colorTeam3, 4 colorTeam4.
fn binding(ui: &UiSettings, kind: u8) -> Option<&str> {
    Some(match kind {
        0 => &ui.color_ally,
        1 => &ui.color_enemy,
        2 => &ui.color_me,
        3 => &ui.color_team3,
        4 => &ui.color_team4,
        _ => return None,
    })
}

/// GetPaletteEntry(type)'s row name: a GLOBAL type's bound row, a UI-* type's own.
pub fn palette_row(ui: &UiSettings, kind: u8) -> Option<&str> {
    binding(ui, kind).or_else(|| kind.checked_sub(5).and_then(|i| UI_TYPES.get(usize::from(i)).copied()))
}

/// GetPaletteEntry(type) in one column (the bundle's palette or palette_game). No row with that name: the entry
/// the function makes when its search finds none, (1, 0, 1, 1) in every column (@766-1034).
pub fn palette_colour(column: &IndexMap<String, Rgba>, ui: &UiSettings, kind: u8) -> Rgba {
    palette_row(ui, kind).and_then(|n| column.get(n)).copied().unwrap_or([1.0, 0.0, 1.0, 1.0])
}

/// PaletteForIndex(index) (@11-147): the PaletteEntryType of a palette index 0..4 (Ally, Enemy, Team3, Team4, Me);
/// None for another.
pub fn palette_for_index(index: i32) -> Option<u8> {
    usize::try_from(index).ok().and_then(|i| [0u8, 1, 3, 4, 2].get(i).copied())
}

// Apply UI Material Colors (@0-1246): mvs_view::skins::central_palette, through Bundle::central_palette_rows.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_by_type_and_the_missing_row() {
        let ui = UiSettings::default();
        assert_eq!(palette_row(&ui, 0), Some(ui.color_ally.as_str()));
        assert_eq!(palette_row(&ui, 6), Some("UI-MainButton"));
        assert_eq!(palette_row(&ui, 8), None);
        let mut col = IndexMap::new();
        col.insert("UI-White".to_string(), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(palette_colour(&col, &ui, 5), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(palette_colour(&col, &ui, 0), [1.0, 0.0, 1.0, 1.0]);
        assert_eq!(palette_for_index(4), Some(2));
        assert_eq!(palette_for_index(5), None);
    }
}
