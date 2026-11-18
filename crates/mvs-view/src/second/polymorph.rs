//! Buff_PolymorphParent / Buff_C020Polymorph's Polymorph mesh on its victim: a view of the mesh's character (the
//! Creature's skin) or of a kit's prop (Taz's chicken), placed at the bottom of the polymorph capsule, posed with the
//! buff's montage over the victim's own machine in the mesh's set.
//!
//! The port's own: a view per fighter and per Polymorph Mesh another fighter's kit can put on it, made with the match
//! (super), used again by the fighter's next polymorph of that mesh.
//! Deviation: Creature_AnimBP's and Chicken_AnimBP's own graphs are not evaluated: under the montages the mesh is posed
//! with the victim's machine (its anim, time, crossfade and key) in the mesh set's clip of the same role (an AnimSet
//! field), at the victim's time (wrapped for a clip of the set's looping list, held at its end otherwise); a clip of
//! no role is the set's idle.
//! The mesh character's pose hook reads the victim as a stand-in of it without its character's own state
//! (PoseOf::stand_in: Rick's Creature hook reads no Reindog state).
//! Deviation: a montage stopped by Montage_Stop with a blend time shows no blend-out (the sim's stopPlay rule).
//! A Polymorph Mesh the items part owns (the drumstick's Buff_Chicken with no Taz in the match) is the items entry's
//! draw '<buff>.Polymorph' (its mesh, the buff's Materials as its variants, the clips of its folder), posed with the
//! items part's set of the buff's AnimBP class as a kit's prop; its views are made with the match for every fighter,
//! in a match with items.

use mvs_data::def::AnimSet;
use mvs_data::fighter::{AnimAsset, Map};
use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::combat::polymorph::{ITEMS_OWNER, polymorph_owner};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::AnimKey;
use mvs_sim::fighter::anim::montages::instance_data;
use mvs_sim::fighter::common::char_data;
use mvs_sim::names::Name;
use mvs_sim::world::flow::items_data_of;

use std::collections::HashMap;

use super::{STEP, SecondView, warn_once};
use crate::anim::{Clips, asset_name};
use crate::frame::SecondKind;
use crate::pose::{Rec, ViewPoser};
use crate::skins;
use crate::view::{Loco, PoseRules, ViewKey};

/// A set's role names in the AnimSet's key order (its string fields and hrN).
fn set_roles(s: &AnimSet) -> Vec<Option<String>> {
    let mut v = vec![
        s.idle.clone(),
        s.idle_land.clone(),
        s.alt_enter.clone(),
        s.alt_loop.clone(),
        s.crouch.clone(),
        s.crouch_land.clone(),
        s.run.clone(),
        s.run_stop.clone(),
        s.run_stop2.clone(),
        s.crouch_in.clone(),
        s.crouch_out.clone(),
        s.run_land.clone(),
        s.wall_enter.clone(),
        s.wall_idle.clone(),
        s.fall.clone(),
        s.fast_fall.clone(),
        s.jump1.clone(),
        s.jump2.clone(),
        s.hr_front.clone(),
        s.hr_back.clone(),
    ];
    v.extend((0..4).map(|k| s.hr_n.get(k).cloned().flatten()));
    v.extend([
        s.recover.clone(),
        s.fly_back.clone(),
        s.fly_forward.clone(),
        s.fly_back_intro.clone(),
        s.fly_forward_intro.clone(),
        s.wall_bounce_back.clone(),
        s.wall_bounce_front.clone(),
        s.floor_bounce_back.clone(),
        s.floor_bounce_front.clone(),
    ]);
    v
}

