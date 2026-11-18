//! Morty's render module: the entry, the AnimBP's pose hook and the classes' mesh rules.
//!
//! The pose hook: Evil Morty's (C019_S02) EvilMorty slot. C019_AnimBP's last node is a
//! BlendListByBool on IsEvilMortySkin (pose 0: Slot EvilMorty over the cache DefaultAnim; pose 1: DefaultAnim; blend
//! time 0 both ways), which OnSkinApplied sets when AppliedSkin == SkinData'C019_S02' (c019_AnimBP @1180-1218). For
//! that skin, with an EvilMorty track playing (C019_Attack_Air_Axe_Swing_Montage and _Swing2), the view draws the
//! track's sequence in place of the DefaultSlot one and runs its SetSubmeshVisibility notifies (the Trident part's
//! windows). Every other skin's hook does nothing: the shared pose. The track's notifies are the montage's
//! `skin_slot_notifies`, which the gamedata bind keeps aside from the sim's.

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_data::fighter::{Montage, Notify};
use mvs_sim::World;
use mvs_sim::actors::projectile::Projectile;
use mvs_sim::actors::{ActorRef, ShipId};
use mvs_sim::fighter::anim::MontageSlot;
use mvs_sim::fighter::anim::montages::{instance_data, slot_uid};
use mvs_sim::fighter::{FighterId, MontageState, shown_montage};
use mvs_sim::names::Name;
use mvs_vfx::math::{Rows, rotation_rows};

use super::{CharRender, PoseHook, PoseOf};
use crate::actor_view::{SkinnedActor, Xform};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule};
use crate::anim::{Clips, HeadScaleNode};
use crate::anim_nodes::truthy;
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;
use crate::view::{CharView, PoseRules};

pub(crate) const RENDER: CharRender = CharRender {
    // The big head: C019_AnimBP's ModifyBone_4 and ModifyBone_1 (scale Additive, bone space, <- HeadScale; Pawn_C019
    // @1380).
    head_scale: &[
        HeadScaleNode {
            node: "ModifyBone_4",
            bone: "fml_un_C_head",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
        HeadScaleNode {
            node: "ModifyBone_1",
            bone: "fml_un_C_Face_att",
            factor: 1.0,
            gated: false,
            moves: [0.0; 3],
            scale: true,
        },
    ],
    // C019's mirror table leaves the body alone; the VisMesh scale reflects it.
    turns: true,
    pose: Some(morty_pose),
    ..CharRender::GENERIC
};

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[
    (SHIP_CLS, MeshRule { rot: None, mesh: Some(ship_view) }),
    ("C019_CookGrenade_C", MeshRule { rot: Some(grenade_rot), mesh: None }),
    ("C019_CookGrenade_NoGrav_C", MeshRule { rot: Some(grenade_rot), mesh: None }),
    ("c019_CookGrenade_Split_C", MeshRule { rot: Some(grenade_rot), mesh: None }),
    ("C019_WhipProjectile_C", MeshRule { rot: Some(whip_rot), mesh: None }),
];

// ---- the shots' rotations (the rules' `rot`) ----

/// C019_CookGrenade's ProjectileMovement has no bRotationFollowsVelocity; its particle component is sprites in local
/// space.
// Unverified: its actor rotation (the spawn transform's) is taken as zero.
fn grenade_rot(_w: &World, _pr: &Projectile) -> Option<Rows> {
    Some(rotation_rows(0.0, 0.0, 0.0))
}

/// C019_WhipProjectile: no bRotationFollowsVelocity, the spawn rotation (the owner's facing).
fn whip_rot(_w: &World, pr: &Projectile) -> Option<Rows> {
    let h = std::f64::consts::FRAC_PI_2;
    Some(rotation_rows(0.0, if pr.facing > 0.0 { -h } else { h }, 0.0))
}

// ---- C019_Ship ----
//
// The mesh: Ship_Skelmesh (SM_c019_Ship_SkelMesh, no animation = its reference pose, CastShadow false) with
// BeginPlay's dynamic instances of c019_Props2_Base_MIC_Local (slot 0) and C019_Ship_Glass_MAT (slot 1); the Reveal
// timeline (PlayFromStart at BeginPlay, linear 1 -> 0 over 0.7 s) sets Reveal on both each update. The Blueprint sets
// no Team on them. The actor's rotation is yaw -90 (BeginPlay's K2_SetActorRotation); the component's relative
// location is turned with it and its scale is RelativeScale3D.X. Placed at the sim's place of the latest tick (once a
// tick). The portal and the rings are the effects' (mvs-render fx/chars/morty.rs).
// Deviation: the timeline's time is taken as the actor's ticks of 1/60 s, the spawn frame's included, so it ends on
// the 42nd.
// The glass (slot 1, c019_Ship_Glass_MAT, translucent DefaultLit) is drawn: the drawmat kind builds it and the
// morty-combat pose tape's ship (t4361) has the materials on both slots.

const SHIP_CLS: &str = "C019_Ship_C";
const SHIP_MESH: &str = "Ship_Skelmesh";
const STEP: f64 = 1.0 / 60.0;
const REVEAL_SECONDS: f64 = 0.7;

/// A ship's mesh in its reference pose.
#[derive(Debug)]
struct ShipView {
    id: ShipId,
    view: SkinnedActor,
    /// Ship_Skelmesh's RelativeLocation, cm.
    rel: [f64; 3],
    scale: f64,
}

fn ship_view(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Ship(id) = a else { return None };
    let sh = w.actors.ships.get(id)?;
    let c = cs.get(SHIP_MESH)?;
    let from = mvs_sim::fighter::common::actor_def_owner(w, sh.owner, sh.cls);
    let (path, keys) = host.draws_of(w, from)?.actor(SHIP_CLS, None)?;
    let info = host.mesh(&path)?;
    Some(Box::new(ShipView { id, view: SkinnedActor::new(info, &keys), rel: c.loc, scale: c.scale[0] }))
}

impl ActorMesh for ShipView {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        _alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let rot = rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0);
        let l = self.rel;
        let mut loc = [0.0, -at.x, at.y];
        for (k, v) in loc.iter_mut().enumerate() {
            *v += l[0] * rot[k] + l[1] * rot[3 + k] + l[2] * rot[6 + k];
        }
        self.view.set(&Xform { loc, rot, scale: [self.scale; 3] });
        let age = w.actors.ships.get(self.id).map_or(0, |s| s.age);
        let reveal = (1.0 - f64::from(age) * STEP / REVEAL_SECONDS).max(0.0);
        for m in self.view.mats.iter_mut().flatten() {
            crate::pose::set_scalar(m, "Reveal", reveal);
        }
        self.view.visible = shown;
        self.view.draw(out);
    }
}

