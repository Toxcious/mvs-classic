//! Comp_FighterVisibilityManager's InvisibilityCount and mesh sets.
//! AddInvisibility: count + 1, and at count 1 the meshes are hidden; RemoveInvisibility: only above 0, count - 1,
//! and at 0 they are shown again (AddInvisibility @295-1029, RemoveInvisibility @344-1773).
//!
//! `Visibility::invisible` is "the count is above 0" (what the renderer reads, and what the intro writes directly:
//! ReadyForIntro's one Add and its one Remove, in the World's intro); `invis_more` is the count above 1, `None` at 0
//! so a fighter nobody hides twice keeps its state as it was.

use super::montages::char_data;
use crate::Real;
use crate::fighter::FighterId;
use crate::names::Name;
use crate::registry::signals::FighterSignal;
use crate::world::World;
use indexmap::IndexSet;

/// `invisibilityCount`.
pub fn invisibility_count(w: &World, f: FighterId) -> Real {
    let v = &w.fighter(f).vis;
    if v.invisible { 1.0 + v.invis_more.unwrap_or(0.0) } else { 0.0 }
}

/// `addInvisibility`: at count 1 AddInvisibility hides the main mesh and every non-armor mesh of the owner that is
/// not in AlwaysVisibleMeshes (@757-994): those keep the visibility they had, shown (a mesh enters the set shown, and
/// a hide takes it out: SetSubMeshVisibility @1372), so `invis_shown` starts as that set.
pub fn add_invisibility(w: &mut World, f: FighterId) {
    let v = &mut w.fighter_mut(f).vis;
    if v.invisible {
        v.invis_more = Some(v.invis_more.unwrap_or(0.0) + 1.0);
    } else {
        v.invisible = true;
        v.invis_more = None; // a direct write of `invisible` (the intro) left none
        v.invis_shown = v.always_visible.as_ref().filter(|s| !s.is_empty()).cloned();
    }
    // broadcast AdjustedVisibility @1029, every call
    let n = invisibility_count(w, f);
    crate::fighter::signal_bound(w, f, FighterSignal::AdjustedVisibility, false, n, Name::NONE, None);
}

/// `removeInvisibility`: at count 0 RemoveInvisibility shows the main mesh and every non-armor mesh not in
/// HiddenMeshes (@983-1082): `sub_hidden` alone says what is drawn again.
pub fn remove_invisibility(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    if !fi.vis.invisible {
        return;
    }
    let v = &mut fi.vis;
    if let Some(m) = v.invis_more.filter(|&m| m != 0.0 && !m.is_nan()) {
        let m = m - 1.0;
        v.invis_more = if m <= 0.0 { None } else { Some(m) };
    } else {
        v.invisible = false;
        v.invis_shown = None;
        // (with ArmorVisibilityCount > 0 the armor meshes not in HiddenArmor are shown again @1818-2396: the copies
        // a SetupInitialVisibility hid among them)
        if fi.armor.armor_vis.unwrap_or(0.0) > 0.0 {
            fi.armor.armor_hid = false;
        }
    }
    // broadcast AdjustedVisibility @1773 (count was > 0)
    let n = invisibility_count(w, f);
    crate::fighter::signal_bound(w, f, FighterSignal::AdjustedVisibility, false, n, Name::NONE, None);
}

/// A sub-mesh tag in lower case, as the sets hold it: the tag itself when it is, else its lower case's interned name
/// (`None`: the match has no such name, so no mesh of the character carries it).
fn lower(w: &World, tag: Name) -> Option<(Name, String)> {
    let s = w.names.str(tag);
    let t = s.to_lowercase();
    if t == s { Some((tag, t)) } else { w.names.get(&t).map(|n| (n, t)) }
}