/// The AnimBP class defaults' keys of [`set_roles`]' roles.
const ROLE_KEYS: [&str; 33] = [
    "Anim_Idle",
    "Anim_IdleLand",
    "Anim_IdleAltEnter",
    "Anim_IdleAltLoop",
    "Anim_Crouch",
    "Anim_CrouchLand",
    "Anim_Run",
    "Anim_RunStop",
    "Anim_RunStop_2",
    "Anim_CrouchIn",
    "Anim_CrouchOut",
    "Anim_RunLand",
    "Anim_WallEnter",
    "Anim_WallIdle",
    "Anim_Fall_01",
    "Anim_FastFall",
    "BlendSpace_Jump_01",
    "BlendSpace_Jump_02",
    "Anim_HitReact_Front",
    "Anim_HitReact_Back",
    "Anim_HitReact_N_01",
    "Anim_HitReact_N_02",
    "Anim_HitReact_N_03",
    "Anim_HitReact_N_04",
    "Anim_HitReact_Recover",
    "Anim_HitReact_FlyBack",
    "Anim_HitReact_FlyForward",
    "Anim_HitReact_FlyBack_Intro",
    "Anim_HitReact_FlyForward_Intro",
    "Anim_HitReact_WallBounceBack",
    "Anim_HitReact_WallBounceFront",
    "Anim_HitReact_FloorBounceBack",
    "Anim_HitReact_FloorBounceFront",
];

/// Base_AnimBP's looping sequence players' keys (animSetFrom's `looping`).
const LOOPING_KEYS: [&str; 8] = [
    "Anim_IdleAltLoop",
    "Anim_Idle",
    "Anim_Run",
    "Anim_Fall_01",
    "Anim_Crouch",
    "Anim_WallIdle",
    "Anim_HitReact_FlyBack",
    "Anim_HitReact_FlyForward",
];

/// The mesh's own set: its roles (as [`set_roles`]) and looping clips.
#[derive(Debug)]
struct RoleSet {
    roles: Vec<Option<String>>,
    looping: Vec<String>,
}

impl RoleSet {
    fn of(s: &AnimSet) -> RoleSet {
        RoleSet { roles: set_roles(s), looping: s.looping.iter().flatten().cloned().collect() }
    }

    /// `polymorphAnimSet`: a kit's (or the items part's) set of an AnimBP class (its class defaults), each clip
    /// through the data's anim assets `assets` (an AnimComposite's or a sequence's first sample).
    fn of_props(assets: &Map<AnimAsset>, st: &Props) -> RoleSet {
        let a = |k: &str| -> Option<String> {
            let v = st.get(k).and_then(Value::as_str)?;
            let s = assets.get(v).and_then(|x| x.samples.as_ref()).and_then(|s| s.first());
            Some(s.map_or(v, String::as_str).to_owned())
        };
        let on = |k: &str| st.get(k).and_then(Value::as_bool) == Some(true);
        // animSetFrom: UseFallBlendspace needs the data's fallSpace of the base set, which a kit's prop set is not
        // (a lookup there would fail), so a prop set has no fall samples to loop
        assert!(
            !on("UseFallBlendspace"),
            "polymorph anim set: UseFallBlendspace without its FallBlendSpace in the data"
        );
        let mut looping: Vec<String> = LOOPING_KEYS.iter().filter_map(|k| a(k)).collect();
        // (and CrouchRun's, SequencePlayer_54, where the set can reach it)
        if on("UseCrouchRun") {
            looping.push(a("Anim_CrouchRun").expect("polymorph anim set: UseCrouchRun without an Anim_CrouchRun"));
        }
        RoleSet { roles: ROLE_KEYS.iter().map(|k| a(k)).collect(), looping }
    }
}

/// One fighter's view of one Polymorph Mesh.
#[derive(Debug)]
struct Poly {
    s: SecondView,
    victim: FighterId,
    from: Name,
    mesh: String,
    /// The clips' character (the mesh's), or the items part's draw.
    key: String,
    prop: bool,
    /// The items part's draw: posed with the items part's clips.
    items: bool,
    /// None: a prop whose kit has no set of the buff's AnimBP class (warned; no pose between montages).
    set: Option<RoleSet>,
    /// The victim set's role names -> the mesh set's clip, and the set it was made of.
    roles: HashMap<String, String>,
    roles_of: usize,
    idle: Option<usize>,
}

/// The Polymorph views of a match.
#[derive(Debug, Default)]
pub(crate) struct Polymorphs {
    views: Vec<Poly>,
}