/// c019_AnimBP @1180: EqualEqual_ObjectObject(AppliedSkin, SkinData'C019_S02').
const SKIN: &str = "C019_S02";
/// The skin's slot (C019_AnimBP's Slot node over DefaultAnim).
const SLOT: &str = "EvilMorty";
const SUB: &str = "AnimNotify_SetSubmeshVisibility_C";

/// The hook of a skin other than C019_S02, or of a slaved view: the shared pose.
#[derive(Debug)]
struct MortyPose;

impl PoseRules for MortyPose {}
impl PoseHook for MortyPose {}

/// A montage's EvilMorty track as C019_S02's view draws it.
#[derive(Debug)]
struct SlotPlan {
    /// The DefaultSlot segment's sequence -> the EvilMorty segment's.
    anims: Vec<(Name, Name)>,
    /// The shown windows of each part this skin draws: [start, end) in montage time, merged.
    shows: Vec<Show>,
}

#[derive(Debug)]
struct Show {
    part: usize,
    from: Vec<f64>,
    to: Vec<f64>,
    until_blend_out: bool,
}

/// C019_S02's hook (`mine`).
///
/// C019_AnimBP's last node is a BlendListByBool on IsEvilMortySkin (pose 0: Slot EvilMorty over the cache
/// DefaultAnim; pose 1: DefaultAnim; blend time 0 both ways), which OnSkinApplied sets when AppliedSkin ==
/// SkinData'C019_S02' (c019_AnimBP @1180-1218). Two montages have an EvilMorty track, C019_Attack_Air_Axe_Swing_Montage
/// and _Swing2: the skin's copy of the DefaultSlot sequence (Skins/S02/Animations), on the same segments. For that
/// skin, with such a track playing, the Slot node's output is the copy's pose, so the view:
/// - draws the EvilMorty track's sequence at the fighter's anim time in place of the DefaultSlot one (`anim`; the
///   segments are checked to be the same, else it warns and draws the DefaultSlot's);
/// - runs that track's sequence notifies, which the sim leaves out (the gamedata bind keeps them aside in the
///   montage's `skin_slot_notifies`: the sim runs the default skin): SetSubmeshVisibility Axe hidden (C019_S02 has no
///   Axe part: nothing to draw) and Trident shown, ReverseAtNotifyEnd and ReverseWhenAnimBlendsOut. Trident is a part
///   the sim files no tag of (the default skin has none), so it is hidden (Skin::part_init) save in those windows.
///
/// Why the view alone: the copies differ from the default sequences in the cape, the face, the prop bones
/// (fml_un_C_prop1 and the prop roots under it, the Trident chain) and HitCollision01 at frames 1-9 of Axe_Swing (its
/// charge, before the hit window). Every bone a hurt box or the hits' RightHand shape is on, and HitCollision01 in the
/// hit windows, has the same key at every frame (the two .mva of each, compared bone by bone): the hits are the same.
// Unverified: the window test is the montage position after the tick's advance inside [t, t + dur] of the notify, two
// windows of one notify instance less than a tick apart (the segment boundary of Axe_Swing at 0.5) taken as one, as
// the queued dispatch's End and Begin in one tick leave it; the sim's own notify-state dispatch is not run.
#[derive(Debug)]
struct EvilMortyPose {
    /// The view's part keys, lower case (partIndex).
    part_keys: Vec<String>,
    /// Per montage name its plan, None where it has no EvilMorty track.
    plans: IndexMap<String, Option<SlotPlan>>,
    /// The montage position the view last saw: a window's end is sent on the update after the one its end falls in
    /// (the queued dispatch lists a state whose end lies in (prev, cur]: TriggerAnimNotifies), as the sim's Axe.
    last_m: Option<Name>,
    last_pos: f64,
    prev_pos: f64,
}

