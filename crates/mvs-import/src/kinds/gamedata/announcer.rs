//! The announcer's VO pack.
//!
//! The pack's path: PreferencesManager LoadStoredAnnouncerPackAssetData (GlobalEquippables.AnnouncerPack.AssetPath,
//! LoadAssetFromStringBlocking @200); DefaultAnnouncerPack is its fallback for an empty path or one that is no
//! AnnouncerPackData (@390 / @414). The rows: PandaGameState's Comp_Actor_VO.InitVOData(pack.DTVO) (@20401) takes the
//! whole table; kept are the rows of the aliases the port posts and the tutorial's, as [alias, tags, priority, event
//! name, length] (fighter.vo's layout). Every pack of the build gives each posted alias exactly one row. Not taken:
//! the aliases no ported graph posts (oneminuteremaining, secondsuntilstart_*, countdown, rematch*, store, toast,
//! matchpoint_generic).
//!
//! Every pack of the build is bound too (`announcer_packs`): an earlier importer read another pack from the build by
//! its path when it was asked for; the native game reads only the store.

use mvs_data::{AnnouncerPack, AnnouncerPacks, Value};

use super::shape::{fail, properties, ref_name};
use super::vo_rows::vo_rows;
use crate::kind::{Cx, KindError};

/// The aliases the port posts: the match's five (World.announce) and the pre-match screen's two (UI_PreMatch @16531
/// / @29394).
pub const ANNOUNCER_POSTED: [&str; 7] = [
    "announcer_timeout",
    "announcer_matchpoint_blue",
    "announcer_matchpoint_red",
    "announcer_victory",
    "announcer_defeat",
    "announcer_characterselect_intro",
    "announcer_characterselect_outro",
];

/// The tutorial actors' lines (TutorialActor Start Tutorial's IntroVOSoundAlias @9832, a step's VOSoundAlias @9316 /
/// @829: BPI_PlayVO on the game state): DTVO_Announcer's `vo_announcer_tutorial_*` rows, any case.
const TUTORIAL_ANNOUNCER: &str = "vo_announcer_tutorial_";

pub const DEFAULT_ANNOUNCER_PACK: &str =
    "/Game/Panda_Main/Blueprints/Rewards/AnnouncerPacks/DefaultAnnouncerPack.DefaultAnnouncerPack";
pub const ANNOUNCER_PACK_CLASS: &str = "/Script/MultiVersus.AnnouncerPackData";
/// The folder the packs are listed under (the cosmetics catalogue's, `kinds/cosmetics.rs`).
const PACKS_ROOT: &str = "/Game/Panda_Main";

/// Whether the pack keeps a row of `alias`: a posted alias, or a tutorial line.
fn kept(alias: &str) -> bool {
    let low = alias.to_lowercase();
    ANNOUNCER_POSTED.contains(&low.as_str()) || low.starts_with(TUTORIAL_ANNOUNCER)
}

/// The AnnouncerPackData at `path` with its DTVO table's rows of the posted and the tutorial's aliases, in table
/// order; an error when the path is no AnnouncerPackData.
pub fn announcer_pack(cx: &Cx<'_>, path: &str) -> Result<AnnouncerPack, KindError> {
    let o = cx.object(path)?;
    if o.class() != ANNOUNCER_PACK_CLASS {
        return fail(format!("{path} is a {}, not an AnnouncerPackData", o.class()));
    }
    let Some(Value::Str(dtvo)) = properties(&o)?.shift_remove("DTVO") else {
        return fail(format!("{path}: no DTVO"));
    };
    let rows = vo_rows(cx, &dtvo, &kept)?;
    Ok(AnnouncerPack {
        path: path.to_owned(),
        name: ref_name(&Value::Str(path.to_owned()))?,
        rows: rows.into_iter().map(|(a, t, p, ev, len)| (a, t, p, ev.unwrap_or_default(), len)).collect(),
    })
}

/// DefaultAnnouncerPack.
pub fn announcer(cx: &Cx<'_>) -> Result<AnnouncerPack, KindError> {
    announcer_pack(cx, DEFAULT_ANNOUNCER_PACK)
}

/// Every AnnouncerPackData the asset catalogue lists under /Game/Panda_Main (the Collection's announcer list is
/// these, less the hidden ones), each by its object path, in the catalogue's order.
pub fn announcer_packs(cx: &Cx<'_>) -> Result<AnnouncerPacks, KindError> {
    let listed = cx.catalog.list_packages(PACKS_ROOT, true, Some(ANNOUNCER_PACK_CLASS));
    let packs = listed.iter().map(|p| announcer_pack(cx, &crate::game::exported(p))).collect::<Result<Vec<_>, _>>()?;
    if !packs.iter().any(|p| p.path.eq_ignore_ascii_case(DEFAULT_ANNOUNCER_PACK)) {
        return fail(format!("the announcer packs listed under {PACKS_ROOT} lack {DEFAULT_ANNOUNCER_PACK}"));
    }
    Ok(AnnouncerPacks { packs })
}