impl Polymorphs {
    /// A view for each fighter and each Polymorph Mesh another fighter's kit can put on it, and, in a match with
    /// items, each one a buff of the items part can.
    pub(super) fn new(p: &mut ViewPoser, store: &mvs_store::Store, w: &World) -> crate::Result<Polymorphs> {
        let mut out = Polymorphs::default();
        let ids = w.all_ids();
        let items = items_data_of(w).map(|d| &d.buffs);
        for &i in &ids {
            let kits =
                ids.iter().filter(|&&j| j != i).filter_map(|&j| char_data(w, w.fighter(j).ch).fighter.buffs.as_ref());
            for (cls, def) in kits.chain(items).flatten() {
                let Some(mesh) = def.get("Polymorph Mesh").and_then(Value::as_str).filter(|m| !m.is_empty()) else {
                    continue;
                };
                let Some(cls_n) = w.names.get(cls) else { continue };
                let Some(from) = polymorph_owner(w, cls_n, mesh) else { continue };
                if out.views.iter().any(|v| v.victim == i && v.from == from && v.mesh == mesh) {
                    continue;
                }
                let anim_cls = def.get("PolymorphAnimClass").and_then(Value::as_str).unwrap_or("");
                let v = if from == ITEMS_OWNER {
                    build_items(p, w, i, cls, mesh, anim_cls)
                } else {
                    build(p, store, w, i, from, mesh, anim_cls)?
                };
                out.views.extend(v);
            }
        }
        Ok(out)
    }

    pub(super) fn views_mut(&mut self) -> impl Iterator<Item = &mut SecondView> {
        self.views.iter_mut().map(|v| &mut v.s)
    }

    pub(super) fn reset(&mut self) {}

    /// `place`: each fighter's buff takes its view, poses it, or hands it back (`items`: the items part's clips, in a
    /// match with items).
    pub(super) fn place(&mut self, w: &World, clips: &mut HashMap<String, Clips>, mut items: Option<&mut Clips>) {
        for f in w.all_ids() {
            let cur = self.views.iter().position(|v| v.victim == f && v.s.active);
            let Some(p) = w.fighter(f).shapes.polymorph.as_ref() else {
                if let Some(c) = cur {
                    self.views[c].s.drop_view();
                }
                continue;
            };
            let mesh = w.names.str(p.mesh);
            let cur = match cur {
                Some(c) if Some(self.views[c].from) == p.from && self.views[c].mesh == mesh => Some(c),
                Some(c) => {
                    self.views[c].s.drop_view();
                    None
                }
                None => None,
            };
            let at = match cur {
                Some(c) => c,
                None => {
                    let Some(from) = p.from else { continue };
                    let Some(c) = self.views.iter().position(|v| v.victim == f && v.from == from && v.mesh == mesh)
                    else {
                        warn_once(format!(
                            "Polymorph: no view of {mesh} ({}) was made for the match: not drawn",
                            w.names.str(from)
                        ));
                        continue;
                    };
                    self.views[c].s.take();
                    c
                }
            };
            let v = &mut self.views[at];
            let c = if v.items { items.as_deref_mut() } else { clips.get_mut(&v.key) };
            let Some(c) = c else { continue };
            tick(v, w, f, c);
        }
    }
}