impl PoseRules for EvilMortyPose {}

impl EvilMortyPose {
    /// The view's part of a sub-mesh tag (partIndex).
    fn part(&self, tag: &str) -> Option<usize> {
        let low = tag.to_lowercase();
        self.part_keys.iter().position(|k| *k == low)
    }

    /// planOf: the montage's plan, made at its first sight.
    fn plan_of(&mut self, w: &World, m: &Montage, clips: &Clips) {
        if self.plans.contains_key(&m.name) {
            return;
        }
        let mut plan = None;
        let def = m.slots.iter().find(|s| s.slot == "DefaultSlot");
        if let Some(em) = m.slots.iter().find(|s| s.slot == SLOT) {
            let same = def.is_some_and(|d| {
                d.segments.len() == em.segments.len()
                    && d.segments.iter().zip(&em.segments).all(|(a, b)| {
                        a.start == b.start && a.a0 == b.a0 && a.a1 == b.a1 && a.rate == b.rate && a.loops == b.loops
                    })
            });
            if !same {
                warn_once(format!(
                    "Morty pose: {}'s EvilMorty track is not on the DefaultSlot's segments: the DefaultSlot's \
                     pose is drawn",
                    m.name
                ));
            }
            let mut anims: Vec<(&str, &str)> = Vec::new();
            if let (true, Some(d)) = (same, def) {
                for (a, b) in d.segments.iter().zip(&em.segments) {
                    // (a Map: a sequence on two segments takes the last one's)
                    match anims.iter_mut().find(|(x, _)| *x == a.anim) {
                        Some(e) => e.1 = &b.anim,
                        None => anims.push((&a.anim, &b.anim)),
                    }
                }
            }
            if let Some((_, b)) = anims.iter().find(|(_, b)| clips.get(b).is_none()) {
                warn_once(format!("Morty pose: clip {b} is not loaded: the DefaultSlot pose is drawn"));
                anims.clear();
            }
            let anims = anims.iter().filter_map(|(a, b)| Some((w.names.get(a)?, w.names.get(b)?))).collect();
            let mut by_inst: IndexMap<&str, Vec<&Notify>> = IndexMap::new();
            for n in m.skin_slot_notifies.iter().filter(|n| n.slot.as_deref() == Some(SLOT)) {
                if n.cls != SUB {
                    continue;
                }
                let tag = n.p.get("SubmeshTag").and_then(Value::as_str).unwrap_or("None");
                if self.part(tag).is_none() {
                    continue; // (Axe: no part in C019_S02)
                }
                if !truthy(n.p.get("Visible")) || !truthy(n.p.get("ReverseAtNotifyEnd")) {
                    warn_once(format!(
                        "Morty pose: {} {} ({tag} {}, no reverse at its end) is not ported",
                        m.name,
                        n.inst.as_deref().unwrap_or("null"),
                        truthy(n.p.get("Visible"))
                    ));
                    continue;
                }
                by_inst.entry(n.inst.as_deref().unwrap_or("")).or_default().push(n);
            }
            let mut shows = Vec::new();
            for ns in by_inst.values_mut() {
                ns.sort_by(|a, b| a.t.total_cmp(&b.t));
                let (mut from, mut to): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
                for n in ns.iter() {
                    match to.last_mut() {
                        Some(last) if n.t - *last < 1.0 / 60.0 => *last = last.max(n.t + n.dur),
                        _ => {
                            from.push(n.t);
                            to.push(n.t + n.dur);
                        }
                    }
                }
                let tag = ns[0].p.get("SubmeshTag").and_then(Value::as_str).unwrap_or("None");
                let Some(part) = self.part(tag) else { continue };
                shows.push(Show { part, from, to, until_blend_out: truthy(ns[0].p.get("ReverseWhenAnimBlendsOut")) });
            }
            plan = Some(SlotPlan { anims, shows });
        }
        self.plans.insert(m.name.clone(), plan);
    }
}