/// `setSubmeshAlwaysVisible`: Comp_FighterVisibilityManager.SetSubmeshAlwaysVisible (@0-1290;
/// AnimNotify_SetSubmeshAlwaysVisible Begin true, End false), the non-armor half (the port has no armor meshes). No
/// mesh of the tag (Get Mesh @134): nothing. True, the mesh not in AlwaysVisibleMeshes yet (@705): when it is in
/// HiddenMeshes, SetSubMeshVisibility(tag, true) @793 (out of HiddenMeshes; shown only at count 0), then AddUnique
/// to AlwaysVisibleMeshes @827 and SetVisibility(true) @895, at any count: under an AddInvisibility it is drawn
/// (`invis_shown`). False: only RemoveItem from AlwaysVisibleMeshes @1127; the mesh keeps its visibility, so one
/// shown under an AddInvisibility stays shown until a hide or the count's next 0 -> 1.
/// A Disguise's clone meshes are not looked at (the build's Get Mesh reads the pawn's MeshManager).
pub fn set_submesh_always_visible(w: &mut World, f: FighterId, tag: Name, on: bool) {
    let Some((t, ts)) = lower(w, tag) else { return };
    let cd = char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    if !cd.def.sub_meshes.contains(&ts) {
        return;
    }
    let v = &mut w.fighter_mut(f).vis;
    if !on {
        if let Some(a) = &mut v.always_visible
            && a.shift_remove(&t)
            && a.is_empty()
        {
            v.always_visible = None;
        }
        return;
    }
    if v.always_visible.as_ref().is_some_and(|a| a.contains(&t)) {
        return;
    }
    if w.fighter(f).pose.sub_hidden.contains(&t) {
        // SetSubMeshVisibility(tag, true) @793, its UpdatedSubMesh(tag, true) broadcast @388 included
        super::slot_mix::set_sub_mesh(w, f, tag, true);
    }
    let fi = w.fighter_mut(f);
    let v = &mut fi.vis;
    v.always_visible.get_or_insert_with(IndexSet::new).insert(t);
    if v.invisible {
        v.invis_shown.get_or_insert_with(IndexSet::new).insert(t);
    }
}

/// `subMeshHidden`: SetSubMeshVisibility(tag, false)'s part in the two sets above, RemoveItem from
/// AlwaysVisibleMeshes @1372 and the mesh hidden @1432. `t` in lower case.
pub fn sub_mesh_hidden(w: &mut World, f: FighterId, t: Name) {
    let v = &mut w.fighter_mut(f).vis;
    if let Some(a) = &mut v.always_visible
        && a.shift_remove(&t)
        && a.is_empty()
    {
        v.always_visible = None;
    }
    if let Some(a) = &mut v.invis_shown
        && a.shift_remove(&t)
        && a.is_empty()
    {
        v.invis_shown = None;
    }
}

/// `setupInitialVisibility`: Comp_FighterVisibilityManager.SetupInitialVisibility (@0-868, behind
/// InitialVisibilitySet, which Reset @0 clears first): HiddenMeshes cleared @55, InvisibilityCount = 0 @96 (written,
/// not through RemoveInvisibility: the VFX it hid are not shown again), MainMeshVisible = true @119 and the main mesh
/// shown @275, then every Comp_Pawn_MeshManager mesh shown when it has the PrimaryMesh tag (@817) and else hidden
/// and added to HiddenMeshes (@962-996): the skin's initial set, CharDef.subHidden (the class's own, not a
/// size-scaled copy). Not written: AlwaysVisibleMeshes, BlendOutDependentNotifies, ArmorVisibilityCount,
/// VFXAlwaysVisible. Then broadcast OnSetInitialVisibility @868: the pawn's `initial_visibility` hook. The classes
/// that bind it are AnimBPs (Bugs Bunny's, C015's, C020's, Finn's, Garnet's and C023B's), whose handlers are
/// drawing state: a view takes the same moment from the respawn it sees; no sim entry binds the hook yet.
/// The armor copies are Comp_Pawn_MeshManager meshes (GetAllMeshes: ArmorMeshes' values) whose tags are their
/// source's without PrimaryMesh / RecieveArmor (Comp_Armor U@1230-1498): hidden @962 whatever the count, which this
/// leaves. They are shown again by AddArmorVisibility's 0 -> 1 (@106) and RemoveInvisibility at 0 with the count
/// above 0 (above); until then `armor_hid`.
/// Deviation: SetSubMeshVisibility(tag, true) with the count above 0 shows that tag's copy alone (@752-868), and
/// SetSubmeshAlwaysVisible too (@1040-1088): not ported (the copies stay hidden whole); taken as attached to the
/// main mesh, so its SetVisibility(true, false) @275 and a PrimaryMesh sub-mesh's (true, true) @817 do not reach
/// them (their attach parent is not read).
pub fn setup_initial_visibility(w: &mut World, f: FighterId) {
    let base = w.fighter(f).base_ch;
    let cd = char_data(&w.data, &w.names, base).expect("the fighter's base character data");
    let hidden: Vec<Name> = cd
        .def
        .sub_hidden
        .iter()
        .map(|t| w.names.get(t).unwrap_or_else(|| panic!("the sub-mesh tag {t} is not among the match's names")))
        .collect();
    let fi = w.fighter_mut(f);
    fi.pose.sub_hidden.clear();
    fi.pose.sub_hidden.extend(hidden);
    fi.vis.invisible = false;
    fi.vis.invis_more = None;
    fi.vis.invis_shown = None;
    fi.armor.armor_hid = fi.armor.armor_vis.unwrap_or(0.0) > 0.0;
    if let Some(h) = fi.hooks.char.initial_visibility {
        h(w, f);
    }
}