/// `build`: the view of `mesh` on fighter `i` from the data of `from` (a character key).
fn build(
    p: &mut ViewPoser,
    store: &mvs_store::Store,
    w: &World,
    i: FighterId,
    from: Name,
    mesh: &str,
    anim_cls: &str,
) -> crate::Result<Option<Poly>> {
    let key = w.names.str(from).to_owned();
    let Some(cd) = w.data.chars.get(&key) else { return Ok(None) };
    let Some(colour) = p.rec_for(i).map(|r| r.colour) else { return Ok(None) };
    let prop = cd.fighter.skin.mesh != mesh;
    let (rec, set) = if !prop {
        // (the held character of the buff's mesh in its default skin, the victim's variant and stencil; the
        // relative location and scale are the buff's)
        let skin = skins::skin_of(&cd.fighter, None)?;
        let mut rec = p.make_rec(store, w, i, &key, skin, colour, Some(SecondKind::Polymorph))?;
        rec.view.mesh_scale = 1.0;
        rec.view.mesh_z = 0.0;
        (rec, Some(RoleSet::of(&cd.def.anim)))
    } else {
        // (the kit's own prop mesh, its actor materials in the victim's variant)
        let cls = asset_name(mesh);
        let Some(rec) = p.prop_rec(store, w, i, &key, cls, colour)? else {
            warn_once(format!("Polymorph: the mesh {mesh} has no actors entry in {key}'s draws: not drawn"));
            return Ok(None);
        };
        let set = cd.fighter.polymorph_anim_sets.as_ref().and_then(|m| m.get(anim_cls));
        if set.is_none() {
            warn_once(format!(
                "Polymorph: {key}'s data has no anim set of {} for {mesh}: no pose between montages",
                if anim_cls.is_empty() { "no PolymorphAnimClass" } else { anim_cls }
            ));
        }
        (rec, set.map(|s| RoleSet::of_props(&cd.fighter.anim_assets, &s.set)))
    };
    let clips = p.clips_of(&key);
    let idle = set.as_ref().and_then(|s| s.roles[0].as_deref()).and_then(|n| clips.and_then(|c| c.get(n)));
    if set.is_some() && idle.is_none() {
        warn_once(format!("Polymorph: the idle clip of {mesh} is not loaded ({key})"));
    }
    let skeleton = if prop { None } else { Some(key.as_str()) };
    Ok(Some(Poly {
        s: SecondView::new(rec, SecondKind::Polymorph, skeleton),
        victim: i,
        from,
        mesh: mesh.to_owned(),
        key,
        prop,
        items: false,
        set,
        roles: HashMap::new(),
        roles_of: 0,
        idle,
    }))
}

/// `build`'s items branch: the view of the items part's draw `<cls>.Polymorph` on fighter `i`, in the victim's
/// variant, posed with the items part's set of `anim_cls`.
fn build_items(p: &ViewPoser, w: &World, i: FighterId, cls: &str, mesh: &str, anim_cls: &str) -> Option<Poly> {
    let colour = p.rec_for(i)?.colour;
    let key = format!("{cls}.Polymorph");
    let Some(rec) = p.items_rec(w, i, &key, colour) else {
        warn_once(format!("Polymorph: the items draw {key} is not loaded (the items part's draws)"));
        return None;
    };
    let items = items_data_of(w)?;
    let set = items.polymorph_anim_sets.get(anim_cls).map(|s| RoleSet::of_props(&items.anim_assets, &s.set));
    if set.is_none() {
        warn_once(format!(
            "Polymorph: the items part has no anim set of {} for {mesh}: no pose between montages",
            if anim_cls.is_empty() { "no PolymorphAnimClass" } else { anim_cls }
        ));
    }
    let idle = set.as_ref().and_then(|s| s.roles[0].as_deref()).and_then(|n| p.item_clip(&key, n));
    if set.is_some() && idle.is_none() {
        warn_once(format!("Polymorph: the idle clip of {mesh} is not loaded (the items part)"));
    }
    Some(Poly {
        s: SecondView::new(rec, SecondKind::Polymorph, None),
        victim: i,
        from: ITEMS_OWNER,
        mesh: mesh.to_owned(),
        key,
        prop: true,
        items: true,
        set,
        roles: HashMap::new(),
        roles_of: 0,
        idle,
    })
}

/// `loco`: the victim's locomotion clip in the mesh's set (the header's Deviation) and its time.
fn loco(v: &mut Poly, w: &World, f: FighterId, clips: &Clips) -> (Option<usize>, f64) {
    let Some(set) = &v.set else { return (None, 0.0) };
    let own = mvs_sim::fighter::anim_set_def(w, f);
    let at = std::ptr::from_ref(own) as usize;
    if v.roles_of != at {
        v.roles.clear();
        for (a, b) in set_roles(own).into_iter().zip(&set.roles) {
            if let (Some(a), Some(b)) = (a, b) {
                v.roles.entry(a).or_insert_with(|| b.clone());
            }
        }
        v.roles_of = at;
    }
    let fi = w.fighter(f);
    let name = v.roles.get(w.names.str(fi.anim.anim));
    let clip = match name {
        Some(n) => clips.get(n),
        None => v.idle,
    };
    if let Some(n) = name
        && clip.is_none()
    {
        warn_once(format!("Polymorph: clip not loaded: {n} ({})", v.key));
    }
    let Some(c) = clip else { return (v.idle, 0.0) };
    let len = clips.clips[c].anim.length;
    let t = f64::from(fi.anim.anim_time);
    let looping = name.is_some_and(|n| set.looping.contains(n));
    let time = if looping && len > 0.0 {
        t.rem_euclid(f64::from(mvs_sim::math::narrow(len)))
    } else {
        t.clamp(0.0, f64::from(mvs_sim::math::narrow(len)))
    };
    (Some(c), time)
}

