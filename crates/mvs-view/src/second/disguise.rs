//! Buff_Disguise's DisguiseMesh: the copy of
//! the victim's view on the owner's pawn, its sub-meshes by master pose, in the victim's skin and the owner's
//! variant and stencil, posed with the copied montage over the victim's idle.
//!
//! The port's own, for an owner whose kit gives the buff: one copy per other fighter of
//! the match, made with it (super), taken by the buff and handed back when it ends.
//! Deviation: an owner whose kit does not give the buff (a buff a test gives) gets no copy: the buff appearing would
//! make one; here it is warned and not drawn.
//! Deviation: the static clones (SDM@10400-11219: the victim's StaticMeshComponents copied onto the second mesh, Arya
//! copying Taz's C015_Neutral_Normal_Ground's pot) are not made.
//! Deviation: the victim's pose hook reads the owner (a full stand-in would also give it the copy's montage in place of
//! the owner's, the victim's data and the Disguise's sub-mesh map): a hook that reads its fighter's montage or
//! sub-mesh map reads the owner's.
//! Unverified: what the victim's AnimBP shows outside a montage on the owner's pawn is not decoded: the copy is posed
//! with the victim's idle there.

use mvs_sim::World;
use mvs_sim::combat::disguise::{disguise_mesh_y, disguise_shown};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::instance_data;
use mvs_sim::names::Name;

use std::collections::HashMap;

use super::{STEP, SecondView, warn_once};
use crate::anim::Clips;
use crate::frame::SecondKind;
use crate::pose::{Rec, ViewPoser};
use crate::skins::{FighterColour, Variant};
use crate::view::{Loco, PoseRules, ViewKey};

/// The characters whose kit gives the buff (Arya's).
const WARM: [&str; 1] = [mvs_sim::chars::arya::KEY];

/// One copy: `owner` has the buff, `victim` is the fighter it copies.
#[derive(Debug)]
struct CopyView {
    s: SecondView,
    owner: FighterId,
    victim: FighterId,
    /// The victim's character (the clips').
    key: String,
    /// The parts' DisguiseSubmeshes keys (the victim's tags, lower case).
    tags: Vec<Option<String>>,
    idle: Option<usize>,
    posed: bool,
}

/// The Disguise copies of a match.
#[derive(Debug, Default)]
pub(crate) struct Copies {
    views: Vec<CopyView>,
}

impl Copies {
    /// For each fighter whose character gives the buff, a copy of each other fighter.
    pub(super) fn new(p: &mut ViewPoser, store: &mvs_store::Store, w: &World) -> crate::Result<Copies> {
        let mut out = Copies::default();
        let ids = w.all_ids();
        for &i in &ids {
            if !WARM.contains(&w.names.str(w.fighter(i).ch)) {
                continue;
            }
            for &v in &ids {
                if v != i
                    && let Some(c) = build(p, store, w, i, v)?
                {
                    out.views.push(c);
                }
            }
        }
        Ok(out)
    }

    pub(super) fn views_mut(&mut self) -> impl Iterator<Item = &mut SecondView> {
        self.views.iter_mut().map(|v| &mut v.s)
    }

    pub(super) fn reset(&mut self) {}

    /// `place`: each owner's buff takes its copy of the victim, poses it, or hands it back.
    pub(super) fn place(&mut self, w: &World, clips: &mut HashMap<String, Clips>) {
        for f in w.all_ids() {
            let d = w.fighter(f).shapes.disguise.as_ref();
            let cur = self.views.iter().position(|v| v.owner == f && v.s.active);
            let cur = match (cur, d) {
                (Some(c), Some(d)) if self.views[c].victim == d.from => Some(c),
                (Some(c), _) => {
                    self.views[c].s.drop_view();
                    None
                }
                (None, _) => None,
            };
            let Some(d) = d else { continue };
            let at = match cur {
                Some(c) => c,
                None => {
                    let Some(c) = self.views.iter().position(|v| v.owner == f && v.victim == d.from) else {
                        warn_once(format!(
                            "disguise: no copy was made for {}'s Disguise (its kit gives no buff of it): not drawn",
                            w.names.str(w.fighter(f).ch)
                        ));
                        continue;
                    };
                    self.views[c].s.take();
                    self.views[c].posed = false;
                    c
                }
            };
            let v = &mut self.views[at];
            let Some(c) = clips.get_mut(&v.key) else { continue };
            tick(v, w, f, c);
        }
    }
}