/// The shown montage's state and data (`f.shownMontage`).
fn shown(w: &World, f: FighterId) -> Option<(&MontageState, &Montage)> {
    let fi = w.fighter(f);
    shown_montage(w, f).and_then(|dash| {
        let st = if dash { fi.montages.dash_montage.as_ref() } else { fi.montages.montage.as_ref() }?;
        let uid = slot_uid(w, f, if dash { MontageSlot::Dash } else { MontageSlot::Main })?;
        Some((st, instance_data(&w.data, w, f, uid)?))
    })
}

impl PoseHook for EvilMortyPose {
    fn set(&mut self, w: &World, f: FighterId, view: &mut CharView, clips: &Clips) {
        let sm = shown(w, f);
        // (a frame with no tick between keeps the position before the last change)
        match sm {
            Some((st, _)) if Some(st.montage.name) == self.last_m => {
                if st.pos != self.last_pos {
                    self.prev_pos = self.last_pos;
                    self.last_pos = st.pos;
                }
            }
            _ => {
                self.last_m = sm.map(|(st, _)| st.montage.name);
                self.last_pos = sm.map_or(0.0, |(st, _)| st.pos);
                self.prev_pos = self.last_pos;
            }
        }
        let prev = self.prev_pos;
        let Some((st, m)) = sm else { return };
        self.plan_of(w, m, clips);
        // (a part the sim files no tag of stays hidden under AddInvisibility: the parts alone drawn)
        let Some(Some(p)) = self.plans.get(&m.name) else { return };
        if view.parts_only {
            return;
        }
        let blending_out = st.blending_out();
        for s in &p.shows {
            if s.until_blend_out && blending_out {
                continue;
            }
            for (from, to) in s.from.iter().zip(&s.to) {
                if st.pos >= *from && prev < *to {
                    view.set_part_visible(s.part, true);
                }
            }
        }
    }

    fn anim(&self, w: &World, f: FighterId) -> Option<Name> {
        let (_, m) = shown(w, f)?;
        let p = self.plans.get(&m.name)?.as_ref()?;
        let a = w.fighter(f).anim.anim;
        p.anims.iter().find(|(d, _)| *d == a).map(|(_, e)| *e)
    }
}

fn morty_pose(_view: &mut CharView, _clips: &Clips, of: &PoseOf<'_>) -> Box<dyn PoseHook> {
    // (mine: the skin and not slaved, a Disguise's copy running no AnimBP of its own)
    if of.skin.name == SKIN && !of.slaved {
        return Box::new(EvilMortyPose {
            part_keys: of.part_keys.iter().map(|k| k.to_lowercase()).collect(),
            plans: IndexMap::new(),
            last_m: None,
            last_pos: 0.0,
            prev_pos: 0.0,
        });
    }
    Box::new(MortyPose)
}