/// Handle Crouch (PP@13949) on the template capsule, in world units through the capsule's Z scale.
fn mesh_y(w: &World, f: FighterId, crouched: bool) -> f64 {
    let fi = w.fighter(f);
    let b = &char_data(w, fi.base_ch).def;
    let z = fi.body.actor_scale.map_or(fi.body.size_scale, |a| a[1]);
    f64::from(mvs_sim::fighter::actor_y(w, f))
        - if crouched { b.cap_radius.max(b.crouched_half) } else { b.cap_half } * f64::from(z)
}

/// `tick`: one presentation step of the taken view of fighter `f`.
fn tick(v: &mut Poly, w: &World, f: FighterId, clips: &mut Clips) {
    let fi = w.fighter(f);
    let p = fi.shapes.polymorph.as_ref().expect("a polymorph");
    let shown = fi.life.dead == 0.0 && mvs_sim::fighter::anim::visibility::invisibility_count(w, f) <= 1.0;
    let (x, y) = (fi.body.x, mesh_y(w, f, p.crouched));
    let lc = loco(v, w, f, clips);
    let Rec { view, pose, colour, outlines_on, .. } = &mut v.s.rec;
    if !v.prop {
        view.head_scale = f64::from(p.head);
    }
    *outlines_on = colour.outline && mvs_sim::fighter::outlines_on(w, f);
    view.place(f64::from(x), y, f64::from(fi.body.facing_dir));
    let m = p.play.as_ref().map(|pp| &pp.m);
    let halted = m.is_some_and(|m| m.ended && !m.terminated);
    let over = m.is_some_and(|m| m.terminated || m.ended);
    let md = m.and_then(|m| instance_data(&w.data, w, f, m.montage.uid));
    let mp = match (m, md) {
        (Some(m), Some(md)) if !over => m.pose(md, None),
        _ => None,
    };
    if let Some(h) = pose {
        h.set(w, f, view, clips);
    }
    let key = match fi.anim.anim_key {
        AnimKey::None => ViewKey::Empty,
        AnimKey::Montage(n) => ViewKey::Anim(n),
        AnimKey::Gen(g) => ViewKey::Gen(g),
    };
    let locos = Loco { dur: f64::from(fi.anim.loco_blend.0), hermite: fi.anim.loco_blend.1, key };
    let rules = pose.as_deref_mut().map(|p| p as &mut (dyn PoseRules + 'static));
    match (m, md, mp) {
        (Some(m), Some(md), Some((si, gi, t))) => {
            let anim = &md.slots[si].segments[gi].anim;
            let clip = clips.get(anim);
            if clip.is_none() {
                warn_once(format!("Polymorph: clip not loaded: {anim} ({})", v.key));
            }
            let wt = m.shown_weight(md, &|s| eprintln!("warning: {s}"));
            if wt < 1.0 {
                view.slot_under(lc.0, lc.1, wt, ViewKey::Gen(fi.anim.loco_gen));
            }
            let name = Some(m.montage.name);
            view.pose(clips, clip, t, name, md.blend_in, md.blend_out, STEP, false, &locos, None, &[], rules);
        }
        _ => {
            if halted {
                view.montage_gone(0.0);
            }
            view.pose(clips, lc.0, lc.1, None, 0.0, 0.0, STEP, false, &locos, None, &[], rules);
        }
    }
    v.s.placed(w, f, f64::from(x), y, shown);
}