/// `build`: owner `i`'s copy of fighter `v`'s view.
fn build(
    p: &mut ViewPoser,
    store: &mvs_store::Store,
    w: &World,
    i: FighterId,
    v: FighterId,
) -> crate::Result<Option<CopyView>> {
    let (Some(o), Some(r)) = (p.rec_for(i), p.rec_for(v)) else { return Ok(None) };
    let owner: FighterColour = o.colour;
    let (key, skin) = (r.key.clone(), r.skin.clone());
    // GetMaterialColor: the owner's colour index (a local online owner's LocalPlayerMaterialOverride is not reached:
    // the poser's match is played offline)
    let variant = if owner.color_index == 1 { Variant::Blue } else { Variant::Red };
    // (the overlay's choice is the owner's IsLocalOnlinePlayer, its colour value 0.6)
    let local = owner.team == 0.6;
    let colour = FighterColour {
        variant,
        armor_local: local,
        armor_team: if local { 0.5 } else { f64::from(owner.color_index) },
        ..owner
    };
    let armor_main = p.skinned(&key, &skin.name);
    let tags = skin.part_tags.iter().map(|t| t.as_ref().map(|t| t.to_lowercase())).collect();
    let mut rec = p.make_rec(store, w, i, &key, skin, colour, Some(SecondKind::Disguise))?;
    rec.armor_main = Some(armor_main);
    let idle = char_idle(w, v).and_then(|n| p.clips_of(&key).and_then(|c| c.get(&n)));
    if idle.is_none() {
        warn_once(format!("disguise: the idle clip of {key} is not loaded"));
    }
    Ok(Some(CopyView {
        s: SecondView::new(rec, SecondKind::Disguise, Some(&key)),
        owner: i,
        victim: v,
        key,
        tags,
        idle,
        posed: false,
    }))
}

/// The victim's set's idle (`from.animSet.idle`).
fn char_idle(w: &World, v: FighterId) -> Option<String> {
    mvs_sim::fighter::anim_set_def(w, v).idle.clone()
}

/// `tick`: one presentation step of owner `f`'s taken copy.
fn tick(v: &mut CopyView, w: &World, f: FighterId, clips: &mut Clips) {
    let fi = w.fighter(f);
    let d = fi.shapes.disguise.as_ref().expect("a disguise");
    let shown = disguise_shown(w, f) && fi.life.dead == 0.0;
    let (x, y) = (fi.body.x, disguise_mesh_y(w, f));
    let Rec { view, pose, colour, outlines_on, skin, armor_on, .. } = &mut v.s.rec;
    // (a part the sim keeps no tag of: its first visibility)
    for (k, t) in v.tags.iter().enumerate() {
        view.part_on[k] = match t {
            Some(t) => !w.names.get(t).is_some_and(|n| d.sub_hidden.contains(&n)),
            None => skin.part_init[k],
        };
    }
    // The owner's ArmorVisibilityCount (@20577: each copy from it alone, whatever its sub-mesh shows)
    *armor_on = fi.armor.armor_vis.unwrap_or(0.0) > 0.0;
    view.size_scale = f64::from(fi.body.size_scale);
    *outlines_on = colour.outline && mvs_sim::fighter::outlines_on(w, f);
    view.place(f64::from(x), f64::from(y), f64::from(fi.body.facing_dir));
    // The copy's own blend-out over the idle; a copy halted by Montage_Stop(0.0) is the idle at once.
    let c = d.copy.as_ref().map(|c| &c.m);
    let halted = c.is_some_and(|m| m.ended && !m.terminated);
    let over = c.is_some_and(|m| m.terminated || m.ended);
    let md = c.and_then(|m| instance_data(&w.data, w, f, m.montage.uid));
    let mp = match (c, md) {
        (Some(m), Some(md)) if !over => m.pose(md, None),
        _ => None,
    };
    if mp.is_some() || shown || !v.posed {
        if let Some(h) = pose.as_mut() {
            h.set(w, f, view, clips);
        }
        // (pose() with no `loco`: the clip's own key, the 0.1 s crossfade)
        let idle_key = ViewKey::Clip(v.idle.unwrap_or(usize::MAX), Name::NONE);
        let loco = Loco { dur: 0.1, hermite: false, key: idle_key };
        let rules = pose.as_deref_mut().map(|p| p as &mut (dyn PoseRules + 'static));
        match (c, md, mp) {
            (Some(m), Some(md), Some((si, gi, t))) => {
                let anim = &md.slots[si].segments[gi].anim;
                let clip = clips.get(anim);
                if clip.is_none() {
                    warn_once(format!("disguise: clip not loaded: {anim} ({})", v.key));
                }
                let wt = m.shown_weight(md, &|s| eprintln!("warning: {s}"));
                if wt < 1.0 {
                    view.slot_under(v.idle, 0.0, wt, idle_key);
                }
                let name = Some(m.montage.name);
                view.pose(clips, clip, t, name, md.blend_in, md.blend_out, STEP, false, &loco, None, &[], rules);
            }
            _ => {
                if halted {
                    view.montage_gone(0.0);
                }
                view.pose(clips, v.idle, 0.0, None, 0.0, 0.0, STEP, false, &loco, None, &[], rules);
            }
        }
        v.posed = true;
    }
    v.s.placed(w, f, f64::from(x), f64::from(y), shown);
}
